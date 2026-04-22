## Summary

<!-- What does this PR change and why? Keep it short; details go below. -->

## Changes

<!-- Bullet the user-visible or structural changes. -->
-
-

## Testing

<!-- How did you verify this works? Commands run, manual steps, screenshots for UI changes. -->
- [ ] `cargo fmt --check`
- [ ] `cargo clippy -- -D warnings`
- [ ] `cargo test`
- [ ] Manually exercised the affected flows

## Screenshots / Recordings

<!-- For UI changes, drop a screenshot or asciinema/GIF here. Delete this section if not applicable. -->

## Version Label

<!--
Apply exactly one of these labels so the release workflow picks the right bump:

- `version:major` - breaking change (CLI flags, config schema, stored data, public API)
- `version:minor` - new feature, new command, new keybinding
- `version:patch` - bug fix, docs, refactor, CI/build-only change

Leave unlabeled only for internal changes that should not cut a release.
-->

## Related Issues

<!-- Use `Closes: #123` / `Refs: #123` to link issues. -->

## Checklist

- [ ] Commits follow [Conventional Commits](https://www.conventionalcommits.org/)
- [ ] Code follows the layering rules in [CONTRIBUTING.md](../CONTRIBUTING.md) (no UI imports in `core/`, no state mutation in `ui/`)
- [ ] New code is covered by tests where practical
- [ ] Documentation (README, in-repo docs, help overlay) updated if behavior changed
- [ ] A `version:*` label is applied (or this change intentionally ships without a release)
