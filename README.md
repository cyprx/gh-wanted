# gh-wanted

**Find where you're needed.**

A Rust terminal app for following GitHub repositories, catching up on activity, and finding issues to contribute to. Organize repositories with private local tags, narrow your search, and save the combinations you use most.

![gh-wanted Today view showing recent activity, catch-up items, and update details](docs/images/demo.png)

*Demo data. Try it without a GitHub account with `cargo run -- --demo`.*

## Features

- **Today and catch-up:** recent issues, comments, PR changes, and on-demand reviews. Earlier unread activity stays until you acknowledge it.
- **Repository organization:** combine GitHub topics, private local tags, and text filters.
- **Issue discovery:** search across matching repositories by label, state, assignee status, and keywords, with seven- or 30-day update windows.
- **Readable details:** focused List/Details panes, wrapped descriptions, basic Markdown blocks, and keyboard scrolling.
- **Saved focuses:** reopen named repository and issue filters.
- **Efficient refresh:** incremental activity checkpoints, up to three concurrent feed workers, shared rate-limit pauses, and local timing metrics.

GitHub operations are read-only. Tags, saved focuses, and read state stay on your machine.

## Get started

Install Rust **1.88 or newer**, its platform build prerequisites, and the GitHub CLI (`gh`). Run these commands from the repository directory:

```sh
# Explore fictional data without authentication.
cargo run --locked -- --demo

# Use your GitHub account.
gh auth login --hostname github.com
cargo run --locked
```

To install the executable locally:

```sh
cargo install --locked --path .
gh-wanted
```

On macOS or Linux, the included Nix development shell supplies Rust tooling and GitHub CLI:

```sh
nix develop path:.
cargo run --locked -- --demo
```

A terminal of **100 × 30** or larger is recommended. Compact layouts work from 42 × 14; your terminal controls the font.

## Your daily workflow

The app opens on **Today**. After account verification it displays cached activity and refreshes in the background. Today includes read and unread items from your local calendar day; catch-up contains earlier unread activity. Press `a` to acknowledge an item. Opening it or refreshing does not mark it read.

Press `b` to organize repositories. Add local tags with `t`, then filter with `/`:

```text
topic:rust tag:priority
tag:"needs review"
```

Topics and tags combine with AND. Other text searches repository names and descriptions. Tags are private, normalized to lowercase, and saved only when you press Enter.

Press `i` to discover issues across the matching repositories:

```text
label:"good first issue" unassigned
days:30 label:bug state:open
days:7 state:all documentation
```

The default is **open issues updated in the last seven days**. Use `days:30` for a wider window. Multiple labels must all match; keywords search the title and body as a case-insensitive phrase. Pull requests are excluded from the Issues list.

Press Enter to focus Details. Use `j/k` or PgUp/PgDn to scroll, and Esc to return to List. Details expands to full width on narrow terminals. Press `o` to open the issue in your browser.

Press `s` to save your repository and issue filters as a named focus. Press `f`, select it, and press Enter to reopen it. Focus membership follows your current repositories and tags.

## Keyboard controls

| Key | Action |
| --- | --- |
| `d` / `i` / `b` / `f` | Today / Issues / Repos / Focuses |
| `j/k` or arrows | Move through a list; scroll focused Issue Details |
| Enter | Focus Repo/Issue Details, open a focus, or submit input |
| Esc | Cancel input, leave focused Details, or clear the current filter |
| Tab | Toggle expanded details |
| PgUp / PgDn | Scroll details or feed status |
| `/` | Edit the repository or issue filter |
| `t` | Edit local repository tags |
| `s` | Save a focus from Repos or Issues |
| `o` | Open the selected repository, issue, or activity in a browser |
| `a` | Toggle acknowledgment in Today |
| `u` | Toggle unread-only activity |
| `e` | Show activity checkpoints and errors |
| `v` | Fetch reviews for selected PR activity |
| `r` | Refresh the current view |
| `?` | Show help |
| `q` or Ctrl-C | Quit |

Browser opening is disabled in demo mode.

## Refresh and data

Activity refreshes every 15 minutes while the app is running. The initial activity window is 24 hours; later refreshes resume from saved checkpoints with overlap and deduplication. A failed feed preserves its checkpoint and cached data.

**Repos → `r` reloads the watched-repository list.** Today refreshes activity for repositories already loaded. Newly watched repositories need a Repo refresh first. GitHub custom notification subscriptions may be absent from the watched-list API; the app currently imports only repositories returned by that API. Releases are not currently tracked.

Issue windows use **last updated time**, so an old issue with recent activity can appear. Quiet older issues are outside the window. Changing the window requests a refresh; if another refresh is running, retry with `r` afterward.

There is one refresh at a time, with up to three feed workers. Known-active repositories are prioritized. Each refresh binds one credential and verifies its account; switching the CLI account affects the next refresh. Credentials are held in memory for requests and are not written to the app's database or logs.

Cached repositories, tags, focuses, activity, and acknowledgments are stored in `state.sqlite3` under the platform's local application-data directory. Data is isolated by GitHub account. Issues are held in memory. Demo mode uses temporary in-memory data.

Each CLI call has a 30-second timeout and a 16 MiB output limit. Very busy windows can still exceed these limits; incomplete results remain labeled and cached data is retained. Rate-limit deadlines apply to manual retries too. Polling is not a complete event audit, and nothing runs after the app closes.

To inspect local refresh measurements:

```sh
gh-wanted --refresh-metrics
# From source:
cargo run --locked -- --refresh-metrics
```

This prints the log location and summaries without contacting GitHub. Logs rotate at roughly 10 MiB total and contain no response bodies or credentials. See [refresh monitoring](docs/refresh-monitoring.md) for fields and interpretation.

## Development

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Tests use synthetic GitHub responses, temporary databases, and terminal test backends. They do not require GitHub authentication. On Linux, build the app and run `python3 tests/pty_smoke.py` for the demo keyboard walkthrough.

Docker build targets are also available:

```sh
docker build --target test -t gh-wanted-test .
docker build --target build -t gh-wanted-build .
docker build --target artifact --output type=local,dest=dist .
```

The artifact target exports a Linux executable for Docker's architecture. Build on your host for a native executable.
