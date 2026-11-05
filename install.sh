#!/bin/sh
# ev installer.
#
# Puts ONE self-contained program called `ev` on your PATH. No sudo, nothing else is touched,
# and once it is installed you can delete everything you downloaded.
#
#   curl -fsSL https://github.com/FahimSifnatul/ev/releases/latest/download/install.sh | sh
#   ./install.sh                  from a clone of the repo (builds from the clone)
#   ./install.sh --uninstall      remove it again
#
# Needs: curl or wget. Building from source (from a clone, or for a machine with no ready-made file) needs
# Rust's cargo: https://rustup.rs
#
# Optional environment variables:
#   EV_INSTALL_DIR      where to put the file             (default: ~/.local/bin)
#   EV_VERSION          release tag such as v0.1.0        (default: the latest release)
#   EV_NO_MODIFY_PATH=1 do not edit any shell startup file
#   EV_REPO, EV_BASE_URL, EV_REF   for forks, mirrors and testing

set -eu

NAME=ev
REPO="${EV_REPO:-FahimSifnatul/ev}"
BASE="${EV_BASE_URL:-https://github.com}"
VERSION="${EV_VERSION:-latest}"
DIR="${EV_INSTALL_DIR:-$HOME/.local/bin}"
REF="${EV_REF:-master}"
MARK="# added by the ev installer"
TMP=""

say()  { printf '%s\n' "$*"; }
warn() { printf 'ev installer: %s\n' "$*" >&2; }
die()  { warn "$*"; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }
cleanup() { [ -n "$TMP" ] && rm -rf "$TMP"; return 0; }
trap cleanup EXIT
trap 'exit 130' INT TERM

usage() {
  cat <<'EOF'
ev installer: puts one self-contained program called `ev` on your PATH.

  install.sh                    install (or update)
  install.sh --uninstall        remove it again
  install.sh --no-modify-path   do not edit shell startup files

Environment: EV_INSTALL_DIR (default ~/.local/bin), EV_VERSION (e.g. v0.1.0),
EV_NO_MODIFY_PATH=1. Needs curl or wget; building from source needs Rust's cargo.
EOF
}

# ---------------------------------------------------------------- arguments
ACTION=install
for arg in "$@"; do
  case "$arg" in
    --uninstall) ACTION=uninstall ;;
    --no-modify-path) EV_NO_MODIFY_PATH=1 ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $arg (try --help)" ;;
  esac
done

# ---------------------------------------------------------------- uninstall
if [ "$ACTION" = uninstall ]; then
  target="$DIR/$NAME"
  [ -f "$target" ] || target=$(command -v "$NAME" 2>/dev/null || true)
  if [ -z "$target" ] || [ ! -f "$target" ]; then die "ev is not installed (looked in $DIR and on your PATH)."; fi
  # only delete a file that really is ev, never some other program with the same name
  "$target" --version 2>/dev/null | grep -q "^ev " || die "$target does not look like ev, leaving it alone."
  if [ -w "$(dirname "$target")" ]; then rm -f "$target"; else sudo rm -f "$target"; fi
  say "Removed $target"
  say "(Lines added to your shell startup file, and your variables in ~/.config/ev, are left in place; delete them if you like.)"
  exit 0
fi

fetch() {  # fetch URL DEST
  if have curl; then curl -fsSL "$1" -o "$2"; else wget -q -O "$2" "$1"; fi
}

# the release file made for this machine: ev-macos-aarch64, ev-linux-x86_64 ...
platform() {
  case "$(uname -s)" in
    Darwin) os=macos ;;
    Linux) os=linux ;;
    *) os=$(uname -s | tr '[:upper:]' '[:lower:]') ;;
  esac
  case "$(uname -m)" in
    x86_64|amd64) arch=x86_64 ;;
    arm64|aarch64) arch=aarch64 ;;
    *) arch=$(uname -m) ;;
  esac
  printf '%s-%s' "$os" "$arch"
}

TMP=$(mktemp -d "${TMPDIR:-/tmp}/ev-install.XXXXXX")

# ---------------------------------------------------------------- get the file
build_from() {  # build_from SOURCE_DIR
  have cargo || die "building ev from source needs Rust's cargo, which is not installed.
  Install Rust (one command, no sudo):   curl --proto '=https' -sSf https://sh.rustup.rs | sh
  then open a new terminal and run this installer again."
  say "Building ev from source (a minute or two the first time)..."
  ( cd "$1" && cargo build --release --locked --quiet ) || die "the build failed."
  cp "${CARGO_TARGET_DIR:-$1/target}/release/$NAME" "$TMP/$NAME"
}

verify_checksum() {
  want=$(cut -d' ' -f1 "$TMP/$NAME.sha256")
  if have sha256sum; then got=$(sha256sum "$TMP/$NAME" | cut -d' ' -f1)
  elif have shasum; then got=$(shasum -a 256 "$TMP/$NAME" | cut -d' ' -f1)
  else return 0
  fi
  [ "$got" = "$want" ] || die "the download is corrupted (checksum mismatch). Nothing was installed."
}

is_source() { [ -f "$1/Cargo.toml" ] && [ -f "$1/src/main.rs" ]; }

SELF_DIR=""
if [ -f "$0" ]; then SELF_DIR=$(cd "$(dirname "$0")" 2>/dev/null && pwd) || SELF_DIR=""; fi

if [ -n "$SELF_DIR" ] && is_source "$SELF_DIR"; then
  FROM_CLONE=1
  build_from "$SELF_DIR"
else
  FROM_CLONE=0
  have curl || have wget || die "curl or wget is needed to download ev."
  if [ "$VERSION" = latest ]; then REL="$BASE/$REPO/releases/latest/download"; else REL="$BASE/$REPO/releases/download/$VERSION"; fi
  ASSET="$NAME-$(platform)"
  say "Downloading ev ($ASSET)..."
  if fetch "$REL/$ASSET" "$TMP/$NAME" 2>/dev/null; then
    if fetch "$REL/$ASSET.sha256" "$TMP/$NAME.sha256" 2>/dev/null; then verify_checksum; fi
  else
    # no release published yet, or none made for this machine: build the current source instead
    say "No ready-made $ASSET found, building from source..."
    fetch "$BASE/$REPO/archive/refs/heads/$REF.tar.gz" "$TMP/src.tar.gz" \
      || die "could not download from $BASE/$REPO (is the repository public and the name right?)"
    tar -xzf "$TMP/src.tar.gz" -C "$TMP"
    SRC=""
    for d in "$TMP"/*/; do is_source "${d%/}" && SRC=${d%/}; done
    [ -n "$SRC" ] || die "the downloaded source is not what I expected."
    build_from "$SRC"
  fi
fi

# ---------------------------------------------------------------- install
chmod 755 "$TMP/$NAME"
"$TMP/$NAME" --version >/dev/null 2>&1 || die "the downloaded file does not run, nothing was installed."

if mkdir -p "$DIR" 2>/dev/null && [ -w "$DIR" ]; then
  cp "$TMP/$NAME" "$DIR/$NAME.new" && chmod 755 "$DIR/$NAME.new" && mv -f "$DIR/$NAME.new" "$DIR/$NAME"
else
  have sudo || die "$DIR is not writable and sudo is not available. Set EV_INSTALL_DIR to a folder you own."
  say "$DIR needs administrator rights."
  sudo mkdir -p "$DIR" && sudo cp "$TMP/$NAME" "$DIR/$NAME" && sudo chmod 755 "$DIR/$NAME"
fi
INSTALLED=$("$DIR/$NAME" --version)
say "Installed $INSTALLED at $DIR/$NAME"

# ---------------------------------------------------------------- load variables in new terminals
# `ev init` adds a line to your shell startup file that loads the variables you set with ev
"$DIR/$NAME" init || warn "could not set up the shell hook; run '$NAME init' yourself later."

# ---------------------------------------------------------------- put it on the PATH
on_path() { case ":$PATH:" in *":$DIR:"*) return 0 ;; esac; return 1; }

add_to_path() {
  case "$DIR" in "$HOME"/*) shown="\$HOME${DIR#"$HOME"}" ;; *) shown=$DIR ;; esac
  shell=$(basename "${SHELL:-sh}")
  case "$shell" in
    zsh)  rc="${ZDOTDIR:-$HOME}/.zshrc" ;;
    bash) if [ "$(uname -s)" = Darwin ]; then          # Terminal.app starts login shells: .bashrc is not read
            if [ -f "$HOME/.bash_profile" ] || [ ! -f "$HOME/.profile" ]; then rc="$HOME/.bash_profile"; else rc="$HOME/.profile"; fi
          else rc="$HOME/.bashrc"; fi ;;
    fish) rc="${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/ev.fish" ;;
    *)    rc="$HOME/.profile" ;;
  esac
  if [ "$shell" = fish ]; then
    line="fish_add_path $shown  $MARK"
  else
    line="export PATH=\"$shown:\$PATH\"  $MARK"
  fi
  if [ -f "$rc" ] && grep -qF "$line" "$rc"; then return 0; fi
  mkdir -p "$(dirname "$rc")"
  printf '\n%s\n' "$line" >> "$rc"
  say "Added $shown to your PATH in $rc"
}

if on_path; then
  :
elif [ "${EV_NO_MODIFY_PATH:-0}" = 1 ]; then
  say "$DIR is not on your PATH. Add it yourself:  export PATH=\"$DIR:\$PATH\""
else
  add_to_path
  NEED_NEW_SHELL=1
fi

# another ev earlier on the PATH (Homebrew, cargo install...) would hide this one
other=$(command -v "$NAME" 2>/dev/null || true)
if [ -n "$other" ] && [ "$other" != "$DIR/$NAME" ]; then
  warn "note: $other comes first on your PATH and will run instead of the one just installed."
fi

# ---------------------------------------------------------------- done
say ""
if [ "${NEED_NEW_SHELL:-0}" = 1 ]; then
  say "Open a new terminal window, then run:   ev --help"
  say "(or in this window right now:   export PATH=\"$DIR:\$PATH\")"
else
  say "Ready. Try:   ev --help"
fi
say ""
say "Variables you set with ev (ev set NAME value) are loaded by every new terminal."
if [ "$FROM_CLONE" = 1 ]; then
  say ""
  say "ev is one self-contained program, so you can delete the folder you installed from."
fi
