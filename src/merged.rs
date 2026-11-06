//! Every variable the user can see, each with the place it comes from.
//!
//! Layers are applied from weakest to strongest; a later layer replaces the
//! value and the source of an earlier one: process environment, app
//! providers (labelled with the app's name), then ev's own store.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;

use crate::{apps, store};

/// Source label of variables that no other layer claims.
const PROCESS: &str = "env";
/// Source label of ev's own store: the user registry on Windows.
const EV_STORE: &str = if cfg!(windows) { "user" } else { "ev" };

pub struct Row {
    pub name: String,
    pub value: String,
    pub source: String,
    /// A pending `unset` marker: the name is listed, but has no value.
    pub deleted: bool,
}

/// Rows keyed by folded name, so Windows treats `Path` and `PATH` as one.
#[derive(Default)]
pub struct Merged(BTreeMap<String, Row>);

fn fold(name: &str) -> String {
    if cfg!(windows) {
        name.to_uppercase()
    } else {
        name.to_string()
    }
}

impl Merged {
    /// Add or replace variables; they now come from `source`.
    pub fn overlay(&mut self, vars: impl IntoIterator<Item = (String, String)>, source: &str) {
        for (name, value) in vars {
            let row = Row {
                name: name.clone(),
                value,
                source: source.to_string(),
                deleted: false,
            };
            self.0.insert(fold(&name), row);
        }
    }

    /// Rename the source of process variables that `labels` knows about.
    fn relabel(&mut self, labels: &BTreeMap<String, String>) {
        let folded: BTreeMap<String, &String> =
            labels.iter().map(|(k, v)| (fold(k), v)).collect();
        for (key, row) in &mut self.0 {
            if row.source == PROCESS {
                if let Some(label) = folded.get(key) {
                    row.source = (*label).clone();
                }
            }
        }
    }

    fn hide(&mut self, names: &BTreeSet<String>) {
        for name in names {
            self.0.remove(&fold(name));
        }
    }

    /// List `names` as deleted markers: empty value, no live variable.
    fn mark_deleted(&mut self, names: &BTreeSet<String>) {
        for name in names {
            let row = Row {
                name: name.clone(),
                value: String::new(),
                source: format!("{EV_STORE} (deleted)"),
                deleted: true,
            };
            self.0.entry(fold(name)).or_insert(row);
        }
    }

    pub fn get(&self, name: &str) -> Option<&Row> {
        self.0.get(&fold(name))
    }

    pub fn into_rows(self) -> Vec<Row> {
        self.0.into_values().collect()
    }
}

/// Layers over the process environment: `labels` only name where a process
/// variable was defined, `apps` replace values and take the app's name as
/// source, `ev` replaces values, `hidden` names are dropped.
fn merge(
    process: impl IntoIterator<Item = (String, String)>,
    labels: &BTreeMap<String, String>,
    apps: Vec<(&str, BTreeMap<String, String>)>,
    ev: BTreeMap<String, String>,
    hidden: &BTreeSet<String>,
) -> Merged {
    let mut merged = Merged::default();
    merged.overlay(process, PROCESS);
    merged.relabel(labels);
    for (name, vars) in apps {
        merged.overlay(vars, name);
    }
    let ev = ev
        .into_iter()
        .filter(|(name, _)| !keeps_process_value(name, &merged, cfg!(windows)))
        .collect::<Vec<_>>();
    merged.overlay(ev, EV_STORE);
    merged.hide(hidden);
    merged
}

/// On Windows the process PATH is the expanded user+system value, while the
/// store only holds the user part (unexpanded), so the process value wins.
fn keeps_process_value(name: &str, merged: &Merged, windows: bool) -> bool {
    windows && name.eq_ignore_ascii_case("PATH") && merged.get("PATH").is_some()
}

/// Only what ev itself manages (`ev list --ev`).
pub fn ev_only() -> Result<Merged> {
    Ok(ev_layer(store::list()?, &store::deleted()?))
}

/// ev's values plus its pending-delete markers (rows with `deleted` set).
fn ev_layer(vars: BTreeMap<String, String>, deleted: &BTreeSet<String>) -> Merged {
    let mut merged = Merged::default();
    merged.overlay(vars, EV_STORE);
    merged.mark_deleted(deleted);
    merged
}

/// The process environment, labelled and overridden by ev's store.
pub fn collect() -> Result<Merged> {
    let process = std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)));
    Ok(merge(process, &labels(), apps::listed(), store::list()?, &store::deleted()?))
}

/// Where the system says a variable is defined: startup files on unix,
/// the machine registry on Windows.
#[cfg(unix)]
fn labels() -> BTreeMap<String, String> {
    let home = std::env::var("HOME").ok();
    crate::sources::scan_all()
        .into_iter()
        .map(|(var, file)| (var, tilde(&file, home.as_deref())))
        .collect()
}

#[cfg(windows)]
fn labels() -> BTreeMap<String, String> {
    // best effort: without machine access the variables just stay `env`
    store::machine_names()
        .unwrap_or_default()
        .into_iter()
        .map(|name| (name, "system".to_string()))
        .collect()
}

/// `/home/me/.zshrc` becomes `~/.zshrc`.
#[cfg(unix)]
fn tilde(path: &str, home: Option<&str>) -> String {
    match home.filter(|h| !h.is_empty()).map(|h| h.trim_end_matches('/')) {
        Some(home) => match path.strip_prefix(home) {
            Some(rest) if rest.starts_with('/') => format!("~{rest}"),
            _ => path.to_string(),
        },
        None => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn map(items: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs(items).into_iter().collect()
    }

    fn rows(merged: Merged) -> Vec<(String, String, String)> {
        merged
            .into_rows()
            .into_iter()
            .map(|r| (r.name, r.value, r.source))
            .collect()
    }

    fn row(name: &str, value: &str, source: &str) -> (String, String, String) {
        (name.into(), value.into(), source.into())
    }

    #[test]
    fn process_vars_are_labelled_env() {
        let merged = merge(pairs(&[("A", "1")]), &BTreeMap::new(), Vec::new(), BTreeMap::new(), &BTreeSet::new());
        assert_eq!(rows(merged), [row("A", "1", "env")]);
    }

    #[test]
    fn labels_rename_the_source_but_keep_the_value() {
        let merged = merge(
            pairs(&[("A", "1"), ("B", "2")]),
            &map(&[("A", "~/.zshrc"), ("C", "~/.profile")]),
            Vec::new(),
            BTreeMap::new(),
            &BTreeSet::new(),
        );
        assert_eq!(rows(merged), [row("A", "1", "~/.zshrc"), row("B", "2", "env")]);
    }

    #[test]
    fn ev_store_wins_over_process_and_labels() {
        let merged = merge(
            pairs(&[("A", "old")]),
            &map(&[("A", "~/.zshrc")]),
            Vec::new(),
            map(&[("A", "new"), ("Z", "9")]),
            &BTreeSet::new(),
        );
        assert_eq!(rows(merged), [row("A", "new", EV_STORE), row("Z", "9", EV_STORE)]);
    }

    #[test]
    fn apps_sit_between_process_and_ev_store() {
        let merged = merge(
            pairs(&[("A", "proc"), ("B", "proc"), ("C", "proc")]),
            &map(&[("A", "~/.zshrc")]),
            vec![("go", map(&[("A", "go"), ("B", "go"), ("D", "go")]))],
            map(&[("B", "ev")]),
            &BTreeSet::new(),
        );
        assert_eq!(
            rows(merged),
            [
                row("A", "go", "go"),
                row("B", "ev", EV_STORE),
                row("C", "proc", "env"),
                row("D", "go", "go"),
            ]
        );
    }

    #[test]
    fn hidden_names_are_dropped() {
        let hidden: BTreeSet<String> = ["A".to_string()].into();
        let merged = merge(pairs(&[("A", "1"), ("B", "2")]), &BTreeMap::new(), Vec::new(), BTreeMap::new(), &hidden);
        assert!(merged.get("A").is_none());
        assert_eq!(merged.get("B").map(|r| r.value.as_str()), Some("2"));
    }

    #[test]
    fn windows_keeps_process_path_over_user_store_path() {
        let mut merged = Merged::default();
        merged.overlay(pairs(&[("PATH", "/full:/expanded")]), PROCESS);
        assert!(keeps_process_value("Path", &merged, true));
        assert!(!keeps_process_value("Path", &merged, false));
        assert!(!keeps_process_value("FOO", &merged, true));
        // no process PATH: the store value is all there is
        assert!(!keeps_process_value("Path", &Merged::default(), true));
    }

    #[test]
    fn ev_layer_lists_deleted_markers_with_empty_value() {
        let deleted: BTreeSet<String> = ["GONE".to_string(), "A".to_string()].into();
        let merged = ev_layer(map(&[("A", "1")]), &deleted);
        let got: Vec<_> = merged
            .into_rows()
            .into_iter()
            .map(|r| (r.name, r.value, r.source, r.deleted))
            .collect();
        assert_eq!(
            got,
            [
                ("A".to_string(), "1".to_string(), EV_STORE.to_string(), false),
                ("GONE".to_string(), String::new(), format!("{EV_STORE} (deleted)"), true),
            ]
        );
    }

    #[test]
    fn later_overlay_replaces_earlier() {
        let mut merged = Merged::default();
        merged.overlay(pairs(&[("A", "1")]), "env");
        merged.overlay(pairs(&[("A", "2")]), "some-app");
        assert_eq!(rows(merged), [row("A", "2", "some-app")]);
    }

    #[cfg(unix)]
    #[test]
    fn tilde_shortens_paths_under_home() {
        assert_eq!(tilde("/home/me/.zshrc", Some("/home/me")), "~/.zshrc");
        assert_eq!(tilde("/home/me/.zshrc", Some("/home/me/")), "~/.zshrc");
        assert_eq!(tilde("/home/meow/.zshrc", Some("/home/me")), "/home/meow/.zshrc");
        assert_eq!(tilde("/etc/profile", Some("/home/me")), "/etc/profile");
        assert_eq!(tilde("/etc/profile", None), "/etc/profile");
        assert_eq!(tilde("/etc/profile", Some("")), "/etc/profile");
    }
}
