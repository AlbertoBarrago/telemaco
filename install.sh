#!/usr/bin/env bash
# Install telemaco, then optionally register it as an MCP server with the
# AI agents found on this machine.
#
#   ./install.sh                  interactive
#   ./install.sh --yes            accept every default, register nothing
#   ./install.sh --prefix ~/bin   install somewhere else
#   ./install.sh --from-source    skip the release download and build locally
#
# Nothing is written outside the install prefix and the agent config files you
# explicitly approve. Every config touched is backed up first.
set -uo pipefail

REPO="AlbertoBarrago/telemaco"
PREFIX="${PREFIX:-$HOME/.local/bin}"
ASSUME_YES=0
FROM_SOURCE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --yes|-y)      ASSUME_YES=1; shift ;;
    --prefix)      PREFIX="$2"; shift 2 ;;
    --from-source) FROM_SOURCE=1; shift ;;
    # Print the header comment, whatever its length, rather than a line range
    # that silently drifts as the file is edited.
    -h|--help)     awk 'NR>1 && /^#/ { sub(/^# ?/, ""); print; next } NR>1 { exit }' "$0"; exit 0 ;;
    *)             echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

bold()  { printf '\033[1m%s\033[0m\n' "$*"; }
ok()    { printf '  \033[32m%s\033[0m\n' "$*"; }
warn()  { printf '  \033[33m%s\033[0m\n' "$*"; }
die()   { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

ask() {
  # ask "question" -> 0 for yes. Defaults to no when not a terminal.
  [[ $ASSUME_YES -eq 1 ]] && return 1
  [[ -t 0 ]] || return 1
  local reply
  read -r -p "  $1 [y/N] " reply </dev/tty || return 1
  [[ "$reply" =~ ^[YySs] ]]
}

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
  # Returns non-zero when no published release carries our asset, so the caller
  # can fall back to a source build instead of failing outright.
  #
  # Three ways in, because the obvious one does not always work: the public
  # /releases/latest/download/ URL 404s on a PRIVATE repository even with a
  # token, so a private checkout needs the gh CLI or the API asset endpoint.
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

BIN="$PREFIX/telemaco"
[[ -x "$BIN" ]] || die "install failed: $BIN is missing"
ok "$("$BIN" --version) -> $BIN"

case ":$PATH:" in
  *":$PREFIX:"*) ;;
  *) warn "$PREFIX is not on your PATH. Add: export PATH=\"$PREFIX:\$PATH\"" ;;
esac

# ---------------------------------------------------------------------- MCP ---

printf '\n'
bold "MCP registration"
echo "  telemaco can act as an MCP server, giving an agent a real browser:"
echo "  navigate, click, fill forms, read pages as Markdown, screenshot, PDF."

# Merge {"mcpServers": {"telemaco": {...}}} into a JSON file, preserving the
# rest of it. Refuses rather than guesses when the file is not valid JSON,
# because these files hold the user's other servers.
merge_json_config() {
  local file="$1" key="${2:-mcpServers}"
  python3 - "$file" "$BIN" "$key" <<'PY'
import json, os, shutil, sys
path, binary, key = sys.argv[1], sys.argv[2], sys.argv[3]
data = {}
if os.path.exists(path) and os.path.getsize(path) > 0:
    try:
        with open(path) as f:
            data = json.load(f)
    except json.JSONDecodeError as e:
        print(f"    not valid JSON ({e}); leaving it alone", file=sys.stderr)
        sys.exit(3)
    if not isinstance(data, dict):
        print("    top level is not an object; leaving it alone", file=sys.stderr)
        sys.exit(3)
    shutil.copy2(path, path + ".telemaco-backup")
servers = data.setdefault(key, {})
if not isinstance(servers, dict):
    print(f"    '{key}' is not an object; leaving it alone", file=sys.stderr)
    sys.exit(3)
if servers.get("telemaco", {}).get("command") == binary:
    print("    already registered, nothing to do")
    sys.exit(4)
servers["telemaco"] = {"command": binary, "args": ["mcp"]}
os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
tmp = path + ".telemaco-tmp"
with open(tmp, "w") as f:
    json.dump(data, f, indent=2)
    f.write("\n")
os.replace(tmp, path)
PY
}

register() {
  local label="$1" file="$2" key="${3:-mcpServers}"
  local out rc
  out="$(merge_json_config "$file" "$key" 2>&1)"; rc=$?
  case $rc in
    0) ok "$label: registered"
       [[ -f "$file.telemaco-backup" ]] && printf '    backup: %s\n' "$file.telemaco-backup" ;;
    4) ok "$label: $out" ;;
    *) warn "$label: not changed"; [[ -n "$out" ]] && printf '%s\n' "$out" ;;
  esac
}

FOUND=0

# Claude Code ships a CLI that owns its own config; prefer it over file surgery.
if command -v claude >/dev/null; then
  FOUND=1
  if ask "Register with Claude Code?"; then
    if claude mcp list 2>/dev/null | grep -q '^telemaco'; then
      ok "Claude Code: already registered"
    elif claude mcp add telemaco -- "$BIN" mcp >/dev/null 2>&1; then
      ok "Claude Code: registered"
    else
      warn "Claude Code: 'claude mcp add' failed; add it by hand with:"
      printf '    claude mcp add telemaco -- %s mcp\n' "$BIN"
    fi
  fi
fi

if [[ "$OS" == macos ]]; then
  CLAUDE_DESKTOP="$HOME/Library/Application Support/Claude/claude_desktop_config.json"
else
  CLAUDE_DESKTOP="$HOME/.config/Claude/claude_desktop_config.json"
fi
if [[ -d "$(dirname "$CLAUDE_DESKTOP")" ]]; then
  FOUND=1
  ask "Register with Claude Desktop?" && register "Claude Desktop" "$CLAUDE_DESKTOP"
fi

if [[ -d "$HOME/.cursor" ]]; then
  FOUND=1
  ask "Register with Cursor?" && register "Cursor" "$HOME/.cursor/mcp.json"
fi

if [[ -d "$HOME/.codeium/windsurf" ]]; then
  FOUND=1
  ask "Register with Windsurf?" && register "Windsurf" "$HOME/.codeium/windsurf/mcp_config.json"
fi

# Zed's settings.json accepts comments, and rewriting it as strict JSON would
# silently delete them. Print the snippet instead of editing the file.
ZED="$HOME/.config/zed/settings.json"
if [[ -f "$ZED" ]]; then
  FOUND=1
  if ask "Show the Zed snippet? (its settings file allows comments, so this one is manual)"; then
    printf '    add to %s:\n\n' "$ZED"
    printf '      "context_servers": {\n'
    printf '        "telemaco": { "command": { "path": "%s", "args": ["mcp"] } }\n' "$BIN"
    printf '      }\n\n'
  fi
fi

[[ $FOUND -eq 0 ]] && warn "no supported agent detected; telemaco is installed and usable from the CLI"

printf '\n'
bold "Done"
printf '  Try it:  %s fetch https://example.com --dump markdown\n' "${BIN##*/}"
printf '  MCP:     %s mcp        (stdio)\n' "${BIN##*/}"
printf '  Restart any agent you just registered so it picks up the server.\n'
