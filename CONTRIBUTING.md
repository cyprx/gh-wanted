# Contributing

Small fixes, clearer docs, and reproducible bug reports are welcome. For a larger feature, open an issue first so we can agree on scope.

## Run locally

Install Rust and GitHub CLI (`gh`). CI uses Rust 1.94.

```sh
cargo run --locked -- --demo
```

Demo mode needs no GitHub account. For real data, run `gh auth login --hostname github.com`, then `cargo run --locked`.

## Check your change

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

On Linux, also run `cargo build --locked` and `python3 tests/pty_smoke.py` for the terminal walkthrough. Some GitHub CLI contract tests run only on Unix; CI covers Linux, Windows, and macOS.

Keep pull requests focused. Describe the problem, the behavior after your change, and how you tested it. Add a regression test for behavior changes where practical. Use synthetic data in tests; do not commit credentials or your local database.

## Report a bug

Include your OS, terminal, app version or commit, steps to reproduce, and what you expected. A screenshot helps for layout problems. For refresh problems, include relevant output from `gh-wanted --refresh-metrics` after checking it for information you don't want to share.

Contributions are licensed under the repository's [MIT license](LICENSE).

Local tracking adds schema 4 with a separate table; migration preserves watched repositories, tags, focuses, and activity. Older binaries reject schema 4. When testing a downgrade, use a pre-upgrade database backup; do not change the schema version by hand on real data.
