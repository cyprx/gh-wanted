# gh-wanted

**Find where you're needed.**

A terminal app for catching up on watched GitHub repositories and finding issues to contribute to.

![gh-wanted demo: Today activity and focused details](docs/images/demo.png)

## What it does

- **Today:** catch up on issues, comments, PR changes, and reviews. Keep track of what you've read.
- **Issues:** find work by label, keyword, or assignee, updated in the last 7 or 30 days.
- **Repos & Focuses:** organize repositories with private tags and save your favorite filters.

Refresh follows the active tab, pausing and resuming as you move. GitHub access is read-only; tags, saved filters, and read state stay on your machine.

## Get started

Install Rust **1.88+** and the GitHub CLI (`gh`), then run from this repository:

```sh
# Try fictional data without an account
cargo run --locked -- --demo

# Connect to GitHub
gh auth login --hostname github.com
cargo run --locked
```

To install locally: `cargo install --locked --path .`, then run `gh-wanted`.
A terminal of **100 × 30** or larger is recommended.

## Controls

| Key | Action |
| --- | --- |
| `d` / `i` / `b` / `f` | Today / Issues / Repos / Focuses |
| `j/k` or arrows | Move through lists or scroll focused details |
| Enter / Esc | Focus details / return to list |
| PgUp / PgDn | Scroll details by page |
| Tab / `o` | Expand details / open in browser |
| `/` / `t` / `s` | Filter / edit local tags / save focus |
| `a` / `u` | Mark activity read or unread / show unread only |
| `r` / `e` | Refresh / show Today sync details |
| `?` / `q` | All shortcuts / quit |

Example filters:

```text
# Repos
topic:rust tag:priority

# Issues
label:"good first issue" unassigned
days:30 label:bug state:open
```

Newly watched repository missing? Press `b`, then `r`. GitHub custom notification subscriptions may be absent from its watched-list API. Releases aren't tracked yet. Browser opening is disabled in demo mode.

## Development

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

CI checks Linux, Windows, and macOS. To publish, bump the version in `Cargo.toml` and `Cargo.lock`, then push to `release`; the workflow tests, builds, and publishes `v<version>` with binary archives and checksums. Existing tags are never replaced.

For refresh diagnostics, run `gh-wanted --refresh-metrics`. See [refresh monitoring](docs/refresh-monitoring.md).
