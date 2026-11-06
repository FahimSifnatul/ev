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

    fn zshenv(&self) -> std::path::PathBuf {
        self.root.path().join(".zshenv")
    }

    fn rc(&self) -> std::path::PathBuf {
        self.root.path().join(".zshrc")
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ev"))
            .args(args)
            .env_clear()
            .env("EV_HOME", self.ev_home())
            .env("HOME", self.root.path())
            .env("ZDOTDIR", self.root.path())
            .env("SHELL", "/bin/zsh")
            .env("EV_SYSTEM_ROOT", self.root.path().join("sys"))
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
    assert_eq!(env.ok(&["list", "--ev"]), "A=1\nFOO=bar baz\n");
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
fn unset_sets_empty() {
    let env = Env::new();
    env.ok(&["set", "FOO", "bar"]);
    env.ok(&["unset", "FOO"]);
    assert_eq!(env.ok(&["get", "FOO"]), "\n");
    assert_eq!(env.read("vars"), "FOO=\n");
    assert_eq!(env.read("env.sh"), "export FOO=''\n");
    assert_eq!(env.ok(&["list", "--ev"]), "FOO=\n");
}

#[test]
fn delete_records_marker() {
    let env = Env::new();
    env.ok(&["set", "FOO", "bar"]);
    env.ok(&["delete", "FOO"]);
    assert!(!env.run(&["get", "FOO"]).status.success());
    assert_eq!(env.read("vars"), "-FOO\n");
    assert_eq!(env.read("env.sh"), "unset FOO\n");
    assert_eq!(env.read("env.fish"), "set -e FOO\n");
}

#[test]
fn piped_list_ev_omits_deleted_markers() {
    let env = Env::new();
    env.ok(&["set", "KEEP", "1"]);
    env.ok(&["set", "FOO", "bar"]);
    env.ok(&["delete", "FOO"]);
    assert_eq!(env.ok(&["list", "--ev"]), "KEEP=1\n");
}

#[test]
fn rm_alias_deletes() {
    let env = Env::new();
    env.ok(&["add", "FOO", "1"]);
    assert_eq!(env.ok(&["get", "FOO"]), "1\n");
    env.ok(&["rm", "FOO"]);
    assert_eq!(env.read("vars"), "-FOO\n");
    assert_eq!(env.ok(&["list", "--ev"]), "");
}

#[test]
fn unset_no_longer_aliases_delete() {
    let env = Env::new();
    env.ok(&["set", "FOO", "1"]);
    env.ok(&["unset", "FOO"]);
    assert_eq!(env.read("vars"), "FOO=\n");
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
    std::fs::write(env.zshenv(), "export KEEP=1\n").unwrap();
    env.ok(&["init"]);
    let first = std::fs::read_to_string(env.zshenv()).unwrap();
    env.ok(&["init"]);
    let rc = std::fs::read_to_string(env.zshenv()).unwrap();
    assert_eq!(rc, first);
    assert!(rc.starts_with("export KEEP=1\n"));
    assert_eq!(rc.matches("# added by ev").count(), 1);
    assert!(!env.rc().exists(), "init must not touch .zshrc");
    assert!(Path::new(&env.ev_home().join("env.sh")).exists());
    assert!(Path::new(&env.ev_home().join("env.fish")).exists());
}

#[test]
fn init_leaves_old_zshrc_hook_alone() {
    let env = Env::new();
    let old = "[ -f \"/old/env.sh\" ] && . \"/old/env.sh\"  # added by ev\n";
    std::fs::write(env.rc(), old).unwrap();
    env.ok(&["init"]);
    assert_eq!(std::fs::read_to_string(env.rc()).unwrap(), old);
    assert!(std::fs::read_to_string(env.zshenv())
        .unwrap()
        .contains("# added by ev"));
}

#[test]
fn init_hooks_every_bash_file() {
    let env = Env::new();
    let bash = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_ev"))
            .args(args)
            .env_clear()
            .env("EV_HOME", env.ev_home())
            .env("HOME", env.root.path())
            .env("SHELL", "/bin/bash")
            .env("EV_SYSTEM_ROOT", env.root.path().join("sys"))
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    };
    bash(&["init"]);
    let files: &[&str] = if cfg!(target_os = "macos") {
        &[".bash_profile"]
    } else {
        &[".bashrc", ".profile"]
    };
    let mut seen = Vec::new();
    for name in files {
        let text = std::fs::read_to_string(env.root.path().join(name)).unwrap();
        assert_eq!(text.matches("# added by ev").count(), 1, "{name}");
        seen.push(text);
    }
    bash(&["init"]);
    for (name, before) in files.iter().zip(&seen) {
        let after = std::fs::read_to_string(env.root.path().join(name)).unwrap();
        assert_eq!(&after, before, "{name} changed on second init");
    }
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
    std::fs::write(env.zshenv(), original).unwrap();
    env.ok(&["init"]);
    let rc = std::fs::read(env.zshenv()).unwrap();
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
fn unset_keeps_description() {
    let env = Env::new();
    env.ok(&["set", "FOO", "bar", "-d", "still here"]);
    env.ok(&["unset", "FOO"]);
    assert_eq!(env.read("descriptions"), "FOO=still here\n");
}

#[test]
fn delete_removes_description() {
    let env = Env::new();
    env.ok(&["set", "FOO", "bar", "-d", "gone soon"]);
    env.ok(&["set", "KEEP", "1", "-d", "stay"]);
    env.ok(&["delete", "FOO"]);
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
    assert_eq!(env.ok(&["list", "--ev"]), "FOO=bar\nHOME=/h\n");
}

#[test]
fn delete_comments_out_rc_line() {
    let env = Env::new();
    let rc = env.rc();
    std::fs::write(&rc, "export FOO=old\nexport KEEP=1\n").unwrap();
    env.ok(&["set", "FOO", "bar"]);
    let out = env.ok(&["--yes", "delete", "FOO"]);
    assert!(out.contains("Done"), "{out}");
    assert_eq!(
        std::fs::read_to_string(&rc).unwrap(),
        ": # removed by ev: export FOO=old\nexport KEEP=1\n"
    );
    assert_eq!(
        std::fs::read_to_string(env.root.path().join(".zshrc.ev-bak")).unwrap(),
        "export FOO=old\nexport KEEP=1\n"
    );
    assert_eq!(env.read("vars"), "-FOO\n");
}

#[test]
fn delete_leaves_path_append() {
    let env = Env::new();
    let rc = env.rc();
    let original = "export PATH=\"$PATH:/opt/x\"\n";
    std::fs::write(&rc, original).unwrap();
    let out = env.run(&["--yes", "delete", "PATH"]);
    assert!(out.status.success());
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), original);
    assert!(!env.root.path().join(".zshrc.ev-bak").exists());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains(".zshrc:1:"), "{stderr}");
    assert_eq!(env.read("vars"), "-PATH\n");
}

#[test]
fn delete_with_rc_hits_needs_yes_without_terminal() {
    let env = Env::new();
    std::fs::write(env.rc(), "export FOO=old\n").unwrap();
    assert!(!env.run(&["delete", "FOO"]).status.success());
    assert_eq!(std::fs::read_to_string(env.rc()).unwrap(), "export FOO=old\n");
    assert_eq!(env.read("vars"), "");
}

#[test]
fn list_includes_inherited_process_vars() {
    let env = Env::new();
    let out = Command::new(env!("CARGO_BIN_EXE_ev"))
        .arg("list")
        .env_clear()
        .env("EV_HOME", env.ev_home())
        .env("HOME", env.root.path())
        .env("EV_SYSTEM_ROOT", env.root.path().join("sys"))
        .env("INHERITED_VAR", "from-parent")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.lines().any(|l| l == "INHERITED_VAR=from-parent"), "{text}");
}

#[test]
fn get_finds_inherited_process_var() {
    let env = Env::new();
    let out = Command::new(env!("CARGO_BIN_EXE_ev"))
        .args(["get", "INHERITED_VAR"])
        .env_clear()
        .env("EV_HOME", env.ev_home())
        .env("HOME", env.root.path())
        .env("EV_SYSTEM_ROOT", env.root.path().join("sys"))
        .env("INHERITED_VAR", "from-parent")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "from-parent\n");
}

#[test]
fn ev_store_overrides_inherited_value() {
    let env = Env::new();
    env.ok(&["set", "INHERITED_VAR", "mine"]);
    let out = Command::new(env!("CARGO_BIN_EXE_ev"))
        .arg("list")
        .env_clear()
        .env("EV_HOME", env.ev_home())
        .env("HOME", env.root.path())
        .env("EV_SYSTEM_ROOT", env.root.path().join("sys"))
        .env("INHERITED_VAR", "from-parent")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = text.lines().filter(|l| l.starts_with("INHERITED_VAR")).collect();
    assert_eq!(lines, ["INHERITED_VAR=mine"]);
}

#[test]
fn list_ev_shows_only_managed_vars() {
    let env = Env::new();
    env.ok(&["set", "MINE", "1"]);
    let out = Command::new(env!("CARGO_BIN_EXE_ev"))
        .args(["list", "--ev"])
        .env_clear()
        .env("EV_HOME", env.ev_home())
        .env("HOME", env.root.path())
        .env("EV_SYSTEM_ROOT", env.root.path().join("sys"))
        .env("INHERITED_VAR", "from-parent")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "MINE=1\n");
}

#[test]
fn delete_marker_hides_inherited_var() {
    let env = Env::new();
    env.ok(&["delete", "INHERITED_VAR"]);
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ev"))
            .args(args)
            .env_clear()
            .env("EV_HOME", env.ev_home())
            .env("HOME", env.root.path())
            .env("EV_SYSTEM_ROOT", env.root.path().join("sys"))
            .env("INHERITED_VAR", "from-parent")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let text = String::from_utf8(run(&["list"]).stdout).unwrap();
    assert!(!text.contains("INHERITED_VAR"), "{text}");
    assert!(!run(&["get", "INHERITED_VAR"]).status.success());
}

// --- app providers (go) ---

fn go_installed() -> bool {
    Command::new("go")
        .arg("version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

impl Env {
    /// Like `run`, but with `go` on PATH and its config file inside the tempdir.
    fn run_go(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ev"))
            .args(args)
            .env_clear()
            .env("EV_HOME", self.ev_home())
            .env("HOME", self.root.path())
            .env("ZDOTDIR", self.root.path())
            .env("SHELL", "/bin/zsh")
            .env("EV_SYSTEM_ROOT", self.root.path().join("sys"))
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("GOENV", self.root.path().join("goenv"))
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn run_go_with(&self, args: &[&str], name: &str, value: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ev"))
            .args(args)
            .env_clear()
            .env("EV_HOME", self.ev_home())
            .env("HOME", self.root.path())
            .env("ZDOTDIR", self.root.path())
            .env("SHELL", "/bin/zsh")
            .env("EV_SYSTEM_ROOT", self.root.path().join("sys"))
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("GOENV", self.root.path().join("goenv"))
            .env(name, value)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn ok_go(&self, args: &[&str]) -> String {
        let out = self.run_go(args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
}

#[test]
fn set_then_get_go_var() {
    if !go_installed() {
        return;
    }
    let env = Env::new();
    env.ok_go(&["set", "GOPROXY", "direct", "--app", "go"]);
    assert_eq!(env.ok_go(&["get", "GOPROXY", "--app", "go"]), "direct\n");
    // the value lives in go's file, not in ev's store
    assert_eq!(env.read("vars"), "");
    // defaults are readable too
    assert!(!env.ok_go(&["get", "GOOS", "--app", "go"]).trim().is_empty());
}

#[test]
fn unset_empties_go_var() {
    if !go_installed() {
        return;
    }
    let env = Env::new();
    env.ok_go(&["set", "GOPROXY", "direct", "--app", "go"]);
    env.ok_go(&["unset", "GOPROXY", "--app", "go"]);
    assert!(!env.run_go(&["get", "GOPROXY", "--app", "go"]).status.success());
}

#[test]
fn delete_resets_go_var() {
    if !go_installed() {
        return;
    }
    let env = Env::new();
    env.ok_go(&["set", "GOPROXY", "direct", "--app", "go"]);
    env.ok_go(&["delete", "GOPROXY", "--app", "go"]);
    assert_ne!(env.ok_go(&["get", "GOPROXY", "--app", "go"]), "direct\n");
    assert!(!std::fs::read_to_string(env.root.path().join("goenv"))
        .unwrap_or_default()
        .contains("GOPROXY=direct"));
}

#[test]
fn delete_without_app_also_clears_go_var() {
    if !go_installed() {
        return;
    }
    let env = Env::new();
    env.ok_go(&["set", "GOPROXY", "direct", "--app", "go"]);
    env.ok_go(&["--yes", "delete", "GOPROXY"]);
    assert_ne!(env.ok_go(&["get", "GOPROXY", "--app", "go"]), "direct\n");
    assert_eq!(env.read("vars"), "-GOPROXY\n");
}

#[test]
fn list_shows_go_source() {
    if !go_installed() {
        return;
    }
    let env = Env::new();
    env.ok_go(&["set", "GOPROXY", "direct", "--app", "go"]);
    assert!(env.ok_go(&["list"]).lines().any(|l| l == "GOPROXY=direct"));
    // go values are not ev-managed
    assert!(!env.ok_go(&["list", "--ev"]).contains("GOPROXY"));
}

#[test]
fn unknown_app_is_rejected() {
    let env = Env::new();
    let out = env.run(&["set", "A", "1", "--app", "nope"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown app 'nope'"));
    assert_eq!(env.read("vars"), "");
}

#[test]
fn unavailable_app_is_rejected() {
    // no PATH at all: go cannot be found
    let env = Env::new();
    for args in [
        &["get", "GOOS", "--app", "go"][..],
        &["set", "GOPROXY", "x", "--app", "go"][..],
    ] {
        let out = env.run(args);
        assert!(!out.status.success());
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("not available"),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    // the merged list just skips it
    env.ok(&["list"]);
}

#[test]
fn delete_with_go_holder_needs_yes_without_terminal() {
    if !go_installed() {
        return;
    }
    let env = Env::new();
    env.ok_go(&["set", "GOPROXY", "direct", "--app", "go"]);
    let out = env.run_go(&["delete", "GOPROXY"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("go env -u GOPROXY"), "{stderr}");
    // nothing changed
    assert_eq!(env.read("vars"), "");
    assert_eq!(env.ok_go(&["get", "GOPROXY", "--app", "go"]), "direct\n");
}

#[test]
fn os_environment_values_are_not_go_settings() {
    if !go_installed() {
        return;
    }
    let env = Env::new();
    std::fs::write(env.rc(), "export GOPATH=/from/rc\n").unwrap();
    let out = env.run_go_with(&["list"], "GOPATH", "/from/rc");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.lines().any(|l| l == "GOPATH=/from/rc"), "{text}");
    // not go's: --app go has nothing to remove and says so
    let out = env.run_go_with(&["delete", "GOPATH", "--app", "go"], "GOPATH", "/from/rc");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Nothing to do"), "{stdout}");
    assert!(!stdout.contains("Done"), "{stdout}");
    // and plain delete does not offer go as a holder
    let out = env.run_go_with(&["--yes", "delete", "GOPATH"], "GOPATH", "/from/rc");
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("go env -u"));
}

#[test]
fn sudo_hint_names_the_key_and_only_for_system_files() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new();
    // system files must live outside HOME
    let sys = TempDir::new().unwrap();
    let etc = sys.path().join("etc");
    std::fs::create_dir_all(&etc).unwrap();
    let profile = etc.join("profile");
    std::fs::write(&profile, "export MYKEY=1\n").unwrap();
    std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o444)).unwrap();
    let rc = env.root.path().join(".zshenv");
    std::fs::write(&rc, "export MYKEY=2\n").unwrap();
    std::fs::set_permissions(&rc, std::fs::Permissions::from_mode(0o444)).unwrap();
    if std::fs::OpenOptions::new().write(true).open(&rc).is_ok() {
        return; // running as root: permissions do not apply
    }
    let out = Command::new(env!("CARGO_BIN_EXE_ev"))
        .args(["--yes", "delete", "MYKEY"])
        .env_clear()
        .env("EV_HOME", env.ev_home())
        .env("HOME", env.root.path())
        .env("SHELL", "/bin/zsh")
        .env("EV_SYSTEM_ROOT", sys.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<&str> = stderr.lines().filter(|l| l.contains("could not edit")).collect();
    assert_eq!(lines.len(), 2, "{stderr}");
    let sys_line = lines.iter().find(|l| l.contains("etc/profile")).unwrap();
    assert!(sys_line.contains("sudo ev delete MYKEY --yes"), "{stderr}");
    let user_line = lines.iter().find(|l| l.contains(".zshenv")).unwrap();
    assert!(!user_line.contains("sudo"), "{stderr}");
}

#[test]
fn heredoc_lines_are_not_rc_hits() {
    let env = Env::new();
    let original = "cat <<EOF > /tmp/never\nexport FOO=doc\nEOF\n";
    std::fs::write(env.rc(), original).unwrap();
    // no rc hit, so no confirmation is needed even without --yes
    env.ok(&["delete", "FOO"]);
    assert_eq!(std::fs::read_to_string(env.rc()).unwrap(), original);
}

#[test]
fn parallel_sets_do_not_lose_updates() {
    let env = Env::new();
    let children: Vec<_> = (0..20)
        .map(|i| {
            Command::new(env!("CARGO_BIN_EXE_ev"))
                .args(["set", &format!("K{i}"), "v"])
                .env_clear()
                .env("EV_HOME", env.ev_home())
                .env("HOME", env.root.path())
                .env("ZDOTDIR", env.root.path())
                .env("SHELL", "/bin/zsh")
                .env("EV_SYSTEM_ROOT", env.root.path().join("sys"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    for mut child in children {
        assert!(child.wait().unwrap().success(), "parallel ev set failed");
    }
    let listed = env.ok(&["list", "--ev"]);
    for i in 0..20 {
        assert!(listed.contains(&format!("K{i}=v\n")), "missing K{i}: {listed}");
    }
}

#[test]
fn set_warns_when_later_startup_line_overrides() {
    let env = Env::new();
    env.ok(&["init"]);
    let hook = std::fs::read_to_string(env.zshenv()).unwrap();
    std::fs::write(env.zshenv(), format!("{hook}export FOO=2\n")).unwrap();
    let out = env.run(&["set", "FOO", "1"]);
    assert!(out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    let want = format!(
        "ev: warning: FOO is also set in {}:",
        env.zshenv().display()
    );
    assert!(err.contains(&want), "{err}");
    assert!(err.contains(", after ev loads; that value wins in new shells"), "{err}");

    std::fs::write(env.zshenv(), format!("export BAR=2\n{hook}")).unwrap();
    let out = env.run(&["set", "BAR", "1"]);
    assert!(out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("warning: BAR"), "{err}");
}
