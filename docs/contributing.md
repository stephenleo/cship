# Contributing

Thank you for contributing to CShip!

## Prerequisites

- [Rust toolchain](https://rustup.rs) (stable)

## Checks to run before submitting a PR

CI enforces four checks on every pull request. Run them locally before pushing to avoid failing builds:

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

- Create `src/modules/{name}.rs` and update `src/modules/mod.rs` — no other files required.
- Module signature must be exactly: `pub fn render(ctx: &Context, cfg: &CshipConfig) -> Option<String>`
- All config structs go in `src/config.rs` with `#[derive(Debug, Deserialize, Default)]` and `pub Option<T>` fields.
- Absent data → explicit `match` + `tracing::warn!` + `None`. Disabled flag → silent `None`. (Exception: `context_bar` renders an empty bar with `tracing::debug!` when context data is absent — this is intentional UX.)

## Opening a PR

1. Make sure all four checks above pass.
2. Describe what your change does and why in the PR description.
3. Reference any related issues with `Closes #<issue-number>`.
