//! Finds (and comments out) definitions of a variable in shell startup files.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::{env, fs};

use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub file: PathBuf,
    pub line_no: usize,
    pub text: String,
    pub editable: bool,
    /// Why ev leaves the line alone (empty when `editable`).
    pub why: &'static str,
}

const WHY_SELF: &str = "it appends to itself";
const WHY_COMPLEX: &str = "it is not a simple one-line assignment";
const WHY_PAM: &str = "this file is not a shell script";
const WHY_ROOT: &str = "ev does not edit files in your home directory when run as root";

#[derive(Debug, Default)]
pub struct Report {
    pub commented: Vec<Hit>,
    pub left: Vec<Hit>,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug)]
pub struct Skipped {
    pub file: PathBuf,
    pub reason: String,
}

struct Roots {
    home: Option<PathBuf>,
    system: PathBuf,
    /// ev's own config dir: its generated files are not user definitions
    skip: Option<PathBuf>,
    /// A `ZDOTDIR` other than home: zsh reads its startup files from there
    zdotdir: Option<PathBuf>,
    /// Running as root: only files outside `home` may be edited, so `sudo`
    /// never leaves root-owned files in the user's home.
    root: bool,
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// `ZDOTDIR` when it is set, non-empty and not the home directory itself.
pub fn custom_zdotdir(home: Option<&Path>) -> Option<PathBuf> {
    env::var_os("ZDOTDIR")
        .filter(|z| !z.is_empty())
        .map(PathBuf::from)
        .filter(|z| Some(z.as_path()) != home)
}

impl Roots {
    fn from_env() -> Self {
        Roots {
            home: home_dir(),
            system: env::var_os("EV_SYSTEM_ROOT").map_or_else(|| PathBuf::from("/"), PathBuf::from),
            skip: crate::store::config_dir().ok(),
            zdotdir: custom_zdotdir(home_dir().as_deref()),
            root: running_as_root(),
        }
    }

    fn files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if let Some(home) = &self.home {
            for name in USER_FILES {
                files.push(home.join(name));
            }
            files.extend(dir_files(&home.join(".config/fish/conf.d"), "fish"));
        }
        if let Some(zdotdir) = &self.zdotdir {
            for name in ZSH_FILES {
                files.push(zdotdir.join(name));
            }
        }
        let etc = |name: &str| self.system.join("etc").join(name);
        files.push(etc("profile"));
        files.extend(dir_files(&etc("profile.d"), "sh"));
        for name in SYSTEM_FILES {
            files.push(etc(name));
        }
        files.retain(|f| !self.skip.as_ref().is_some_and(|dir| f.starts_with(dir)));
        files
    }
}

const USER_FILES: [&str; 7] = [
    ".zshenv",
    ".zprofile",
    ".zshrc",
    ".bashrc",
    ".bash_profile",
    ".profile",
    ".config/fish/config.fish",
];
const ZSH_FILES: [&str; 3] = [".zshenv", ".zprofile", ".zshrc"];
const SYSTEM_FILES: [&str; 6] = [
    "zshenv",
    "zprofile",
    "zshrc",
    "bashrc",
    "bash.bashrc",
    "environment",
];
const HOOK_MARKER: &[u8] = b"# added by ev";
const REMOVED: &[u8] = b"# removed by ev: ";

/// True when ev runs as root on behalf of someone else, as `sudo` does on
/// macOS where HOME stays the user's: writing there would leave root-owned
/// files in their home. A root user in root's own home is not affected.
pub fn running_as_root() -> bool {
    let home_owner = home_dir().and_then(|h| fs::metadata(h).ok()).map(|m| {
        use std::os::unix::fs::MetadataExt;
        m.uid()
    });
    root_in_foreign_home(euid(), home_owner)
}

fn euid() -> u32 {
    extern "C" {
        // from the C library std already links; no extra dependency
        fn geteuid() -> u32;
    }
    // SAFETY: geteuid takes no arguments, has no preconditions and cannot fail.
    unsafe { geteuid() }
}

/// True when the effective user is root, in any home.
pub fn euid_is_root() -> bool {
    euid() == 0
}

/// Root (`euid` 0) with a HOME that belongs to another user, or none at all.
fn root_in_foreign_home(euid: u32, home_owner: Option<u32>) -> bool {
    euid == 0 && home_owner != Some(0)
}

/// Whether `file` lies inside `home` (by its own name or the file it links to).
fn in_home(file: &Path, home: Option<&Path>) -> bool {
    let Some(home) = home else { return false };
    if file.starts_with(home) {
        return true;
    }
    match (fs::canonicalize(file), fs::canonicalize(home)) {
        (Ok(file), Ok(home)) => file.starts_with(home),
        _ => false,
    }
}

/// Root may edit only files outside the user's home directory.
fn root_may_edit(file: &Path, home: Option<&Path>) -> bool {
    !in_home(file, home)
}

/// True when `file` is outside the current user's home directory.
pub fn is_system_file(file: &Path) -> bool {
    !in_home(file, home_dir().as_deref())
}

fn dir_files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect();
    found.sort();
    found
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_blank(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

fn trim_start(s: &[u8]) -> &[u8] {
    let n = s.iter().take_while(|&&b| is_blank(b)).count();
    &s[n..]
}

fn strip_cr(s: &[u8]) -> &[u8] {
    s.strip_suffix(b"\r").unwrap_or(s)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// `(key, rest, is_fish)` when the line assigns an exported variable. For
/// sh-style lines `rest` follows the `=`; for fish it follows the name.
fn parse_assign(line: &[u8]) -> Option<(&str, &[u8], bool)> {
    const SH: [&[u8]; 3] = [b"export", b"typeset -x", b"declare -x"];
    const FISH: [&[u8]; 3] = [b"set -gx", b"set -Ux", b"set -x"];
    let line = trim_start(line);
    let strip = |prefixes: &[&[u8]]| {
        prefixes.iter().find_map(|p| {
            let after = line.strip_prefix(*p)?;
            after
                .first()
                .is_some_and(|&b| is_blank(b))
                .then(|| trim_start(after))
        })
    };
    let fish = strip(&FISH);
    let rest = fish.or_else(|| strip(&SH)).unwrap_or(line);
    let fish = fish.is_some();
    let n = rest.iter().take_while(|&&b| is_ident(b)).count();
    if n == 0 || rest[0].is_ascii_digit() {
        return None;
    }
    let (key, after) = rest.split_at(n);
    let key = std::str::from_utf8(key).ok()?;
    if fish {
        return (after.is_empty() || is_blank(after[0])).then_some((key, after, true));
    }
    after.strip_prefix(b"=").map(|value| (key, value, false))
}

/// True when the value refers to the variable being defined (`$KEY`, `${KEY`).
fn refers_to(value: &[u8], key: &str) -> bool {
    let key = key.as_bytes();
    (0..value.len()).any(|i| {
        if value[i] != b'$' {
            return false;
        }
        let mut j = i + 1;
        if value.get(j) == Some(&b'{') {
            j += 1;
        }
        value[j..].starts_with(key) && !value.get(j + key.len()).is_some_and(|&b| is_ident(b))
    })
}

/// True when the value runs onto another line or chains other commands, so
/// commenting out this one line would be wrong.
/// In sh-style lines, an unquoted blank followed by anything but a comment
/// means more words (`A=1 B=2`, `A=1 cmd`), which we must not drop.
fn is_complex(value: &[u8], fish: bool) -> bool {
    let value = strip_cr(value);
    let (mut quote, mut depth, mut braces, mut i) = (None::<u8>, 0i32, 0i32, 0);
    while i < value.len() {
        let b = value[i];
        match quote {
            Some(b'\'') => {
                if b == b'\'' {
                    quote = None;
                }
            }
            Some(q) => {
                if b == b'\\' {
                    i += 1;
                    if i >= value.len() {
                        return true;
                    }
                } else if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'\\' => {
                    i += 1;
                    if i >= value.len() {
                        return true;
                    }
                }
                b'\'' | b'"' | b'`' => quote = Some(b),
                b' ' | b'\t' if !fish && depth == 0 && braces == 0 => {
                    let rest = trim_start(&value[i..]);
                    if rest.is_empty() || rest[0] == b'#' {
                        break;
                    }
                    return true;
                }
                b'{' if i > 0 && value[i - 1] == b'$' => braces += 1,
                b'}' if braces > 0 => braces -= 1,
                b'#' if i == 0 || is_blank(value[i - 1]) => break,
                b'(' => depth += 1,
                b')' => depth -= 1,
                b';' | b'&' | b'|' => return true,
                _ => {}
            },
        }
        i += 1;
    }
    quote.is_some() || depth > 0 || braces > 0
}

struct Found<'a> {
    line_no: usize,
    key: &'a str,
    line: &'a [u8],
    why: &'static str,
}

/// A heredoc delimiter and whether `<<-` lets it be indented with tabs.
type Heredoc = (Vec<u8>, bool);

/// The shell state left at the end of one line.
struct LineEnd {
    /// a quote still open (the string runs onto the next line)
    quote: Option<u8>,
    /// heredocs started on this line, in order
    heredocs: Vec<Heredoc>,
    /// ends with a backslash: the next line continues this one
    continued: bool,
}

/// Follows quotes, comments and heredoc operators through one line, starting
/// inside `quote` when an earlier line left one open. Rough on purpose: it
/// only has to tell which later lines are not commands of their own.
fn lex_line(line: &[u8], mut quote: Option<u8>, fish: bool) -> LineEnd {
    let mut heredocs = Vec::new();
    let mut arith = 0;
    let mut i = 0;
    while i < line.len() {
        let b = line[i];
        match quote {
            Some(b'\'') if !fish => {
                if b == b'\'' {
                    quote = None;
                }
            }
            Some(q) => {
                // `$` stands for bash's `$'...'`, which allows `\'`
                let close = if q == b'$' { b'\'' } else { q };
                if b == b'\\' {
                    i += 1;
                } else if b == close {
                    quote = None;
                }
            }
            None => match b {
                b'\\' if i + 1 == line.len() => {
                    return LineEnd {
                        quote,
                        heredocs,
                        continued: true,
                    };
                }
                b'\\' => i += 1,
                b'\'' if !fish && i > 0 && line[i - 1] == b'$' => quote = Some(b'$'),
                b'\'' | b'"' | b'`' => quote = Some(b),
                b'#' if i == 0 || is_blank(line[i - 1]) || b";&|(".contains(&line[i - 1]) => break,
                b'(' if line.get(i + 1) == Some(&b'(') => {
                    arith += 1;
                    i += 1;
                }
                b')' if arith > 0 && line.get(i + 1) == Some(&b')') => {
                    arith -= 1;
                    i += 1;
                }
                b'<' if !fish && arith == 0 && line.get(i + 1) == Some(&b'<') => {
                    if line.get(i + 2) == Some(&b'<') {
                        i += 3; // here-string
                        continue;
                    }
                    let mut j = i + 2;
                    let tabs = line.get(j) == Some(&b'-');
                    if tabs {
                        j += 1;
                    }
                    while j < line.len() && is_blank(line[j]) {
                        j += 1;
                    }
                    let (word, next) = heredoc_word(line, j);
                    if !word.is_empty() {
                        heredocs.push((word, tabs));
                    }
                    i = next;
                    continue;
                }
                _ => {}
            },
        }
        i += 1;
    }
    LineEnd {
        quote,
        heredocs,
        continued: false,
    }
}

/// The delimiter word at `line[j..]` with its quotes removed, and where it ends.
fn heredoc_word(line: &[u8], mut j: usize) -> (Vec<u8>, usize) {
    let mut word = Vec::new();
    while j < line.len() {
        let b = line[j];
        match b {
            b'\'' | b'"' => {
                j += 1;
                while j < line.len() && line[j] != b {
                    word.push(line[j]);
                    j += 1;
                }
                j += 1;
            }
            b'\\' => {
                if let Some(&next) = line.get(j + 1) {
                    word.push(next);
                }
                j += 2;
            }
            _ if is_blank(b) || b";&|<>()".contains(&b) => break,
            _ => {
                word.push(b);
                j += 1;
            }
        }
    }
    (word, j.min(line.len()))
}

/// Every assignment line, skipping lines that only continue an earlier one:
/// heredoc bodies, the rest of a multi-line string, and backslash
/// continuations.
fn assignments(bytes: &[u8], fish_file: bool) -> Vec<Found<'_>> {
    let mut found = Vec::new();
    let mut quote = None;
    let mut continued = false;
    let mut pending: std::collections::VecDeque<Heredoc> = Default::default();
    let mut heredoc: Option<Heredoc> = None;
    for (i, raw) in bytes.split(|&b| b == b'\n').enumerate() {
        let line = strip_cr(raw);
        if let Some((delim, tabs)) = &heredoc {
            let end = if *tabs {
                &line[line.iter().take_while(|&&b| b == b'\t').count()..]
            } else {
                line
            };
            if end == delim.as_slice() {
                heredoc = pending.pop_front();
            }
            continue;
        }
        let inside = quote.is_some() || continued;
        let end = lex_line(line, quote, fish_file);
        quote = end.quote;
        continued = end.continued;
        pending.extend(end.heredocs);
        if quote.is_none() && !continued {
            // heredoc bodies start once the command line is complete
            heredoc = pending.pop_front();
        }
        if inside || contains(line, HOOK_MARKER) {
            continue;
        }
        let Some((key, value, fish)) = parse_assign(line) else {
            continue;
        };
        let why = if refers_to(value, key) {
            WHY_SELF
        } else if is_complex(value, fish) {
            WHY_COMPLEX
        } else {
            ""
        };
        found.push(Found {
            line_no: i + 1,
            key,
            line,
            why,
        });
    }
    found
}

fn is_fish_file(file: &Path) -> bool {
    file.extension().is_some_and(|e| e == "fish")
}

fn scan_in(roots: &Roots, key: &str) -> Vec<Hit> {
    let pam_env = roots.system.join("etc/environment");
    let mut hits = Vec::new();
    for file in roots.files() {
        let Ok(bytes) = fs::read(&file) else { continue };
        for found in assignments(&bytes, is_fish_file(&file))
            .into_iter()
            .filter(|f| f.key == key)
        {
            let why = if file == pam_env {
                // pam_env syntax, not shell: a `:` line would break it
                WHY_PAM
            } else if roots.root && !root_may_edit(&file, roots.home.as_deref()) {
                WHY_ROOT
            } else {
                found.why
            };
            hits.push(Hit {
                file: file.clone(),
                line_no: found.line_no,
                text: String::from_utf8_lossy(found.line).into_owned(),
                editable: why.is_empty(),
                why,
            });
        }
    }
    hits
}

fn scan_all_in(roots: &Roots) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    for file in roots.files() {
        let Ok(bytes) = fs::read(&file) else { continue };
        for found in assignments(&bytes, is_fish_file(&file)) {
            vars.entry(found.key.to_string())
                .or_insert_with(|| file.display().to_string());
        }
    }
    vars
}

/// Every definition of `key` in the shell startup files.
pub fn scan(key: &str) -> Vec<Hit> {
    scan_in(&Roots::from_env(), key)
}

/// Every variable assigned in those files, with the first file that does it.
pub fn scan_all() -> BTreeMap<String, String> {
    scan_all_in(&Roots::from_env())
}

/// Comment out each editable hit (`<indent>: # removed by ev: <line>` in sh
/// files, `# removed by ev: <line>` in fish files), saving the
/// original file as `<file>.ev-bak` first when `backup` is set and no backup
/// exists yet. Files that cannot be changed are reported, not errors.
pub fn comment_out(hits: &[Hit], backup: bool) -> Result<Report> {
    let mut report = Report::default();
    // keyed by resolved path so two names for one file (a symlinked
    // ~/.profile) are edited once
    let mut by_file: BTreeMap<PathBuf, (&Path, Vec<&Hit>)> = BTreeMap::new();
    for hit in hits {
        if hit.editable {
            let real = fs::canonicalize(&hit.file).unwrap_or_else(|_| hit.file.clone());
            by_file
                .entry(real)
                .or_insert((&hit.file, Vec::new()))
                .1
                .push(hit);
        } else {
            report.left.push(hit.clone());
        }
    }
    for (file, file_hits) in by_file.into_values() {
        match edit_file(file, &file_hits, backup) {
            Ok(()) => report.commented.extend(file_hits.into_iter().cloned()),
            Err(err) => report.skipped.push(Skipped {
                file: file.to_path_buf(),
                reason: format!("{err:#}"),
            }),
        }
    }
    Ok(report)
}

fn edit_file(file: &Path, hits: &[&Hit], backup: bool) -> Result<()> {
    // edit through symlinks so a dotfile manager's link stays a link
    let target = fs::canonicalize(file).with_context(|| format!("resolving {}", file.display()))?;
    let bytes = fs::read(&target).with_context(|| format!("reading {}", target.display()))?;
    let meta = fs::metadata(&target)?;
    fs::OpenOptions::new()
        .write(true)
        .open(&target)
        .context("not writable")?;

    let wanted: BTreeSet<usize> = hits.iter().map(|h| h.line_no).collect();
    let mut out = Vec::with_capacity(bytes.len() + wanted.len() * REMOVED.len());
    let mut done = 0;
    for (i, raw) in bytes.split_inclusive(|&b| b == b'\n').enumerate() {
        let line_no = i + 1;
        if !wanted.contains(&line_no) {
            out.extend_from_slice(raw);
            continue;
        }
        let body = raw.strip_suffix(b"\n").unwrap_or(raw);
        let unchanged = hits.iter().any(|h| {
            h.line_no == line_no
                && h.text.as_bytes() == String::from_utf8_lossy(strip_cr(body)).as_bytes()
        });
        if !unchanged {
            bail!("line {line_no} changed since it was scanned");
        }
        let indent = raw.len() - trim_start(raw).len();
        let is_fish = is_fish_file(file)
            || is_fish_file(&target)
            || parse_assign(body).is_some_and(|(_, _, fish)| fish);
        if !is_fish {
            // a no-op, not a bare comment, so an `if`/`case`/function body
            // whose only statement this was stays valid shell
            out.extend_from_slice(&raw[..indent]);
            out.extend_from_slice(b": ");
            out.extend_from_slice(REMOVED);
            out.extend_from_slice(&raw[indent..]);
        } else {
            out.extend_from_slice(REMOVED);
            out.extend_from_slice(raw);
        }
        done += 1;
    }
    if done != wanted.len() {
        bail!("file changed since it was scanned");
    }

    if backup {
        let mut bak = target.as_os_str().to_owned();
        bak.push(".ev-bak");
        let bak = PathBuf::from(bak);
        if fs::symlink_metadata(&bak).is_err() {
            fs::copy(&target, &bak).with_context(|| format!("backing up to {}", bak.display()))?;
        }
    }
    write_keeping_mode(&target, &out, &meta)
}

/// Like `store::write_atomic`, but keeps the original permissions.
fn write_keeping_mode(path: &Path, content: &[u8], meta: &fs::Metadata) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".ev-tmp");
    let tmp = PathBuf::from(tmp);
    let written = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(content))
        .and_then(|()| fs::set_permissions(&tmp, meta.permissions()))
        .and_then(|()| fs::rename(&tmp, path));
    if let Err(err) = written {
        let _ = fs::remove_file(&tmp);
        return Err(err).with_context(|| format!("writing {}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    struct Fx {
        _dir: TempDir,
        home: PathBuf,
        sys: PathBuf,
    }

    impl Fx {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            let home = dir.path().join("home");
            let sys = dir.path().join("sys");
            fs::create_dir_all(&home).unwrap();
            fs::create_dir_all(&sys).unwrap();
            Fx {
                _dir: dir,
                home,
                sys,
            }
        }

        fn roots(&self) -> Roots {
            Roots {
                home: Some(self.home.clone()),
                system: self.sys.clone(),
                skip: Some(self.home.join(".config/ev")),
                zdotdir: None,
                root: false,
            }
        }

        fn scan_as_root(&self, key: &str) -> Vec<Hit> {
            scan_in(
                &Roots {
                    root: true,
                    ..self.roots()
                },
                key,
            )
        }

        fn sh_ok(&self, path: &Path) -> Option<bool> {
            let probe = std::process::Command::new("sh")
                .arg("-c")
                .arg("true")
                .status()
                .ok()?;
            assert!(probe.success());
            Some(
                std::process::Command::new("sh")
                    .arg("-n")
                    .arg(path)
                    .status()
                    .unwrap()
                    .success(),
            )
        }

        fn write(&self, rel: &str, content: impl AsRef<[u8]>) -> PathBuf {
            let path = self.home.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, content).unwrap();
            path
        }

        fn write_sys(&self, rel: &str, content: &str) -> PathBuf {
            let path = self.sys.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, content).unwrap();
            path
        }

        fn scan(&self, key: &str) -> Vec<Hit> {
            scan_in(&self.roots(), key)
        }
    }

    #[test]
    fn scans_zsh_files_under_zdotdir() {
        let fx = Fx::new();
        let zdot = fx.home.join("zdot");
        fs::create_dir_all(&zdot).unwrap();
        fs::write(zdot.join(".zshrc"), "export FOO=1\n").unwrap();
        assert!(fx.scan("FOO").is_empty());
        let roots = Roots {
            zdotdir: Some(zdot.clone()),
            ..fx.roots()
        };
        let hits = scan_in(&roots, "FOO");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file, zdot.join(".zshrc"));
    }

    #[test]
    fn matches_every_assignment_form() {
        let fx = Fx::new();
        fx.write(
            ".zshrc",
            "export FOO=1\nFOO=2\n  typeset -x FOO=3\ndeclare -x FOO=4\n\texport FOO='a b'\n",
        );
        let hits = fx.scan("FOO");
        assert_eq!(hits.len(), 5);
        assert!(hits.iter().all(|h| h.editable));
        assert_eq!(hits[0].line_no, 1);
        assert_eq!(hits[0].text, "export FOO=1");
        assert_eq!(hits[2].text, "  typeset -x FOO=3");
    }

    #[test]
    fn matches_fish_forms() {
        let fx = Fx::new();
        fx.write(
            ".config/fish/config.fish",
            "set -gx FOO 1\nset -Ux FOO 2\nset -x FOO 3\n",
        );
        fx.write(".config/fish/conf.d/a.fish", "set -gx FOO 4\n");
        fx.write(".config/fish/conf.d/notes.txt", "set -gx FOO 5\n");
        let hits = fx.scan("FOO");
        assert_eq!(hits.len(), 4);
        assert!(hits.iter().all(|h| h.editable));
    }

    #[test]
    fn ignores_non_assignments() {
        let fx = Fx::new();
        fx.write(
            ".zshrc",
            "# export FOO=1\necho export FOO=1\nFOOBAR=1\nXFOO=1\nFOO+=1\nexport FOO\nunset FOO\n",
        );
        assert!(fx.scan("FOO").is_empty());
    }

    #[test]
    fn self_references_are_not_editable() {
        let fx = Fx::new();
        fx.write(
            ".zshrc",
            "PATH=$PATH:/x\nexport PATH=\"$PATH:/y\"\nexport PATH=${PATH}:/z\nexport PATH=/a:$PATHX\n",
        );
        let hits = fx.scan("PATH");
        let editable: Vec<bool> = hits.iter().map(|h| h.editable).collect();
        assert_eq!(editable, vec![false, false, false, true]);
    }

    #[test]
    fn fish_self_reference_is_not_editable() {
        let fx = Fx::new();
        fx.write(".config/fish/config.fish", "set -gx PATH $PATH /x\n");
        assert!(!fx.scan("PATH")[0].editable);
    }

    #[test]
    fn multi_line_and_compound_are_not_editable() {
        let fx = Fx::new();
        fx.write(
            ".zshrc",
            "export A=one \\\n  two\nexport A=\"open\nstill\"\nexport A='x\ny'\nexport A=$(\n  foo)\nexport A=1; echo hi\n",
        );
        let hits = fx.scan("A");
        assert_eq!(hits.len(), 5);
        assert!(hits.iter().all(|h| !h.editable), "{hits:?}");
    }

    #[test]
    fn balanced_quotes_and_comments_stay_editable() {
        let fx = Fx::new();
        fx.write(
            ".zshrc",
            "export A=\"it's\"\nexport A='say \"hi\"'\nexport A=1 # it's fine\nexport A=\"a\\\"b\"\n",
        );
        let hits = fx.scan("A");
        assert_eq!(hits.len(), 4);
        assert!(hits.iter().all(|h| h.editable), "{hits:?}");
    }

    #[test]
    fn skips_ev_hook_and_config_dir() {
        let fx = Fx::new();
        fx.write(
            ".zshrc",
            "FOO=\"$HOME/x\"  # added by ev\n[ -f \"/h/env.sh\" ] && . \"/h/env.sh\"  # added by ev\n",
        );
        fx.write(".config/ev/env.fish", "set -gx FOO 'x'\n");
        assert!(fx.scan("FOO").is_empty());
        assert!(scan_all_in(&fx.roots()).is_empty());
    }

    #[test]
    fn scans_system_files_under_root() {
        let fx = Fx::new();
        fx.write_sys("etc/profile", "export FOO=1\n");
        fx.write_sys("etc/profile.d/x.sh", "export FOO=2\n");
        fx.write_sys("etc/profile.d/x.conf", "export FOO=3\n");
        fx.write_sys("etc/environment", "FOO=4\n");
        fx.write_sys("etc/zshenv", "export FOO=5\n");
        fx.write_sys("etc/bash.bashrc", "export FOO=6\n");
        let hits = fx.scan("FOO");
        assert_eq!(hits.len(), 5);
        assert!(hits.iter().all(|h| h.file.starts_with(&fx.sys)));
    }

    #[test]
    fn etc_environment_is_report_only() {
        let fx = Fx::new();
        let env_file = fx.write_sys("etc/environment", "FOO=4\n");
        let hits = fx.scan("FOO");
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].editable);
        assert_eq!(hits[0].why, WHY_PAM);
        let report = comment_out(&hits, true).unwrap();
        assert_eq!(report.left.len(), 1);
        assert_eq!(fs::read_to_string(&env_file).unwrap(), "FOO=4\n");
    }

    #[test]
    fn unindented_block_body_stays_valid_shell() {
        let fx = Fx::new();
        let rc = fx.write(
            ".profile",
            "if true; then\nexport A=1\nfi\nf() {\nA=2\n}\ncase x in\nx)\nexport A=3\n;;\nesac\necho ok\n",
        );
        let hits = fx.scan("A");
        assert_eq!(hits.len(), 3);
        if let Some(ok) = fx.sh_ok(&rc) {
            assert!(ok, "fixture itself must be valid");
        }
        comment_out(&hits, true).unwrap();
        if let Some(ok) = fx.sh_ok(&rc) {
            assert!(ok, "{}", fs::read_to_string(&rc).unwrap());
        }
    }

    #[test]
    fn root_mode_edits_only_files_outside_home() {
        let fx = Fx::new();
        let user = fx.write(".zshrc", "export FOO=1\n");
        let sys = fx.write_sys("etc/profile", "export FOO=2\n");
        let hits = fx.scan_as_root("FOO");
        assert_eq!(hits.len(), 2);
        let user_hit = hits.iter().find(|h| h.file == user).unwrap();
        assert!(!user_hit.editable);
        assert_eq!(user_hit.why, WHY_ROOT);
        assert!(hits.iter().find(|h| h.file == sys).unwrap().editable);
        comment_out(&hits, true).unwrap();
        assert_eq!(fs::read_to_string(&user).unwrap(), "export FOO=1\n");
        assert!(!fx.home.join(".zshrc.ev-bak").exists());
        assert_eq!(
            fs::read_to_string(&sys).unwrap(),
            ": # removed by ev: export FOO=2\n"
        );
    }

    #[test]
    fn root_mode_is_root_in_someone_elses_home() {
        assert!(root_in_foreign_home(0, Some(501)));
        assert!(root_in_foreign_home(0, None));
        assert!(!root_in_foreign_home(0, Some(0)));
        assert!(!root_in_foreign_home(501, Some(501)));
        assert!(!root_in_foreign_home(501, None));
    }

    #[test]
    fn root_filter_follows_links_into_home() {
        let fx = Fx::new();
        let real = fx.write("dotfiles/profile", "x\n");
        let link = fx.sys.join("linked");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(!root_may_edit(&link, Some(&fx.home)));
        assert!(!root_may_edit(&fx.home.join(".zshrc"), Some(&fx.home)));
        assert!(root_may_edit(&fx.sys.join("etc/profile"), Some(&fx.home)));
        assert!(root_may_edit(&fx.sys.join("etc/profile"), None));
    }

    #[test]
    fn heredoc_bodies_are_not_assignments() {
        let fx = Fx::new();
        fx.write(
            ".zshrc",
            "cat <<EOF > /tmp/x\nexport FOO=in\nFOO=in\nEOF\nexport FOO=after1\n\
             cat <<-'END'\n\tFOO=in\n\tEND\nFOO=after2\n\
             cat << \"Q\" ; cat <<R\nFOO=in\nQ\nFOO=in\nR\nexport FOO=after3\n\
             x=$((1<<2))\nFOO=after4\ncat <<<\"FOO=here\"\nFOO=after5\n",
        );
        let texts: Vec<String> = fx.scan("FOO").into_iter().map(|h| h.text).collect();
        assert_eq!(
            texts,
            [
                "export FOO=after1",
                "FOO=after2",
                "export FOO=after3",
                "FOO=after4",
                "FOO=after5"
            ]
        );
        let all = scan_all_in(&fx.roots());
        assert!(all.contains_key("FOO"));
        fx.write(".zshrc", "cat <<EOF\nONLY_IN_DOC=1\nEOF\n");
        assert!(!scan_all_in(&fx.roots()).contains_key("ONLY_IN_DOC"));
    }

    #[test]
    fn open_strings_hide_following_lines() {
        let fx = Fx::new();
        fx.write(
            ".zshrc",
            "export MSG=\"line one\nFOO=in\nexport FOO=in\"\nFOO=after1\n\
             echo 'a\nFOO=in'\nFOO=after2\necho $'it\\'s'\nFOO=after3\n\
             echo it's # comment\nFOO=in\n'\nFOO=after4\n",
        );
        let texts: Vec<String> = fx.scan("FOO").into_iter().map(|h| h.text).collect();
        assert_eq!(
            texts,
            ["FOO=after1", "FOO=after2", "FOO=after3", "FOO=after4"]
        );
        assert_eq!(fx.scan("MSG").len(), 1);
    }

    #[test]
    fn fish_quotes_allow_escaped_quote() {
        let fx = Fx::new();
        fx.write(".config/fish/config.fish", "echo 'it\\'s'\nset -gx FOO 1\n");
        assert_eq!(fx.scan("FOO").len(), 1);
    }

    #[test]
    fn scan_all_maps_each_var_to_first_file() {
        let fx = Fx::new();
        fx.write(".zshenv", "export A=1\n");
        fx.write(".zshrc", "export A=2\nexport B=2\n");
        fx.write_sys("etc/profile", "export B=3\nexport C=3\n");
        let all = scan_all_in(&fx.roots());
        assert_eq!(all.len(), 3);
        assert_eq!(all["A"], fx.home.join(".zshenv").display().to_string());
        assert_eq!(all["B"], fx.home.join(".zshrc").display().to_string());
        assert_eq!(all["C"], fx.sys.join("etc/profile").display().to_string());
    }

    #[test]
    fn comment_out_replaces_line_and_backs_up_once() {
        let fx = Fx::new();
        let rc = fx.write(".zshrc", "# top\nexport FOO=1\nexport KEEP=2\n  FOO=3\n");
        let report = comment_out(&fx.scan("FOO"), true).unwrap();
        assert_eq!(report.commented.len(), 2);
        assert_eq!(
            fs::read_to_string(&rc).unwrap(),
            "# top\n: # removed by ev: export FOO=1\nexport KEEP=2\n  : # removed by ev: FOO=3\n"
        );
        let bak = fx.home.join(".zshrc.ev-bak");
        let original = "# top\nexport FOO=1\nexport KEEP=2\n  FOO=3\n";
        assert_eq!(fs::read_to_string(&bak).unwrap(), original);

        // a later run must not overwrite the first backup
        fs::write(&rc, "export FOO=9\n").unwrap();
        comment_out(&fx.scan("FOO"), true).unwrap();
        assert_eq!(fs::read_to_string(&bak).unwrap(), original);
    }

    #[test]
    fn comment_out_can_skip_backup() {
        let fx = Fx::new();
        fx.write(".zshrc", "export FOO=1\n");
        comment_out(&fx.scan("FOO"), false).unwrap();
        assert!(!fx.home.join(".zshrc.ev-bak").exists());
    }

    #[test]
    fn comment_out_keeps_missing_trailing_newline() {
        let fx = Fx::new();
        let rc = fx.write(".zshrc", "a=1\nexport FOO=1");
        comment_out(&fx.scan("FOO"), true).unwrap();
        assert_eq!(
            fs::read_to_string(&rc).unwrap(),
            "a=1\n: # removed by ev: export FOO=1"
        );
    }

    #[test]
    fn comment_out_never_edits_non_editable() {
        let fx = Fx::new();
        let rc = fx.write(".zshrc", "export PATH=$PATH:/x\nexport PATH=/y\n");
        let report = comment_out(&fx.scan("PATH"), true).unwrap();
        assert_eq!(report.left.len(), 1);
        assert_eq!(report.commented.len(), 1);
        assert_eq!(
            fs::read_to_string(&rc).unwrap(),
            "export PATH=$PATH:/x\n: # removed by ev: export PATH=/y\n"
        );
    }

    #[test]
    fn comment_out_preserves_non_utf8_bytes() {
        let fx = Fx::new();
        let rc = fx.write(
            ".zshrc",
            b"echo \xff\xfe\nexport FOO=\xe9\nalias x='\xc3\x28'\n".as_slice(),
        );
        let report = comment_out(&fx.scan("FOO"), true).unwrap();
        assert_eq!(report.commented.len(), 1);
        assert_eq!(
            fs::read(&rc).unwrap(),
            b"echo \xff\xfe\n: # removed by ev: export FOO=\xe9\nalias x='\xc3\x28'\n".to_vec()
        );
    }

    #[test]
    fn comment_out_preserves_mode_and_symlinks() {
        let fx = Fx::new();
        let real = fx.write("dotfiles/zshrc", "export FOO=1\n");
        fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).unwrap();
        let link = fx.home.join(".zshrc");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        comment_out(&fx.scan("FOO"), true).unwrap();
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read_to_string(&real).unwrap(),
            ": # removed by ev: export FOO=1\n"
        );
        let mode = fs::metadata(&real).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }

    #[test]
    fn comment_out_skips_unwritable_file() {
        let fx = Fx::new();
        let ro = fx.write(".zshenv", "export FOO=1\n");
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o444)).unwrap();
        if fs::OpenOptions::new().write(true).open(&ro).is_ok() {
            return; // running as root: permissions do not apply
        }
        let rw = fx.write(".zshrc", "export FOO=2\n");
        let report = comment_out(&fx.scan("FOO"), true).unwrap();
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].file, ro);
        assert_eq!(report.commented.len(), 1);
        assert_eq!(fs::read_to_string(&ro).unwrap(), "export FOO=1\n");
        assert!(!fx.home.join(".zshenv.ev-bak").exists());
        assert_eq!(
            fs::read_to_string(&rw).unwrap(),
            ": # removed by ev: export FOO=2\n"
        );
    }

    #[test]
    fn comment_out_skips_file_changed_since_scan() {
        let fx = Fx::new();
        let rc = fx.write(".zshrc", "export FOO=1\n");
        let hits = fx.scan("FOO");
        fs::write(&rc, "echo different\n").unwrap();
        let report = comment_out(&hits, true).unwrap();
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(fs::read_to_string(&rc).unwrap(), "echo different\n");
    }

    #[test]
    fn inline_prefix_and_multi_assignments_are_not_editable() {
        let fx = Fx::new();
        fx.write(
            ".zshrc",
            "FOO=1 cmd\nexport FOO=1 B=2\nFOO=1 B=2\nexport FOO=1 B=$FOO\nexport B=1 FOO=2\n",
        );
        let hits = fx.scan("FOO");
        assert_eq!(hits.len(), 4, "{hits:?}");
        assert!(hits.iter().all(|h| !h.editable), "{hits:?}");
    }

    #[test]
    fn spaces_inside_quotes_and_substitutions_stay_editable() {
        let fx = Fx::new();
        fx.write(
            ".zshrc",
            "export A=\"x y\"\nexport A=1 # comment\nexport A=$(echo a b)\nexport A=${B:-x y}\nexport A='p q'  \n",
        );
        let hits = fx.scan("A");
        assert_eq!(hits.len(), 5);
        assert!(hits.iter().all(|h| h.editable), "{hits:?}");
    }

    #[test]
    fn fish_values_with_spaces_stay_editable() {
        let fx = Fx::new();
        fx.write(".config/fish/config.fish", "set -gx A one two\n");
        assert!(fx.scan("A")[0].editable);
    }

    #[test]
    fn comment_out_uses_noop_for_indented_sh_lines_only() {
        let fx = Fx::new();
        let rc = fx.write(".zshrc", "export A=1\n\texport A=2\n    A=3\n");
        fx.write(".config/fish/config.fish", "set -gx A 1\n  set -gx A 2\n");
        comment_out(&fx.scan("A"), true).unwrap();
        assert_eq!(
            fs::read_to_string(&rc).unwrap(),
            ": # removed by ev: export A=1\n\t: # removed by ev: export A=2\n    : # removed by ev: A=3\n"
        );
        assert_eq!(
            fs::read_to_string(fx.home.join(".config/fish/config.fish")).unwrap(),
            "# removed by ev: set -gx A 1\n# removed by ev:   set -gx A 2\n"
        );
    }

    #[test]
    fn indented_block_body_stays_valid_shell() {
        let fx = Fx::new();
        let rc = fx.write(".profile", "if true; then\n  export A=1\nfi\necho ok\n");
        let Ok(probe) = std::process::Command::new("sh")
            .arg("-c")
            .arg("true")
            .status()
        else {
            return; // no sh available
        };
        assert!(probe.success());
        comment_out(&fx.scan("A"), true).unwrap();
        let check = std::process::Command::new("sh")
            .arg("-n")
            .arg(&rc)
            .status()
            .unwrap();
        assert!(check.success(), "{}", fs::read_to_string(&rc).unwrap());
    }

    #[test]
    fn symlinked_rc_is_edited_once() {
        let fx = Fx::new();
        let real = fx.write(".bashrc", "export FOO=1\n");
        std::os::unix::fs::symlink(&real, fx.home.join(".profile")).unwrap();
        let hits = fx.scan("FOO");
        assert_eq!(hits.len(), 2);
        let report = comment_out(&hits, true).unwrap();
        assert!(report.skipped.is_empty(), "{report:?}");
        assert_eq!(
            fs::read_to_string(&real).unwrap(),
            ": # removed by ev: export FOO=1\n"
        );
    }
}
