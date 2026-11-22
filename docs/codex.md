# Codex CLI (experimental)

cship can render its statusline in the footer of [Codex CLI](https://github.com/openai/codex) as well as Claude Code. The same `cship.toml` / `starship.toml` config drives both.

::: warning Experimental on both sides
Custom status lines are behind an experimental flag in Codex (first shipped in `rust-v0.171.0`). The protocol may change between Codex releases, and cship's support will follow it.
:::

## Setup

The installers (`install.sh` / `install.ps1`, see the [Guide](/)) detect Codex 0.171.0 or newer and set it up for you. If you installed another way, or added Codex later, run:

```sh
cship codex-setup
```

It adds the three settings shown below to `$CODEX_HOME/config.toml` (default `~/.codex/config.toml`), touching only keys that are not set yet: an existing `status_line_provider` is kept, comments and formatting are preserved, and the file is backed up as `config.toml.cship-backup-<timestamp>` before any change. Restart Codex afterwards. Pass `--codex-home <dir>` to target another Codex home.

### Manual setup

1. Install cship as usual and make sure `cship` is on your `PATH`.
2. Register cship as a stdio MCP server in Codex:

   ```sh
   codex mcp add cship -- cship codex-mcp
   ```

   To use a specific config file, pass it before the subcommand:

   ```sh
   codex mcp add cship -- cship --config /absolute/path/to/cship.toml codex-mcp
   ```

3. In Codex, open `/experimental`, enable **Custom status line**, then restart the TUI. If the setting doesn't persist, use the `config.toml` form below.
4. Start a new conversation, open `/statusline`, choose **Choose a status line provider**, and select **cship**.

You can set the same thing in `~/.codex/config.toml` instead of using the menus:

```toml
[features]
tui_extensions_statusline = true

[tui]
status_line_provider = { server = "cship", tool = "statusline" }

[mcp_servers.cship]
command = "cship"
args = ["codex-mcp"]
```

If the footer falls back to the built-in line, press **F2** in Codex to see the failed attempt.

## Environment variables and logs

Codex starts MCP servers with a small allowlisted environment (`HOME`, `PATH`, `USER`, `LANG`, `TERM`, `TZ` and a few others). Variables exported in your shell, such as `RUST_LOG`, `CSHIP_ACCOUNT`, `STARSHIP_CONFIG` or `XDG_CACHE_HOME`, do **not** reach cship unless you pass them in the server entry. Set a fixed value with `env`, or forward a variable from Codex's own environment with `env_vars`:

```toml
[mcp_servers.cship]
command = "cship"
args = ["codex-mcp"]
env = { RUST_LOG = "warn" }
env_vars = ["CSHIP_ACCOUNT", "STARSHIP_CONFIG"]
```

cship writes its diagnostics to stderr, which Codex records in its own log as `MCP server stderr (cship): …` lines. Because Codex polls every second, cship writes each distinct message once and repeats it at most every 5 minutes while it keeps happening.

## How it works

Codex does not pipe JSON to a command the way Claude Code does. `cship codex-mcp` runs as a long-lived MCP server; about once a second Codex asks it for the footer and includes a snapshot of the current thread. cship maps that snapshot onto the same fields its modules already read, renders your configured lines, and converts the styled output into the UI tree Codex expects.

Config is reloaded on every read, so edits to `cship.toml` show up without restarting Codex. Config discovery is the same as in Claude Code, starting from the thread's working directory.

## What maps from Codex

| cship module / token | Codex source |
|---|---|
| `$cship.model` | thread model display name and id |
| `$cship.effort` | model reasoning level |
| `$cship.cost` | thread cost (`total_cost_usd` only; duration and line counts are not provided) |
| `$cship.context_bar`, `$cship.context_window.*` | thread context usage and total token counts. `$cship.context_window.used_tokens` uses the tokens currently in context, which Codex reports as one total; `$cship.context_window.current_usage.input_tokens` shows that same total |
| `$cship.usage_limits` | account rate limits from the `codex` bucket; the 300-minute window is shown as 5h and the 10080-minute window as 7d |
| `$cship.workspace.current_dir`, `$cship.cwd` | thread working directory |
| `$cship.session_id` | thread id |
| `$cship.version` | Codex CLI version |
| Starship passthrough (`$git_branch`, ...) | runs `starship module` in the thread's directory, as usual |

## Differences from Claude Code

- **Colors.** ANSI names (`red`, `green`, ...) and 256-color indices 0–15 follow your Codex theme as ANSI names; bright variants share the base name, except bright black, which becomes gray. Hex values and 256-color indices 16–255 are sent as `#RRGGBB`.
- **No text backgrounds.** Codex only supports backgrounds on whole rows, so `bg:` styles are dropped. Bold, italic and underline are kept; dimmed, blink and reverse are dropped.
- **`$fill`** renders as a single space. Codex 0.171.0 has no way to right-align part of the footer, and `[cship.fill]` `symbol` and `style` are not drawn.
- **Spacing.** Codex lays out footer text like HTML: runs of spaces collapse to one and leading spaces on a line are dropped. Separators such as `" | "` still work; padding with several spaces does not.
- **Multiple lines** (`lines = [...]` or `$line_break`) show as separate footer lines.
- **Not available:** `$cship.vim`, `$cship.agent`, `$cship.output_style`, `$cship.workspace.project_dir`, the `$cship.cost` duration and line-count sub-fields, the `cache_*` fields of `$cship.context_window.current_usage` and `$cship.context_window.exceeds_200k` have no Codex equivalent and render nothing. `$cship.transcript_path` shows cship's internal per-thread cache path. `$cship.account` only works through the [`CSHIP_ACCOUNT`](/faq#account-setup) environment variable, since the Claude keychain login describes a different account.
- **Usage limits** come only from the Codex snapshot. The Anthropic OAuth lookup that fills per-model limits in Claude Code is skipped, so `[cship.usage_limits] stdin_only` makes no difference here.
- **Usage limits and cost depend on your Codex plan.** cship shows only 5h and 7d windows. Free plans report a single 30-day window, so the `usage_limits` segment stays hidden. Cost is reported only on Business and Enterprise plans, so `$cship.cost` stays empty on other plans.
- **Read budget.** Codex allows one second per read, and an error or timeout stops polling. cship gives each render 800 ms and returns the last good footer if it runs over (for example, a slow Starship module on first run). A render still running after 10 seconds (say, `git` on a stalled network drive) is abandoned so the footer can recover. Starship results are cached for 5 seconds per thread under `~/.cache/cship/codex/` (or `$XDG_CACHE_HOME/cship/codex/` when forwarded through `env_vars`); cache files untouched for 7 days are removed when the server starts.

## Known Codex issues

- [openai/codex#62629](https://github.com/openai/codex/issues/62629): polling can stop after a read timeout. Reselect the provider in `/statusline` to restart it.
