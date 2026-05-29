# Contributing to Agent View

Thanks for your interest in contributing! Agent View is a Rust/ratatui terminal UI for managing AI coding agent sessions via tmux. This document covers how to set up a development environment, the expected code style, and the pull request workflow.

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (the toolchain is pinned in `rust-toolchain.toml`)
- [tmux](https://github.com/tmux/tmux)
- [prek](https://github.com/j178/prek) (`cargo install --locked prek`)

### Setup

```bash
git clone https://github.com/doyled-it/agent-view.git
cd agent-view
prek install
cargo build
cargo test
```

## Development Workflow

1. **Create a branch.** Never commit directly to `main`. Use a descriptive branch name such as `feat/routine-pinning` or `fix/status-flicker`.
2. **Make your changes.** Keep commits focused and follow the commit-message conventions below.
3. **Run the full check locally** before pushing:
   ```bash
   cargo fmt --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-features
   cargo build --release --all-features
   ```
   These are the same commands run by `prek run --all-files` and CI.
4. **Push your branch and open a pull request** against `main`. Fill out the PR template.
5. **Apply a version label** (see below) so the release automation can pick the right semver bump on merge.

### Commit Hooks

The repository uses [prek](https://github.com/j178/prek) with the tracked native
`prek.toml` config. Cargo does not have a Poetry/uv-style developer dependency
group for standalone tools; `[dev-dependencies]` are for crates linked into
tests/examples/benches. Keep `prek` installed as a local developer tool.

Do **not** bypass hooks with `--no-verify` -- if a hook fails, fix the
underlying issue and commit again.

## Code Style

- `cargo fmt` is the source of truth for formatting.
- `cargo clippy --all-targets --all-features -- -D warnings` must be clean; warnings are errors in CI.
- Prefer match guards over `if`-blocks inside match arms (clippy `collapsible_match`).
- Keep layer boundaries intact:
  - `src/core/` -- business logic, storage, tmux integration. No UI imports.
  - `src/ui/` -- ratatui rendering only. Do not mutate app state here.
  - `src/input/` -- keyboard handlers that mutate `App`.
  - `src/app.rs` -- central `App` struct and overlay enums.
- All colors come from `Theme` (`src/ui/theme.rs`). Never hardcode colors in rendering code.
- Storage goes through `src/core/storage.rs` (SQLite via `rusqlite` with the `bundled` feature).

## Testing

- Tests live alongside source in `#[cfg(test)] mod tests` blocks.
- Storage tests use in-memory SQLite (`:memory:`).
- There is no mocking framework -- use real implementations where practical.
- New features and bug fixes should include tests whenever the code under change is testable.

## Commit Messages

Use the [Conventional Commits](https://www.conventionalcommits.org/) format:

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

Common types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `chore`, `ci`, `build`.

Breaking changes use `!` after the type (`feat!: drop Linux arm64 support`) or a `BREAKING CHANGE:` footer.

Examples:

```
feat(routines): add raw cron expression input

fix(status): debounce tmux pane polling to prevent flicker

refactor(storage): extract migration runner into its own module
```

## Pull Requests

- Keep PRs focused on a single concern when possible.
- Fill in the PR template -- summary, testing notes, screenshots for UI changes.
- Link related issues (`Closes: #123`).
- CI must be green before a maintainer will review.
- A maintainer will merge via the GitHub UI once approved.

### Version Labels

On merge, the `Version Bump` workflow reads PR labels to decide the next semver bump, updates `Cargo.toml` / `CHANGELOG.md`, tags the release, and triggers the release workflow. Apply exactly **one** of:

| Label | Bump | When to use |
|-------|------|-------------|
| `version:major` | MAJOR | Breaking changes to CLI flags, config schema, stored data, or public API |
| `version:minor` | MINOR | New features, new commands, new keybindings |
| `version:patch` | PATCH | Bug fixes, docs, refactors, CI/build-only changes |

If no `version:*` label is present, no release is cut -- use this for purely internal changes that should land but not ship.

Do not bump versions manually in `Cargo.toml` -- the workflow handles it.

## Reporting Bugs and Requesting Features

Use the issue templates in [`.github/ISSUE_TEMPLATE`](.github/ISSUE_TEMPLATE). For security issues, see [SECURITY.md](SECURITY.md) -- do not open a public issue.

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
