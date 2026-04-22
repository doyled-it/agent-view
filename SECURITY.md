# Security Policy

## Supported Versions

Agent View is a small, actively maintained project. Security fixes are applied to the latest released version on the `main` branch. Older versions do not receive patches -- please upgrade to the latest release.

| Version | Supported |
|---------|-----------|
| Latest release (`main`) | Yes |
| Older releases | No |

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Report vulnerabilities privately through GitHub's [Security Advisories](https://github.com/doyled-it/agent-view/security/advisories/new) flow. This creates a private report visible only to the maintainers.

When reporting, please include as much of the following as you can:

- A description of the issue and its potential impact
- Steps to reproduce (ideally a minimal proof of concept)
- The version of Agent View, tmux, and the operating system you observed it on
- Any logs, stack traces, or screenshots that help demonstrate the issue

## Response Expectations

This is a hobbyist / small-team project, so response times are best-effort rather than contractual:

- **Acknowledgement:** within 7 days of the report
- **Initial assessment:** within 14 days
- **Fix or mitigation:** depends on severity and complexity; critical issues are prioritized

You will be credited in the release notes for the fix unless you request otherwise.

## Scope

In scope:

- The Agent View binary (`agent-view` / `av`)
- The install and uninstall scripts (`install.sh`, `uninstall.sh`)
- Persisted data in `~/.agent-view/` (SQLite database, config, logs)
- Scheduled routine execution via macOS LaunchAgent

Out of scope:

- Vulnerabilities in tmux, Claude Code, Gemini CLI, OpenCode, Codex CLI, or other tools that Agent View orchestrates -- please report those upstream.
- Issues that require an attacker to already have local shell access with the same privileges as the user running Agent View.
- Denial-of-service caused by running the tool against unreasonably large numbers of tmux sessions.

## Safe Harbor

Good-faith security research conducted in line with this policy is welcome. We will not pursue legal action for research that:

- Avoids harm to users or their data
- Does not access data beyond what is needed to demonstrate the issue
- Gives the project reasonable time to respond before public disclosure
