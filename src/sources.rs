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
}

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
}

impl Roots {
    fn from_env() -> Self {
        Roots {
            home: env::var_os("HOME")
                .filter(|h| !h.is_empty())
                .map(PathBuf::from),
            system: env::var_os("EV_SYSTEM_ROOT").map_or_else(|| PathBuf::from("/"), PathBuf::from),
            skip: crate::store::config_dir().ok(),
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
const SYSTEM_FILES: [&str; 6] = ["zshenv", "zprofile", "zshrc", "bashrc", "bash.bashrc", "environment"];
const HOOK_MARKER: &[u8] = b"# added by ev";
const REMOVED: &[u8] = b"# removed by ev: ";

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

/// `(key, rest)` when the line assigns an exported variable. For sh-style
/// lines `rest` follows the `=`; for fish it follows the name.
fn parse_assign(line: &[u8]) -> Option<(&str, &[u8])> {
    const SH: [&[u8]; 3] = [b"export", b"typeset -x", b"declare -x"];
    const FISH: [&[u8]; 3] = [b"set -gx", b"set -Ux", b"set -x"];
    let line = trim_start(line);
    let strip = |prefixes: &[&[u8]]| {
        prefixes.iter().find_map(|p| {
            let after = line.strip_prefix(*p)?;
            after.first().is_some_and(|&b| is_blank(b)).then(|| trim_start(after))
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
        return (after.is_empty() || is_blank(after[0])).then_some((key, after));
    }
    after.strip_prefix(b"=").map(|value| (key, value))
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
fn is_complex(value: &[u8]) -> bool {
    let value = strip_cr(value);
    let (mut quote, mut depth, mut i) = (None::<u8>, 0i32, 0);
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
                b'\'' | b'"' => quote = Some(b),
                b'#' if i == 0 || is_blank(value[i - 1]) => break,
                b'(' => depth += 1,
                b')' => depth -= 1,
                b';' | b'&' | b'|' => return true,
                _ => {}
            },
        }
        i += 1;
    }
    quote.is_some() || depth > 0
}

struct Found<'a> {
    line_no: usize,
    key: &'a str,
    line: &'a [u8],
    editable: bool,
}

fn assignments(bytes: &[u8]) -> Vec<Found<'_>> {
    bytes
        .split(|&b| b == b'\n')
        .enumerate()
        .filter(|(_, line)| !contains(line, HOOK_MARKER))
        .filter_map(|(i, line)| {
            let (key, value) = parse_assign(line)?;
            Some(Found {
                line_no: i + 1,
                key,
                line: strip_cr(line),
                editable: !refers_to(value, key) && !is_complex(value),
            })
        })
        .collect()
}

fn scan_in(roots: &Roots, key: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    for file in roots.files() {
        let Ok(bytes) = fs::read(&file) else { continue };
        for found in assignments(&bytes).into_iter().filter(|f| f.key == key) {
            hits.push(Hit {
                file: file.clone(),
                line_no: found.line_no,
                text: String::from_utf8_lossy(found.line).into_owned(),
                editable: found.editable,
            });
        }
    }
    hits
}

fn scan_all_in(roots: &Roots) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    for file in roots.files() {
        let Ok(bytes) = fs::read(&file) else { continue };
        for found in assignments(&bytes) {
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
#[allow(dead_code)] // consumed by `ev list` in a later change
pub fn scan_all() -> BTreeMap<String, String> {
    scan_all_in(&Roots::from_env())
}

/// Comment out each editable hit (`# removed by ev: <line>`), saving the
/// original file as `<file>.ev-bak` first when `backup` is set and no backup
/// exists yet. Files that cannot be changed are reported, not errors.
pub fn comment_out(hits: &[Hit], backup: bool) -> Result<Report> {
    let mut report = Report::default();
    let mut by_file: BTreeMap<&Path, Vec<&Hit>> = BTreeMap::new();
    for hit in hits {
        if hit.editable {
            by_file.entry(&hit.file).or_default().push(hit);
        } else {
            report.left.push(hit.clone());
        }
    }
    for (file, file_hits) in by_file {
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
        let unchanged = hits
            .iter()
            .any(|h| h.line_no == line_no && h.text.as_bytes() == String::from_utf8_lossy(strip_cr(body)).as_bytes());
        if !unchanged {
            bail!("line {line_no} changed since it was scanned");
        }
        out.extend_from_slice(REMOVED);
        out.extend_from_slice(raw);
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
            Fx { _dir: dir, home, sys }
        }

        fn roots(&self) -> Roots {
            Roots {
                home: Some(self.home.clone()),
                system: self.sys.clone(),
                skip: Some(self.home.join(".config/ev")),
            }
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
            "# top\n# removed by ev: export FOO=1\nexport KEEP=2\n# removed by ev:   FOO=3\n"
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
        assert_eq!(fs::read_to_string(&rc).unwrap(), "a=1\n# removed by ev: export FOO=1");
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
            "export PATH=$PATH:/x\n# removed by ev: export PATH=/y\n"
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
            b"echo \xff\xfe\n# removed by ev: export FOO=\xe9\nalias x='\xc3\x28'\n".to_vec()
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
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), "# removed by ev: export FOO=1\n");
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
        assert_eq!(fs::read_to_string(&rw).unwrap(), "# removed by ev: export FOO=2\n");
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
}
