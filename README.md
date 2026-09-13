# gh-wanted

Find where you're needed. A Rust TUI for organizing watched GitHub repositories by their upstream topics and your own local tags.

## Milestones 1 through 3

- Import watched repositories from github.com through `gh`.
- Read GitHub topics and edit private local tags.
- Combine topic, local-tag, and text filters.
- Cache repository metadata and tags in SQLite, isolated by GitHub account ID.
- Explore fictional repositories with `--demo`, using temporary in-memory storage.
- Browse issues across filtered repositories, with label, keyword, state, and unassigned filters.
- Save named focuses that restore both filters and follow current repository membership.
- Review Today and earlier unread activity with persistent checkpoints and local acknowledgment.

The full contributions workspace, CI tracking, and local Git actions belong to later milestones.

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

The interface uses a charcoal palette with amber navigation, teal GitHub topics, and padded list/detail panels. The active tab and selected row are labeled as well as colored. A terminal of 100 × 30 or larger is recommended; compact views work from 42 × 14. Font choice remains controlled by your terminal.

| Key | Action |
| --- | --- |
| j/k or arrows | Move through the current list |
| Tab | Toggle full details |
| / | Edit filter; Enter applies, Esc cancels |
| t | Edit comma-separated local tags; Enter saves |
| i | Browse issues from all matching repositories |
| b | Return to repositories |
| s | Save repository and issue filters as a named focus |
| f | List saved focuses; Enter reopens one |
| d | Show Today and catch-up activity |
| a | Toggle local acknowledgment of selected activity |
| u | Toggle unread-only activity |
| e | Show/hide activity feed checkpoints and failures |
| v | Load reviews for the selected PR activity on demand |
| o | Open selected issue/activity in the browser (disabled in demo) |
| PgUp/PgDn | Scroll details or feed status |
| r | Refresh the current view |
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

The database upgrades transactionally from schema 1 or 2 to schema 3, preserving repositories, tags, and focuses. Older milestone binaries cannot open schema 3.

## Today and catch-up

Press `d` after connecting. Activity sync starts after repository loading, then runs every 15 minutes while the app is open. It fetches all watched repositories; the current repository filter controls what the daily view displays. Repository and issue discovery refreshes remain manual. All refreshes share one worker, so a slow request does not block navigation or overlap another sync.

Today uses the host's local calendar date, including timezone and daylight-saving rules. It includes both read and unread activity. Catch-up contains earlier unread items and keeps them until you explicitly acknowledge them with `a`. Opening details or refreshing never marks activity read. `u` shows only unread items. Activity, read state, and checkpoints survive restart, partitioned by GitHub account.

Each issue/comment feed starts with the last 24 hours. That initial boundary persists even if the first fetch fails. Later refreshes query from the last successful checkpoint with a 60-second overlap, deduplicate source IDs and versions, and atomically save activity with the new checkpoint. A failed page, timeout, cancellation, or failed save leaves the checkpoint unchanged. `e` shows each feed's checkpoint and error; `r` retries after any rate-limit deadline. Authentication/permission failures pause automatic requests until a manual retry. Rate-limit response headers set the minimum retry time; no background process runs after quitting.

The feed distinguishes new issues, issue changes, comments, PR changes, and submitted reviews. Author association is shown as supplied by GitHub; a generic timestamp change is never described as a maintainer reply or a CI result. Select PR activity and press `v` to fetch its reviews. Review fetching is on demand and uses its own checkpoint; it rereads the initial review window to detect review-state changes. Pending reviews are excluded. Review discussion threads and CI checks belong to later contributions work.

Polling captures available snapshots, not a complete event audit: deleted/inaccessible records and intermediate edits may be unavailable, late indexing outside the overlap may be missed, and review edits without a changed state or source timestamp may not create another item. A long absence can exceed the 16 MiB activity-feed limit; the feed remains visibly incomplete and its checkpoint does not advance. Issue/comment activity pages each retain the 30-second CLI timeout. Demo data includes Today, catch-up, acknowledgment, and a PR review without using real GitHub data.

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

Refresh runs in the background, with one refresh at a time and up to three feed workers. Each refresh captures a credential in memory and verifies its GitHub account once. Switching the active CLI account affects the next refresh; in-flight results remain attached to the verified account. Previously active repositories are fetched first using persisted activity and in-session issue results. Each `gh` call has a 30-second timeout and a 16 MiB combined output limit. Errors preserve stored data; corrupt or newer database schemas are reported instead of reset. Press `r` to retry authentication or connectivity failures; rate-limit deadlines also apply to manual retries.

## Development checks

In Issues, Enter focuses Details and Esc returns to List. The active pane has an
accent border. List uses `j/k` to select issues; Details uses `j/k` to scroll and
PgUp/PgDn to move a viewport. Tab toggles expanded details, and `o` opens the issue
in a browser. Narrow terminals show Details full-width when focused. Descriptions
support basic Markdown headings, lists, quotes, and fenced code blocks, with
wrapped text and scrolling bounded to the rendered content.

Issue discovery defaults to issues updated within the last seven days. In Issues,
press `/` and add `days:30` for the last 30 days, or `days:7` to switch back.
Changing the window requests a refresh; if another refresh is running, press `r`
after it finishes. Saved focuses preserve this filter; older focuses default to
seven days. The window is applied on GitHub before pagination. Very busy windows
can still exceed the 16 MiB limit; failed refreshes preserve cached results and
remain marked incomplete. Today/catch-up checkpoints are unaffected.

The app opens on Today with cached activity after account verification. Tabs are
ordered Today, Issues, Repos, Focuses; existing `d`, `i`, `b`, and `f` shortcuts
are unchanged. Background account loading preserves the tab you have selected.

Normal refreshes save bounded local timing and request diagnostics beside the
database. Run `cargo run -- --refresh-metrics` to print the log path and retained
run summaries. Demo mode produces no measurements. See
[refresh monitoring](docs/refresh-monitoring.md) for recorded fields, retention,
and measurement limits.

```sh
nix develop path:. -c cargo fmt --check
nix develop path:. -c cargo clippy --locked --all-targets -- -D warnings
nix develop path:. -c cargo test --locked
```

Tests use synthetic GitHub responses, fake CLI executables, temporary SQLite databases, and Ratatui's test backend. They do not require GitHub authentication. See `docs/superpowers/plans/2026-09-11-gh-wanted.md` for the broader plan.
