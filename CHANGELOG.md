# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

## [1.6.0] - 2026-05-03

### Added
- Added `filled_char` and `empty_char` config to `cship.context_bar` for fully customizable progress-bar glyphs ([@HotThoughts](https://github.com/HotThoughts), [#155](https://github.com/stephenleo/cship/pull/155))
- Added `extra_usage_format` and per-model format fields to `cship.usage_limits`, with pace tracking, an active-window indicator, and sub-field renderers for the new fields parsed from the OAuth API ([@nh13](https://github.com/nh13), [#152](https://github.com/stephenleo/cship/pull/152))
- Added `currency_symbol` and `conversion_rate` config to `cship.cost` so the displayed amount can be expressed in any currency ([@sephml](https://github.com/sephml), [#165](https://github.com/stephenleo/cship/pull/165))

### Changed
- `cship.cost` thresholds (`warn_threshold`, `critical_threshold`) are now evaluated against the converted display value (`total_cost_usd × conversion_rate`) instead of raw USD, so they live in the same currency as the displayed amount. Users with both thresholds and a non-`1.0` `conversion_rate` should restate their thresholds in the display currency ([#167](https://github.com/stephenleo/cship/pull/167))

### Fixed
- Brought the Windows PowerShell installer to parity with the macOS/Linux installer, and stopped emitting a UTF-8 BOM when writing `cship.toml` / `settings.json` (BOM was rejected by the Rust `toml` parser) ([@sephml](https://github.com/sephml), [#160](https://github.com/stephenleo/cship/pull/160))
- `passthrough` now strips `$line_break` and `$character` from `$starship_prompt` output so an embedded Starship prompt renders cleanly inside a cship layout ([#163](https://github.com/stephenleo/cship/pull/163))
- Addressed code-review follow-ups for the `usage_limits` rewrite ([#169](https://github.com/stephenleo/cship/pull/169))

### Docs
- Unified agent instructions into a single `CLAUDE.md` source ([#168](https://github.com/stephenleo/cship/pull/168))

## [1.5.1] - 2026-04-20

### Fixed
- Fixed `$cship.version` to display the installed cship binary version instead of the Claude Code CLI version — now uses compile-time `CARGO_PKG_VERSION`, ensuring the statusline reflects the actual installed binary version

### Added
- Added `-v` / `--version` CLI flags to print the cship binary version

## [1.5.0] - 2026-04-19

### Fixed
- Fixed `install.sh` failing in non-interactive environments (Docker `RUN`, CI pipelines) — the installer no longer opens `/dev/tty` when no terminal is available, instead printing skip messages for optional dependencies

### Added
- Added `--yes` / `-y` flag to `install.sh` to auto-install all optional dependencies (Starship, `libsecret-tools` on Linux) without prompting

## [1.4.2] - 2026-04-19

### Added
- Added `cship.peak_usage` module — shows a configurable peak-time indicator during Anthropic's high-load hours (Mon–Fri 07:00–17:00 US Pacific by default), with zero new dependencies ([@timoklein](https://github.com/timoklein))

### Fixed
- Fixed upgrade via re-running the install script — now runs `cship uninstall` first to remove stale binaries from all locations (e.g. `~/.cargo/bin`) before installing the latest release
- Fixed clippy warnings in `explain.rs` and `cli.rs` tests ([@nh13](https://github.com/nh13))

## [1.4.1] - 2026-03-28

### Added
- Added Windows support — native builds for x86_64 and arm64, PowerShell installer/uninstaller, and Windows docs ([@tkm3d1a](https://github.com/tkm3d1a))
- Added `context_window.used_tokens` module ([@0xRaduan](https://github.com/0xRaduan))
- Added `{remaining}` placeholder to usage limits format strings ([@tkm3d1a](https://github.com/tkm3d1a))
- Added ability to read `rate_limits` from Claude Code stdin before falling back to the OAuth API ([@0xRaduan](https://github.com/0xRaduan))

### Fixed
- Fixed context bar showing blank at the start of a fresh session — now renders an empty 0% bar
- Fixed token counts being truncated instead of rounded in display
- Fixed crash when stdin contains partial rate_limits data

### Changed
- Updated PowerShell installer URL to `cship.dev` domain

## [1.3.0] - 2026-03-14

### Added
- Added `$starship_prompt` token to format strings — embed your full Starship prompt inside a cship layout

## [1.2.0] - 2026-03-14

### Added
- Added configurable cache TTL for usage limits — set `ttl` in `[cship.usage_limits]` to control how long API results are cached ([@RedesignedRobot](https://github.com/RedesignedRobot))

## [1.1.2] - 2026-03-13

### Added
- VitePress documentation site deployed to GitHub Pages (`cship.dev`)
- Hero GIF and annotated hero image in README

## [1.1.1] - 2026-03-12

### Fixed
- Minor documentation and workflow fixes

## [1.1.0] - 2026-03-11

### Added
- `warn_threshold` / `critical_threshold` support on `cost` subfields
- `warn_threshold` / `critical_threshold` support on `context_window` subfields
- `invert_threshold` on `context_window.remaining_percentage` to fix inverted threshold semantics
- GitHub badges in README

## [1.0.0] - 2026-03-09

### Added
- Initial stable release
- Native modules: `model`, `cost`, `context_bar`, `context_window`, `vim`, `agent`, `cwd`, `session_id`, `version`, `output_style`, `workspace`, `usage_limits`
- Starship passthrough with 5s session-hashed file cache
- Per-module `format` strings (Starship-compatible syntax)
- `cship explain` subcommand for self-service debug
- `cship uninstall` subcommand
- `curl | bash` installer with Starship and libsecret-tools detection
- GitHub Actions release pipeline (macOS arm64/x86_64, Linux musl arm64/x86_64)
- crates.io publication

## [0.0.1-rc1] - 2026-03-08

### Added
- First release candidate
