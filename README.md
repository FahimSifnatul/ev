# ev

Set permanent, user-level environment variables from one command, on macOS, Linux and Windows.

```
ev set API_KEY abc123
```

No more editing `~/.zshrc`, or digging through the Windows environment dialog.

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
ev set EDITOR vim          # set (alias: add)
ev get EDITOR              # print one value
ev list                    # print everything ev manages, as NAME=value
ev unset EDITOR            # remove (aliases: delete, rm)
ev set LD_PRELOAD x --yes  # skip the confirmation for critical variables (-y)
```

Changes apply to **new terminals**. Terminals that are already open keep their old environment.

## Critical variables

Changing some variables can break your shell or system, so `ev` warns and asks for confirmation first:

- `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `TERM`, `LANG`, `TMPDIR`
- Windows: `USERPROFILE`, `SYSTEMROOT`, `WINDIR`, `COMSPEC`, `PATHEXT`, `PSMODULEPATH`, `APPDATA`, `LOCALAPPDATA`, `TEMP`, `TMP`, `PROGRAMFILES`
- anything starting with `LD_` or `DYLD_` (dynamic linker)

Pass `--yes` to skip the prompt (required when stdin is not a terminal, such as in scripts).

## Where the data lives

- **Windows:** `HKCU\Environment` (the same place as the system's "Environment Variables" dialog).
- **macOS / Linux:** `~/.config/ev/` (or `$XDG_CONFIG_HOME/ev`, or `$EV_HOME`) holds the variables and generated
  `env.sh` / `env.fish` files. `ev init` adds one line to your shell startup file (`.zshrc`, `.bashrc`,
  `.bash_profile`/`.profile`, or fish's `conf.d/ev.fish`) that loads them.

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

```sh
cargo build --release   # binary at target/release/ev
cargo test
```

## License

MIT
