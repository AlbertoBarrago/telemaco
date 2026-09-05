#!/usr/bin/env bash
# Install telemaco and add it to your PATH.
#
#   ./install.sh                  install to ~/.local/bin
#   ./install.sh --yes            answer yes to every prompt
#   ./install.sh --prefix ~/bin   install to custom directory
#   ./install.sh --from-source    build locally with cargo instead of downloading
#   ./install.sh --uninstall      remove telemaco from the install prefix
#
# Once installed, configure your AI coding assistants with:
#   telemaco install
#   telemaco install --folder /path/to/project
set -uo pipefail

REPO="AlbertoBarrago/telemaco"
PREFIX="${PREFIX:-$HOME/.local/bin}"
ASSUME_YES=0
FROM_SOURCE=0
UNINSTALL=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --yes|-y)      ASSUME_YES=1; shift ;;
    --prefix)      PREFIX="$2"; shift 2 ;;
    --prefix=*)    PREFIX="${1#*=}"; shift ;;
    --uninstall)   UNINSTALL=1; shift ;;
    --from-source) FROM_SOURCE=1; shift ;;
    -h|--help)     awk 'NR>1 && /^#/ { sub(/^# ?/, ""); print; next } NR>1 { exit }' "$0"; exit 0 ;;
    *)             echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

bold()  { printf '\033[1m%s\033[0m\n' "$*"; }
ok()    { printf '  \033[32m%s\033[0m\n' "$*"; }
warn()  { printf '  \033[33m%s\033[0m\n' "$*"; }
die()   { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

ask() {
  # ask "question" -> 0 for yes. Defaults to no when there is nobody to ask.
  # --yes answers yes: it used to answer no, which made --yes --uninstall
  # print "aborted" and remove nothing.
  [[ $ASSUME_YES -eq 1 ]] && return 0
  # The prompt reads from /dev/tty, so that is what has to be testable. Testing
  # stdin instead silently answered no to everything under `curl ... | bash`,
  # the way most people run this.
  [[ -r /dev/tty ]] || return 1
  local reply
  read -r -p "  $1 [y/N] " reply </dev/tty || return 1
  [[ "$reply" =~ ^[YySs] ]]
}

if [[ $UNINSTALL -eq 1 ]]; then
  bold "Uninstalling telemaco"
  if ! ask "Remove telemaco from $PREFIX?"; then
    echo "aborted"; exit 0
  fi
  rm -f "$PREFIX/telemaco" "$PREFIX/telemaco-worker"
  ok "Removed telemaco from $PREFIX"
  echo "  Note: To remove agent configurations, run: telemaco uninstall"
  exit 0
fi

# ---------------------------------------------------------------- platform ---

case "$(uname -s)" in
  Darwin) OS=macos ;;
  Linux)  OS=linux ;;
  *)      die "unsupported OS: $(uname -s). Windows users: build from source." ;;
esac
case "$(uname -m)" in
  arm64|aarch64) ARCH=aarch64 ;;
  x86_64|amd64)  ARCH=x86_64 ;;
  *)             die "unsupported architecture: $(uname -m)" ;;
esac
ASSET="telemaco-${ARCH}-${OS}.tar.gz"

# ------------------------------------------------------------------ install ---

bold "Installing telemaco"

# Test hook: when TELEMACO_TEST_BIN points at an existing executable, skip
# the release download and source build and install that binary instead.
if [[ -n "${TELEMACO_TEST_BIN:-}" && -x "$TELEMACO_TEST_BIN" ]]; then
  mkdir -p "$PREFIX"
  install -m 0755 "$TELEMACO_TEST_BIN" "$PREFIX/telemaco"
  ok "installed test binary"
else

install_from_source() {
  command -v cargo >/dev/null || die "cargo not found. Install Rust from https://rustup.rs"
  [[ -f Cargo.toml ]] || die "run this from a telemaco checkout, or install a release build"
  warn "building from source: the first build compiles V8 and takes several minutes"
  local features=render
  if command -v cmake >/dev/null; then
    features=render,stealth
  else
    warn "cmake not found, building without stealth (BoringSSL needs CMake)"
  fi
  CARGO_INCREMENTAL=0 cargo build --release -p telemaco-cli --bins --features "$features" \
    || die "build failed"
  mkdir -p "$PREFIX"
  install -m 0755 target/release/telemaco "$PREFIX/telemaco"
  [[ -f target/release/telemaco-worker ]] \
    && install -m 0755 target/release/telemaco-worker "$PREFIX/telemaco-worker"
  ok "built with features: $features"
}

install_from_release() {
  local tmp; tmp="$(mktemp -d)"
  local got=1

  if command -v gh >/dev/null && gh auth status >/dev/null 2>&1; then
    if gh release download --repo "$REPO" --pattern "$ASSET" --dir "$tmp" --clobber >/dev/null 2>&1; then
      got=0
    fi
  fi

  if [[ $got -ne 0 ]]; then
    local token="${GH_TOKEN:-${GITHUB_TOKEN:-}}"
    if [[ -n "$token" ]]; then
      local id
      id="$(curl -fsSL -H "Authorization: Bearer $token" \
              "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
            | python3 -c "import json,sys
try: r = json.load(sys.stdin)
except Exception: sys.exit(0)
for a in r.get('assets', []):
    if a['name'] == '$ASSET':
        print(a['id']); break" 2>/dev/null)"
      if [[ -n "$id" ]] && curl -fsSL -o "$tmp/$ASSET" \
           -H "Authorization: Bearer $token" -H "Accept: application/octet-stream" \
           "https://api.github.com/repos/$REPO/releases/assets/$id" 2>/dev/null; then
        got=0
      fi
    fi
  fi

  if [[ $got -ne 0 ]]; then
    curl -fsSL --retry 2 -o "$tmp/$ASSET" \
      "https://github.com/$REPO/releases/latest/download/$ASSET" 2>/dev/null && got=0
  fi

  if [[ $got -ne 0 ]] || ! tar xzf "$tmp/$ASSET" -C "$tmp" 2>/dev/null; then
    rm -rf "$tmp"; return 1
  fi

  mkdir -p "$PREFIX"
  install -m 0755 "$tmp/telemaco" "$PREFIX/telemaco"
  [[ -f "$tmp/telemaco-worker" ]] && install -m 0755 "$tmp/telemaco-worker" "$PREFIX/telemaco-worker"
  rm -rf "$tmp"
  ok "installed the published $ASSET"
}

if [[ $FROM_SOURCE -eq 1 ]]; then
  install_from_source
elif install_from_release; then
  :
else
  warn "no published release carries $ASSET yet"
  install_from_source
fi
fi

BIN="$PREFIX/telemaco"
[[ -x "$BIN" ]] || die "install failed: $BIN is missing"
ok "$("$BIN" --version) -> $BIN"

# --------------------------------------------------------------------- PATH ---

add_to_path() {
  local prefix="$1"
  case ":$PATH:" in
    *":$prefix:"*) return 0 ;;
  esac

  local shell_rc=""
  local shell_name
  shell_name="$(basename "${SHELL:-bash}")"
  case "$shell_name" in
    zsh)  shell_rc="$HOME/.zshrc" ;;
    bash) [[ -f "$HOME/.bash_profile" ]] && shell_rc="$HOME/.bash_profile" || shell_rc="$HOME/.bashrc" ;;
    fish) shell_rc="$HOME/.config/fish/config.fish" ;;
    *)    shell_rc="$HOME/.profile" ;;
  esac

  if [[ -n "$shell_rc" ]]; then
    local line="export PATH=\"$prefix:\$PATH\""
    if [[ "$shell_name" == "fish" ]]; then
      line="fish_add_path $prefix"
    fi
    if grep -qF -- "$prefix" "$shell_rc" 2>/dev/null; then
      # Already on file: the current shell just has not read it yet. Saying
      # "not on your PATH, add this line" here was plain wrong.
      ok "$prefix is already in $shell_rc; open a new shell to pick it up"
      return 0
    fi
    if ask "Add $prefix to PATH in $shell_rc?"; then
      echo "" >> "$shell_rc"
      echo "# Telemaco CLI" >> "$shell_rc"
      echo "$line" >> "$shell_rc"
      ok "Added $prefix to $shell_rc"
      export PATH="$prefix:$PATH"
      return 0
    fi
  fi
  warn "$prefix is not on your PATH. Add to your shell profile:"
  printf '    export PATH="%s:$PATH"\n' "$prefix"
}

add_to_path "$PREFIX"

# ----------------------------------------------------------------- Next step ---

printf '\n'
bold "✨ Telemaco installed successfully!"
echo "  Binary: $BIN"
echo ""
echo "🤖 Next step: Configure your AI coding assistants"
echo "  Run the interactive installer to set up MCP servers, memory files, and hooks:"
echo ""
echo "    telemaco install"
echo ""
echo "  Or configure a specific project directory:"
echo "    telemaco install --folder /path/to/project"
echo ""
