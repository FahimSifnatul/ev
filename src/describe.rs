use std::collections::BTreeMap;

const BUILTIN: &[(&str, &str)] = &[
    ("PATH", "directories searched for programs"),
    ("HOME", "user's home directory"),
    ("USER", "current user name"),
    ("SHELL", "user's login shell"),
    ("TERM", "terminal type"),
    ("LANG", "default locale"),
    ("LC_ALL", "overrides all locale settings"),
    ("TMPDIR", "directory for temporary files"),
    ("EDITOR", "default text editor"),
    ("VISUAL", "default full-screen editor"),
    ("PAGER", "program used to page output"),
    ("XDG_CONFIG_HOME", "base directory for user config"),
    ("XDG_DATA_HOME", "base directory for user data"),
    ("GOPATH", "go workspace directory"),
    ("GOROOT", "go installation directory"),
    ("JAVA_HOME", "java installation directory"),
    ("CARGO_HOME", "cargo data and binaries directory"),
    ("RUSTUP_HOME", "rustup toolchains directory"),
    ("PYTHONPATH", "extra python module search paths"),
    ("VIRTUAL_ENV", "active python virtual environment"),
    ("NODE_ENV", "node.js runtime environment name"),
    ("NODE_PATH", "extra node.js module search paths"),
    ("HTTP_PROXY", "proxy for HTTP requests"),
    ("HTTPS_PROXY", "proxy for HTTPS requests"),
    ("NO_PROXY", "hosts that bypass the proxy"),
    ("USERPROFILE", "user's profile directory"),
    ("APPDATA", "roaming application data directory"),
    ("LOCALAPPDATA", "local application data directory"),
    ("TEMP", "directory for temporary files"),
    ("TMP", "directory for temporary files"),
    ("PATHEXT", "extensions treated as executable"),
    ("COMSPEC", "command interpreter path"),
    ("SYSTEMROOT", "windows system directory"),
    ("PROGRAMFILES", "program installation directory"),
    ("EV_HOME", "where ev stores its files"),
];

pub fn builtin(key: &str) -> Option<&'static str> {
    BUILTIN
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|(_, text)| *text)
}

/// The user's note for `key` if there is one, else the built-in description.
pub fn description(key: &str, notes: &BTreeMap<String, String>) -> Option<String> {
    note(key, notes)
        .filter(|text| !text.trim().is_empty())
        .cloned()
        .or_else(|| builtin(key).map(str::to_string))
}

#[cfg(unix)]
fn note<'a>(key: &str, notes: &'a BTreeMap<String, String>) -> Option<&'a String> {
    notes.get(key)
}

// Windows variable names are case-insensitive
#[cfg(windows)]
fn note<'a>(key: &str, notes: &'a BTreeMap<String, String>) -> Option<&'a String> {
    notes
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|(_, text)| text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes(key: &str, text: &str) -> BTreeMap<String, String> {
        BTreeMap::from([(key.to_string(), text.to_string())])
    }

    #[test]
    fn builtin_texts_follow_style() {
        for (name, text) in BUILTIN {
            assert!(text.starts_with(|c: char| c.is_ascii_lowercase()), "{name}");
            assert!(!text.ends_with('.'), "{name}");
            assert!(text.split_whitespace().count() < 8, "{name}");
        }
    }

    #[test]
    fn builtin_hit() {
        assert_eq!(builtin("HOME"), Some("user's home directory"));
    }

    #[test]
    fn builtin_is_case_insensitive() {
        assert_eq!(builtin("home"), builtin("HOME"));
        assert!(builtin("home").is_some());
    }

    #[test]
    fn unknown_key_has_no_description() {
        assert_eq!(builtin("MY_CUSTOM_VAR"), None);
        assert_eq!(description("MY_CUSTOM_VAR", &BTreeMap::new()), None);
    }

    #[test]
    fn user_note_overrides_builtin() {
        assert_eq!(
            description("HOME", &notes("HOME", "mine")),
            Some("mine".to_string())
        );
    }

    #[test]
    fn blank_note_is_ignored() {
        assert_eq!(
            description("HOME", &notes("HOME", "  ")),
            Some("user's home directory".to_string())
        );
        assert_eq!(description("MY_VAR", &notes("MY_VAR", "")), None);
    }

    #[test]
    fn builtin_used_without_note() {
        assert_eq!(
            description("HOME", &notes("OTHER", "x")),
            Some("user's home directory".to_string())
        );
    }
}
