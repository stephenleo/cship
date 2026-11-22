# Contributing to CShip

Thank you for contributing! This guide explains how to get your changes ready to pass CI.

## Prerequisites

- [Rust toolchain](https://rustup.rs) (stable)

## Running checks before submitting a PR

CI enforces four checks. Run them locally before pushing:

```sh
# 1. Format — CI runs `cargo fmt --check`; fix formatting first
cargo fmt

# 2. Lint — all Clippy warnings are treated as errors
cargo clippy -- -D warnings

# 3. Tests
cargo test

# 4. Release build
cargo build --release
```

If any of these fail locally, they will fail in CI. Fix them before opening a PR.

## Adding a new module

### BMAD (Recommended)

This project is built with [BMAD](https://github.com/bmad-code-org/BMAD-METHOD).

1. Install BMAD in this repo. `npx bmad-method install`
2. Run `/bmad-quick-dev` with a GitHub issue URL or a description of your change — it guides you from intent through spec, implementation, and review.

### Manual (Not recommended)

See the non-negotiable code patterns in `CLAUDE.md`.

- Create `src/modules/{name}.rs` and update `src/modules/mod.rs`. If the module needs config fields, also add a struct to `src/config.rs`.
- Module signature: `pub fn render(ctx: &Context, cfg: &CshipConfig) -> Option<String>`
- All config structs go in `src/config.rs` with `#[derive(Debug, Deserialize, Default)]` and `pub Option<T>` fields.
- Absent data → explicit `match` + `tracing::warn!` + `None`. Disabled flag → silent `None`. (Exception: `context_bar` renders an empty bar with `tracing::debug!` when context data is absent — this is intentional UX. Likewise `usage_limits` hides the 5h or 7d window with `tracing::debug!` when the account has none, since that is a normal plan state.)

## Updating documentation

Most documentation lives in `docs/`; `CONTRIBUTING.md` at the repo root mirrors `docs/contributing.md` (see the last bullet). Update the relevant files alongside your code change:

- **New module or config field** — add the config option to `docs/configuration.md`. If the module has non-obvious UX, add a `docs/faq.md` entry too.
- **Removed module or config field** — delete or mark deprecated the corresponding entry in `docs/configuration.md`.
- **Behaviour change** — update any affected section in `docs/configuration.md` or `docs/faq.md`.
- **New showcase or example** — add the `cship.toml` config block to `docs/showcase.md` and place any screenshot or GIF in `docs/examples/`.
- **This guide** — `CONTRIBUTING.md` (repo root) and `docs/contributing.md` are kept in sync manually; if you edit either for any reason, update the other to match.

## Opening a PR

1. Make sure all four checks above pass.
2. Describe what your change does and why in the PR description.
3. Reference any related issues with `Closes #<issue-number>`.
