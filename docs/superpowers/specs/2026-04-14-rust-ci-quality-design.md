# Rust CI + Code Quality Design

## Goal

Add CI pipelines for the Rust codebase with strict quality gates, fix all existing lint/format issues, and improve test coverage on core modules.

## Scope

1. Fix all existing rustfmt and clippy issues
2. GitHub Actions CI for `main` and `release/*` branches
3. GitLab CI mirroring the same gates plus MITRE-specific release
4. Test coverage improvements for core modules

## CI Gates

All gates must pass for a PR to merge. No exceptions.

### GitHub Actions — `.github/workflows/rust-test.yml`

Triggers: PRs and pushes to `main`, `release/*`

Jobs:
- **fmt**: `cargo fmt --check`
- **clippy**: `cargo clippy -- -D warnings`
- **test**: `cargo test`
- **build**: `cargo build --release`

Matrix: ubuntu-latest (primary), macos-latest (verify cross-platform)

### GitHub Actions — `.github/workflows/rust-release.yml`

Triggers: tags matching `v*`

Jobs:
- Cross-compile release binaries for: linux-x64, linux-aarch64, darwin-x64, darwin-arm64
- Create GitHub release with artifacts
- Uses `cross` crate for cross-compilation

### GitLab CI — `.gitlab-ci.yml`

Mirrors GitHub gates (fmt, clippy, test, build) plus:
- `build-binaries` stage: cross-compile on tag push
- `create-release` stage: publish to GitLab package registry
- `bump-version` stage: version bump + changelog on main merge (existing behavior, adapted for Rust — reads version from `Cargo.toml` instead of `package.json`)

Triggers: same as GitHub — `main`, `release/*` for gates, tags for release.

## Existing Issues to Fix

### rustfmt

Code has formatting diffs. Run `cargo fmt` to fix all. No behavior changes.

### clippy (26 warnings)

Categories:
- **Dead code** (~10): unused functions (`validate_branch_name`, `parse_worktree_list`, `Worktree` struct), unused methods (`set_acknowledged`, `delete_group`, `close`, `conn`, `remove_session`, `format_line`). Fix: add `#[allow(dead_code)]` for public API methods that will be used later, remove truly dead code.
- **Style** (~8): `map_or` → `is_some_and`, redundant closures, manual arithmetic checks, `unwrap_or_default` simplifications, needless borrows. Fix: apply clippy suggestions.
- **Structure** (~2): large enum variant size difference, derivable impl. Fix: Box large variants, derive instead of manual impl.

## Test Coverage Improvements

### Target Modules (pure logic, no terminal needed)

| Module | Current Tests | Areas to Add |
|--------|--------------|-------------|
| `src/types.rs` | 4 | SortMode::next cycle, SortMode::label, sort_priority ordering, ActivityEvent::format_line edge cases |
| `src/app.rs` | 2 | select_all_visible, push_activity cap at 100, search_matches with various queries |
| `src/core/groups.rs` | 7 | Sort mode interactions with pinning, all 4 sort modes, empty group handling |
| `src/core/session.rs` | 12 | More notification edge cases (recently detached expiry, sustained running threshold) |
| `src/core/config.rs` | 5 | save_config roundtrip, save_config creates directory, load invalid path |
| `src/core/logger.rs` | 5 | SessionLogger line counting, multiple rotations |
| `src/core/tokens.rs` | 9 | Edge cases (single token, large numbers, mixed content) |
| `src/core/storage.rs` | 20 | v4 pinned/tokens CRUD, add_tokens incremental, set_pinned toggle |

### Skip (not worth testing)

- `src/ui/*.rs` — rendering functions need a terminal frame, not unit-testable without mocking ratatui
- `src/main.rs` — event loop, requires real terminal + tmux

### Coverage Tool

Use `cargo-tarpaulin` for coverage measurement. Not gated in CI (too slow for every PR), but runnable locally with `cargo tarpaulin --out html`.

## Pre-commit Hook

Add `rustfmt.toml` (default config) and a pre-commit check:

```yaml
repos:
  - repo: local
    hooks:
      - id: cargo-fmt
        name: cargo fmt
        entry: cargo fmt --check
        language: system
        pass_filenames: false
      - id: cargo-clippy
        name: cargo clippy
        entry: cargo clippy -- -D warnings
        language: system
        pass_filenames: false
```

## Branch Strategy

New branch `feat/rust-ci` from `release/rust`. Merge back via PR on GitHub.
