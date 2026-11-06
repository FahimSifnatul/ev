mod danger;
mod describe;
mod notes;
mod output;
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
    },
    /// Keep a variable but make its value empty
    Unset { key: String },
    /// Remove a variable completely
    #[command(alias = "rm")]
    Delete { key: String },
    /// Print a variable's value
    Get { key: String },
    /// List all variables
    List,
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
        Command::Set { key, value, desc } => {
            store::validate_key(&key)?;
            store::validate_value(&value)?;
            if let Some(text) = &desc {
                notes::validate_text(text)?;
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
        Command::Unset { key } => {
            store::validate_key(&key)?;
            guard(&key, cli.yes)?;
            store::set(&key, "")?;
            done();
        }
        Command::Delete { key } => {
            store::validate_key(&key)?;
            guard(&key, cli.yes)?;
            store::delete(&key)?;
            if let Err(err) = notes::remove(&key) {
                eprintln!("ev: warning: {err:#}");
            }
            done();
        }
        Command::Get { key } => match store::get(&key)? {
            Some(value) => print_value(&key, &value),
            None => std::process::exit(1),
        },
        Command::List => print_list(store::list()?),
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

fn print_list(vars: BTreeMap<String, String>) {
    if !io::stdout().is_terminal() {
        for (key, value) in vars {
            println!("{key}={value}");
        }
        return;
    }
    let notes = load_notes();
    let rows: Vec<_> = vars
        .into_iter()
        .map(|(key, value)| {
            let desc = describe::description(&key, &notes).unwrap_or_default();
            (key, value, desc)
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

fn guard(key: &str, yes: bool) -> Result<()> {
    match danger::reason(key) {
        Some(reason) => {
            eprintln!("\x1b[33mwarning: {key} {reason}\x1b[0m");
            confirm(yes)
        }
        None => Ok(()),
    }
}

fn confirm(yes: bool) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !io::stdin().is_terminal() {
        bail!("refusing to change a critical variable without --yes (stdin is not a terminal)");
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
