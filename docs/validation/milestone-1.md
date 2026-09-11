# Milestone 1 validation

Implemented watched-repository browsing, GitHub topics, persistent account-isolated local tags, AND filters, quoted tag values, background refresh, and an in-memory demo. GitHub calls are read-only. The UI cancels pending GitHub processes on normal exit.

## Checks

- 23 offline tests passed in Docker: filtering, tag editing, refresh/edit races, account isolation, SQLite rollback and corruption, pagination, CLI errors, output limits, timeout, cancellation, and terminal rendering.
- Formatting and Clippy with warnings denied passed in Docker.
- Docker test and release build targets verified.
- Interactive demo: edited a tag, applied `topic:rust tag:priority`, observed one matching repository, and exited with terminal settings restored.
- Locked Nix devShell resolved successfully and provided Cargo 1.97.0. Docker uses Rust 1.94.

## Remaining verification

The host's GitHub CLI is not authenticated, so live watched-repository import and topic-field coverage have not been exercised. Tests use synthetic API fixtures. Authenticate with `gh auth login --hostname github.com`, then run the app through the devShell for this check.

Native macOS compilation and terminal behavior have not been tested; compilation and interactive smoke testing used Linux containers. The devShell itself was checked on macOS. Windows is not a milestone-1 target.

## Scope notes

Refresh is manual. Daily updates, issues, focus persistence, GitHub CLI extension packaging, and local Git actions remain later work. No GitHub writes, commits, releases, or global Rust installation were performed.
