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

**Prerequisites:**

1. Install `libsecret-tools`:
   ```sh
   # Debian/Ubuntu/WSL2
   sudo apt-get install -y libsecret-tools
   ```

2. Store your Claude Code OAuth token with `secret-tool`:
   ```sh
   secret-tool store --label="Claude Code" service "claude.ai" account "claude-code"
   ```
   When prompted for a password, paste your OAuth token.

   You can find your token in `~/.claude/.credentials.json` (look for the `access_token` field) or by logging out and back into Claude Code.

3. Run `cship explain` to verify the token is found and the usage limits module is rendering.

**macOS:** CShip reads the OAuth token from the macOS Keychain automatically — no manual setup required.

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

**Usage limits** data is cached:
- Cache TTL: **configurable (default 60 seconds)**, or until the rate-limit reset window passes (whichever comes first). Set `[cship.usage_limits] ttl` to increase the cache interval if you run many concurrent sessions.
- The first call in a session always fetches fresh data; subsequent calls within the configured TTL return the cached value.
- If the cache seems stale, check that your OAuth token is valid (re-login to Claude Code if needed).

You can see the current cache state by running `cship explain` — it shows the usage limits value being rendered and any warnings if the API call failed.
