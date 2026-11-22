# FAQ

## Does cship conflict with Starship?

**No.** cship and Starship serve different purposes:

- **Starship** renders your *shell prompt* (the line before your commands in the terminal).
- **CShip** renders Claude Code's *statusline* (the line shown at the bottom of the Claude Code UI, inside the AI session).

They operate in completely separate contexts and never interfere with each other. In fact, they work better together — CShip can invoke any Starship module as a passthrough, so your Starship-configured git status, directory, and language runtime indicators can appear right in your Claude Code statusline.

---

## Why not just use Starship for the Claude Code statusline?

Starship is a shell prompt renderer — it reads shell context (environment variables, git state, file system). It has no knowledge of Claude Code internals like session cost, context window usage, model name, or API limits.

Claude Code exposes this session data via a JSON feed piped to the statusline command on every render cycle. CShip is purpose-built to consume that JSON feed and render it with the same TOML-based customization model you already know from Starship.

In short: Starship knows about your *shell*, CShip knows about your *Claude Code session*. Together they cover everything.

---

## How do I debug my config?

Run `cship explain`:

```sh
cship explain
```

This shows:
- Which config file was loaded (and from where)
- Each module's current rendered value
- Any warnings about missing data, misconfiguration, or disabled modules

`cship explain` reads from `~/.config/cship/sample-context.json` if no stdin is piped, so it works outside of a Claude Code session. On first run, CShip auto-creates this file with representative values.

---

## How do I set up usage limits on Linux/WSL2? {#usage-limits-linux}

The CShip `usage_limits` module fetches data from the Anthropic API using your Claude Code OAuth token, which is stored in the OS credential store.

**Linux/WSL2:** No manual setup required. Claude Code writes its credential to `~/.claude/.credentials.json` when you log in (or to `$CLAUDE_CONFIG_DIR/.credentials.json` when that is set; see **Multiple accounts** below), and CShip reads the OAuth token from that file. If the file is missing, log in to Claude Code again (`/login`).

Only when that file is absent does CShip fall back to `secret-tool` (from `libsecret-tools`), looking up the service `Claude Code-credentials`. The stored secret must be the full contents of `.credentials.json` (the JSON containing `claudeAiOauth.accessToken`), not the bare token. Claude Code itself does not write to the keyring on Linux, so most setups never use this fallback.

Run `cship explain` to verify the token is found and the usage limits module is rendering.

**macOS:** CShip reads the OAuth token from the macOS Keychain automatically — no manual setup required.

**Multiple accounts (`CLAUDE_CONFIG_DIR`):** CShip reads the credential of the session's own config dir, the same way Claude Code stores it: `$CLAUDE_CONFIG_DIR/.credentials.json` on Linux/WSL2 and Windows, and the Keychain item `Claude Code-credentials-<hash>` on macOS (unsuffixed `Claude Code-credentials` when `CLAUDE_CONFIG_DIR` is unset). The Linux `secret-tool` fallback uses the same suffixed name. So `usage_limits` and `account` follow whichever account the session runs as.

---

## What do `{pace}` and `{active}` mean in the usage_limits format strings?

**`{pace}`** is the signed headroom versus linear consumption of the current window. If you were perfectly on pace to land at 100% exactly when the window resets, `{pace}` would be `0%`. A positive value (e.g. `+20%`) means you have 20 percentage points of headroom over the linear pace — you can comfortably keep going. A negative value (e.g. `-15%`) means you're 15 points ahead of pace and will hit the limit before reset unless you slow down. It renders as `?` when the reset time is unknown.

`{pace}` is available in `five_hour_format`, `seven_day_format`, and each per-model format (`opus_format`, `sonnet_format`, `cowork_format`, `oauth_apps_format`).

**`{active}`** is only available in `extra_usage_format`. It renders `⚡` when either the 5h or 7d window is at 100% — meaning fresh requests are now drawing down extra credits rather than plan credits — and `💤` otherwise. Pair it with `{used}` / `{limit}` (dollars) and `{pct}` (utilization of the monthly extra-credit cap) for a one-glance view of supplemental spend.

```toml
[cship.usage_limits]
five_hour_format   = "5h {pct}% ({pace})"
seven_day_format   = "7d {pct}% ({pace})"
extra_usage_format = "{active} ${used}/${limit}"
```

---

## How do I show the session and weekly limits on separate lines, each with its own bar and color?

`$cship.usage_limits` renders both windows together and colors the whole segment off the *higher* of the two (so a hot 5h window turns the 7d text red as well). The per-window tokens render each window on its own and color it off **its own** utilization:

- `$cship.usage_limits.session` — the 5-hour window (alias `.five_hour`)
- `$cship.usage_limits.weekly` — the 7-day window (alias `.seven_day`)

On an account with no 5-hour limit, `$cship.usage_limits` shows only the 7d window and `$cship.usage_limits.session` renders nothing. `cship explain` reports this as "this account has no 5-hour usage window".

Combine them with the `{bar}` placeholder (available in `five_hour_format` / `seven_day_format`, sized by `bar_width`) to get an independent progress bar per window:

```toml
[cship]
lines = ["$cship.usage_limits.session", "$cship.usage_limits.weekly"]

[cship.usage_limits]
five_hour_format   = "5h {bar} {pct}%"
seven_day_format   = "7d {bar} {pct}%"
bar_width          = 20
warn_threshold     = 50
warn_style         = "yellow"
critical_threshold = 80
critical_style     = "bold red"
```

Now the 5h bar can be green while the 7d bar is red (or vice versa), each escalating on its own percentage.

---

## I'm on a Claude Enterprise plan and `cship.usage_limits` shows nothing — why?

Claude Enterprise reports usage through `extra_usage` (monthly credit pool)
rather than the standard 5-hour and 7-day windows used for Pro and Max.
cship now renders the `extra_usage` line directly when standard fields are
absent, with threshold styling applied to monthly credit utilization.

If you still see no output, run `cship explain` — it will surface the
specific reason (missing credential, expired token, or extra-credit billing
not enabled on your Enterprise account).

---

## Why does `$cship.effort` show nothing?

The `effort` module renders the session's reasoning effort level, which Claude Code only sends when the **active model supports the effort parameter**. If you're on a model without effort support, the field is absent and the module renders nothing — switch to a model that supports `/effort` (or run `cship explain` to confirm whether an `effort` value is present in the context).

The value reflects live `/effort` changes, so it updates on the next render after you change it mid-session.

---

## How do I set up the `cship.account` module, and why is it empty? {#account-setup}

`cship.account` shows which Anthropic account you're signed in to (work vs
personal). It reads your account profile from the OAuth `/api/oauth/profile`
endpoint using the OAuth token in your OS credential store — the same
credential `usage_limits` uses — so the [Linux/WSL2 notes above](#usage-limits-linux)
apply here too. The profile is cached for
24 hours and re-fetched automatically when you switch accounts.

If the module renders nothing, run `cship explain cship.account`. It
distinguishes the three causes: no credential found (authenticate in Claude
Code), credential present but the profile fetch failed (token may have expired,
or the API was unreachable — re-authenticate), or a malformed credential.

To map raw organization names to short labels:

```toml
[cship.account.labels]
"Fulcrum Genomics" = "work"
"Personal Workspace" = "personal"
```

---

## Can I right-align modules? (the `$fill` token)

Yes — use `$fill`. It expands to fill the remaining horizontal space, pushing whatever follows it to the right; multiple `$fill` on one line split the space evenly (so the last segment is flush-right). It mirrors Starship's `fill` module and is configured via `[cship.fill]`.

```toml
[cship]
lines = ["$cship.model $cship.cost $fill $cship.context_bar $fill $cship.usage_limits"]

[cship.fill]
symbol = "·"
style  = "fg:#414868"
```

**Important caveat:** Claude Code does not tell the statusline how wide the terminal is ([claude-code#22115](https://github.com/anthropics/claude-code/issues/22115)). cship recovers the width best-effort by reading the controlling terminal of a parent process — which works in **macOS/Linux terminals** but not on **Windows** or the **web/desktop apps**, where it falls back to `[cship] width` (if set) and then 80 columns. If alignment is off by a few columns, tune `[cship] width_offset` (default 3) or pin `[cship] width` to your terminal's width.

---

## How does the peak-time indicator handle time zones and DST?

The `peak_usage` module checks whether the current time falls within the configured peak window in **US Pacific time**. It computes the UTC→Pacific offset internally — PDT (UTC−7) from the second Sunday of March through the first Sunday of November, PST (UTC−8) the rest of the year.

There are no dependencies on system locale or `TZ` environment variable; the DST boundaries are calculated from the UTC date using Tomohiko Sakamoto's day-of-week algorithm.

**Defaults:** Mon–Fri, 07:00–17:00 Pacific. To customise:

```toml
[cship.peak_usage]
start_hour = 9   # 9 AM Pacific
end_hour   = 18  # 6 PM Pacific (exclusive)
```

Set `end_hour = 24` to mean "through end of day". Weekends always return nothing.

---

## Why is my cost or context not updating?

**Cost and context window** data comes from Claude Code's JSON feed, which is updated on every statusline render (every time Claude Code calls `cship`). If these values appear stuck, check:

- The statusline command is correctly set in `~/.claude/settings.json`:
  ```json
  { "statusLine": { "type": "command", "command": "cship" } }
  ```
- Run `cship explain` to confirm cship is receiving a valid JSON context.

**Usage limits** data is fetched from the Anthropic API and shared between sessions:
- **Refresh interval:** `[cship.usage_limits] ttl` (default **60 seconds**). All `cship` processes that use the same Claude config directory share one fetch schedule and one state file, so running more sessions doesn't multiply API calls, and a new session within the interval shows the last fetched data instead of fetching again.
- **Where it lives:** `<CLAUDE_CONFIG_DIR or ~/.claude>/cship/cache/usage-limits-state-v3.json`, plus a `usage-limits-fetch.lock` file next to it. Both are owner-only on Unix. Deleting them is safe; cship recreates them on the next fetch.
- **Errors back off instead of retrying every render.** A rate limit (HTTP 429) waits for the server's `Retry-After`, or doubles from `ttl` when there is none, up to 15 minutes. Any other unexpected HTTP status waits 30 seconds and doubles with each repeat, up to 5 minutes. A network error or an unreadable response retries after 30 seconds. A rejected token (HTTP 401) is retried after 15 minutes, or at once when you log in again and the token changes. None of these retries come sooner than `ttl`, and a `ttl` longer than a limit above takes its place. Last known data keeps rendering in the meantime.
- **Unreadable credential:** if the OAuth token cannot be read for a render (for example a keychain lookup that times out), cship keeps showing the last fetched data for up to 15 minutes and does not call the API.
- **Window resets:** when a usage window is about to reset, the next fetch is scheduled for a few seconds after the reset so the new window appears promptly. A reset never causes two fetches closer than 30 seconds apart (or `ttl`, if that is shorter).
- **Turning it off:** with `[cship.usage_limits] stdin_only = true`, cship shows only the 5h/7d values from Claude Code's session JSON and never reads the credential or calls the API. Per-model and extra-usage values need the API, so they don't render in this mode.
- **Session-only:** the OAuth fetch only runs for a normal Claude Code session (one that provides a `transcript_path`), so rendering with `cship` by hand on arbitrary input never calls the API.
- If the data seems stale, check that your OAuth token is valid (re-login to Claude Code if needed).

Run `cship explain` to see the usage limits value being rendered and a hint when nothing rendered. For the reason a fetch failed or was skipped, run a render with `RUST_LOG=warn` set.

## Why are some characters missing from my path or model name?

cship strips terminal control characters (ESC, BEL, the C0/C1 ranges, and DEL — including tab, carriage return, and newline) from untrusted session JSON fields such as `cwd`, `transcript_path`, `model`, and `workspace` before rendering. This is a security measure: it prevents a malicious directory or model name from injecting raw escape sequences that could spoof your terminal title, move the cursor, or write to the clipboard via OSC 52 ([CWE-150](https://cwe.mitre.org/data/definitions/150.html)).

Normal values are never affected — only control bytes are removed. If a path or name shows up with characters missing, it contained control bytes, which are stripped by design and cannot be disabled.
