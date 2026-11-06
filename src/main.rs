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
#[command(name = "ev", version, about = "Manage permanent user-level environment variables")]
struct Cli {
    /// Skip the confirmation prompt for critical variables
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
                app.delete(&key)?;
                return app_done(app.name());
            }
            guard(&key, cli.yes)?;
            #[cfg(unix)]
            let rc_hits = confirm_rc_edits(&key, cli.yes)?;
            let holders = app_holders(&key);
            store::delete(&key)?;
            #[cfg(unix)]
            comment_out_rc_lines(&rc_hits);
            for app in holders {
                match app.delete(&key) {
                    Ok(()) => println!("Removed {key} from {}", app.name()),
                    Err(err) => eprintln!("ev: warning: {}: {err:#}", app.name()),
                }
            }
            if let Err(err) = notes::remove(&key) {
                eprintln!("ev: warning: {err:#}");
            }
            done();
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
        for row in vars {
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

/// Show where shell startup files also define `key` and ask before editing them.
#[cfg(unix)]
fn confirm_rc_edits(key: &str, yes: bool) -> Result<Vec<sources::Hit>> {
    let hits = sources::scan(key);
    if hits.is_empty() {
        return Ok(hits);
    }
    eprintln!("{key} is also defined in your shell startup files:");
    for hit in &hits {
        eprintln!("  {}:{}: {}", hit.file.display(), hit.line_no, hit.text);
    }
    if hits.iter().any(|h| h.editable) {
        eprintln!("ev will comment these lines out (originals are kept in <file>.ev-bak).");
        confirm(yes)?;
    }
    Ok(hits)
}

/// The `-KEY` marker already hides the variable, so problems here only warn.
#[cfg(unix)]
fn comment_out_rc_lines(hits: &[sources::Hit]) {
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
            "ev: left {}:{} as is (it appends to itself or spans several lines); edit it by hand",
            hit.file.display(),
            hit.line_no
        );
    }
    for skipped in &report.skipped {
        eprintln!(
            "ev: could not edit {}: {}. For system files run 'sudo ev delete <KEY>'.",
            skipped.file.display(),
            skipped.reason
        );
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
