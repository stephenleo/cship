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

1. Install BMAD in this repo.
2. Run `/bmad-quick-dev` with a GitHub issue URL or a description of your change — it guides you from intent through spec, implementation, and review.

### Manual (Not recommended)

See the non-negotiable code patterns in `CLAUDE.md`.

- Create `src/modules/{name}.rs` and update `src/modules/mod.rs` — no other files required.
- Module signature: `pub fn render(ctx: &Context, cfg: &CshipConfig) -> Option<String>`
- All config structs go in `src/config.rs` with `#[derive(Debug, Deserialize, Default)]` and `pub Option<T>` fields.

## Opening a PR

1. Make sure all four checks above pass.
2. Describe what your change does and why in the PR description.
3. Reference any related issues with `Closes #<issue-number>`.
