//! Go keeps its own settings in a file managed by `go env -w`.

use std::collections::BTreeMap;
use std::process::Command;

use anyhow::{bail, Context, Result};

pub struct Go;

impl super::App for Go {
    fn name(&self) -> &'static str {
        "go"
    }

    fn available(&self) -> bool {
        super::on_path("go")
    }

    fn list(&self) -> Result<BTreeMap<String, String>> {
        read_env_file(&go(&["env", "GOENV"])?)
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        // the effective value: Go's file, the OS environment, or the default
        let value = go(&["env", key])?;
        let value = value.strip_suffix('\n').unwrap_or(&value);
        Ok((!value.is_empty()).then(|| value.to_string()))
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        go(&["env", "-w", &format!("{key}={value}")]).map(drop)
    }

    fn unset(&self, key: &str) -> Result<()> {
        self.set(key, "")
    }

    fn delete(&self, key: &str) -> Result<()> {
        go(&["env", "-u", key]).map(drop)
    }

    fn delete_command(&self, key: &str) -> String {
        format!("go env -u {key}")
    }
}

fn go(args: &[&str]) -> Result<String> {
    let out = Command::new("go")
        .args(args)
        .output()
        .context("could not run go")?;
    if !out.status.success() {
        bail!(
            "go {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    String::from_utf8(out.stdout).context("go printed text that is not UTF-8")
}

/// The settings in Go's own file (`KEY=VALUE` lines, the last one wins).
/// Values from the OS environment are not Go's, so only this file counts.
fn parse_env_file(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| !key.is_empty() && *key != "GOENV")
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// Read the file `go env GOENV` names; none (or `off`) means no settings.
fn read_env_file(path: &str) -> Result<BTreeMap<String, String>> {
    let path = path.trim_end_matches(['\r', '\n']);
    if path.is_empty() || path == "off" {
        return Ok(BTreeMap::new());
    }
    match std::fs::read(path) {
        Ok(bytes) => Ok(parse_env_file(&String::from_utf8_lossy(&bytes))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(err) => Err(err).with_context(|| format!("reading {path}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_env_file_lines() {
        let map = parse_env_file(
            "GOPROXY=direct\r\n# note\n\nGOFLAGS=\nBAD\nGOPRIVATE=a=b\nGOPROXY=off\nGOENV=/x\n",
        );
        assert_eq!(map.len(), 3, "{map:?}");
        assert_eq!(map["GOPROXY"], "off");
        assert_eq!(map["GOFLAGS"], "");
        assert_eq!(map["GOPRIVATE"], "a=b");
    }

    #[test]
    fn missing_or_disabled_file_has_no_settings() {
        let dir = tempfile::TempDir::new().unwrap();
        let missing = dir.path().join("none");
        assert!(read_env_file(missing.to_str().unwrap()).unwrap().is_empty());
        assert!(read_env_file("off\n").unwrap().is_empty());
        assert!(read_env_file("\n").unwrap().is_empty());
        let file = dir.path().join("env");
        std::fs::write(&file, "GOOS=linux\n").unwrap();
        let path = format!("{}\n", file.display());
        assert_eq!(read_env_file(&path).unwrap()["GOOS"], "linux");
    }
}
