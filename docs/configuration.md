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

Displays total session cost with threshold-based colour escalation. The display currency and conversion rate are configurable; the underlying value is always `total_cost_usd` (USD). Thresholds are evaluated against the converted display value (`total_cost_usd × conversion_rate`); configure them in your display currency.

**Token:** `$cship.cost`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `disabled` | `bool` | `false` | Hide this module |
| `style` | `string` | `"green"` | Base ANSI style |
| `symbol` | `string` | `""` | Prefix symbol |
| `format` | `string` | `"[$symbol$value]($style)"` | Format string |
| `warn_threshold` | `float` | — | Display-currency amount at which style switches to `warn_style` |
| `warn_style` | `string` | `"yellow"` | Style applied when cost ≥ `warn_threshold` |
| `critical_threshold` | `float` | — | Display-currency amount at which style switches to `critical_style` |
| `critical_style` | `string` | `"bold red"` | Style applied when cost ≥ `critical_threshold` |
| `currency_symbol` | `string` | `"$"` | Symbol prepended to the displayed value (e.g. `"£"`, `"€"`) |
| `conversion_rate` | `float` | `1.0` | Multiplier applied to `total_cost_usd` before display; thresholds are evaluated against the converted value, so express them in your display currency |

**Variables:** `$value` (e.g. `$1.23` or `£0.97` with a custom currency), `$symbol`, `$style`

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
| `filled_char` | `string` | `"█"` | Character used for the filled portion. Any Unicode character is allowed. |
| `empty_char` | `string` | `"░"` | Character used for the empty portion. Any Unicode character is allowed. |
| `warn_threshold` | `float` | — | % at which style switches to `warn_style` |
| `warn_style` | `string` | `"yellow"` | Style at warn level |
| `critical_threshold` | `float` | — | % at which style switches to `critical_style` |
| `critical_style` | `string` | `"bold red"` | Style at critical level |
| `empty_style` | `string` | — | Style for the bar when no context data is available (e.g., `"dim"`) |

```toml
[cship.context_bar]
width              = 10
symbol             = " "
warn_threshold     = 40.0
warn_style         = "yellow"
critical_threshold = 70.0
critical_style     = "bold red"
```

For a circle-style bar (`●●●●○○○○○○40%`):

```toml
[cship.context_bar]
filled_char = "●"
empty_char  = "○"
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
| `$cship.context_window.used_tokens` | Token counts computed from `current_usage` (`input_tokens + cache_creation + cache_read`) and `context_window_size`. Percentage from the API's `used_percentage` field — may not equal `used/total × 100` as the API includes overhead (e.g. system prompt, tool schemas). Renders as e.g. `8%(79k/1000k)`. Returns nothing before first API call. |

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

Displays 5-hour and 7-day API utilization percentages with time-to-reset.

**Data sources (in priority order):**

1. **stdin `rate_limits`** — Claude Code (v2.1+) sends `rate_limits` directly in the session JSON for Pro/Max subscribers. When present, cship uses this data immediately with zero latency and no credential setup required.
2. **OAuth API fetch** — Falls back to fetching from `https://api.anthropic.com/api/oauth/usage` using your OAuth token (stored in the OS credential store). Results are cached for the configured TTL (default 60s).

**Tokens:**

| Token | Renders |
|-------|---------|
| `$cship.usage_limits` | Combined: 5h + 7d + per-model (when present) + extra usage (when enabled) |
| `$cship.usage_limits.per_model` | Only the per-model 7-day breakdown (opus/sonnet/cowork/oauth) |
| `$cship.usage_limits.opus` | 7-day Opus utilization only |
| `$cship.usage_limits.sonnet` | 7-day Sonnet utilization only |
| `$cship.usage_limits.cowork` | 7-day Cowork utilization only |
| `$cship.usage_limits.oauth_apps` | 7-day OAuth-apps utilization only |
| `$cship.usage_limits.extra_usage` | Extra-credits display (only when the account has extra usage enabled) |

The sub-tokens let you place sections independently in your `lines` layout — e.g., keep the 5h/7d pair on one row and push per-model onto a second row.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `disabled` | `bool` | `false` | Hide this module |
| `style` | `string` | — | Base ANSI style |
| `format` | `string` | — | Reserved; use the per-section formats below |
| `five_hour_format` | `string` | `"5h: {pct}% resets in {reset}"` | Format for the 5h window |
| `seven_day_format` | `string` | `"7d: {pct}% resets in {reset}"` | Format for the 7d window |
| `opus_format` | `string` | `"opus {pct}%"` | Format for the 7-day Opus section |
| `sonnet_format` | `string` | `"sonnet {pct}%"` | Format for the 7-day Sonnet section |
| `cowork_format` | `string` | `"cowork {pct}%"` | Format for the 7-day Cowork section |
| `oauth_apps_format` | `string` | `"oauth {pct}%"` | Format for the 7-day OAuth-apps section |
| `extra_usage_format` | `string` | `"{active} extra: {pct}% (${used}/${limit})"` | Format for the extra-usage section |
| `separator` | `string` | `" \| "` | String placed between sections |
| `warn_threshold` | `float` | — | % at which style switches to `warn_style` |
| `warn_style` | `string` | `"yellow"` | Style at warn level |
| `critical_threshold` | `float` | — | % at which style switches to `critical_style` |
| `critical_style` | `string` | `"bold red"` | Style at critical level |
| `ttl` | `integer` | `60` | Cache refresh interval in seconds. Increase to reduce API pressure when running multiple concurrent sessions. |

**Placeholders** (available in all `*_format` strings):

| Placeholder | Meaning |
|-------------|---------|
| `{pct}` | Percentage used as integer (e.g. `23`) |
| `{remaining}` | Percentage remaining as integer (e.g. `77`) |
| `{reset}` | Time-until-reset string (e.g. `4h12m`) |
| `{pace}` | Signed headroom vs linear consumption — `+20%` (under pace), `-15%` (over pace), or `?` when unknown |

**Additional placeholders in `extra_usage_format`:**

| Placeholder | Meaning |
|-------------|---------|
| `{used}` | Extra credits consumed, in dollars (e.g. `12.34`) |
| `{limit}` | Monthly extra-credit limit, in dollars (e.g. `50`) |
| `{active}` | `⚡` when 5h or 7d utilization is at 100% (actively consuming extra credits), `💤` otherwise |

**Prerequisites:** If Claude Code sends `rate_limits` in its session JSON (v2.1+, Pro/Max plans), no setup is needed for the 5h/7d totals. Per-model breakdowns and extra-usage data always come from the OAuth API — on Linux/WSL2 install `libsecret-tools` and store your OAuth token with `secret-tool`. See [FAQ](/faq#usage-limits-linux) for setup instructions.

```toml
[cship.usage_limits]
ttl                = 300       # 5 minutes; increase if you run many concurrent sessions
five_hour_format   = "5h {pct}% ({pace}, {reset})"
seven_day_format   = "7d {pct}% ({pace}, {reset})"
opus_format        = "opus {pct}%"
sonnet_format      = "sonnet {pct}%"
extra_usage_format = "{active} extra {pct}% (${used}/${limit})"
separator          = " | "
warn_threshold     = 70.0
warn_style         = "bold yellow"
critical_threshold = 90.0
critical_style     = "bold red"
```

### Composing with sub-tokens

Place per-model and extra usage on a separate line from the 5h/7d summary:

```toml
[cship]
lines = [
  "$cship.model $cship.cost",
  "$cship.usage_limits",
  "$cship.usage_limits.per_model $cship.usage_limits.extra_usage",
]
```

Each sub-token returns nothing when its data is absent, so the row collapses cleanly on accounts without a given breakdown.

### Hiding a usage period

To hide one of the two main windows, set its format **and** the separator to empty strings. For example, to show only the 5-hour window:

```toml
[cship.usage_limits]
seven_day_format = ""
separator        = ""
```

To show only the 7-day window:

```toml
[cship.usage_limits]
five_hour_format = ""
separator        = ""
```

Setting both formats to `""` effectively hides the combined token. Per-model sections render only when the API returns data for that model, so they disappear automatically on accounts that don't expose a given breakdown.

---

## `[cship.peak_usage]` — Peak-Time Indicator

Shows when Anthropic's peak-time rate limiting is likely active, based on current time relative to US Pacific business hours. Returns nothing outside peak hours so the indicator disappears entirely.

The check is purely time-based (Mon–Fri, default 07:00–17:00 Pacific) — no network calls.

**Token:** `$cship.peak_usage`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `disabled` | `bool` | `false` | Hide this module |
| `style` | `string` | — | ANSI style |
| `symbol` | `string` | `"⏰ "` | Prefix symbol |
| `format` | `string` | `"[$symbol$value]($style)"` | Format string; `$value` = `Peak` |
| `start_hour` | `integer` | `7` | Start of peak window in US Pacific time (0–23) |
| `end_hour` | `integer` | `17` | End of peak window in US Pacific time, exclusive (0–24). Use `24` to mean through end of day |

**Variables:** `$value` (`Peak`), `$symbol`, `$style`

US Pacific DST is handled automatically — PDT (UTC−7) from the second Sunday of March to the first Sunday of November, PST (UTC−8) otherwise.

```toml
[cship.peak_usage]
symbol     = "⏰ "
style      = "fg:#e0af68"
start_hour = 7
end_hour   = 17
```

---

## `[cship.starship_prompt]` — Full Starship Prompt

Renders your entire Starship-configured prompt in a single call. Unlike per-module passthrough (e.g., `$directory`, `$git_branch`), this token invokes `starship prompt` to produce the complete rendered prompt with all configured modules.

**Token:** `$starship_prompt`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `disabled` | `bool` | `false` | Set to `true` to hide this token silently |

```toml
[cship.starship_prompt]
disabled = false
```
