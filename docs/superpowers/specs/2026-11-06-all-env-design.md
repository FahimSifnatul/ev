# ev: manage every env variable (OS, shell, app) + split unset/delete

## Context

User expectation: `ev list` shows every variable visible from the CLI (OS + shell + ev), and ev can set/unset/delete any of them permanently. Before this change:

- `ev list` read only ev's own store (`~/.config/ev/vars` via `store::list`; `HKCU\Environment` on Windows). Fresh install = empty output. Variables from `.zshrc`, `/etc/profile`, the parent process were invisible.
- `unset` / `delete` / `rm` were one command. Unix wrote a `-KEY` marker rendered as `unset KEY` in `env.sh`; Windows deleted the registry value.
- No app-level vars (e.g. Go's `GOOS`, `GOARCH` stored by `go env -w`).

Decisions from user:
- `list` = everything, with SOURCE column.
- `unset KEY` = keep variable, value becomes empty.
- `delete KEY` = remove completely, **including editing source files (user rc files and system files)**.
- App-level = tool-managed env stores, Go as the example.

## Design

### 1. unset vs delete (`src/main.rs`, `src/store/{unix,windows}.rs`)
- New `Delete { key }` subcommand, alias `rm`. `delete`/`rm` are no longer aliases of `Unset`.
- `unset`: `store::set(key, "")` (unix: `export KEY=''`; Windows: empty REG_SZ). Description kept. Many programs treat an empty variable as undefined; the README says so.
- `delete`: existing marker path (`store::delete`, unix `-KEY` marker rendered as `unset KEY`) + source-file edits (section 3) + `notes::remove`. Windows: delete the HKCU value; if the variable is defined in HKLM, delete it when elevated, else fail with "run as administrator".
- Both go through the existing `guard()` for critical vars.

### 2. list/get see everything (`src/main.rs`, `src/output.rs`, `src/merged.rs`, `src/sources.rs`)
- Merge, later wins: process env (`std::env::vars_os`, skip non-UTF-8) < app providers < ev store. Pending ev changes (not yet in this shell) show the store value.
- SOURCE per row: `ev` on unix (`user` on Windows) for the ev store; a file path (shortened with `~`) where an `export KEY=` / `KEY=` / `set -gx KEY` line was found (first hit from the section 3 scanner, unix only); the app name (`go`) for app settings; `system` for HKLM and `user` for HKCU on Windows; otherwise `env` (inherited).
- `--ev` flag on `list`: only ev-managed (previous behavior).
- Terminal table gains a SOURCE column (`output::table`). Piped output stays `NAME=value` (scripts unchanged).
- `get`: same merge order; exit 1 only if the variable is found nowhere.
- Variables with a pending delete marker in the ev store are hidden from list even if the current process still has them.

### 3. Source-file scanner/editor (new `src/sources.rs`, unix only)
- Files: user `~/.zshenv ~/.zprofile ~/.zshrc ~/.bashrc ~/.bash_profile ~/.profile ~/.config/fish/config.fish ~/.config/fish/conf.d/*.fish` (skipping ev's own hook line and ev's config dir files); system `/etc/profile /etc/profile.d/*.sh /etc/zshenv /etc/zprofile /etc/zshrc /etc/bashrc /etc/bash.bashrc /etc/environment`.
- Match, anchored with optional leading whitespace: `export KEY=`, `KEY=`, `typeset -x KEY=`, `declare -x KEY=`, fish `set -gx KEY` / `set -Ux KEY` / `set -x KEY`. A line whose value appends to or references the variable itself (`KEY=$KEY:...`, `KEY="$KEY..."`, `${KEY}`), or whose value continues onto the next line (trailing backslash, unbalanced quote), or that holds several assignments or inline prefixes (`FOO=1 cmd`, `export A=1 B=2`), or that is a compound line with `;`, `&` or `|`, is `editable: false`: reported, never edited. `PATH=$PATH:...` style appends are therefore not deleted automatically.
- `comment_out`: for each editable hit, write `<file>.ev-bak` once (a copy of the original, only if no backup exists yet), then replace the line with `# removed by ev: <original line>`. Indented lines become `<indent>: # removed by ev: <trimmed line>` so `if`/`case`/function bodies stay valid shell. Work on bytes (non-UTF-8 safe), preserve everything else and the trailing newline. Symlinks are resolved so a file is edited once and the link is kept. Atomic write, preserving the file mode. Unwritable file: skipped and reported, not an error.
- Wired into `delete` (unix only): after the guard, scan; if there are hits, print each `file:line: text`; if any is editable, confirm `[y/N]` unless `--yes`; then `comment_out`; print non-editable lines and skipped files with advice (`sudo ev delete <KEY>` for system files). ev never elevates itself. The `-KEY` marker is still written as a fallback, so the variable is hidden in new shells even when a source file cannot be edited.

### 4. App providers (new `src/apps/mod.rs`, `src/apps/go.rs`)
- Small trait: `name()`, `available()`, `list() -> BTreeMap`, `get`, `set(k,v)`, `unset(k)`, `delete(k)`.
- Go: `list` = `go env -json -changed` (needs Go 1.21+; defaults are not listed); `get` = the `-changed` list, falling back to `go env KEY` for defaults; `set` = `go env -w K=V`; `unset` = `go env -w K=`; `delete` = `go env -u K`. `available()` = `go` found on PATH. In the merged list, an app that is unavailable or fails is skipped with a warning; `--app go` on an unavailable app is an error.
- CLI: `--app <name>` on `set/unset/delete/get`. Without `--app`, `set` goes to the ev store as before. `--desc` cannot be combined with `--app`. `delete KEY` without `--app` also removes `KEY` from every available app that currently has it, reporting `Removed KEY from <app>` (after the same confirm).
- Go only for now; the trait makes adding others (npm config, etc.) a one-file change. Not building more providers.

### Non-goals
- Editing files outside the listed rc/system set; launchd/GUI app env; auto sudo; deleting PATH-append lines.
