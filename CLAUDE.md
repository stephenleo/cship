# cship — AI Agent Instructions

## Environment Quirks
- WSL2 ENOENT race: after any `cargo init` or file write, verify content with Read before proceeding

## Non-Negotiable Code Patterns
- Module interface (never deviate): `pub fn render(ctx: &Context, cfg: &CshipConfig) -> Option<String>`
- Disabled flag → silent `None` (no warn); absent data → explicit `match` + `tracing::warn!` + `None`
  - Exception: `context_bar` intentionally renders a 0% empty bar (styled via `empty_style`) when `context_window` is absent, rather than returning `None`. This is a deliberate UX choice — showing an empty bar is more informative than showing nothing. It uses `tracing::debug!` (not `warn!`) because absence is the normal state at session start.
- Never use `?` operator on paths that require a warning — use explicit `match`
- stdout owned by `main.rs` only; all module diagnostics via `tracing::*` macros; no `eprintln!` anywhere
- Exception: CLI-action subcommands (e.g. `uninstall`, `explain`) may use `println!` directly — the stdout rule applies to the rendering pipeline only
- All config structs: `#[derive(Debug, Deserialize, Default)]`, all fields `pub Option<T>`
- Never add `deny_unknown_fields` to any struct — omitted intentionally on both `Context` and config structs so future versions can add fields without breaking deserialization

## Before Submitting a PR
- Follow all guidelines in [CONTRIBUTING.md](CONTRIBUTING.md) before opening or updating a pull request

## Project Structure
- Adding a native module: create `src/modules/{name}.rs` + update `src/modules/mod.rs` only (2 files max)
- Config structs → `src/config.rs` only; ANSI logic → `src/ansi.rs` only; threshold styling → `ansi::apply_style_with_threshold`
