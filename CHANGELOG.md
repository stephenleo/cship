# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

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
