---
layout: home

hero:
  name: "⚓ CShip"
  text: ""
  tagline: Bring Starship's power to Claude Code Status Line.
  actions:
    - theme: brand
      text: Get Started
      link: /#install-curl
    - theme: alt
      text: Configure
      link: /configuration

features:
  - icon: 🎨
    title: Fully Customizable
    details: Configure every module with Starship-compatible TOML. Colors, symbols, thresholds — your statusline, your rules.
  - icon: ⚡
    title: Blazing Fast
    details: Written in Rust with a ≤10ms render budget.
  - icon: 🔌
    title: Starship Passthrough
    details: Embed any Starship module (git_branch, directory, language runtimes) right next to native CShip modules.
  - icon: 💰
    title: Session Insights
    details: Track cost, context window usage, API limits, vim mode, agent name, and more — all from Claude Code's live JSON feed.
---

## What is CShip?

`cship` renders a live statusline for [Claude Code](https://claude.ai/code) sessions.

It reads Claude Code's session JSON from stdin and renders styled text using a simple TOML config file — the same format as [Starship](https://starship.rs).

If you've already invested in Starship customization, CShip slots right in: add `[cship.*]` sections to your existing `starship.toml` (or use a dedicated `~/.config/cship.toml`), reference native CShip modules alongside any Starship module, and get a unified statusline that speaks both languages.

## Install {#install-curl}

### macOS / Linux {#install-macos-linux}

```sh
curl -fsSL https://cship.dev/install.sh | bash
```

Auto-detects your OS and architecture (macOS arm64/x86_64, Linux x86_64/aarch64), downloads the binary to `~/.local/bin/cship`, creates a starter config at `~/.config/cship.toml`, and wires the `statusLine` entry in `~/.claude/settings.json`.

Optional dependencies ([Starship](https://starship.rs) for passthrough modules, and `libsecret-tools` on Linux for usage limits) are handled as follows:

- **Interactive terminal** — the installer prompts you for each.
- **`--yes` / `-y`** — auto-installs all optional deps without prompting:
  ```sh
  curl -fsSL https://cship.dev/install.sh | bash -s -- --yes
  ```
- **Non-interactive** (Docker `RUN`, CI pipelines, no TTY) — optional deps are skipped automatically; the installer prints instructions for manual installation.

### Windows {#install-windows}

Run this one-liner in PowerShell (5.1 or later):

```powershell
irm https://cship.dev/install.ps1 | iex
```

Installs to `%USERPROFILE%\.local\bin\cship.exe`, writes config to `%USERPROFILE%\.config\cship.toml`, and registers the statusline in `%USERPROFILE%\.claude\settings.json`.

> You can inspect the script before running: [install.ps1](https://cship.dev/install.ps1)

### Cargo Install {#install-cargo}

Requires the Rust toolchain.

```sh
cargo install cship
```

After installing with `cargo` on **macOS / Linux**, wire the statusline manually in `~/.claude/settings.json`:

```json
{
  "statusLine": { "type": "command", "command": "cship" }
}
```

After installing with `cargo` on **Windows**, wire the statusline manually in `%APPDATA%\\Claude\\settings.json`:

```json
{
  "statusLine": { "type": "command", "command": "cship" }
}
```

## Nerd Fonts (optional)

CShip supports [Nerd Fonts](https://www.nerdfonts.com) — patched fonts that add thousands of icons your terminal can render as glyphs. With a Nerd Font active, you can use icon symbols as `symbol` values in any module config instead of plain text or emoji.

**Install a Nerd Font:**

1. Download any font from **[nerdfonts.com](https://www.nerdfonts.com/font-downloads)** (popular picks: JetBrainsMono Nerd Font, FiraCode Nerd Font, Hack Nerd Font)
2. Install it on your system and set it as your terminal's font
3. Use Nerd Font glyphs in your `cship.toml`:

```toml
[cship.model]
symbol = "󰚩 "   # nf-md-robot

[cship.context_bar]
symbol = " "   # nf-oct-cpu
```

::: tip Finding more glyphs
Browse [nerdfonts.com/cheat-sheet](https://www.nerdfonts.com/cheat-sheet) to find any icon and paste it directly into your `cship.toml`.
:::

→ The [Showcase](/showcase#_6-nerd-fonts) has a full Nerd Fonts config example.

## Quick Start

Create `~/.config/cship.toml` (on Windows: `%USERPROFILE%\.config\cship.toml`):

```toml
[cship]
lines = ["$cship.model  $cship.cost  $cship.context_bar"]

[cship.cost]
warn_threshold     = 2.0
warn_style         = "yellow"
critical_threshold = 5.0
critical_style     = "bold red"

[cship.context_bar]
width              = 10
warn_threshold     = 40.0
warn_style         = "yellow"
critical_threshold = 70.0
critical_style     = "bold red"
```

Open a Claude Code session — your statusline will show the model name, session cost (turning yellow at $2, red at $5), and a 10-character context bar (warming up at 40%, going critical at 70%).

→ [Full Configuration Reference](/configuration)
→ [Showcase — ready-to-use configs](/showcase)

## Debugging

Run `cship explain` to inspect what CShip sees from Claude Code's context JSON:

```sh
cship explain
```

This shows each module's current rendered value, the config file path in use, and any warnings about missing data or misconfiguration.

To check the installed binary version:

```sh
cship --version   # or: cship -v
```

## Inspired by [Starship](https://starship.rs)