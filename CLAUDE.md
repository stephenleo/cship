# cship — Claude Code Instructions

## Environment Quirks
- WSL2 ENOENT race: after any `cargo init` or file write, verify content with Read before proceeding
- Security hook blocks `Write` tool on `.github/workflows/` files — use Bash heredoc instead

## Non-Negotiable Code Patterns
- Module interface (never deviate): `pub fn render(ctx: &Context, cfg: &CshipConfig) -> Option<String>`
- Disabled flag → silent `None` (no warn); absent data → explicit `match` + `tracing::warn!` + `None`
- Never use `?` operator on paths that require a warning — use explicit `match`
- stdout owned by `main.rs` only; all module diagnostics via `tracing::*` macros; no `eprintln!` anywhere
- Exception: CLI-action subcommands (e.g. `uninstall`, `explain`) may use `println!` directly — the stdout rule applies to the rendering pipeline only
- All config structs: `#[derive(Debug, Deserialize, Default)]`, all fields `pub Option<T>`

## Project Structure
- Adding a native module: create `src/modules/{name}.rs` + update `src/modules/mod.rs` only (2 files max)
- Config structs → `src/config.rs` only; ANSI logic → `src/ansi.rs` only; threshold styling → `ansi::apply_style_with_threshold`
