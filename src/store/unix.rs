use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::{env, fs};

use anyhow::{Context, Result};

const MARKER: &str = "# added by ev";

#[derive(Debug, PartialEq)]
enum Entry {
    Set(String, String),
    Unset(String),
}

fn parse(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|line| match line.strip_prefix('-') {
            Some(key) => Some(Entry::Unset(key.to_string())),
            None => line
                .split_once('=')
                .map(|(k, v)| Entry::Set(k.to_string(), v.to_string())),
        })
        .collect()
}

fn serialize(entries: &[Entry]) -> String {
    entries
        .iter()
        .map(|entry| match entry {
            Entry::Set(k, v) => format!("{k}={v}\n"),
            Entry::Unset(k) => format!("-{k}\n"),
        })
        .collect()
}

fn key_of(entry: &Entry) -> &str {
    match entry {
        Entry::Set(k, _) | Entry::Unset(k) => k,
    }
}

fn replace(mut entries: Vec<Entry>, new: Entry) -> Vec<Entry> {
    entries.retain(|e| key_of(e) != key_of(&new));
    entries.push(new);
    entries
}

fn quote_sh(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn quote_fish(value: &str) -> String {
    format!("'{}'", value.replace('\\', r"\\").replace('\'', r"\'"))
}

fn render_sh(entries: &[Entry]) -> String {
    entries
        .iter()
        .map(|entry| match entry {
            Entry::Set(k, v) => format!("export {k}={}\n", quote_sh(v)),
            Entry::Unset(k) => format!("unset {k}\n"),
        })
        .collect()
}

fn render_fish(entries: &[Entry]) -> String {
    entries
        .iter()
        .map(|entry| match entry {
            Entry::Set(k, v) => format!("set -gx {k} {}\n", quote_fish(v)),
            Entry::Unset(k) => format!("set -e {k}\n"),
        })
        .collect()
}

fn home() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

fn xdg_config() -> Result<PathBuf> {
    match env::var_os("XDG_CONFIG_HOME") {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => Ok(home()?.join(".config")),
    }
}

pub(crate) fn config_dir() -> Result<PathBuf> {
    match env::var_os("EV_HOME") {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => Ok(xdg_config()?.join("ev")),
    }
}

/// Every startup file that should load ev's generated env file for the
/// current shell, with whether it is fish syntax.
fn rc_files() -> Result<Vec<(PathBuf, bool)>> {
    let shell = env::var("SHELL").unwrap_or_default();
    let home = home()?;
    let files = match shell.rsplit('/').next().unwrap_or_default() {
        // .zshenv is read by every zsh invocation (scripts, `zsh -c`), but
        // .zshrc and .zprofile come after it, so hook .zshrc too and ev wins
        // in interactive shells. A ZDOTDIR exported from ~/.zshenv is only
        // known after that file ran, so ~/.zshenv is hooked as well.
        "zsh" => {
            let zdot = crate::sources::custom_zdotdir(Some(&home));
            let mut files = Vec::new();
            if let Some(zdot) = &zdot {
                files.push((home.join(".zshenv"), false));
                files.push((zdot.join(".zshenv"), false));
                files.push((zdot.join(".zshrc"), false));
            } else {
                files.push((home.join(".zshenv"), false));
                files.push((home.join(".zshrc"), false));
            }
            files
        }
        "bash" if cfg!(target_os = "macos") => {
            // Terminal.app starts login shells, which do not read .bashrc
            let profile = home.join(".profile");
            let bash_profile = home.join(".bash_profile");
            vec![(
                if bash_profile.exists() || !profile.exists() {
                    bash_profile
                } else {
                    profile
                },
                false,
            )]
        }
        // .bashrc covers interactive shells, .profile login shells
        "bash" => vec![
            (home.join(".bashrc"), false),
            (home.join(".profile"), false),
        ],
        "fish" => vec![(xdg_config()?.join("fish/conf.d/ev.fish"), true)],
        _ => vec![(home.join(".profile"), false)],
    };
    Ok(files)
}

/// Definitions of `key` in startup files that sit after ev's hook line in the
/// same file, so they override ev's value in new shells. Best effort: any
/// error yields no hits.
pub fn overriding_hits(key: &str) -> Vec<crate::sources::Hit> {
    let (Ok(dir), Ok(files)) = (config_dir(), rc_files()) else {
        return Vec::new();
    };
    let mut hooks = Vec::new();
    for (file, fish) in files {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let hook = hook_line(&dir, fish);
        if let Some(i) = text.lines().position(|l| l.trim_end() == hook) {
            hooks.push((file, i + 1));
        }
    }
    later_than_hooks(crate::sources::scan(key), &hooks)
}

/// The hits in a hooked file whose line number is greater than that file's
/// hook line number.
fn later_than_hooks(
    hits: Vec<crate::sources::Hit>,
    hooks: &[(PathBuf, usize)],
) -> Vec<crate::sources::Hit> {
    hits.into_iter()
        .filter(|h| hooks.iter().any(|(f, n)| *f == h.file && h.line_no > *n))
        .collect()
}

fn hook_line(dir: &Path, fish: bool) -> String {
    let dir = dir.display();
    if fish {
        format!("test -f \"{dir}/env.fish\"; and source \"{dir}/env.fish\"  {MARKER}")
    } else {
        format!("[ -f \"{dir}/env.sh\" ] && . \"{dir}/env.sh\"  {MARKER}")
    }
}

pub(crate) fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}.tmp", std::process::id()));
    let tmp = PathBuf::from(tmp);
    // values may be secrets: owner-only
    let written = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(content.as_bytes()))
        .with_context(|| format!("writing {}", tmp.display()));
    if let Err(err) = written {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    fs::rename(&tmp, path).with_context(|| {
        let _ = fs::remove_file(&tmp);
        format!("replacing {}", path.display())
    })
}

fn read_entries(dir: &Path) -> Result<Vec<Entry>> {
    match fs::read_to_string(dir.join("vars")) {
        Ok(text) => Ok(parse(&text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err).context("reading vars"),
    }
}

fn write_all(dir: &Path, entries: &[Entry]) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    // `vars` is the source of truth and goes last: it is the commit point
    write_atomic(&dir.join("env.sh"), &render_sh(entries))?;
    write_atomic(&dir.join("env.fish"), &render_fish(entries))?;
    write_atomic(&dir.join("vars"), &serialize(entries))
}

/// Takes the exclusive store lock; it is released when the file is dropped.
pub(crate) fn lock_store(dir: &Path) -> Result<fs::File> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(dir.join("lock"))
        .context("opening lock file")?;
    lock.lock().context("locking store")?;
    Ok(lock)
}

fn change(new: Entry) -> Result<()> {
    let dir = config_dir()?;
    let lock = lock_store(&dir)?;
    write_all(&dir, &replace(read_entries(&dir)?, new))?;
    drop(lock);
    // the hint is advisory: a failed check must not fail a saved change
    if matches!(hook_installed(&dir), Ok(false)) {
        eprintln!("Run 'ev init' once so new shells load these variables.");
    }
    Ok(())
}

fn has_hook(rc: &Path, line: &str) -> Result<bool> {
    match fs::read(rc) {
        // lossy: the rc may hold non-UTF-8 bytes we must not choke on
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).lines().any(|l| l == line)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| format!("reading {}", rc.display())),
    }
}

/// True only when every target file already has the hook.
fn hook_installed(dir: &Path) -> Result<bool> {
    for (rc, fish) in rc_files()? {
        if !has_hook(&rc, &hook_line(dir, fish))? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn list() -> Result<BTreeMap<String, String>> {
    Ok(read_entries(&config_dir()?)?
        .into_iter()
        .filter_map(|entry| match entry {
            Entry::Set(k, v) => Some((k, v)),
            Entry::Unset(_) => None,
        })
        .collect())
}

/// Names with a pending delete: they must not show up as variables.
pub fn deleted() -> Result<BTreeSet<String>> {
    Ok(read_entries(&config_dir()?)?
        .into_iter()
        .filter_map(|entry| match entry {
            Entry::Unset(k) => Some(k),
            Entry::Set(..) => None,
        })
        .collect())
}

pub fn set(key: &str, value: &str) -> Result<()> {
    change(Entry::Set(key.to_string(), value.to_string()))
}

pub fn delete(key: &str) -> Result<()> {
    change(Entry::Unset(key.to_string()))
}

pub fn init() -> Result<()> {
    let dir = config_dir()?;
    let lock = lock_store(&dir)?;
    let entries = read_entries(&dir)?;
    write_all(&dir, &entries)?;
    drop(lock);

    for (rc, fish) in rc_files()? {
        let line = hook_line(&dir, fish);
        if has_hook(&rc, &line)? {
            println!("{} already loads ev", rc.display());
            continue;
        }
        if let Some(parent) = rc.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&rc)
            .with_context(|| format!("opening {}", rc.display()))?;
        write!(file, "\n{line}\n").with_context(|| format!("writing {}", rc.display()))?;
        println!(
            "Added ev to {}. Open a new terminal to load it.",
            rc.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_entry(k: &str, v: &str) -> Entry {
        Entry::Set(k.into(), v.into())
    }

    #[test]
    fn parses_set_and_unset_lines() {
        let entries = parse("A=1\n-B\nC=x=y z\n\n");
        assert_eq!(
            entries,
            vec![
                set_entry("A", "1"),
                Entry::Unset("B".into()),
                set_entry("C", "x=y z")
            ]
        );
    }

    #[test]
    fn serialize_round_trips() {
        let entries = vec![set_entry("A", "it's $HOME = ok"), Entry::Unset("B".into())];
        assert_eq!(parse(&serialize(&entries)), entries);
    }

    #[test]
    fn replace_overrides_existing_key() {
        let entries = replace(
            vec![set_entry("A", "1"), set_entry("B", "2")],
            Entry::Unset("A".into()),
        );
        assert_eq!(entries, vec![set_entry("B", "2"), Entry::Unset("A".into())]);
    }

    #[test]
    fn quotes_for_sh() {
        assert_eq!(quote_sh("it's $x = a b"), r"'it'\''s $x = a b'");
    }

    #[test]
    fn quotes_for_fish() {
        assert_eq!(quote_fish(r"it's a\b"), r"'it\'s a\\b'");
    }

    #[test]
    fn renders_env_files() {
        let entries = vec![set_entry("A", "it's"), Entry::Unset("B".into())];
        assert_eq!(render_sh(&entries), "export A='it'\\''s'\nunset B\n");
        assert_eq!(render_fish(&entries), "set -gx A 'it\\'s'\nset -e B\n");
    }

    #[test]
    fn finds_hits_after_hook_only() {
        let hit = |f: &str, n: usize| crate::sources::Hit {
            file: PathBuf::from(f),
            line_no: n,
            text: String::new(),
            editable: true,
            why: "",
        };
        let hooks = vec![(PathBuf::from("/h/.zshenv"), 3)];
        let hits = vec![
            hit("/h/.zshenv", 5),
            hit("/h/.zshenv", 1),
            hit("/h/.zshrc", 9),
        ];
        assert_eq!(later_than_hooks(hits, &hooks), vec![hit("/h/.zshenv", 5)]);
    }

    #[test]
    fn hook_lines_carry_marker() {
        let dir = Path::new("/d");
        assert_eq!(
            hook_line(dir, false),
            "[ -f \"/d/env.sh\" ] && . \"/d/env.sh\"  # added by ev"
        );
        assert_eq!(
            hook_line(dir, true),
            "test -f \"/d/env.fish\"; and source \"/d/env.fish\"  # added by ev"
        );
    }
}
