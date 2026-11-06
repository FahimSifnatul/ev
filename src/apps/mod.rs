//! Tools that keep their own settings (like `go env -w`), managed through
//! `--app <name>` and shown in `ev list` under the tool's name.

mod go;

use std::collections::BTreeMap;

use anyhow::{bail, Result};

pub trait App {
    fn name(&self) -> &'static str;
    /// Whether the tool is installed, so it can be asked anything at all.
    fn available(&self) -> bool;
    /// The variables the tool has set itself.
    fn list(&self) -> Result<BTreeMap<String, String>>;
    /// One variable's effective value, defaults included.
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
    /// Keep the variable but make its value empty.
    fn unset(&self, key: &str) -> Result<()>;
    /// Forget the variable so the tool falls back to its default.
    fn delete(&self, key: &str) -> Result<()>;
}

pub fn all() -> Vec<Box<dyn App>> {
    vec![Box::new(go::Go)]
}

/// The app called `name`, or an error saying why it cannot be used.
pub fn find(name: &str) -> Result<Box<dyn App>> {
    let apps = all();
    let known: Vec<&str> = apps.iter().map(|a| a.name()).collect();
    match apps.into_iter().find(|a| a.name() == name) {
        Some(app) if app.available() => Ok(app),
        Some(app) => bail!("app '{}' is not available (not found on PATH)", app.name()),
        None => bail!("unknown app '{name}' (known apps: {})", known.join(", ")),
    }
}

/// Every available app with the variables it currently has set. An app that
/// cannot answer only warns, so one broken tool never hides the rest.
pub fn listed() -> Vec<(&'static str, BTreeMap<String, String>)> {
    all()
        .into_iter()
        .filter(|app| app.available())
        .filter_map(|app| match app.list() {
            Ok(vars) => Some((app.name(), vars)),
            Err(err) => {
                eprintln!("ev: warning: {}: {err:#}", app.name());
                None
            }
        })
        .collect()
}

/// Is `program` an executable file in one of the PATH directories?
fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let file = if cfg!(windows) {
        format!("{program}.exe")
    } else {
        program.to_string()
    };
    std::env::split_paths(&path).any(|dir| dir.join(&file).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_app_is_an_error_naming_the_known_ones() {
        let err = find("nope").err().unwrap().to_string();
        assert!(err.contains("unknown app 'nope'"), "{err}");
        assert!(err.contains("go"), "{err}");
    }

    #[test]
    fn registry_has_go() {
        assert!(all().iter().any(|a| a.name() == "go"));
    }
}
