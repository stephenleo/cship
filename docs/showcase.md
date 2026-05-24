# Showcase

Ready-to-use `cship.toml` configurations — from the recommended full-featured setup down to a minimal single-line bar. Each can be dropped into `~/.config/cship.toml`.


---

## 1. Hero / Recommended

My personal setup, end to end. Top row: `$starship_prompt` running Starship's [Catppuccin Powerline preset](https://starship.rs/presets/catppuccin-powerline). Bottom row: model, effort, cost, context bar, 7-day per-model usage, extra credits, peak-hours indicator — thresholds escalate cool → warn → critical as budgets fill.

![Hero cship statusline](./examples/01.png)

**`~/.config/cship.toml`**

```toml
[cship]
lines = [
  "$starship_prompt",
  "$cship.model $cship.effort $cship.cost $cship.context_bar $cship.usage_limits $cship.peak_usage",
]

[cship.model]
symbol = " "
style  = "bold cyan"

[cship.effort]
symbol      = "⚡ "
style       = "fg:#7dcfff"
high_style  = "fg:#e0af68"
xhigh_style = "bold fg:#e0af68"
max_style   = "bold fg:#f7768e"

[cship.context_bar]
symbol             = " "
filled_char        = "●"
empty_char         = "○"
format             = "[$symbol$value]($style)"
width              = 10
style              = "fg:#7dcfff"
warn_threshold     = 40.0
warn_style         = "fg:#e0af68"
critical_threshold = 70.0
critical_style     = "bold fg:#f7768e"

[cship.cost]
symbol             = "💰 "
style              = "fg:#a9b1d6"
warn_threshold     = 10
warn_style         = "fg:#e0af68"
critical_threshold = 50
critical_style     = "bold fg:#f7768e"

[cship.usage_limits]
five_hour_format   = " 5h {pct}% ({reset})"
seven_day_format   = " 7d {pct}% ({reset})"
separator          = " "
warn_threshold     = 50.0
warn_style         = "fg:#e0af68"
critical_threshold = 80.0
critical_style     = "bold fg:#f7768e"

[cship.peak_usage]
symbol = "⏰ "
style  = "fg:#e0af68"
```

**`~/.config/starship.toml`** — Starship's [Catppuccin Powerline preset](https://starship.rs/presets/catppuccin-powerline):

```toml
"$schema" = 'https://starship.rs/config-schema.json'

format = """
[](red)\
$os\
$username\
[](bg:peach fg:red)\
$directory\
[](bg:yellow fg:peach)\
$git_branch\
$git_status\
[](fg:yellow bg:green)\
$c\
$rust\
$golang\
$nodejs\
$php\
$java\
$kotlin\
$haskell\
$python\
[](fg:green bg:sapphire)\
$conda\
[](fg:sapphire bg:lavender)\
$time\
[ ](fg:lavender)\
$cmd_duration\
$line_break\
$character"""

palette = 'catppuccin_mocha'

[os]
disabled = false
style = "bg:red fg:crust"
format = "[$symbol ]($style)"

[os.symbols]
Macos = "󰀵"
# (full OS symbol list trimmed for brevity — see the preset link above)

[username]
show_always = false
style_user = "bg:red fg:crust"
style_root = "bg:red fg:crust"
format = '[ $user]($style)'

[directory]
style = "bg:peach fg:crust"
format = "[ $path ]($style)"
truncation_length = 3
truncation_symbol = "…/"

[directory.substitutions]
"Documents" = "󰈙 "
"Downloads" = " "
"Music" = "󰝚 "
"Pictures" = " "
"Developer" = "󰲋 "

[git_branch]
symbol = ""
style = "bg:yellow"
format = '[[ $symbol $branch ](fg:crust bg:yellow)]($style)'

[git_status]
style = "bg:yellow"
format = '[[($all_status$ahead_behind )](fg:crust bg:yellow)]($style)'

[nodejs]
symbol = ""
style = "bg:green"
format = '[[ $symbol( $version) ](fg:crust bg:green)]($style)'

[rust]
symbol = ""
style = "bg:green"
format = '[[ $symbol( $version) ](fg:crust bg:green)]($style)'

[golang]
symbol = ""
style = "bg:green"
format = '[[ $symbol( $version) ](fg:crust bg:green)]($style)'

[python]
symbol = ""
style = "bg:green"
format = '[[ $symbol( $version)(\(#$virtualenv\)) ](fg:crust bg:green)]($style)'

[conda]
symbol = "  "
style = "fg:crust bg:sapphire"
format = '[$symbol$environment ]($style)'
ignore_base = false

[time]
disabled = false
time_format = "%R"
style = "bg:lavender"
format = '[[  $time ](fg:crust bg:lavender)]($style)'

[line_break]
disabled = true

[character]
success_symbol = '[❯](bold fg:green)'
error_symbol = '[❯](bold fg:red)'
vimcmd_symbol = '[❮](bold fg:green)'

[cmd_duration]
show_milliseconds = true
format = "⏳ $duration "
style = "bg:lavender"
show_notifications = true
min_time_to_notify = 45000

# Catppuccin Mocha palette — full palette + frappe/latte/macchiato variants
# omitted for brevity. Grab them from the preset link above.
[palettes.catppuccin_mocha]
rosewater = "#f5e0dc"
flamingo  = "#f2cdcd"
pink      = "#f5c2e7"
mauve     = "#cba6f7"
red       = "#f38ba8"
maroon    = "#eba0ac"
peach     = "#fab387"
yellow    = "#f9e2af"
green     = "#a6e3a1"
teal      = "#94e2d5"
sky       = "#89dceb"
sapphire  = "#74c7ec"
blue      = "#89b4fa"
lavender  = "#b4befe"
text      = "#cdd6f4"
crust     = "#11111b"
```

---

## 2. Minimal

One clean row. Model, cost with colour thresholds, context bar.

![Minimal cship statusline](./examples/03.gif)

```toml
[cship]
lines = ["$cship.model  $cship.cost  $cship.context_bar"]

[cship.cost]
style              = "green"
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

---

## 3. Git-Aware Developer

Two rows: Starship git status on top, Claude session below.

Starship passthrough (`$directory`, `$git_branch`, `$git_status`) requires [Starship](https://starship.rs) to be installed. Each Claude family gets its own colour via `haiku_style` / `sonnet_style` / `opus_style` so you can tell which model you're talking to at a glance.

![Git-aware cship statusline](./examples/04.png)

```toml
[cship]
lines = [
  "$directory $git_branch $git_status",
  "$cship.model  $cship.cost  $cship.context_bar",
]

[cship.model]
symbol       = "🤖 "
haiku_style  = "bold green"
sonnet_style = "bold cyan"
opus_style   = "bold magenta"

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

---

## 4. Cost Guardian

Shows cost, lines changed, rolling API usage limits, and a peak-time indicator. Colour escalates as budgets fill. Displays the cost in GBP via `currency_symbol` + `conversion_rate` — thresholds are evaluated against the converted display value, so restate them in your display currency.

![Cost guardian cship statusline](./examples/05.png)

```toml
[cship]
lines = [
  "$cship.model $cship.cost +$cship.cost.total_lines_added -$cship.cost.total_lines_removed",
  "$cship.context_bar $cship.usage_limits $cship.peak_usage",
]

[cship.model]
style = "bold purple"

[cship.cost]
currency_symbol    = "£"
conversion_rate    = 0.79
warn_threshold     = 0.8     # thresholds are in the display currency (GBP)
warn_style         = "bold yellow"
critical_threshold = 2.4
critical_style     = "bold red"

[cship.context_bar]
width              = 10
warn_threshold     = 40.0
warn_style         = "yellow"
critical_threshold = 70.0
critical_style     = "bold red"

[cship.usage_limits]
ttl                = 60        # cache TTL in seconds; increase if running many concurrent sessions
five_hour_format   = "5h {pct}%"
seven_day_format   = "7d {pct}%"
separator          = " "
warn_threshold     = 70.0
warn_style         = "bold yellow"
critical_threshold = 90.0
critical_style     = "bold red"

[cship.peak_usage]
style = "bold yellow"
```

---

## 5. Material Hex

Every style value is a `fg:#rrggbb` hex colour — no named colours anywhere. Amber warns, coral criticals. Uses `filled_char` / `empty_char` to swap the default blocky bar for dotted glyphs (`●` / `○`).

![Material Hex cship statusline](./examples/06.png)

```toml
[cship]
lines = [
  "$cship.model $cship.cost",
  "$cship.context_bar $cship.usage_limits",
]

[cship.model]
style = "fg:#c3e88d"

[cship.cost]
style              = "fg:#82aaff"
warn_threshold     = 2.0
warn_style         = "fg:#ffcb6b"
critical_threshold = 6.0
critical_style     = "bold fg:#f07178"

[cship.context_bar]
width              = 10
filled_char        = "●"
empty_char         = "○"
style              = "fg:#89ddff"
warn_threshold     = 40.0
warn_style         = "fg:#ffcb6b"
critical_threshold = 70.0
critical_style     = "bold fg:#f07178"

[cship.usage_limits]
five_hour_format   = "5h {pct}%"
seven_day_format   = "7d {pct}%"
separator          = " "
warn_threshold     = 70.0
warn_style         = "fg:#ffcb6b"
critical_threshold = 90.0
critical_style     = "bold fg:#f07178"
```

---

## 6. Tokyo Night

Three-row layout for polyglot developers. Starship handles language runtimes and git; cship handles session data. Styled with the [Tokyo Night](https://github.com/folke/tokyonight.nvim) colour palette.

![Tokyo Night cship statusline](./examples/07.png)

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
symbol             = "💰 "
style              = "fg:#a9b1d6"
warn_threshold     = 2.0
warn_style         = "fg:#e0af68"
critical_threshold = 5.0
critical_style     = "bold fg:#f7768e"

[cship.usage_limits]
five_hour_format   = "⌛ 5h {pct}%"
seven_day_format   = "📅 7d {pct}%"
separator          = " "
warn_threshold     = 70.0
warn_style         = "fg:#e0af68"
critical_threshold = 90.0
critical_style     = "bold fg:#f7768e"
```

---

## 7. Nerd Fonts

Requires a [Nerd Font](https://www.nerdfonts.com) in your terminal. Icons are embedded as `symbol` values on each module and as literal characters in the format string for Starship passthrough rows. Enables `show_per_model = true` to append the 7-day per-model breakdown to `$cship.usage_limits`, with a custom `sonnet_format` row.

![Nerd Fonts cship statusline](./examples/08.png)

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
  "$cship.model $cship.cost $cship.context_bar $cship.usage_limits",
]

[cship.model]
symbol = " " # nf-fa-microchip
style  = "bold fg:#7aa2f7"

[cship.cost]
symbol             = "💰 "
style              = "fg:#a9b1d6"
warn_threshold     = 2.0
warn_style         = "fg:#e0af68"
critical_threshold = 5.0
critical_style     = "bold fg:#f7768e"

[cship.context_bar]
symbol             = " " # nf-fa-database
format             = "[$symbol$value]($style)"
width              = 10
style              = "fg:#7dcfff"
warn_threshold     = 40.0
warn_style         = "fg:#e0af68"
critical_threshold = 70.0
critical_style     = "bold fg:#f7768e"

[cship.usage_limits]
five_hour_format   = "⌛ 5h {pct}%"
seven_day_format   = "📅 7d {pct}%"
sonnet_format      = "🎼 {pct}%"
separator          = " "
show_per_model     = true
warn_threshold     = 70.0
warn_style         = "fg:#e0af68"
critical_threshold = 90.0
critical_style     = "bold fg:#f7768e"
```

---

## Submit Your Config

Have a beautiful CShip setup? Share it with the community!

Open a pull request to [stephenleo/cship](https://github.com/stephenleo/cship) adding your config to this page.

Include:
- A screenshot or GIF of your statusline in action
- Your full annotated `cship.toml`
- A short description of the design choices
