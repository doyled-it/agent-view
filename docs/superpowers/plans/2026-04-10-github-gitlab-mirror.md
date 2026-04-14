# GitHub-to-GitLab Mirror Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Set up a CI pipeline where GitHub is the clean source of truth and GitLab is a downstream mirror with MITRE-specific files added as an overlay.

**Architecture:** A GitHub Actions workflow triggers a GitLab CI pipeline via webhook on every push to `main` or `v*` tag. The GitLab pipeline fetches from GitHub, rebases a single MITRE overlay commit on top, and force-pushes. On tags, GitLab builds binaries and creates releases independently. GitHub's existing release workflow handles GitHub Releases.

**Tech Stack:** GitHub Actions, GitLab CI/CD, git, curl, shell scripting

**Spec:** `docs/superpowers/specs/2026-04-10-github-gitlab-mirror-design.md`

---

### Important Context

- GitHub remote: `origin` → `git@github.com:doyled-it/agent-view.git`
- GitLab remote: `gitlab` → `git@gitlab.mitre.org:mdoyle/agent-view.git`
- Both `main` branches are currently identical at commit `f8a526c`
- MITRE files currently exist on BOTH repos — they need to be removed from GitHub and isolated into a single overlay commit on GitLab
- The overlay commit adds ONLY files that don't exist on GitHub (`install-mitre.sh`, `.gitlab-ci.yml`). No modifications to shared files like CHANGELOG.md — this eliminates rebase conflicts.
- The existing GitLab CI variable `CI_TAG_TOKEN` is used for pushing to GitLab from CI
- GitLab CI uses `-k` flags for MITRE certificate issues
- `gitlab.mitre.org` must be network-accessible from GitHub Actions runners for the webhook to work. If it's behind a VPN/firewall, fall back to a GitLab scheduled pipeline instead.

---

### Task 1: Create GitHub Actions Mirror Workflow

**Files:**
- Create: `.github/workflows/mirror.yml`

- [ ] **Step 1: Create the workflow file**

```yaml
name: Mirror to GitLab

on:
  push:
    branches: [main]
    tags: ['v*']

jobs:
  trigger-gitlab:
    runs-on: ubuntu-latest
    steps:
      - name: Trigger GitLab pipeline
        env:
          GITLAB_PROJECT_ID: ${{ secrets.GITLAB_PROJECT_ID }}
          GITLAB_TRIGGER_TOKEN: ${{ secrets.GITLAB_TRIGGER_TOKEN }}
        run: |
          REF_NAME="${GITHUB_REF_NAME}"
          TAG=""
          if [[ "$GITHUB_REF" == refs/tags/* ]]; then
            TAG="$REF_NAME"
          fi

          HTTP_STATUS=$(curl -ksS -o /tmp/response.json -w "%{http_code}" \
            -X POST \
            "https://gitlab.mitre.org/api/v4/projects/${GITLAB_PROJECT_ID}/trigger/pipeline" \
            -F "token=${GITLAB_TRIGGER_TOKEN}" \
            -F "ref=main" \
            -F "variables[GITHUB_TAG]=${TAG}")

          echo "GitLab API response (HTTP ${HTTP_STATUS}):"
          cat /tmp/response.json

          if [ "$HTTP_STATUS" -lt 200 ] || [ "$HTTP_STATUS" -ge 300 ]; then
            echo "::error::GitLab trigger failed with HTTP ${HTTP_STATUS}"
            exit 1
          fi

          echo "GitLab pipeline triggered successfully."
```

Notes:
- Always triggers on `ref=main` (that's where `.gitlab-ci.yml` lives on GitLab)
- Passes `GITHUB_TAG` as a pipeline variable — empty string for branch pushes, tag name for tag pushes
- Uses `-k` to handle MITRE certificate issues
- Logs the response for debugging but fails on non-2xx status

- [ ] **Step 2: Verify the file is valid YAML**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/mirror.yml'))"`
Expected: No output (valid YAML)

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/mirror.yml
git commit -m "ci: add GitHub Actions workflow to trigger GitLab mirror sync"
```

---

### Task 2: Add Sync Job to GitLab CI

**Files:**
- Modify: `.gitlab-ci.yml`

- [ ] **Step 1: Add sync stage and sync-from-github job**

Add `sync` as the first stage, and add the new job. The full updated `.gitlab-ci.yml`:

```yaml
stages:
  - sync
  - build
  - release

sync-from-github:
  stage: sync
  image: alpine:3.19
  tags:
    - docker
  variables:
    GIT_STRATEGY: clone
    GIT_DEPTH: 0
  before_script:
    - apk add --no-cache git curl
  script:
    - |
      set -e

      git config user.name "GitLab Sync Bot"
      git config user.email "sync-bot@noreply.gitlab.mitre.org"

      # Add GitHub remote and fetch
      git remote add github "$GITHUB_REMOTE_URL"
      git fetch github main --tags

      # Checkout GitLab main as a proper branch
      git checkout -B main origin/main

      # Find where the MITRE overlay commits begin
      MERGE_BASE=$(git merge-base HEAD github/main)
      GITHUB_HEAD=$(git rev-parse github/main)

      if [ "$MERGE_BASE" = "$GITHUB_HEAD" ]; then
        echo "Already up-to-date with GitHub main."
      else
        OVERLAY_COUNT=$(git rev-list --count "${MERGE_BASE}..HEAD")
        echo "Rebasing ${OVERLAY_COUNT} MITRE overlay commit(s) onto GitHub main..."
        echo "  merge-base: ${MERGE_BASE}"
        echo "  github/main: ${GITHUB_HEAD}"

        if ! git rebase --onto github/main "$MERGE_BASE"; then
          echo "ERROR: Rebase failed — conflicts require manual resolution."
          echo ""
          echo "To resolve locally:"
          echo "  git fetch origin main && git fetch github main"
          echo "  git checkout main && git rebase --onto github/main <merge-base>"
          echo "  # resolve conflicts, then: git push gitlab main --force"
          git rebase --abort
          exit 1
        fi

        echo "Rebase succeeded. Pushing to GitLab main..."
        git push "https://oauth2:${CI_TAG_TOKEN}@gitlab.mitre.org/${CI_PROJECT_PATH}.git" \
          main --force -o ci.skip
      fi

      # If triggered by a tag push, sync the tag to GitLab
      if [ -n "${GITHUB_TAG}" ]; then
        echo "Syncing tag: ${GITHUB_TAG}"
        # Tag the current HEAD (which now has MITRE overlay on top of the tagged content)
        git tag -f "${GITHUB_TAG}"
        git push "https://oauth2:${CI_TAG_TOKEN}@gitlab.mitre.org/${CI_PROJECT_PATH}.git" \
          "${GITHUB_TAG}" --force
        echo "Tag ${GITHUB_TAG} pushed to GitLab."
      fi
  rules:
    - if: '$CI_PIPELINE_SOURCE == "trigger"'

build-binaries:
  stage: build
  image: debian:bookworm-slim
  tags:
    - docker
  before_script:
    - apt-get update && apt-get install -y curl unzip git ca-certificates python3 make g++
    - curl -ksSL https://gitlab.mitre.org/mitre-scripts/mitre-pki/raw/master/os_scripts/install_certs.sh | sh
    - curl -fsSL https://bun.sh/install | bash
    - export BUN_INSTALL="$HOME/.bun"
    - export PATH="$BUN_INSTALL/bin:$PATH"
  script:
    - export BUN_INSTALL="$HOME/.bun"
    - export PATH="$BUN_INSTALL/bin:$PATH"
    - bun install
    - bun run compile
    - ls -lh bin/*.tar.gz
  artifacts:
    paths:
      - bin/*.tar.gz
    expire_in: 1 week
  rules:
    - if: '$CI_PIPELINE_SOURCE == "trigger"'
      when: never
    - if: '$CI_COMMIT_TAG'

bump-version:
  stage: release
  image: debian:bookworm-slim
  tags:
    - docker
  variables:
    CHANGELOG_FILE: CHANGELOG.md
  before_script:
    - apt-get update && apt-get install -y curl jq git ca-certificates
    - curl -ksSL https://gitlab.mitre.org/mitre-scripts/mitre-pki/raw/master/os_scripts/install_certs.sh | sh
  script:
    - |
      set -e

      git fetch origin
      git checkout main
      git pull origin main

      LAST_AUTHOR=$(git log -1 --pretty=format:'%an')
      if [ "$LAST_AUTHOR" = "$COMMIT_BOT_USER" ]; then
        echo "Last commit was from $COMMIT_BOT_USER, skipping bump."
        exit 0
      fi

      PROJECT_ID="$CI_PROJECT_ID"
      COMMIT_SHA=$(git log -1 --pretty=format:'%H')
      MR_RESPONSE=$(curl -ksSL --header "PRIVATE-TOKEN: $CI_TAG_TOKEN" \
        "https://gitlab.mitre.org/api/v4/projects/${PROJECT_ID}/repository/commits/${COMMIT_SHA}/merge_requests")

      MR_IID=$(echo "$MR_RESPONSE" | jq -r '.[0].iid // empty')
      if [ -z "$MR_IID" ]; then
        echo "No MR found for commit $COMMIT_SHA, skipping."
        exit 0
      fi

      LABELS=$(echo "$MR_RESPONSE" | jq -r '.[0].labels[]' 2>/dev/null || true)
      BUMP_TYPE=""
      if echo "$LABELS" | grep -q "Version::Major"; then
        BUMP_TYPE="major"
      elif echo "$LABELS" | grep -q "Version::Minor"; then
        BUMP_TYPE="minor"
      elif echo "$LABELS" | grep -q "Version::Patch"; then
        BUMP_TYPE="patch"
      fi

      if [ -z "$BUMP_TYPE" ]; then
        echo "No version label found on MR !${MR_IID}, skipping."
        exit 0
      fi

      echo "Bump type: $BUMP_TYPE"

      CURRENT_VERSION=$(jq -r '.version' package.json)
      MAJOR=$(echo "$CURRENT_VERSION" | cut -d. -f1)
      MINOR=$(echo "$CURRENT_VERSION" | cut -d. -f2)
      PATCH=$(echo "$CURRENT_VERSION" | cut -d. -f3)

      case "$BUMP_TYPE" in
        major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
        minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
        patch) PATCH=$((PATCH + 1)) ;;
      esac

      NEW_VERSION="${MAJOR}.${MINOR}.${PATCH}"
      echo "Current: $CURRENT_VERSION -> New: $NEW_VERSION"

      jq --arg v "$NEW_VERSION" '.version = $v' package.json > package.json.tmp
      mv package.json.tmp package.json

      MR_COMMITS=$(curl -ksSL --header "PRIVATE-TOKEN: $CI_TAG_TOKEN" \
        "https://gitlab.mitre.org/api/v4/projects/${PROJECT_ID}/merge_requests/${MR_IID}/commits" \
        | jq -r '.[].title' 2>/dev/null || true)

      MR_TITLE=$(echo "$MR_RESPONSE" | jq -r '.[0].title // "No title"')
      MR_DESCRIPTION=$(echo "$MR_RESPONSE" | jq -r '.[0].description // ""')
      MR_AUTHOR=$(echo "$MR_RESPONSE" | jq -r '.[0].author.username // "unknown"')
      TODAY=$(date +%Y-%m-%d)

      CHANGELOG_ENTRY=""
      if [ -n "${AIP_API_KEY:-}" ]; then
        LLM_PROMPT=$(printf 'Generate a changelog entry following Common Changelog conventions.\n\nVersion: %s (%s release, previous: %s)\nMR Title: %s\nMR Author: @%s\nMR Description: %s\nCommits:\n%s\n\nRULES:\n1. Use headings: Changed, Added, Removed, Fixed (only relevant ones)\n2. Imperative mood (Add, Fix, Change, Remove)\n3. Reference MR: (!%s) and author: (@%s)\n4. Output ONLY headings and bullet points, no version header or date.\n5. Do not wrap in code fences.' \
          "$NEW_VERSION" "$BUMP_TYPE" "$CURRENT_VERSION" "$MR_TITLE" "$MR_AUTHOR" "$MR_DESCRIPTION" "$MR_COMMITS" "$MR_IID" "$MR_AUTHOR")

        LLM_PAYLOAD=$(jq -n \
          --arg model "openai/gpt-oss-120b" \
          --arg prompt "$LLM_PROMPT" \
          '{model: $model, messages: [{role: "user", content: $prompt}], temperature: 0.3}')

        LLM_RESPONSE=$(curl -ksSL -X POST \
          "https://models.k8s.aip.mitre.org/v1/chat/completions" \
          -H "Content-Type: application/json" \
          -H "Authorization: Bearer $AIP_API_KEY" \
          -d "$LLM_PAYLOAD" 2>/dev/null || true)

        CHANGELOG_ENTRY=$(echo "$LLM_RESPONSE" | jq -r '.choices[0].message.content // empty' 2>/dev/null || true)
      fi

      if [ -z "$CHANGELOG_ENTRY" ]; then
        printf -v CHANGELOG_ENTRY "### Changed\n\n- %s (!%s) (@%s)" "$MR_TITLE" "$MR_IID" "$MR_AUTHOR"
      fi

      VERSION_HEADER=$(printf "## [%s] - %s" "$NEW_VERSION" "$TODAY")

      if [ -f "$CHANGELOG_FILE" ]; then
        HEADER=$(head -n 1 "$CHANGELOG_FILE")
        REST=$(tail -n +2 "$CHANGELOG_FILE")
        printf "%s\n\n%s\n\n%s\n%s" "$HEADER" "$VERSION_HEADER" "$CHANGELOG_ENTRY" "$REST" > "$CHANGELOG_FILE"
      else
        printf "# Changelog\n\n%s\n\n%s\n" "$VERSION_HEADER" "$CHANGELOG_ENTRY" > "$CHANGELOG_FILE"
      fi

      echo "Updated CHANGELOG.md"

      git config user.name "$COMMIT_BOT_USER"
      git config user.email "${COMMIT_BOT_USER}@mitre.org"

      git add package.json "$CHANGELOG_FILE"
      git commit -m "chore: bump version to ${NEW_VERSION} [skip ci]"

      git tag "v${NEW_VERSION}"
      git push "https://oauth2:${CI_TAG_TOKEN}@gitlab.mitre.org/${CI_PROJECT_PATH}.git" main "v${NEW_VERSION}"

      echo "Version bumped and tagged: v${NEW_VERSION}"
  needs: []
  rules:
    - if: '$CI_PIPELINE_SOURCE == "trigger"'
      when: never
    - if: '$CI_COMMIT_BRANCH == "main"'

create-release:
  stage: release
  image: alpine:3.19
  tags:
    - docker
  before_script:
    - apk add --no-cache curl jq sed git
  script:
    - |
      set -e

      VERSION=$(echo "$CI_COMMIT_TAG" | sed 's/^v//')
      echo "Creating release for version: $VERSION"

      git fetch --tags
      PREV_TAG=$(git tag -l 'v*' | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | grep -B1 "^${CI_COMMIT_TAG}$" | head -1)
      if [ "$PREV_TAG" = "$CI_COMMIT_TAG" ] || [ -z "$PREV_TAG" ]; then
        PREV_TAG=$(git rev-list --max-parents=0 HEAD)
      fi

      COMMITS=$(git log "${PREV_TAG}..${CI_COMMIT_TAG}" --pretty=format:"- %s" --no-merges 2>/dev/null | head -50 || true)

      RELEASE_NOTES=""
      if [ -n "${AIP_API_KEY:-}" ]; then
        LLM_PROMPT=$(printf 'Generate concise release notes for version %s.\n\nCommits:\n%s\n\nRULES:\n1. Use headings: Changed, Added, Removed, Fixed (only relevant ones)\n2. Imperative mood\n3. Combine related commits\n4. Exclude CI/CD, formatting, docs-only, version bump commits\n5. Output ONLY headings and bullet points. No code fences.' \
          "$VERSION" "$COMMITS")

        LLM_PAYLOAD=$(jq -n \
          --arg model "openai/gpt-oss-120b" \
          --arg prompt "$LLM_PROMPT" \
          '{model: $model, messages: [{role: "user", content: $prompt}], temperature: 0.3}')

        LLM_RESPONSE=$(curl -ksSL -X POST \
          "https://models.k8s.aip.mitre.org/v1/chat/completions" \
          -H "Content-Type: application/json" \
          -H "Authorization: Bearer $AIP_API_KEY" \
          -d "$LLM_PAYLOAD" 2>/dev/null || true)

        RELEASE_NOTES=$(echo "$LLM_RESPONSE" | jq -r '.choices[0].message.content // empty' 2>/dev/null || true)
      fi

      if [ -z "$RELEASE_NOTES" ]; then
        RELEASE_NOTES="$COMMITS"
      fi

      INSTALL_CMD='curl -kfsSL https://gitlab.mitre.org/mdoyle/agent-view/-/raw/main/install-mitre.sh | bash'

      DESCRIPTION=$(printf "## What's New in v%s\n\n%s\n\n## Installation\n\n\`\`\`bash\n%s\n\`\`\`\n" "$VERSION" "$RELEASE_NOTES" "$INSTALL_CMD")

      ASSET_LINKS="[]"
      for tarball in bin/*.tar.gz; do
        FILENAME=$(basename "$tarball")
        echo "Uploading $FILENAME..."
        curl -ksSL --header "PRIVATE-TOKEN: $CI_TAG_TOKEN" \
          --upload-file "$tarball" \
          "https://gitlab.mitre.org/api/v4/projects/${CI_PROJECT_ID}/packages/generic/agent-view/${VERSION}/${FILENAME}"

        LINK=$(jq -n \
          --arg name "$FILENAME" \
          --arg url "https://gitlab.mitre.org/api/v4/projects/${CI_PROJECT_ID}/packages/generic/agent-view/${VERSION}/${FILENAME}" \
          '{name: $name, url: $url, link_type: "package"}')

        ASSET_LINKS=$(echo "$ASSET_LINKS" | jq --argjson link "$LINK" '. + [$link]')
      done

      RELEASE_PAYLOAD=$(jq -n \
        --arg tag "$CI_COMMIT_TAG" \
        --arg name "Agent View v${VERSION}" \
        --arg description "$DESCRIPTION" \
        --argjson assets "{\"links\": $ASSET_LINKS}" \
        '{tag_name: $tag, name: $name, description: $description, assets: $assets}')

      curl -ksSL -X POST \
        "https://gitlab.mitre.org/api/v4/projects/${CI_PROJECT_ID}/releases" \
        --header "PRIVATE-TOKEN: $CI_TAG_TOKEN" \
        --header "Content-Type: application/json" \
        --data "$RELEASE_PAYLOAD"

      echo "Release v${VERSION} created successfully."
  needs:
    - build-binaries
  rules:
    - if: '$CI_PIPELINE_SOURCE == "trigger"'
      when: never
    - if: '$CI_COMMIT_TAG'
```

Changes from the current `.gitlab-ci.yml`:
- Added `sync` stage as the first stage
- Added `sync-from-github` job with `rules: - if: '$CI_PIPELINE_SOURCE == "trigger"'`
- Converted `build-binaries` from `only: - tags` to `rules:` syntax, excluding trigger pipelines
- Converted `bump-version` from `only: - main` to `rules:` syntax, excluding trigger pipelines
- Converted `create-release` from `only: - tags` to `rules:` syntax, excluding trigger pipelines
- All existing job scripts are unchanged

- [ ] **Step 2: Validate the YAML**

Run: `python3 -c "import yaml; yaml.safe_load(open('.gitlab-ci.yml'))"`
Expected: No output (valid YAML)

- [ ] **Step 3: Commit**

```bash
git add .gitlab-ci.yml
git commit -m "ci: add sync-from-github job for GitHub mirror pipeline"
```

---

### Task 3: Clean GitHub Main and Establish GitLab Overlay

This task establishes the clean separation: GitHub `main` has no MITRE files, GitLab `main` = GitHub `main` + one overlay commit.

**Files:**
- Remove from GitHub: `install-mitre.sh`, `.gitlab-ci.yml`
- Add to GitHub: `.github/workflows/mirror.yml` (from Task 1)

**Prerequisites:** Tasks 1 and 2 must be completed first.

- [ ] **Step 1: Create a working branch from main**

```bash
git checkout main
git pull origin main
git checkout -b ci/github-gitlab-mirror
```

- [ ] **Step 2: Remove MITRE files and add mirror workflow**

```bash
git rm install-mitre.sh .gitlab-ci.yml
git add .github/workflows/mirror.yml
git commit -m "ci: remove MITRE-specific files and add GitLab mirror trigger

GitHub is now the clean source of truth. MITRE-specific files
(install-mitre.sh, .gitlab-ci.yml) are maintained as an overlay
on the GitLab mirror."
```

- [ ] **Step 3: Push branch to GitHub and merge to main**

```bash
git push origin ci/github-gitlab-mirror
```

Then merge to `main` via GitHub PR (or direct push if preferred for CI setup).

- [ ] **Step 4: Establish GitLab overlay**

After GitHub `main` is clean:

```bash
git fetch origin main
git checkout -B gitlab-overlay origin/main

# Add MITRE files back as the overlay commit
# install-mitre.sh: restore from the commit before removal
git show HEAD~1:install-mitre.sh > install-mitre.sh

# .gitlab-ci.yml: use the updated version with sync job (from Task 2)
# (this file should already be staged/available from Task 2)
git add install-mitre.sh .gitlab-ci.yml
git commit -m "chore(mitre): add MITRE overlay

MITRE-specific files layered on top of GitHub main:
- install-mitre.sh: MITRE GitLab installer with -k cert handling
- .gitlab-ci.yml: build/release pipeline with sync-from-github job"
```

- [ ] **Step 5: Force push overlay to GitLab main**

```bash
git push gitlab gitlab-overlay:main --force
```

This replaces GitLab `main` with: GitHub `main` + 1 MITRE overlay commit.

- [ ] **Step 6: Verify the state**

```bash
# GitLab main should be 1 commit ahead of GitHub main
git log --oneline origin/main..gitlab/main
# Expected: 1 commit — the MITRE overlay

# GitHub main should NOT have MITRE files
git ls-tree origin/main --name-only | grep -E "install-mitre|gitlab-ci"
# Expected: no output

# GitLab main SHOULD have MITRE files
git ls-tree gitlab/main --name-only | grep -E "install-mitre|gitlab-ci"
# Expected: .gitlab-ci.yml, install-mitre.sh
```

---

### Task 4: Configure CI Variables

All manual steps — no code changes.

- [ ] **Step 1: Create GitLab pipeline trigger token**

1. Go to `https://gitlab.mitre.org/mdoyle/agent-view/-/settings/ci_cd`
2. Expand "Pipeline trigger tokens"
3. Create a new token with description "GitHub mirror sync"
4. Copy the token value

- [ ] **Step 2: Add GitHub repository secrets**

1. Go to `https://github.com/doyled-it/agent-view/settings/secrets/actions`
2. Add secret `GITLAB_TRIGGER_TOKEN` with the token from Step 1
3. Add secret `GITLAB_PROJECT_ID` with the GitLab project ID

To find the GitLab project ID:
```bash
curl -ks "https://gitlab.mitre.org/api/v4/projects/mdoyle%2Fagent-view" | python3 -c "import sys, json; print(json.load(sys.stdin)['id'])"
```

- [ ] **Step 3: Add GitLab CI/CD variable**

1. Go to `https://gitlab.mitre.org/mdoyle/agent-view/-/settings/ci_cd`
2. Expand "Variables"
3. Add variable `GITHUB_REMOTE_URL` = `https://github.com/doyled-it/agent-view.git`
   - Protected: Yes, Masked: No

- [ ] **Step 4: Verify `CI_TAG_TOKEN` already exists**

The existing `bump-version` and `create-release` jobs already use `CI_TAG_TOKEN`. Confirm it's present in GitLab CI/CD variables. No action needed if it's already there.

---

### Task 5: End-to-End Verification

- [ ] **Step 1: Test main branch sync**

Push a small change to GitHub `main` (e.g., a README tweak):

```bash
git checkout main
git pull origin main
# Make a small change to README.md
git commit -am "docs: verify mirror pipeline"
git push origin main
```

Then verify:
1. GitHub Actions "Mirror to GitLab" workflow runs: `https://github.com/doyled-it/agent-view/actions`
2. GitLab pipeline triggers: `https://gitlab.mitre.org/mdoyle/agent-view/-/pipelines`
3. `sync-from-github` job succeeds
4. GitLab `main` has the README change PLUS the MITRE overlay commit on top

```bash
git fetch gitlab main
git log --oneline origin/main..gitlab/main
# Expected: 1 commit (the MITRE overlay — rebased onto the new main)
```

- [ ] **Step 2: Test tag sync**

```bash
git tag v0.99.0-test
git push origin v0.99.0-test
```

Verify:
1. GitHub Actions "Mirror to GitLab" workflow runs with `GITHUB_TAG=v0.99.0-test`
2. GitHub Actions "Release" workflow runs and creates a GitHub Release
3. GitLab `sync-from-github` job syncs the tag
4. GitLab `build-binaries` job runs (triggered by the tag)
5. GitLab `create-release` job creates a GitLab Release with binaries

Clean up the test tag after verification:
```bash
git tag -d v0.99.0-test
git push origin --delete v0.99.0-test
# Also clean up on GitLab if the sync succeeded
git push gitlab --delete v0.99.0-test
```

- [ ] **Step 3: Test conflict detection**

Intentionally create a conflict to verify the pipeline fails cleanly:
1. On GitLab, modify a file that also exists on GitHub in the overlay commit
2. Push a conflicting change to GitHub
3. Verify the sync pipeline fails with a clear error message about conflicts
4. Clean up by resolving and force-pushing the overlay

---

### Transition Note

After this pipeline is active:
- All new development happens on GitHub (PRs, code review, merges to `main`)
- GitLab MRs are no longer used for code changes — GitLab `main` is managed by the sync bot
- Existing open GitLab MRs (!4, !5) should be resolved before or shortly after enabling the pipeline
- Version bumping on GitLab (`bump-version` job) will be dormant since there are no GitLab MRs — version management moves to GitHub
- The `GITHUB_TOKEN` GitLab variable from the spec is NOT needed — GitHub's own `release.yml` handles GitHub Releases independently
