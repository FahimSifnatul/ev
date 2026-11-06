mod apps;
mod danger;
mod describe;
mod merged;
mod notes;
mod output;
#[cfg(unix)]
mod sources;
mod store;

use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "ev",
    version,
    about = "Manage permanent environment variables: ev's own user store, app settings, and their definitions in shell startup files"
)]
struct Cli {
    /// Answer yes to every confirmation: changing critical variables, and
    /// letting `delete` edit shell startup and system files and app settings.
    /// Needed when stdin is not a terminal and there is something to confirm
    #[arg(short, long, global = true)]
    yes: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Set a variable permanently
    #[command(alias = "add")]
    Set {
        key: String,
        value: String,
        /// Describe the variable (shown by list and get in a terminal)
        #[arg(short, long)]
        desc: Option<String>,
        /// Change the setting of an app (like go) instead of ev's own store
        #[arg(long)]
        app: Option<String>,
    },
    /// Keep a variable but make its value empty
    Unset {
        key: String,
        /// Change the setting of an app (like go) instead of ev's own store
        #[arg(long)]
        app: Option<String>,
    },
    /// Remove a variable completely
    #[command(alias = "rm")]
    Delete {
        key: String,
        /// Change the setting of an app (like go) instead of ev's own store
        #[arg(long)]
        app: Option<String>,
    },
    /// Print a variable's value
    Get {
        key: String,
        /// Read the setting of an app (like go) instead of the merged environment
        #[arg(long)]
        app: Option<String>,
    },
    /// List all variables
    List {
        /// Only the variables ev manages
        #[arg(long)]
        ev: bool,
    },
    /// Prepare the shell to load ev's variables
    Init,
}

fn main() {
    if let Err(err) = run(Cli::parse()) {
        eprintln!("ev: {err:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Set { key, value, desc, app } => {
            store::validate_key(&key)?;
            store::validate_value(&value)?;
            if let Some(text) = &desc {
                notes::validate_text(text)?;
            }
            if let Some(name) = &app {
                if desc.is_some() {
                    bail!("--desc cannot be used with --app");
                }
                let app = apps::find(name)?;
                guard(&key, cli.yes)?;
                app.set(&key, &value)?;
                return app_done(app.name());
            }
            guard(&key, cli.yes)?;
            store::set(&key, &value)?;
            if let Some(text) = &desc {
                // a blank description clears ours
                let saved = if text.trim().is_empty() {
                    notes::remove(&key)
                } else {
                    notes::save(&key, text)
                };
                saved.context("variable saved, but saving its description failed")?;
            }
            done();
        }
        Command::Unset { key, app } => {
            store::validate_key(&key)?;
            if let Some(name) = &app {
                let app = apps::find(name)?;
                guard(&key, cli.yes)?;
                app.unset(&key)?;
                return app_done(app.name());
            }
            guard(&key, cli.yes)?;
            store::set(&key, "")?;
            done();
        }
        Command::Delete { key, app } => {
            store::validate_key(&key)?;
            if let Some(name) = &app {
                let app = apps::find(name)?;
                guard(&key, cli.yes)?;
                if !app.list()?.contains_key(&key) {
                    println!(
                        "Nothing to do: {key} is not set in {0}'s own settings (it may come from \
                         your environment or shell startup files; see 'ev delete {key}').",
                        app.name()
                    );
                    return Ok(());
                }
                app.delete(&key)?;
                return app_done(app.name());
            }
            guard(&key, cli.yes)?;
            delete_everywhere(&key, cli.yes)?;
        }
        Command::Get { key, app: Some(name) } => match apps::find(&name)?.get(&key)? {
            Some(value) => print_value(&key, &value),
            None => std::process::exit(1),
        },
        Command::Get { key, app: None } => match merged::collect()?.get(&key) {
            Some(row) => print_value(&row.name, &row.value),
            None => std::process::exit(1),
        },
        Command::List { ev } => {
            let vars = if ev { merged::ev_only()? } else { merged::collect()? };
            print_list(vars);
        }
        Command::Init => store::init()?,
    }
    Ok(())
}

fn print_value(key: &str, value: &str) {
    if !io::stdout().is_terminal() {
        println!("{value}");
        return;
    }
    match describe::description(key, &load_notes()) {
        Some(desc) => println!("{value}    # {desc}"),
        None => println!("{value}"),
    }
}

fn print_list(vars: merged::Merged) {
    let vars = vars.into_rows();
    if !io::stdout().is_terminal() {
        // a deleted marker has no value; `NAME=` would look like a set variable
        for row in vars.into_iter().filter(|row| !row.deleted) {
            println!("{}={}", row.name, row.value);
        }
        return;
    }
    let notes = load_notes();
    let rows: Vec<_> = vars
        .into_iter()
        .map(|row| {
            let desc = describe::description(&row.name, &notes).unwrap_or_default();
            (row.name, row.value, row.source, desc)
        })
        .collect();
    print!("{}", output::table(&rows));
}

fn load_notes() -> BTreeMap<String, String> {
    notes::load().unwrap_or_else(|err| {
        eprintln!("ev: warning: {err:#}; descriptions not shown");
        BTreeMap::new()
    })
}

fn done() {
    println!("Done. Open a new terminal to see the change.");
}

fn app_done(app: &str) -> Result<()> {
    println!("Done. {app} uses the change right away.");
    Ok(())
}

/// The available apps that currently have `key` set.
fn app_holders(key: &str) -> Vec<Box<dyn apps::App>> {
    let having: Vec<&str> = apps::listed()
        .into_iter()
        .filter(|(_, vars)| vars.contains_key(key))
        .map(|(name, _)| name)
        .collect();
    apps::all().into_iter().filter(|a| having.contains(&a.name())).collect()
}

fn guard(key: &str, yes: bool) -> Result<()> {
    match danger::reason(key) {
        Some(reason) => {
            eprintln!("\x1b[33mwarning: {key} {reason}\x1b[0m");
            confirm(yes)
        }
        None => Ok(()),
    }
}

/// `ev delete KEY` without `--app`: ev's store, shell startup files, apps
/// that hold the variable, and its description. Under `sudo` (root in the
/// user's home) only files outside that home are touched.
fn delete_everywhere(key: &str, yes: bool) -> Result<()> {
    #[cfg(unix)]
    let as_root = sources::running_as_root();
    #[cfg(not(unix))]
    let as_root = false;
    #[cfg(unix)]
    let rc_hits = sources::scan(key);
    let holders = if as_root { Vec::new() } else { app_holders(key) };

    #[cfg(unix)]
    let rc_editable = show_rc_hits(key, &rc_hits);
    #[cfg(not(unix))]
    let rc_editable = false;
    for app in &holders {
        eprintln!("{key} is also set in {0}'s settings:\n  {0}: {1}", app.name(), app.delete_command(key));
    }
    if rc_editable || !holders.is_empty() {
        eprintln!("ev will make these changes.");
        // one question for all of them, before anything changes
        confirm(yes)?;
    }

    if !as_root {
        store::delete(key)?;
    }
    #[cfg(unix)]
    comment_out_rc_lines(key, &rc_hits);
    for app in holders {
        match app.delete(key) {
            Ok(()) => println!("Removed {key} from {}", app.name()),
            Err(err) => eprintln!("ev: warning: {}: {err:#}", app.name()),
        }
    }
    if as_root {
        eprintln!(
            "ev: running as root: only files outside your home directory were changed. \
             Run 'ev delete {key}' without sudo for ev's own store, your startup files and apps."
        );
        done();
        return Ok(());
    }
    if let Err(err) = notes::remove(key) {
        eprintln!("ev: warning: {err:#}");
    }
    done();
    Ok(())
}

/// List where shell startup files also define `key`; true when ev would edit one.
#[cfg(unix)]
fn show_rc_hits(key: &str, hits: &[sources::Hit]) -> bool {
    if hits.is_empty() {
        return false;
    }
    eprintln!("{key} is also defined in shell startup files:");
    for hit in hits {
        eprintln!("  {}:{}: {}", hit.file.display(), hit.line_no, hit.text);
    }
    let editable = hits.iter().any(|h| h.editable);
    if editable {
        eprintln!("ev will comment the simple ones out (originals are kept in <file>.ev-bak).");
    }
    editable
}

/// ev's own delete marker already hides the variable, so problems here only warn.
#[cfg(unix)]
fn comment_out_rc_lines(key: &str, hits: &[sources::Hit]) {
    if hits.is_empty() {
        return;
    }
    let report = match sources::comment_out(hits, true) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("ev: warning: {err:#}");
            return;
        }
    };
    for hit in &report.commented {
        println!("Commented out {}:{}", hit.file.display(), hit.line_no);
    }
    for hit in &report.left {
        eprintln!(
            "ev: left {}:{} as is ({}); edit it by hand",
            hit.file.display(),
            hit.line_no,
            hit.why
        );
    }
    for skipped in &report.skipped {
        let hint = if sources::is_system_file(&skipped.file) && !sources::euid_is_root() {
            format!(" To edit it, run 'sudo ev delete {key} --yes'.")
        } else {
            String::new()
        };
        eprintln!("ev: could not edit {}: {}.{hint}", skipped.file.display(), skipped.reason);
    }
}

fn confirm(yes: bool) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !io::stdin().is_terminal() {
        bail!("refusing to continue without --yes (stdin is not a terminal)");
    }
    eprint!("Continue? [y/N] ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        Ok(())
    } else {
        bail!("aborted")
    }
}
