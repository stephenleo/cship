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

## Quality Gates (required before any story is complete)
- `cargo clippy -- -D warnings && cargo fmt --check && cargo test`
- Non-Rust deliverables (scripts, workflows, docs): run `git status` + `git add` + verify commit before marking story ready for review
- Shell script stories: include a smoke test exercising the script in piped execution context (`curl | bash`), not just `bash script.sh` directly

## Git Convention
- Branch: `{issue-number}-story-{epic}-{story}-{slug}` (e.g. `13-story-2-1-cost-module`)
- Each story = one PR; include `Closes #{issue}` in PR description

## BMAD Artifact Locations
- Sprint status: `_bmad-output/implementation-artifacts/sprint-status.yaml`
- Stories: `_bmad-output/implementation-artifacts/`
- Planning artifacts: `_bmad-output/planning-artifacts/`
