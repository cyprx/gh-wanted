# gh-wanted

Find where you’re needed.

A Rust terminal app for open-source contributors to organize watched repositories, discover useful issues, and keep track of contributions. Inspired by gh-dash, with repository watching and personal focus at its center.

## Core decisions

- Build a terminal UI in Rust.
- Use GitHub CLI (`gh`) for authentication and GitHub operations.
- Aim to support `gh wanted` as a GitHub CLI extension.
- Organize repositories by their GitHub topics and custom local tags.
- Keep local tags on the user’s machine; never publish them to GitHub.
- Treat issue labels, such as `bug` and `good first issue`, as separate issue filters.

## Main views

- Repositories: import watched repositories, show topics, and manage local tags.
- Today: daily updates covering new issues, maintainer replies, and PR activity.
- Focus: filter repositories by topics and local tags, then find relevant issues.
- Contributions: follow personal issues and PRs, including CI and review status.

Combine repository filters, for example GitHub topic `rust` plus local tag `priority`. Apply issue labels and keywords within that set of repositories. A personal issue shortlist is a proposed addition.

## Initial scope

1. Import watched repositories through `gh`.
2. Display GitHub topics and edit persistent local tags.
3. Filter repositories using topics and local tags together.
4. Show issues across the selected repositories.
5. Provide daily activity summaries and catch up when the app reopens.

Refresh while the app is running. Background scheduling and notifications while closed are outside the initial scope.

## Contribution workflow

Watch repositories → discover issues → choose work → contribute → track PRs.

Later actions could include opening an issue in the browser, cloning or forking a repository, creating a branch, opening an editor, and launching Lazygit for local Git work. Reuse existing tools instead of rebuilding their Git interfaces.

## References

- [gh-dash](https://github.com/dlvhdr/gh-dash): terminal issue and PR dashboard with configurable filters.
- [Lazygit](https://github.com/jesseduffield/lazygit): local Git operations and a potential integration.
- [DevHub](https://github.com/devhubapp/devhub): repository activity watching and saved searches.

## Open decisions

- Rust TUI framework and local storage format.
- Daily update boundaries, refresh interval, and read/unread behavior.
- Saved focus views and personal issue shortlist behavior.
- Packaging and GitHub CLI extension distribution.

The working name is `gh-wanted`. A quick web search found no exact match, but GitHub and crates.io availability have not been conclusively verified.
