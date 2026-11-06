use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
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

/// The shell startup file that should load ev's generated env file.
fn rc_file() -> Result<(PathBuf, bool)> {
    let shell = env::var("SHELL").unwrap_or_default();
    let home = home()?;
    let rc = match shell.rsplit('/').next().unwrap_or_default() {
        "zsh" => (
            env::var_os("ZDOTDIR")
                .map_or(home, PathBuf::from)
                .join(".zshrc"),
            false,
        ),
        "bash" if cfg!(target_os = "macos") => {
            // Terminal.app starts login shells, which do not read .bashrc
            let profile = home.join(".profile");
            let bash_profile = home.join(".bash_profile");
            (
                if bash_profile.exists() || !profile.exists() {
                    bash_profile
                } else {
                    profile
                },
                false,
            )
        }
        "bash" => (home.join(".bashrc"), false),
        "fish" => (xdg_config()?.join("fish/conf.d/ev.fish"), true),
        _ => (home.join(".profile"), false),
    };
    Ok(rc)
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
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    // values may be secrets: owner-only
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(content.as_bytes()))
        .with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
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
    write_atomic(&dir.join("vars"), &serialize(entries))?;
    write_atomic(&dir.join("env.sh"), &render_sh(entries))?;
    write_atomic(&dir.join("env.fish"), &render_fish(entries))
}

fn change(new: Entry) -> Result<()> {
    let dir = config_dir()?;
    write_all(&dir, &replace(read_entries(&dir)?, new))?;
    // the hint is advisory: a failed check must not fail a saved change
    if matches!(hook_installed(&dir), Ok(false)) {
        eprintln!("Run 'ev init' once so new shells load these variables.");
    }
    Ok(())
}

fn hook_installed(dir: &Path) -> Result<bool> {
    let (rc, fish) = rc_file()?;
    match fs::read(&rc) {
        // lossy: the rc may hold non-UTF-8 bytes we must not choke on
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes)
            .lines()
            .any(|l| l == hook_line(dir, fish))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| format!("reading {}", rc.display())),
    }
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
    let entries = read_entries(&dir)?;
    write_all(&dir, &entries)?;

    let (rc, fish) = rc_file()?;
    if hook_installed(&dir)? {
        println!("{} already loads ev", rc.display());
        return Ok(());
    }
    if let Some(parent) = rc.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&rc)
        .with_context(|| format!("opening {}", rc.display()))?;
    write!(file, "\n{}\n", hook_line(&dir, fish))
        .with_context(|| format!("writing {}", rc.display()))?;
    println!(
        "Added ev to {}. Open a new terminal to load it.",
        rc.display()
    );
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
