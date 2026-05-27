# Agent View Agent Instructions

Follow `CLAUDE.md` as the canonical repository guide. This file is tracked so
agents that discover `AGENTS.md` use the same build, test, style, and
architecture instructions.

## Required Local Gates

Run the same commands as CI and pre-commit before committing or pushing:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --release --all-features
```

If `pre-commit` is installed, `pre-commit run --all-files` runs these same
checks.

## Repository Notes

- Rust toolchain selection is pinned in `rust-toolchain.toml`.
- Rendering code belongs under `src/ui/` and should not mutate app state.
- Business logic belongs under `src/core/`; avoid UI imports there.
- Theme colors must come from `src/ui/theme.rs`.
- Tests live beside source in `#[cfg(test)] mod tests` blocks.
