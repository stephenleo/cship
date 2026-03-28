# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [1.4.0] - 2026-03-28

### Added
- Windows support: x86_64 and aarch64-pc-windows-msvc release targets
- Windows installer (`install.ps1`) with WOW64 arch detection and JSON depth hardening
- Windows uninstaller aligned with installer paths
- Windows documentation in README and docs site

### Changed
- PowerShell installer one-liner URL updated to `cship.dev` domain
- CI test `test_remove_statusline_present` guarded as non-Windows only

## [1.3.0] - 2026-03-14

### Added
- `$starship_prompt` token support in format strings with `STARSHIP_SHELL` fix
- Full Starship prompt showcase example in docs

## [1.2.0] - 2026-03-14

### Added
- Configurable TTL for usage limits cache (`ttl` field in `[cship.usage_limits]`)

### Fixed
- GitHub Actions workflow for external PR secret access

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
