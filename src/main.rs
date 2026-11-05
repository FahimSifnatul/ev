mod danger;
mod store;

use std::io::{self, IsTerminal, Write};

use anyhow::{bail, Result};
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
    Set { key: String, value: String },
    /// Remove a variable
    #[command(aliases = ["delete", "rm"])]
    Unset { key: String },
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
        Command::Set { key, value } => {
            store::validate_key(&key)?;
            store::validate_value(&value)?;
            guard(&key, cli.yes)?;
            store::set(&key, &value)?;
            done();
        }
        Command::Unset { key } => {
            store::validate_key(&key)?;
            guard(&key, cli.yes)?;
            store::unset(&key)?;
            done();
        }
        Command::Get { key } => match store::get(&key)? {
            Some(value) => println!("{value}"),
            None => std::process::exit(1),
        },
        Command::List => {
            for (key, value) in store::list()? {
                println!("{key}={value}");
            }
        }
        Command::Init => store::init()?,
    }
    Ok(())
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
