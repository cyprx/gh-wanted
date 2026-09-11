# Milestone 2 validation

Implemented issue discovery across the current repository filter, independent label/keyword/state/unassigned filters, account-isolated saved focuses, issue details, and explicit validated browser opening. Saved focuses contain filter expressions, so membership follows current topics and local tags.

## Checks

- Final Docker run passed: 30 Rust tests, `cargo fmt --check`, Clippy with warnings denied, and the PTY walkthrough. `git diff --check` passed.
- Offline Rust tests cover issue/PR separation, duplicate pages, missing assignees, combined filters, validated browser URLs, explicit paginated request parameters, failed pages, stale-result preservation, account isolation, and selection preservation while results arrive.
- Focus tests cover restart persistence, unique names, account isolation, changing membership, empty matches, schema-1 migration, and rollback on migration conflicts. Existing milestone-1 tests remain enabled.
- Linux PTY flow uses fictional data: add `priority`, apply `topic:rust tag:priority`, browse issues, apply `label:"good first issue" unassigned`, save `Rust starters`, reopen it, read details, verify the demo browser guard, and quit. Terminal attributes and alternate-screen restoration are asserted.
- Run the PTY flow with `python3 tests/pty_smoke.py` after building the debug executable. It uses only Python's standard library and writes its raw terminal transcript to ignored `target/milestone-2-pty.log`.

## Scope and remaining verification

GitHub CLI is unavailable on the current host PATH. Live GitHub responses and actual browser launching were not exercised. Linux Docker provides the Rust build and PTY checks; native macOS and Windows validation remains outstanding.

Issues remain in memory. A focus reload fetches all issue pages for each currently matching repository with explicit `state=all`, `sort=updated`, and `direction=desc`; the default displayed state is open. Existing per-request time/output limits remain in force, and failures are visibly incomplete. Prior issue rows remain marked stale after a failed refresh. No daily sync, GitHub writes, commits, or releases are included.

Focus names are trimmed, case-insensitively unique, and limited to 128 UTF-8 bytes. Definitions use JSON fields `name`, `repository_query`, and `issue_query` in schema 2. Migration preserves existing tags and repositories; milestone-1 binaries reject schema 2.

The endpoint contract was checked against [GitHub's repository issues documentation](https://docs.github.com/en/rest/issues/issues#list-repository-issues), including pagination parameters and PR-shaped records.
