use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::ptr;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
};
use winreg::enums::{
    RegType, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_EXPAND_SZ, REG_SZ,
};
use winreg::types::{FromRegValue, ToRegValue};
use winreg::{RegKey, RegValue};

fn open(access: u32) -> Result<RegKey> {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", access)
        .context("cannot open HKCU\\Environment")
}

const MACHINE_ENV: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

fn defined_machine_wide(key: &str) -> Result<bool> {
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(MACHINE_ENV, KEY_READ)
        .context("cannot open the machine environment")?;
    Ok(machine.get_raw_value(key).is_ok()) // any type counts; not-found or unreadable means no
}

fn value_type(value: &str) -> RegType {
    if value.contains('%') {
        REG_EXPAND_SZ
    } else {
        REG_SZ
    }
}

fn broadcast_change() {
    let param: Vec<u16> = "Environment\0".encode_utf16().collect();
    // SAFETY: `param` is a valid null-terminated wide string that outlives the call.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            param.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5000,
            ptr::null_mut(),
        );
    }
}

pub fn list() -> Result<BTreeMap<String, String>> {
    let mut vars = BTreeMap::new();
    for entry in open(KEY_READ)?.enum_values() {
        let (name, raw) = entry?;
        if matches!(raw.vtype, REG_SZ | REG_EXPAND_SZ) {
            vars.insert(name, String::from_reg_value(&raw)?);
        }
    }
    Ok(vars)
}

/// Windows keeps no delete marker: a deleted value is gone from the registry.
pub fn deleted() -> Result<BTreeSet<String>> {
    Ok(BTreeSet::new())
}

/// Names defined machine-wide (read-only).
pub fn machine_names() -> Result<BTreeSet<String>> {
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(MACHINE_ENV, KEY_READ)
        .context("cannot open the machine environment")?;
    let mut names = BTreeSet::new();
    for entry in machine.enum_values() {
        let (name, raw) = entry?;
        if matches!(raw.vtype, REG_SZ | REG_EXPAND_SZ) {
            names.insert(name);
        }
    }
    Ok(names)
}

pub fn set(key: &str, value: &str) -> Result<()> {
    let raw = RegValue {
        vtype: value_type(value),
        ..value.to_reg_value()
    };
    open(KEY_WRITE)?
        .set_raw_value(key, &raw)
        .with_context(|| format!("cannot write {key}"))?;
    broadcast_change();
    Ok(())
}

/// Deletes the user value and, when the variable is also defined
/// machine-wide, that one too (which needs an elevated process); otherwise
/// the variable would still be there and "Done" would be wrong.
pub fn delete(key: &str) -> Result<()> {
    let user_removed = match open(KEY_WRITE)?.delete_value(key) {
        Ok(()) => {
            broadcast_change();
            true
        }
        Err(e) if e.kind() == ErrorKind::NotFound => false,
        Err(e) => return Err(e).with_context(|| format!("cannot delete {key}")),
    };
    delete_machine_wide(key, user_removed)
}

fn admin_hint(key: &str, user_removed: bool) -> String {
    if user_removed {
        format!(
            "removed the user value of {key}, but it is still defined machine-wide; \
             run as administrator to delete it there too"
        )
    } else {
        format!("{key} is defined machine-wide; run as administrator to delete it")
    }
}

/// Deletes a machine-wide value; needs an elevated (administrator) process.
fn delete_machine_wide(key: &str, user_removed: bool) -> Result<()> {
    if !defined_machine_wide(key)? {
        return Ok(());
    }
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(MACHINE_ENV, KEY_WRITE)
        .map_err(|e| match e.kind() {
            ErrorKind::PermissionDenied => anyhow::anyhow!(admin_hint(key, user_removed)),
            _ => anyhow::Error::new(e).context("cannot open the machine environment"),
        })?;
    match machine.delete_value(key) {
        Ok(()) => {
            broadcast_change();
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::PermissionDenied => bail!(admin_hint(key, user_removed)),
        Err(e) => Err(e).with_context(|| format!("cannot delete {key}")),
    }
}

pub fn init() -> Result<()> {
    println!("Nothing to do: Windows reads the registry directly.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_selects_expand_type() {
        assert_eq!(value_type("plain"), REG_SZ);
        assert_eq!(value_type("%PATH%;x"), REG_EXPAND_SZ);
    }

    #[test]
    fn admin_hint_says_what_was_removed() {
        assert!(admin_hint("A", true).contains("removed the user value of A"));
        assert!(admin_hint("A", true).contains("still defined machine-wide"));
        assert!(!admin_hint("A", false).contains("removed"));
    }

    #[cfg(windows)]
    struct Cleanup(String);

    #[cfg(windows)]
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = delete(&self.0);
        }
    }

    #[cfg(windows)]
    #[test]
    fn registry_round_trip() {
        let name = format!("EV_TEST_{}", std::process::id());
        let _cleanup = Cleanup(name.clone());

        set(&name, "plain").unwrap();
        assert_eq!(
            list().unwrap().get(&name).map(String::as_str),
            Some("plain")
        );

        set(&name, "%TEMP%\\x").unwrap();
        let raw = open(KEY_READ).unwrap().get_raw_value(&name).unwrap();
        assert_eq!(raw.vtype, REG_EXPAND_SZ);
        assert_eq!(list().unwrap()[&name], "%TEMP%\\x");

        delete(&name).unwrap();
        assert!(!list().unwrap().contains_key(&name));
        delete(&name).unwrap();
    }
}
