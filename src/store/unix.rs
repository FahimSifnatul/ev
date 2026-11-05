use anyhow::{bail, Result};
use std::collections::BTreeMap;

pub fn list() -> Result<BTreeMap<String, String>> {
    bail!("not implemented")
}

pub fn set(_key: &str, _value: &str) -> Result<()> {
    bail!("not implemented")
}

pub fn unset(_key: &str) -> Result<()> {
    bail!("not implemented")
}

pub fn init() -> Result<()> {
    bail!("not implemented")
}
