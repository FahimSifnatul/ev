# ev

Set and remove permanent environment variables from one command, on macOS, Linux and Windows. ev keeps your variables in its own user-level store; `ev delete` also cleans up definitions in shell startup files (including system files such as `/etc/profile`) and app settings such as Go's.

```
ev set API_KEY abc123
```

No more editing `~/.zshrc` by hand, or digging through the Windows environment dialog.

## Install

macOS / Linux:

```sh
curl -fsSL https://github.com/FahimSifnatul/ev/releases/latest/download/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://github.com/FahimSifnatul/ev/releases/latest/download/install.ps1 | iex
```

The installers put one program on your PATH, verify its checksum, and need no administrator rights.
On macOS/Linux the installer also runs `ev init` (see below); set `EV_NO_MODIFY_PATH=1` (or pass `--no-modify-path`) to skip it and any other shell startup file edit, then run `ev init` yourself. If there is no ready-made file for your machine, the
shell installer builds from source (needs [Rust](https://rustup.rs)).

## Usage

```sh
ev set EDITOR vim                  # set (alias: add)
ev get EDITOR                      # print one value
ev list                            # print everything visible to the CLI, with where each value comes from
ev list --ev                       # only the variables ev manages
ev set API_URL x -d "staging API"  # attach a description (--desc)
ev set TEMP_DIR '%LOCALAPPDATA%\tmp' --expand  # Windows: store as expandable (see below)
ev unset EDITOR                    # keep it, but make the value empty
ev delete EDITOR                   # remove it completely, including its description (alias: rm)
ev set LD_PRELOAD x --yes          # answer yes to confirmations (-y), see below
```

`unset` and `delete` are different: `unset` keeps the variable (with an empty value) and its description, while `delete` removes the variable, its description, and its definitions in shell startup files (see below). `ev unset` does not alias `delete`.

Changes apply to **new terminals**. Terminals that are already open keep their old environment.

On macOS/Linux, if a shell startup file defines the same variable after ev's hook line, `ev set` warns on stderr: that value wins in new shells. This check is best effort and only looks at the startup files for your `$SHELL`.

### Listing and sources

`ev list` merges, with later sources winning: the variables of the current process, app-managed settings (such as Go), and ev's own store. In a terminal it prints a table with `NAME`, `VALUE`, `SOURCE` and `DESCRIPTION` columns. `SOURCE` is one of:

- `env`: inherited from the parent process, not defined by ev
- `ev`: stored by ev (on Windows, `user`)
- a file such as `~/.zshrc`: a shell startup file defines it (unix)
- `go`: an app setting (`--app go`)
- `user` / `system`: defined in the Windows user or machine registry

`ev get` uses the same merged view. Piped output is still `NAME=value`, so scripts keep working.

In a terminal, `ev list --ev` also shows names with a pending `ev delete` as rows with an empty `VALUE` and `SOURCE` `ev (deleted)`. A deleted name is also `unset` in new shells, so a variable inherited from the parent process can disappear there. Piped output leaves these rows out.

### Deleting from shell startup files (macOS / Linux)

`ev delete KEY` also looks for definitions of `KEY` in your shell startup files (`~/.zshenv`, `~/.zprofile`, `~/.zshrc`, `~/.bashrc`, `~/.bash_profile`, `~/.profile`, fish's `config.fish` and `conf.d/*.fish`) and in the system files (`/etc/profile`, `/etc/profile.d/*.sh`, `/etc/zshenv`, `/etc/zprofile`, `/etc/zshrc`, `/etc/bashrc`, `/etc/bash.bashrc`, `/etc/environment`). It lists every match as `file:line`, asks you to confirm, and then comments each line out as `: # removed by ev: <original line>` (keeping the indentation). The leading `:` is a no-op command, so an `if`, `case` or function body whose only line this was stays valid shell. In fish files the line becomes a plain `# removed by ev: <original line>`. Without a terminal (scripts, CI), `ev delete KEY` stops before changing anything when `KEY` appears in these files or in an app's settings, unless you pass `--yes`.

- The original file is saved once as `<file>.ev-bak` before the first edit.
- Lines are not edited when they are not simple assignments: multiple assignments or inline prefixes (`FOO=1 cmd`, `export A=1 B=2`), appends to the variable itself (`PATH=$PATH:/x`), multi-line values, and compound lines with `;`, `&` or `|`. ev reports these; edit them by hand.
- Lines inside a heredoc body (`<<EOF ... EOF`) or inside a multi-line quoted string are text, not definitions, and are ignored.
- `/etc/environment` is not a shell script (it is read by `pam_env`), so matches there are only reported; edit that file by hand.
- ev never runs itself with elevated rights. System files (outside your home directory) that it cannot write are skipped with a message suggesting `sudo ev delete KEY --yes`.
- Under `sudo` (root running in your home directory, as on macOS where `sudo` keeps `HOME`), `ev delete` edits only files outside your home directory. It leaves ev's own store, your descriptions, your own startup files and app settings alone, so none of them become owned by root; run `ev delete KEY` without `sudo` for those.
- Even when a startup file cannot be edited, `delete` still hides the variable from new shells through ev's own file.

`ev delete` also removes the variable from any app that has it set (see below).

### Apps (`--app`)

Some tools keep their own settings. `--app <name>` on `set`, `unset`, `delete` and `get` changes that tool's settings instead of ev's store. Currently only Go is supported:

```sh
ev set GOOS linux --app go         # go env -w GOOS=linux
ev unset GOOS --app go             # go env -w GOOS=
ev delete GOOS --app go            # go env -u GOOS
ev get GOOS --app go
```

- `ev list` shows Go settings with `SOURCE` `go`: only the variables in Go's own settings file (the one `go env GOENV` names). A `GOPATH` exported in `~/.zshrc` is not a Go setting, and `ev delete GOPATH --app go` says there is nothing to do for it. If `go` is not on your PATH, its settings are left out of the list.
- `ev delete KEY` (without `--app`) shows the `go env -u KEY` it will run next to the startup file lines, and asks once for all of them.
- `--desc` cannot be combined with `--app`.
- App changes apply right away to new `go` commands; they do not need a new terminal.

## Descriptions

In a terminal, `ev list` prints a table with a `DESCRIPTION` column and `ev get` appends `# description` after the value. Values longer than 40 characters are shortened in the table.
When output is piped or redirected, nothing changes: `ev list` prints `NAME=value` lines and `ev get` prints just the value, so scripts keep working.

Well-known variables (`HOME`, `PATH`, `JAVA_HOME`, ...) have built-in descriptions; a description you set with `-d` takes precedence.
`ev set` without `-d` keeps the existing description, and `ev delete` removes it (`ev unset` keeps it).
`ev set KEY value -d ""` clears your description for `KEY`.
Your descriptions are stored in a `descriptions` file: `~/.config/ev/` on macOS/Linux (or `$XDG_CONFIG_HOME/ev`, or `$EV_HOME`), `%APPDATA%\ev` on Windows (or `%EV_HOME%` if set).

## Critical variables

Changing some variables can break your shell or system, so `ev` warns and asks for confirmation first:

- `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `TERM`, `LANG`, `TMPDIR`
- Windows: `USERPROFILE`, `SYSTEMROOT`, `WINDIR`, `COMSPEC`, `PATHEXT`, `PSMODULEPATH`, `APPDATA`, `LOCALAPPDATA`, `TEMP`, `TMP`, `PROGRAMFILES`
- anything starting with `LD_` or `DYLD_` (dynamic linker)

Pass `--yes` (`-y`) to answer yes to every confirmation: for critical variables, and for `ev delete` editing shell startup files, system files and app settings. Without a terminal (such as in scripts), ev stops instead of asking, so `--yes` is required there whenever there is something to confirm.

## Values that start with `-`

Any argument that starts with `-` is read as an option. Put `--` before it to end the options; everything after `--` is taken literally, so options go before it: `ev set KEY -- -value`, `ev set --app go GOFLAGS -- -mod=mod`.

## Windows notes

- Values are stored in `HKCU\Environment`. `ev unset` stores an empty value; many programs treat an empty variable as undefined, which is usually what "unset" means, but a few may still see it as set.
- By default a value is stored literally (`REG_SZ`), so a `%` in it stays a `%`. `--expand` stores it as an expandable string (`REG_EXPAND_SZ`), so `%NAME%` references are expanded when read. `PATH` is always expandable. `--expand` must be repeated on each overwrite: a plain `ev set` stores a literal value again. It cannot be used with `--app`, and it is rejected on macOS/Linux.
- `ev delete` removes the user value. If the variable is also defined machine-wide (in `HKLM`), ev deletes that too when run from an elevated (administrator) prompt; otherwise it stops with an error saying the variable is still defined machine-wide and needs "run as administrator".
- Only the user store is written. Machine-wide values are read (`SOURCE` `system`) but not changed except by `delete` when elevated.
- Shell startup files are not scanned on Windows.

## Where the data lives

- **Windows:** the variables live in `HKCU\Environment` (the same place as the system's "Environment Variables" dialog), but your descriptions live in the `descriptions` file in `%APPDATA%\ev` (or `%EV_HOME%`), not in the registry.
- **macOS / Linux:** `~/.config/ev/` (or `$XDG_CONFIG_HOME/ev`, or `$EV_HOME`) holds the variables and generated
  `env.sh` / `env.fish` files and the `descriptions` file. `ev init` adds one line that loads them to these startup files, depending on your shell:
  - zsh: `${ZDOTDIR:-$HOME}/.zshenv`, so scripts and `zsh -c` see the variables too
  - bash on Linux: `~/.bashrc` and `~/.profile`
  - bash on macOS: `~/.bash_profile` (or `~/.profile` if only that one exists)
  - fish: `conf.d/ev.fish`
  - other shells: `~/.profile`

  A hook line that an earlier version put in `.zshrc` is left alone. Loading twice is harmless.
  Concurrent changes are serialized with a lock file in that config directory.

Limits (macOS / Linux):

- Apps started from the Dock, Finder or a desktop launcher do not see ev's variables; only shells do. (Windows apps do see them.)
- Multi-line values are not supported: newlines and NUL characters are rejected.

## Update and uninstall

Update by running the install command again. To remove ev:

```sh
curl -fsSL https://github.com/FahimSifnatul/ev/releases/latest/download/install.sh | sh -s -- --uninstall
```

```powershell
$env:EV_UNINSTALL = '1'; irm https://github.com/FahimSifnatul/ev/releases/latest/download/install.ps1 | iex
```

Your variables are left in place.

## Build from source

Needs Rust 1.89 or newer (`rust-version` in `Cargo.toml`).

```sh
cargo build --release   # binary at target/release/ev
cargo test
```

## License

MIT
