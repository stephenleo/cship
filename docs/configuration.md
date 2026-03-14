# Configuration Reference

CShip is configured via a TOML file. The config discovery order is:

1. `--config <path>` flag (if provided)
2. `cship.toml` found walking up from the workspace directory
3. `starship.toml` found walking up from the workspace directory
4. `~/.config/cship.toml` (global)
5. `~/.config/starship.toml` (global)
6. Built-in defaults (no config required)

The recommended file is `~/.config/cship.toml`.

## Layout

The `[cship]` section controls the overall layout:

```toml
[cship]
lines = [
  "$cship.model  $cship.cost  $cship.context_bar",
  "$directory $git_branch",
]
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `lines` | `string[]` | `[]` | Each element is one statusline row. Supports `$cship.<module>` tokens and Starship passthrough tokens. |
| `format` | `string` | — | Starship-compatible format string. Split on `$line_break` to produce multiple rows. Takes priority over `lines` when both are set. |

## Format String Syntax

All `format` fields (and the `lines` strings) use Starship-compatible format syntax:

| Syntax | Meaning |
|--------|---------|
| `$value` | Interpolate the variable named `value` |
| `$symbol` | Interpolate the module's configured symbol |
| `[text]($style)` | Render `text` with the ANSI style `$style` |
| `($group)` | Conditional group — renders only if all variables inside it are non-empty |
| `$line_break` | Insert a newline (for use in `format`, not `lines`) |

### Style values

Styles follow the same syntax as Starship:

```
"bold"
"italic"
"underline"
"bold green"
"fg:#c792ea"
"bold fg:#ff5370 bg:#1a1a2e"
"fg:208"              # 256-color index
```

Supported named colors: `black`, `red`, `green`, `yellow`, `blue`, `purple`, `cyan`, `white`, and their `bright_*` variants.
Hex colors: `#RRGGBB` (24-bit) or `#RGB` (shorthand).
256-color: numeric index `0`–`255`.

## Common Module Fields

All native CShip modules share these optional fields:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `disabled` | `bool` | `false` | Set `true` to hide this module entirely (silent `None`) |
| `style` | `string` | module default | ANSI style for the rendered output |
| `symbol` | `string` | module default | Prefix symbol prepended to the value |
| `format` | `string` | `"[$symbol$value]($style)"` | Controls how symbol, value, and style are combined |

---

## `[cship.model]` — Model Name

Displays the active Claude model name.

**Token:** `$cship.model`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `disabled` | `bool` | `false` | Hide this module |
| `style` | `string` | `"bold"` | ANSI style |
| `symbol` | `string` | `""` | Prefix symbol |
| `format` | `string` | `"[$symbol$value]($style)"` | Format string; `$value` = model display name |

**Variables:** `$value` (display name, e.g. `claude-sonnet-4-5`), `$symbol`, `$style`

```toml
[cship.model]
symbol = "🤖 "
style  = "bold fg:#7aa2f7"
```

---

## `[cship.cost]` — Session Cost

Displays total session cost in USD. Supports threshold-based colour escalation.

**Token:** `$cship.cost`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `disabled` | `bool` | `false` | Hide this module |
| `style` | `string` | `"green"` | Base ANSI style |
| `symbol` | `string` | `""` | Prefix symbol |
| `format` | `string` | `"[$symbol$value]($style)"` | Format string |
| `warn_threshold` | `float` | — | USD amount at which style switches to `warn_style` |
| `warn_style` | `string` | `"yellow"` | Style applied when cost ≥ `warn_threshold` |
| `critical_threshold` | `float` | — | USD amount at which style switches to `critical_style` |
| `critical_style` | `string` | `"bold red"` | Style applied when cost ≥ `critical_threshold` |

**Variables:** `$value` (e.g. `$1.23`), `$symbol`, `$style`

```toml
[cship.cost]
warn_threshold     = 2.0
warn_style         = "yellow"
critical_threshold = 5.0
critical_style     = "bold red"
```

### Cost sub-field modules

Individual cost metrics can also be referenced directly:

| Token | Description |
|-------|-------------|
| `$cship.cost.total_cost_usd` | Total cost in USD |
| `$cship.cost.total_duration_ms` | Total wall-clock duration (ms) |
| `$cship.cost.total_api_duration_ms` | Total API time (ms) |
| `$cship.cost.total_lines_added` | Lines added this session |
| `$cship.cost.total_lines_removed` | Lines removed this session |

Each sub-field has its own `[cship.cost.<name>]` section with the same fields as the parent (`style`, `symbol`, `format`, `warn_threshold`, `warn_style`, `critical_threshold`, `critical_style`, `disabled`).

```toml
[cship.cost.total_lines_added]
style = "green"
warn_threshold = 500
warn_style = "yellow"
```

---

## `[cship.context_bar]` — Context Window Progress Bar

Renders a visual ASCII progress bar showing context window usage.

**Token:** `$cship.context_bar`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `disabled` | `bool` | `false` | Hide this module |
| `style` | `string` | `"green"` | Base ANSI style |
| `symbol` | `string` | `""` | Prefix symbol |
| `format` | `string` | `"[$symbol$value]($style)"` | Format string |
| `width` | `integer` | `10` | Number of characters in the bar |
| `warn_threshold` | `float` | — | % at which style switches to `warn_style` |
| `warn_style` | `string` | `"yellow"` | Style at warn level |
| `critical_threshold` | `float` | — | % at which style switches to `critical_style` |
| `critical_style` | `string` | `"bold red"` | Style at critical level |

```toml
[cship.context_bar]
width              = 10
symbol             = " "
warn_threshold     = 40.0
warn_style         = "yellow"
critical_threshold = 70.0
critical_style     = "bold red"
```

---

## `[cship.context_window]` — Context Window Details

Displays detailed context window token counts. The parent token shows used/total summary.

**Token:** `$cship.context_window`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `disabled` | `bool` | `false` | Hide this module |
| `style` | `string` | — | ANSI style |
| `warn_threshold` | `float` | — | % threshold for warning |
| `warn_style` | `string` | — | Style at warn level |
| `critical_threshold` | `float` | — | % threshold for critical |
| `critical_style` | `string` | — | Style at critical level |
| `format` | `string` | — | Format string |

### Context window sub-field modules

| Token | Description |
|-------|-------------|
| `$cship.context_window.used_percentage` | % of context window used |
| `$cship.context_window.remaining_percentage` | % of context window remaining |
| `$cship.context_window.size` | Total context window size (tokens) |
| `$cship.context_window.total_input_tokens` | Total input tokens this session |
| `$cship.context_window.total_output_tokens` | Total output tokens this session |
| `$cship.context_window.current_usage_input_tokens` | Current turn input tokens |
| `$cship.context_window.current_usage_output_tokens` | Current turn output tokens |
| `$cship.context_window.current_usage_cache_creation_input_tokens` | Cache creation tokens |
| `$cship.context_window.current_usage_cache_read_input_tokens` | Cache read tokens |

Each sub-field supports `style`, `symbol`, `format`, `warn_threshold`, `warn_style`, `critical_threshold`, `critical_style`, `disabled`, and `invert_threshold`.

**`invert_threshold`** — set to `true` for metrics where *low* is bad (e.g. `remaining_percentage`): the warning fires when value falls *below* the threshold.

```toml
[cship.context_window.remaining_percentage]
warn_threshold    = 30.0
warn_style        = "yellow"
critical_threshold = 10.0
critical_style    = "bold red"
invert_threshold  = true
```

---

## `[cship.vim]` — Vim Mode

Displays the current vim mode (NORMAL, INSERT, VISUAL, etc.). Returns nothing when vim mode is inactive.

**Token:** `$cship.vim`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `disabled` | `bool` | `false` | Hide this module |
| `style` | `string` | — | Base ANSI style |
| `symbol` | `string` | — | Prefix symbol |
| `format` | `string` | — | Format string |
| `normal_style` | `string` | `"bold green"` | Style applied in NORMAL mode |
| `insert_style` | `string` | `"bold blue"` | Style applied in INSERT mode |

```toml
[cship.vim]
normal_style = "bold green"
insert_style = "bold blue"
```

---

## `[cship.agent]` — Agent Name

Displays the active sub-agent name. Returns nothing when no agent is running.

**Token:** `$cship.agent`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `disabled` | `bool` | `false` | Hide this module |
| `style` | `string` | — | ANSI style |
| `symbol` | `string` | `"↳ "` | Prefix symbol |
| `format` | `string` | — | Format string |

```toml
[cship.agent]
symbol = "↳ "
style  = "fg:#9ece6a"
```

---

## `[cship.session]` — Session Identity

Displays session metadata (session ID, transcript path, output style, cship version).

**Token:** `$cship.session`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `disabled` | `bool` | `false` | Hide this module |
| `style` | `string` | — | ANSI style |
| `symbol` | `string` | — | Prefix symbol |
| `format` | `string` | — | Format string |

---

## `[cship.workspace]` — Workspace Directory

Displays the current working directory or project directory.

**Token:** `$cship.workspace`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `disabled` | `bool` | `false` | Hide this module |
| `style` | `string` | — | ANSI style |
| `symbol` | `string` | — | Prefix symbol |
| `format` | `string` | — | Format string |

---

## `[cship.usage_limits]` — API Usage Limits

Displays 5-hour and 7-day API utilization percentages with time-to-reset. Fetches from the Anthropic API using your OAuth token (stored in the OS credential store). Results are cached for the configured TTL (default 60s) or until the reset window passes.

**Token:** `$cship.usage_limits`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `disabled` | `bool` | `false` | Hide this module |
| `style` | `string` | — | Base ANSI style |
| `format` | `string` | — | Format string |
| `five_hour_format` | `string` | `"5h: {pct}% resets in {reset}"` | Format for the 5h window; `{pct}` and `{reset}` are placeholders |
| `seven_day_format` | `string` | `"7d: {pct}% resets in {reset}"` | Format for the 7d window |
| `separator` | `string` | `" \| "` | String placed between 5h and 7d sections |
| `warn_threshold` | `float` | — | % at which style switches to `warn_style` |
| `warn_style` | `string` | `"yellow"` | Style at warn level |
| `critical_threshold` | `float` | — | % at which style switches to `critical_style` |
| `critical_style` | `string` | `"bold red"` | Style at critical level |
| `ttl` | `integer` | `60` | Cache refresh interval in seconds. Increase to reduce API pressure when running multiple concurrent sessions. |

**Prerequisites:** On Linux/WSL2, install `libsecret-tools` and store your OAuth token with `secret-tool`. See [FAQ](/faq#usage-limits-linux) for setup instructions.

```toml
[cship.usage_limits]
ttl                = 300       # 5 minutes; increase if you run many concurrent sessions
five_hour_format   = "5h {pct}%({reset})"
seven_day_format   = "7d {pct}%({reset})"
separator          = " "
warn_threshold     = 70.0
warn_style         = "bold yellow"
critical_threshold = 90.0
critical_style     = "bold red"
```
