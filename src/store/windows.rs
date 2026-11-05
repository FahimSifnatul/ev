use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
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

/// Reads a string value; the registry matches names case-insensitively.
fn read_string(key: &RegKey, name: &str) -> Result<Option<String>> {
    match key.get_raw_value(name) {
        Ok(raw) if matches!(raw.vtype, REG_SZ | REG_EXPAND_SZ) => {
            Ok(Some(String::from_reg_value(&raw)?))
        }
        Ok(_) => Ok(None),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read {name}")),
    }
}

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

pub fn get(key: &str) -> Result<Option<String>> {
    read_string(&open(KEY_READ)?, key)
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

pub fn unset(key: &str) -> Result<()> {
    match open(KEY_WRITE)?.delete_value(key) {
        Ok(()) => {
            broadcast_change();
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            if defined_machine_wide(key)? {
                bail!("{key} is defined machine-wide; removing it needs an administrator and ev does not edit it.");
            }
            Ok(())
        }
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

    #[cfg(windows)]
    struct Cleanup(String);

    #[cfg(windows)]
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = unset(&self.0);
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

        unset(&name).unwrap();
        assert!(!list().unwrap().contains_key(&name));
        unset(&name).unwrap();
    }
}
