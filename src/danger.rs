const EXACT: &[(&str, &str)] = &[
    (
        "PATH",
        "controls where the shell finds programs; a bad value can break every command",
    ),
    ("HOME", "the user's home directory; many tools rely on it"),
    (
        "USER",
        "the current user name; scripts and tools rely on it",
    ),
    ("LOGNAME", "the login name; scripts and tools rely on it"),
    (
        "SHELL",
        "the login shell; a bad value can break terminals and logins",
    ),
    (
        "TERM",
        "the terminal type; a bad value can break interactive programs",
    ),
    (
        "LANG",
        "the system locale; a bad value can garble text or break tools",
    ),
    (
        "TMPDIR",
        "where temporary files go; a bad value can break many programs",
    ),
    (
        "USERPROFILE",
        "the user's profile directory; many Windows tools rely on it",
    ),
    (
        "SYSTEMROOT",
        "the Windows system directory; a bad value can break Windows programs",
    ),
    (
        "WINDIR",
        "the Windows system directory; a bad value can break Windows programs",
    ),
    (
        "COMSPEC",
        "the Windows command interpreter; a bad value can break cmd and scripts",
    ),
    (
        "PATHEXT",
        "which extensions Windows treats as executable; a bad value can break commands",
    ),
    (
        "PSMODULEPATH",
        "where PowerShell looks for modules; a bad value can break PowerShell",
    ),
    (
        "APPDATA",
        "the roaming application data directory; apps store settings there",
    ),
    (
        "LOCALAPPDATA",
        "the local application data directory; apps store settings there",
    ),
    (
        "TEMP",
        "where temporary files go; a bad value can break many programs",
    ),
    (
        "TMP",
        "where temporary files go; a bad value can break many programs",
    ),
    (
        "PROGRAMFILES",
        "where programs are installed; installers and tools rely on it",
    ),
];

const PREFIXES: &[(&str, &str)] = &[
    (
        "LD_",
        "alters how the dynamic linker loads libraries; a bad value can break or hijack programs",
    ),
    (
        "DYLD_",
        "alters how the dynamic linker loads libraries; a bad value can break or hijack programs",
    ),
];

/// Why changing `key` is risky, or `None` if it is an ordinary variable.
pub fn reason(key: &str) -> Option<&'static str> {
    let key = key.to_ascii_uppercase();
    EXACT
        .iter()
        .find(|(name, _)| *name == key)
        .or_else(|| PREFIXES.iter().find(|(prefix, _)| key.starts_with(prefix)))
        .map(|(_, why)| *why)
}

#[cfg(test)]
mod tests {
    use super::reason;

    #[test]
    fn exact_name_is_dangerous() {
        assert!(reason("PATH").is_some());
    }

    #[test]
    fn match_is_case_insensitive() {
        assert!(reason("path").is_some());
    }

    #[test]
    fn prefixes_are_dangerous() {
        assert!(reason("LD_PRELOAD").is_some());
        assert!(reason("DYLD_LIBRARY_PATH").is_some());
    }

    #[test]
    fn ordinary_name_is_safe() {
        assert_eq!(reason("FOO"), None);
    }
}
