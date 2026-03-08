#!/usr/bin/env bash
set -euo pipefail

# Allow CSHIP_TEST_ROOT to override HOME for all path resolution (testability)
ROOT="${CSHIP_TEST_ROOT:-$HOME}"
INSTALL_DIR="$ROOT/.local/bin"

# ── 1. OS / Arch Detection ────────────────────────────────────────────────────
OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
  Darwin)
    case "$ARCH" in
      arm64)  TARGET="aarch64-apple-darwin" ;;
      x86_64) TARGET="x86_64-apple-darwin" ;;
      *)      echo "Unsupported macOS arch: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  Linux)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-unknown-linux-musl" ;;
      aarch64) TARGET="aarch64-unknown-linux-musl" ;;
      *)       echo "Unsupported Linux arch: $ARCH" >&2; exit 1 ;;
    esac
    ;;
  *)
    echo "Unsupported OS: $OS" >&2; exit 1 ;;
esac

echo "Detected: $OS/$ARCH → target: $TARGET"

# ── 2. Download Binary ────────────────────────────────────────────────────────
BINARY_URL="https://github.com/stephenleo/cship/releases/latest/download/cship-${TARGET}"
mkdir -p "$INSTALL_DIR"
echo "Downloading cship from $BINARY_URL ..."
curl -fsSL "$BINARY_URL" -o "${INSTALL_DIR}/cship"
chmod +x "${INSTALL_DIR}/cship"
if [ ! -s "${INSTALL_DIR}/cship" ]; then
  echo "Error: downloaded binary is empty — check network or release URL" >&2
  rm -f "${INSTALL_DIR}/cship"
  exit 1
fi
echo "Installed cship to ${INSTALL_DIR}/cship"

# ── 3. Linux: libsecret-tools check (usage limits dependency) ─────────────────
if [ "$OS" = "Linux" ] && ! command -v secret-tool >/dev/null 2>&1; then
  printf "Install libsecret-tools? (required for usage limits on Linux) [Y/n] "
  read -r answer </dev/tty
  case "$answer" in
    [Nn]*) echo "Skipping — usage limits module unavailable until installed manually." ;;
    *)     sudo apt-get install -y libsecret-tools ;;
  esac
fi

# ── 4. Starship detection and optional install ────────────────────────────────
if ! command -v starship >/dev/null 2>&1; then
  printf "Starship not found. Install Starship? (required for passthrough modules) [Y/n] "
  read -r answer </dev/tty
  case "$answer" in
    [Nn]*) echo "Skipping Starship install. Native cship modules will still work." ;;
    *)     curl -sS https://starship.rs/install.sh | sh ;;
  esac
fi

# ── 5. starship.toml append — idempotent ─────────────────────────────────────
STARSHIP_CONFIG="$ROOT/.config/starship.toml"
mkdir -p "$(dirname "$STARSHIP_CONFIG")"

CSHIP_BLOCK='# cship — Claude Code statusline
[cship]
lines = ["$cship.model $cship.cost $cship.context_bar"]
'

if grep -q '^\[cship\]' "$STARSHIP_CONFIG" 2>/dev/null; then
  echo "starship.toml already contains [cship] block, skipping."
else
  # Add a blank line separator if appending to an existing file
  if [ -s "$STARSHIP_CONFIG" ]; then
    printf '\n' >> "$STARSHIP_CONFIG"
  fi
  printf '%s' "$CSHIP_BLOCK" >> "$STARSHIP_CONFIG"
  echo "Appended [cship] starter block to $STARSHIP_CONFIG"
fi

# ── 6. ~/.claude/settings.json — wire statusline (via python3) ───────────────
SETTINGS="$ROOT/.claude/settings.json"
if ! command -v python3 >/dev/null 2>&1; then
  echo "Warning: python3 not found. Skipping settings.json update."
  echo "To wire cship manually, add \"statusline\": \"cship\" to $SETTINGS"
elif [ -f "$SETTINGS" ]; then
  python3 - "$SETTINGS" <<'PYEOF' || echo "Warning: failed to update settings.json — add statusLine manually."
import json, sys
path = sys.argv[1]
try:
    with open(path) as f:
        d = json.load(f)
except (json.JSONDecodeError, ValueError) as e:
    print('Warning: ' + path + ' contains invalid JSON: ' + str(e))
    sys.exit(1)
if 'statusLine' not in d:
    d['statusLine'] = {'type': 'command', 'command': 'cship'}
    with open(path, 'w') as f:
        json.dump(d, f, indent=2)
        f.write('\n')
    print('Added statusLine config to ' + path)
else:
    print('"statusLine" already set in ' + path + ', skipping.')
PYEOF
else
  echo "settings.json not found at $SETTINGS — skipping (Claude Code may not be installed yet)."
fi

# ── 7. First-run preview ──────────────────────────────────────────────────────
echo ""
echo "Running cship explain..."
"$INSTALL_DIR/cship" explain || true

echo ""
echo "cship installation complete!"
echo "If ~/.local/bin is not in your PATH, add: export PATH=\"\$HOME/.local/bin:\$PATH\""
