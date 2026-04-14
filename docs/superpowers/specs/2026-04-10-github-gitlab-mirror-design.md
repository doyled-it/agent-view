# GitHub to GitLab Mirror with MITRE Overlay

## Overview

GitHub (`doyled-it/agent-view`) is the public source of truth. GitLab (`mdoyle/agent-view`) is a downstream mirror whose `main` branch equals GitHub `main` plus a small set of MITRE-specific commits rebased on top. A GitHub Actions webhook triggers GitLab CI to sync on every push.

## Architecture

```
GitHub (doyled-it/agent-view)     GitLab (mdoyle/agent-view)
         main ──push──> webhook ──trigger──> CI pipeline
         v* tags ──────────────────────────> CI pipeline
                                                │
                                                ▼
                                        fetch GitHub main
                                        rebase MITRE commits on top
                                        push to GitLab main
                                        (on tags: build + release)
```

## MITRE Overlay

GitLab `main` carries a small number of commits on top of GitHub `main` that add:

- `install-mitre.sh` — MITRE-specific installer using GitLab packages API with `-k` for cert handling
- `.gitlab-ci.yml` — Build, version bump, and release pipeline referencing `gitlab.mitre.org`, MITRE PKI cert scripts, and AIP LLM API for changelog/release note generation
- CHANGELOG entry referencing the MITRE installer

These files do not exist on GitHub. The overlay commits are rebased onto new upstream content on each sync.

## Sync Flow

### Trigger

A GitHub Actions workflow (`.github/workflows/mirror.yml`) fires on push to `main` and `v*` tags. It calls the GitLab pipeline trigger API with the ref/tag as a variable.

### GitLab Sync Job

New `sync` stage in `.gitlab-ci.yml`, running a `sync-from-github` job:

1. Add GitHub as a remote and fetch the target ref
2. Rebase the MITRE overlay commits onto the new `github/main`
3. If rebase conflicts: pipeline fails, manual resolution required
4. If clean: `git push origin main --force-with-lease`

On tag syncs, the existing `build-binaries`, `bump-version`, and `create-release` jobs run afterward as they do today.

### Loop Prevention

The sync job only runs when `CI_PIPELINE_SOURCE == "trigger"`. The force-push from the rebase does not re-trigger the sync.

## CI Variables

| Where   | Variable               | Purpose                                          |
|---------|------------------------|--------------------------------------------------|
| GitHub  | `GITLAB_TRIGGER_TOKEN` | Secret to trigger GitLab pipelines               |
| GitHub  | `GITLAB_PROJECT_ID`    | GitLab project ID for the trigger API call       |
| GitLab  | `GITHUB_REMOTE_URL`    | `https://github.com/doyled-it/agent-view.git`    |

## Components

### GitHub Actions Workflow

`.github/workflows/mirror.yml`:
- Triggers on push to `main` and `v*` tags
- Single step: `curl` POST to `https://gitlab.mitre.org/api/v4/projects/$PROJECT_ID/trigger/pipeline`
- Passes the GitHub ref as a pipeline variable

### GitLab CI Job

`sync-from-github` job in the `sync` stage:
- Image: `alpine` with `git` and `curl`
- Runs only on `trigger` pipeline source
- Fetches GitHub, rebases, pushes

### Existing GitLab CI Jobs

No changes. `build-binaries`, `bump-version`, and `create-release` continue to run on their existing triggers (`main` merges and tags).

## Conflict Handling

Rebase failure causes pipeline failure. Conflicts are resolved manually. Expected to be rare since the overlay is almost entirely files that do not exist on GitHub.

## What Changes

- **New on GitHub:** `.github/workflows/mirror.yml`
- **Modified on GitLab:** `.gitlab-ci.yml` gains a `sync` stage and `sync-from-github` job
- **New GitHub secrets:** `GITLAB_TRIGGER_TOKEN`, `GITLAB_PROJECT_ID`
- **New GitLab CI variable:** `GITHUB_REMOTE_URL`
- **GitLab `main`:** Will be force-pushed (with lease) by the sync job. Safe because direct pushes to GitLab `main` are not part of the workflow.

## Tag and Release Sync

On GitHub tag push:
1. GitHub Actions triggers GitLab pipeline with the tag ref
2. GitLab syncs the tag, rebases MITRE overlay, pushes tag to GitLab
3. Existing GitLab CI `build-binaries` job builds tarballs (triggered by tag)
4. Existing `create-release` job creates the GitLab release with binaries
5. GitHub's own `release.yml` workflow builds and creates the GitHub Release independently

Each platform builds and releases independently. No cross-platform artifact shuttling needed. GitHub `install.sh` points at GitHub Releases, `install-mitre.sh` points at GitLab Releases.
