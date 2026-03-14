# Starship Passthrough

CShip lets you embed any [Starship](https://starship.rs) module directly in your statusline layout, right next to native CShip modules.

## How It Works

Any token in a `lines` format string that doesn't start with `cship.` is treated as a Starship module name. CShip invokes `starship module <name>` as a subprocess, captures its stdout, and splices the output into your statusline.

```toml
[cship]
lines = [
  "$directory $git_branch $git_status",
  "$cship.model  $cship.cost  $cship.context_bar",
]
```

In the example above, `$directory`, `$git_branch`, and `$git_status` are Starship passthrough modules. `$cship.model`, `$cship.cost`, and `$cship.context_bar` are native CShip modules.

**Prerequisite:** [Starship](https://starship.rs) must be installed and on your `$PATH`.

## CSHIP_* Environment Variables

Before each Starship subprocess call, cship sets the following environment variables so your Starship modules can access Claude Code session data:

| Variable | Type | Example | Description |
|----------|------|---------|-------------|
| `CSHIP_MODEL` | string | `claude-sonnet-4-5` | Active model display name |
| `CSHIP_MODEL_ID` | string | `claude-sonnet-4-5-20251022` | Full model identifier |
| `CSHIP_COST_USD` | float | `1.234` | Session cost in USD |
| `CSHIP_CONTEXT_PCT` | float | `42.5` | Context window used (%) |
| `CSHIP_CONTEXT_REMAINING_PCT` | float | `57.5` | Context window remaining (%) |
| `CSHIP_VIM_MODE` | string | `NORMAL` | Current vim mode (empty if inactive) |
| `CSHIP_AGENT_NAME` | string | `claude-code` | Active agent name (empty if none) |
| `CSHIP_SESSION_ID` | string | `abc123...` | Session UUID |
| `CSHIP_CWD` | string | `/home/user/project` | Current working directory |

These variables are available inside any custom Starship module you write. For example, you could create a Starship module that changes colour based on `CSHIP_COST_USD`.


## Cache Behaviour

Passthrough module output is cached for **5 seconds per session** to avoid spawning a new Starship subprocess on every statusline render.

Cache path: `{dirname(transcript_path)}/cship/{transcript_stem}-starship-{module_name}`

The cache is keyed by session transcript path, so different Claude Code sessions maintain independent caches. The cache directory is created automatically if missing.

## Process Details

- The subprocess runs with the working directory set to `workspace.current_dir`
- Stderr from the Starship subprocess is discarded
- If Starship is not found or the subprocess fails, the module renders empty (silent failure — no error shown in the statusline)
- The first call in a session may take a moment; subsequent calls within 5s use the cache

## Example: Mixed Native + Passthrough Config

```toml
[cship]
lines = [
  """
  $directory\
  $git_branch\
  $git_status\
  $python\
  $nodejs\
  $rust
  """,
  "$cship.model $cship.agent",
  "$cship.context_bar $cship.cost $cship.usage_limits",
]

[cship.model]
symbol = "🤖 "
style  = "bold fg:#7aa2f7"

[cship.agent]
symbol = "↳ "
style  = "fg:#9ece6a"

[cship.context_bar]
width              = 10
style              = "fg:#7dcfff"
warn_threshold     = 40.0
warn_style         = "fg:#e0af68"
critical_threshold = 70.0
critical_style     = "bold fg:#f7768e"

[cship.cost]
style              = "fg:#a9b1d6"
warn_threshold     = 2.0
warn_style         = "fg:#e0af68"
critical_threshold = 5.0
critical_style     = "bold fg:#f7768e"
```

The first row uses a multi-line TOML string with `\` line continuations to combine several Starship passthrough modules without spaces between them (Starship handles its own spacing). The second and third rows are pure native cship modules.

---

## $starship_prompt — Full Prompt

In addition to individual Starship module passthrough, CShip supports a special `$starship_prompt` token that renders the **entire configured Starship prompt** in a single call.

**Key difference:** Per-module passthrough (e.g., `$directory`, `$git_branch`) calls `starship module <name>` for each token. The `$starship_prompt` token calls `starship prompt` once, producing the full rendered prompt as configured in your Starship config.

This is useful when you want the complete Starship prompt on one statusline row, followed by CShip-native modules on subsequent rows.

### How it works

- `$starship_prompt` invokes `starship prompt` with `STARSHIP_CONFIG` set to your Starship config file
- Terminal width is read from `$COLUMNS` environment variable (fallback: 80 characters)
- Output is cached for **5 seconds per session** (same as per-module passthrough)
- All `CSHIP_*` environment variables are injected before the subprocess runs, allowing Starship modules to consume Claude Code session data
- If the subprocess fails or Starship is not found, the token renders empty (silent failure)

### Configuration

`$starship_prompt` can be disabled in the config:

```toml
[cship.starship_prompt]
disabled = false  # Set to true to hide this token silently
```

### Example: Two-Row Layout

```toml
[cship]
lines = [
  "$starship_prompt",
  "$cship.model $cship.cost $cship.context_bar $cship.usage_limits",
]

[cship.model]
symbol = "🤖 "
style  = "bold cyan"

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

In this layout, row 1 displays your full Starship-configured prompt (directory, git status, language runtimes, etc.). Row 2 shows Claude Code session data (model, cost, context window usage, API limits).

## Caveats

- Starship must be installed. The CShip curl installer can optionally install it for you.
- Passthrough adds a subprocess call overhead. The 5s cache keeps this negligible after the first render.
- `CSHIP_*` variables reflect the values at render time. Starship modules that consume them will update every 5s (cache TTL).
