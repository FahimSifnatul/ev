use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};

#[cfg(unix)]
use crate::store::{config_dir, write_atomic};

const FILE: &str = "descriptions";

#[cfg(windows)]
fn config_dir() -> Result<std::path::PathBuf> {
    use std::path::PathBuf;

    if let Some(dir) = std::env::var_os("EV_HOME") {
        return Ok(PathBuf::from(dir));
    }
    let appdata = std::env::var_os("APPDATA").context("APPDATA is not set")?;
    Ok(PathBuf::from(appdata).join("ev"))
}

#[cfg(windows)]
fn write_atomic(path: &Path, content: &str) -> Result<()> {
    // unique per process, so concurrent writers do not share a temp file
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}.tmp", std::process::id()));
    let tmp = std::path::PathBuf::from(tmp);
    if let Err(err) = fs::write(&tmp, content) {
        let _ = fs::remove_file(&tmp);
        return Err(err).with_context(|| format!("writing {}", tmp.display()));
    }
    fs::rename(&tmp, path).with_context(|| {
        let _ = fs::remove_file(&tmp);
        format!("replacing {}", path.display())
    })
}

pub fn validate_text(text: &str) -> Result<()> {
    if text.contains(['\n', '\r', '\0']) {
        bail!("invalid description: newlines and NUL characters are not allowed");
    }
    Ok(())
}

fn parse(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, text)| (key.to_string(), text.to_string()))
        .collect()
}

fn render(notes: &BTreeMap<String, String>) -> String {
    notes
        .iter()
        .map(|(key, text)| format!("{key}={text}\n"))
        .collect()
}

fn load_from(dir: &Path) -> Result<BTreeMap<String, String>> {
    match fs::read_to_string(dir.join(FILE)) {
        Ok(text) => Ok(parse(&text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(err) => Err(err).context("reading descriptions"),
    }
}

fn write_to(dir: &Path, notes: &BTreeMap<String, String>) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    write_atomic(&dir.join(FILE), &render(notes))
}

/// The stored spelling of `key`, matching case-insensitively when asked.
fn stored_key(notes: &BTreeMap<String, String>, key: &str, ignore_case: bool) -> Option<String> {
    notes
        .keys()
        .find(|name| {
            if ignore_case {
                name.eq_ignore_ascii_case(key)
            } else {
                *name == key
            }
        })
        .cloned()
}

fn save_in(dir: &Path, key: &str, text: &str, ignore_case: bool) -> Result<()> {
    let mut notes = load_from(dir)?;
    let key = stored_key(&notes, key, ignore_case).unwrap_or_else(|| key.to_string());
    notes.insert(key, text.to_string());
    write_to(dir, &notes)
}

fn remove_in(dir: &Path, key: &str, ignore_case: bool) -> Result<()> {
    let mut notes = load_from(dir)?;
    if let Some(key) = stored_key(&notes, key, ignore_case) {
        notes.remove(&key);
        write_to(dir, &notes)?;
    }
    Ok(())
}

pub fn load() -> Result<BTreeMap<String, String>> {
    load_from(&config_dir()?)
}

pub fn save(key: &str, text: &str) -> Result<()> {
    let dir = config_dir()?;
    // read-modify-write: hold the store lock so parallel runs keep each other's notes
    #[cfg(unix)]
    let _lock = crate::store::lock_store(&dir)?;
    save_in(&dir, key, text, cfg!(windows))
}

pub fn remove(key: &str) -> Result<()> {
    let dir = config_dir()?;
    #[cfg(unix)]
    let _lock = crate::store::lock_store(&dir)?;
    remove_in(&dir, key, cfg!(windows))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn render_then_parse_round_trips() {
        let n = notes(&[("A", "first"), ("B", "second one")]);
        assert_eq!(render(&n), "A=first\nB=second one\n");
        assert_eq!(parse(&render(&n)), n);
    }

    #[test]
    fn parse_skips_malformed_lines() {
        assert_eq!(
            parse("A=1\nno separator\n\nB=2\n"),
            notes(&[("A", "1"), ("B", "2")])
        );
    }

    #[test]
    fn text_may_contain_equals() {
        assert_eq!(parse("A=x=y\n"), notes(&[("A", "x=y")]));
    }

    #[test]
    fn rejects_line_breaks_and_nul() {
        assert!(validate_text("one line = fine").is_ok());
        for bad in ["a\nb", "a\rb", "a\0b"] {
            assert!(validate_text(bad).is_err());
        }
    }

    #[test]
    fn missing_file_loads_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_from(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn save_adds_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        save_in(dir.path(), "A", "one", false).unwrap();
        save_in(dir.path(), "B", "two", false).unwrap();
        save_in(dir.path(), "A", "uno", false).unwrap();
        assert_eq!(
            load_from(dir.path()).unwrap(),
            notes(&[("A", "uno"), ("B", "two")])
        );
    }

    #[test]
    fn save_creates_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("new");
        save_in(&nested, "A", "one", false).unwrap();
        assert_eq!(load_from(&nested).unwrap(), notes(&[("A", "one")]));
    }

    #[test]
    fn remove_deletes_key() {
        let dir = tempfile::tempdir().unwrap();
        save_in(dir.path(), "A", "one", false).unwrap();
        remove_in(dir.path(), "A", false).unwrap();
        assert!(load_from(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn remove_absent_key_does_not_write() {
        let dir = tempfile::tempdir().unwrap();
        remove_in(dir.path(), "A", false).unwrap();
        assert!(!dir.path().join(FILE).exists());
    }

    #[test]
    fn ignore_case_save_replaces_existing_spelling() {
        let dir = tempfile::tempdir().unwrap();
        save_in(dir.path(), "PATH", "one", true).unwrap();
        save_in(dir.path(), "path", "two", true).unwrap();
        assert_eq!(load_from(dir.path()).unwrap(), notes(&[("PATH", "two")]));
    }

    #[test]
    fn ignore_case_remove_finds_key() {
        let dir = tempfile::tempdir().unwrap();
        save_in(dir.path(), "PATH", "one", true).unwrap();
        remove_in(dir.path(), "path", true).unwrap();
        assert!(load_from(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn exact_match_keeps_differently_cased_keys_apart() {
        let dir = tempfile::tempdir().unwrap();
        save_in(dir.path(), "PATH", "one", false).unwrap();
        save_in(dir.path(), "path", "two", false).unwrap();
        remove_in(dir.path(), "Path", false).unwrap();
        assert_eq!(
            load_from(dir.path()).unwrap(),
            notes(&[("PATH", "one"), ("path", "two")])
        );
    }
}
