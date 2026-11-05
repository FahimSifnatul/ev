#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

struct Env {
    root: TempDir,
}

impl Env {
    fn new() -> Self {
        Env {
            root: TempDir::new().unwrap(),
        }
    }

    fn ev_home(&self) -> std::path::PathBuf {
        self.root.path().join("ev")
    }

    fn rc(&self) -> std::path::PathBuf {
        self.root.path().join(".zshrc")
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ev"))
            .args(args)
            .env("EV_HOME", self.ev_home())
            .env("HOME", self.root.path())
            .env("ZDOTDIR", self.root.path())
            .env("SHELL", "/bin/zsh")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.ev_home().join(name)).unwrap_or_default()
    }
}

#[test]
fn set_then_get_and_list() {
    let env = Env::new();
    env.ok(&["set", "FOO", "bar baz"]);
    env.ok(&["set", "A", "1"]);
    assert_eq!(env.ok(&["get", "FOO"]), "bar baz\n");
    assert_eq!(env.ok(&["list"]), "A=1\nFOO=bar baz\n");
}

#[test]
fn get_missing_fails() {
    let env = Env::new();
    assert!(!env.run(&["get", "NOPE"]).status.success());
}

#[test]
fn set_overwrites() {
    let env = Env::new();
    env.ok(&["set", "FOO", "one"]);
    env.ok(&["set", "FOO", "two"]);
    assert_eq!(env.ok(&["get", "FOO"]), "two\n");
    assert_eq!(env.read("vars"), "FOO=two\n");
}

#[test]
fn unset_removes_and_records_marker() {
    let env = Env::new();
    env.ok(&["set", "FOO", "bar"]);
    env.ok(&["unset", "FOO"]);
    assert!(!env.run(&["get", "FOO"]).status.success());
    assert_eq!(env.read("vars"), "-FOO\n");
    assert_eq!(env.read("env.sh"), "unset FOO\n");
    assert_eq!(env.read("env.fish"), "set -e FOO\n");
}

#[test]
fn aliases_work() {
    let env = Env::new();
    env.ok(&["add", "FOO", "1"]);
    assert_eq!(env.ok(&["get", "FOO"]), "1\n");
    env.ok(&["delete", "FOO"]);
    env.ok(&["add", "FOO", "2"]);
    env.ok(&["rm", "FOO"]);
    assert_eq!(env.ok(&["list"]), "");
}

#[test]
fn invalid_key_is_rejected() {
    let env = Env::new();
    let out = env.run(&["set", "1BAD", "x"]);
    assert!(!out.status.success());
    assert_eq!(env.read("vars"), "");
}

#[test]
fn critical_var_needs_yes() {
    let env = Env::new();
    let out = env.run(&["set", "PATH", "x"]);
    assert!(!out.status.success());
    assert_eq!(env.read("vars"), "");

    env.ok(&["--yes", "set", "PATH", "x"]);
    assert_eq!(env.ok(&["get", "PATH"]), "x\n");
}

#[test]
fn env_sh_can_be_sourced() {
    let env = Env::new();
    env.ok(&["set", "FOO", "it's $HOME = ok"]);
    let script = format!(
        ". '{}' && printf %s \"$FOO\"",
        env.ev_home().join("env.sh").display()
    );
    let out = Command::new("sh").args(["-c", &script]).output().unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "it's $HOME = ok");
}

#[test]
fn init_adds_hook_once() {
    let env = Env::new();
    std::fs::write(env.rc(), "export KEEP=1\n").unwrap();
    env.ok(&["init"]);
    env.ok(&["init"]);
    let rc = std::fs::read_to_string(env.rc()).unwrap();
    assert!(rc.starts_with("export KEEP=1\n"));
    assert_eq!(rc.matches("# added by ev").count(), 1);
    assert!(Path::new(&env.ev_home().join("env.sh")).exists());
    assert!(Path::new(&env.ev_home().join("env.fish")).exists());
}

#[test]
fn set_hints_until_init() {
    let env = Env::new();
    let hint = "Run 'ev init' once";
    let out = env.run(&["set", "FOO", "1"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains(hint));
    env.ok(&["init"]);
    let out = env.run(&["set", "FOO", "2"]);
    assert!(!String::from_utf8_lossy(&out.stderr).contains(hint));
}

#[test]
fn init_keeps_non_utf8_rc_bytes() {
    let env = Env::new();
    let original = b"export X=\xff\xfe\n";
    std::fs::write(env.rc(), original).unwrap();
    env.ok(&["init"]);
    let rc = std::fs::read(env.rc()).unwrap();
    assert!(rc.starts_with(original));
    assert!(String::from_utf8_lossy(&rc).contains("# added by ev"));
}

#[test]
fn state_files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new();
    env.ok(&["set", "SECRET", "x"]);
    for name in ["vars", "env.sh", "env.fish"] {
        let mode = std::fs::metadata(env.ev_home().join(name))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "{name}");
    }
}

#[test]
fn set_succeeds_when_rc_cannot_be_located() {
    let env = Env::new();
    let out = Command::new(env!("CARGO_BIN_EXE_ev"))
        .args(["set", "FOO", "bar"])
        .env("EV_HOME", env.ev_home())
        .env_remove("HOME")
        .env_remove("ZDOTDIR")
        .env("SHELL", "/bin/zsh")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(env.read("vars"), "FOO=bar\n");
}

#[test]
fn set_desc_writes_descriptions_file() {
    let env = Env::new();
    env.ok(&["set", "FOO", "bar", "-d", "my foo"]);
    env.ok(&["set", "BAZ", "1", "--desc", "my baz"]);
    assert_eq!(env.read("descriptions"), "BAZ=my baz\nFOO=my foo\n");
}

#[test]
fn set_without_desc_keeps_existing_description() {
    let env = Env::new();
    env.ok(&["set", "FOO", "one", "-d", "keep me"]);
    env.ok(&["set", "FOO", "two"]);
    assert_eq!(env.read("descriptions"), "FOO=keep me\n");
}

#[test]
fn unset_removes_description() {
    let env = Env::new();
    env.ok(&["set", "FOO", "bar", "-d", "gone soon"]);
    env.ok(&["set", "KEEP", "1", "-d", "stay"]);
    env.ok(&["unset", "FOO"]);
    assert_eq!(env.read("descriptions"), "KEEP=stay\n");
}

#[test]
fn blank_desc_clears_description() {
    let env = Env::new();
    env.ok(&["set", "FOO", "bar", "-d", "x"]);
    env.ok(&["set", "KEEP", "1", "-d", "stay"]);
    env.ok(&["set", "FOO", "bar", "-d", "  "]);
    assert_eq!(env.read("descriptions"), "KEEP=stay\n");
    assert_eq!(env.ok(&["get", "FOO"]), "bar\n");
    env.ok(&["set", "KEEP", "1", "-d", ""]);
    assert_eq!(env.read("descriptions"), "");
}

#[test]
fn desc_with_newline_is_rejected_and_nothing_changes() {
    let env = Env::new();
    let out = env.run(&["set", "FOO", "bar", "-d", "two\nlines"]);
    assert!(!out.status.success());
    assert_eq!(env.read("vars"), "");
    assert_eq!(env.read("descriptions"), "");
}

#[test]
fn piped_output_ignores_descriptions() {
    let env = Env::new();
    env.ok(&["set", "FOO", "bar", "-d", "described"]);
    env.ok(&["set", "HOME", "/h", "--yes"]);
    assert_eq!(env.ok(&["get", "FOO"]), "bar\n");
    assert_eq!(env.ok(&["list"]), "FOO=bar\nHOME=/h\n");
}
