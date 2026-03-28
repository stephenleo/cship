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

# ── 5. cship.toml — create minimal config (idempotent) ───────────────────────
CSHIP_CONFIG="$ROOT/.config/cship.toml"
mkdir -p "$(dirname "$CSHIP_CONFIG")"

CSHIP_BLOCK='# cship — Claude Code statusline
# Full config reference: https://cship.dev
[cship]
lines = [
  "$directory$git_branch$git_status$python$nodejs$rust",
  "$cship.model $cship.cost $cship.context_bar $cship.usage_limits"
]

[cship.model]
symbol = "🤖 "
style  = "bold cyan"

[cship.context_bar]
width              = 10
style              = "fg:#7dcfff"
warn_threshold     = 40.0
warn_style         = "fg:#e0af68"
critical_threshold = 70.0
critical_style     = "bold fg:#f7768e"

[cship.cost]
symbol             = "💰 "
style              = "fg:#a9b1d6"
warn_threshold     = 2.0
warn_style         = "fg:#e0af68"
critical_threshold = 5.0
critical_style     = "bold fg:#f7768e"

[cship.usage_limits]
five_hour_format   = "⌛ 5h {pct}% ({reset})"
seven_day_format   = "📅 7d {pct}% ({reset})"
separator          = " "
warn_threshold     = 60.0
warn_style         = "fg:#e0af68"
critical_threshold = 80.0
critical_style     = "bold fg:#f7768e"
'

if [ -f "$CSHIP_CONFIG" ]; then
  echo "cship.toml already exists at $CSHIP_CONFIG, skipping."
else
  printf '%s' "$CSHIP_BLOCK" > "$CSHIP_CONFIG"
  echo "Created minimal cship config at $CSHIP_CONFIG"
fi

# ── 6. ~/.claude/settings.json — wire statusline (via python3) ───────────────
SETTINGS="$ROOT/.claude/settings.json"
if ! command -v python3 >/dev/null 2>&1; then
  echo "Warning: python3 not found. Skipping settings.json update."
  echo "To wire cship manually, add \"statusLine\": {\"type\": \"command\", \"command\": \"cship\"} to $SETTINGS"
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
