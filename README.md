# gh-wanted

Find where you're needed. A Rust TUI for organizing watched GitHub repositories by their upstream topics and your own local tags.

## Milestones 1 and 2

- Import watched repositories from github.com through `gh`.
- Read GitHub topics and edit private local tags.
- Combine topic, local-tag, and text filters.
- Cache repository metadata and tags in SQLite, isolated by GitHub account ID.
- Explore fictional repositories with `--demo`, using temporary in-memory storage.
- Browse issues across filtered repositories, with label, keyword, state, and unassigned filters.
- Save named focuses that restore both filters and follow current repository membership.

Daily digests, PR tracking, and local Git actions belong to later milestones.

## Run locally

Rust tooling runs through the Nix devShell. The TUI runs on the host, using the host's GitHub CLI authentication.

```sh
nix develop path:.
cargo run -- --demo
# Authenticate separately for real repositories:
gh auth login --hostname github.com
cargo run
```

Or run a command directly through the devShell:

```sh
nix develop path:. -c cargo run -- --demo
```

`gh-wanted --help` lists command-line options. The application only makes read requests to GitHub. It never stores tokens or sends local tags to GitHub.

## Keys

| Key | Action |
| --- | --- |
| j/k or arrows | Move through repositories |
| Tab | Toggle full repository details |
| / | Edit filter; Enter applies, Esc cancels |
| t | Edit comma-separated local tags; Enter saves |
| i | Browse issues from all matching repositories |
| b | Return to repositories |
| s | Save repository and issue filters as a named focus |
| f | List saved focuses; Enter reopens one |
| o | Open selected issue in the browser (disabled in demo) |
| PgUp/PgDn | Scroll issue details |
| r | Refresh watched repositories |
| ? | Show keyboard help |
| Esc | Clear active filter when browsing |
| q or Ctrl-C | Quit |

Example filter: `topic:rust tag:priority`. Quote values containing spaces, for example `tag:"needs review"`. All specified topics and tags must match. Other text searches repository names and descriptions, case-insensitively. Topics appear as `#rust`; local tags appear as `+priority`. GitHub topics are read-only.

Tags are trimmed, lowercased, and deduplicated. Blank entries, control characters, and tags longer than 128 UTF-8 bytes are rejected. Clear the whole tag input to remove all tags for that repository. Changes are saved only on Enter.

## Find an issue and save a focus

1. Tag a repository `priority` with `t`, then apply `topic:rust tag:priority` using `/`.
2. Press `i` to load issues from all matching repositories.
3. Press `/` and apply `label:"good first issue" unassigned`. Multiple labels must all match. Add words to search the title or body as a case-insensitive phrase. State defaults to `open`; use `state:closed` or `state:all` to change it.
4. Use arrows to choose an issue, Tab for details, and PgUp/PgDn to scroll. Press `o` to open a validated HTTPS github.com issue URL.
5. Press `s`, enter a unique focus name, and save with Enter. Press `f` and Enter to reopen it, including after restarting normal mode.

Focus names are trimmed, case-insensitively unique, and limited to 128 UTF-8 bytes. Duplicate saves fail without overwriting the existing focus. Definitions store the repository and issue filter expressions as JSON in SQLite, partitioned by GitHub account. They store no fixed repository list: changes to local tags, watched repositories, or refreshed topics change membership. Demo focuses and issues are temporary.

Issue fetches are read-only, per repository, paginated, and explicitly sorted by update time. PR records are excluded. The completion count stays incomplete until every selected repository succeeds. A timeout, failed page, or output-limit failure retains any previous results for that repository with a stale/incomplete indicator; `r` retries. Issues are held in memory and reloaded when a focus is reopened. Large feeds can exceed the existing 30-second/16 MiB request bounds and remain visibly incomplete. There is one background refresh at a time; if another refresh is running, press `r` after it finishes.

The database upgrades transactionally from schema 1 to schema 2, preserving repositories and tags. Older milestone-1 binaries cannot open schema 2.

## Docker builds and tests

Docker is for builds and tests, not for hosting the interactive app or your GitHub credentials. Commands require a running Docker daemon. No credentials or home-directory mounts are needed.

```sh
docker build --target test -t gh-wanted-test .
docker build --target build -t gh-wanted-build .
# Optional: export the Linux executable built for Docker's architecture.
docker build --target artifact --output type=local,dest=dist .
```

The exported Linux executable is not a native macOS binary. Use the devShell for a native host build.

The test target runs formatting checks, Clippy with warnings denied, and the complete offline test suite. Dependencies are fixed by Cargo.lock. The build target compiles the release executable.

## Local data and failure behavior

Persistent data uses the platform local-data directory from the `directories` crate, under application name `gh-wanted`, in `state.sqlite3`. On macOS this is `~/Library/Application Support/gh-wanted/state.sqlite3`. Demo mode never opens this file.

Tags follow stable repository IDs across renames and remain stored when a repository is no longer watched. Each GitHub account has separate cached repositories and tags. The app verifies the active account before loading its cache, so an offline startup cannot load a cached account automatically. Once connected, refresh errors retain cached data with a stale-data message.

Refresh runs in the background, with one sync at a time. Each `gh` call has a 30-second timeout and a 16 MiB combined output limit. Errors preserve stored data; corrupt or newer database schemas are reported instead of reset. Press `r` to retry authentication, connectivity, or rate-limit failures. Refresh is manual in milestone 1.

## Development checks

```sh
nix develop path:. -c cargo fmt --check
nix develop path:. -c cargo clippy --locked --all-targets -- -D warnings
nix develop path:. -c cargo test --locked
```

Tests use synthetic GitHub responses, fake CLI executables, temporary SQLite databases, and Ratatui's test backend. They do not require GitHub authentication. See `docs/superpowers/plans/2026-09-11-gh-wanted.md` for the broader plan.
