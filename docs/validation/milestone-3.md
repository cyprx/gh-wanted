# Milestone 3 validation

Implemented persisted daily activity with local Today grouping, earlier unread catch-up, explicit acknowledgment, and 15-minute refresh while running. Issue, comment, and on-demand PR-review feeds keep independent account/repository checkpoints. No GitHub writes or background daemon are introduced.

## Fetch contracts

- `GET /repos/{owner}/{repo}/issues`: `state=all`, `sort=updated`, `direction=asc`, `since`, 100 records per page. PR-shaped records become PR-change activity; they remain excluded from milestone-2 issue discovery. New issue and issue-change activity use their separate source timestamps.
- `GET /repos/{owner}/{repo}/issues/comments`: `sort=updated`, `direction=asc`, `since`, 100 records per page. Comments remain comments; author association is displayed verbatim rather than inferred from timestamps.
- `GET /repos/{owner}/{repo}/pulls/{number}/reviews`: paginated on demand with `v`. Only submitted reviews are included. Review ID, submission time, and state identify versions; the anchored review window is reread to detect state changes.
- Activity pagination uses response Link headers and bounded CLI calls. A failed later page returns no partial snapshot. A final account check rejects results fetched across an account change.
- Fine-grained private-repository access requires Issues or Pull requests read permission for issue/comment feeds, and Pull requests read permission for reviews. Public resources may be readable without these grants. Authentication remains entirely in `gh`.
- The PR and check endpoints were reviewed, but actual CI checks and the full contributions workspace remain separate work. Check status must come from the Checks API, not an issue timestamp.

Sources: [issues](https://docs.github.com/en/rest/issues/issues#list-repository-issues), [comments](https://docs.github.com/en/rest/issues/comments#list-issue-comments-for-a-repository), [PRs](https://docs.github.com/en/rest/pulls/pulls#list-pull-requests), [reviews](https://docs.github.com/en/rest/pulls/reviews#list-reviews-for-a-pull-request), [checks](https://docs.github.com/en/rest/checks/runs#list-check-runs-for-a-git-reference), and [rate-limit guidance](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api#handle-rate-limit-errors-appropriately).

## Persistence and retry behavior

Schema 3 transactionally adds activity and feed-state tables, preserving schema-1/2 repositories, tags, and focuses. Each feed anchors its first 24-hour history window before requesting data. Later issue/comment fetches overlap the last successful checkpoint by 60 seconds. Activity insertion and checkpoint advancement commit together; acknowledgments are stored independently and survive duplicate results. Superseded requests, clock rollback, failed pages, and invalid snapshots cannot advance a checkpoint.

Authentication and permission errors pause automatic refresh. Rate-limit Retry-After and exhausted X-RateLimit-Reset headers determine the minimum retry time, also enforced for manual requests. Rate/auth failure stops the remaining batch's requests, leaving unsuccessful feeds visibly incomplete. Repository, issue, and activity refreshes share one worker. Review feeds refresh only on demand.

## Verification coverage

Final validation on 2026-09-12 passed in Linux Docker: all 43 Rust tests, `cargo fmt --check`, Clippy with warnings denied, and the full PTY walkthrough. The corrected demo review displays PR #2. Terminal settings and alternate-screen restoration passed. `git diff --check` passed.

Tests cover two-session catch-up, anchored initial history after failure, duplicate/equal timestamp identities, source kinds, future upper bounds, local midnight and timezone changes, acknowledgment versus Today, account switching, atomic migration/commit rollback, stale-result rejection, pending reviews, review-state versions, per-feed failures, CLI pagination, rate-limit headers, scheduler overlap/pause rules, and UI rendering.

The Linux PTY walkthrough covers the previous repository/focus workflow, then Today/catch-up, acknowledgment surviving refresh, unread filtering, on-demand PR reviews, feed checkpoints, and terminal restoration. Reproduce with `python3 tests/pty_smoke.py` after building the debug executable. Its transcript is written to ignored `target/milestone-3-pty.log`.

## Limits and remaining verification

The current host has no `gh` on PATH, so authenticated live GitHub verification was unavailable. Native macOS/Windows builds and actual browser launching remain unverified; Rust and terminal checks use Linux Docker.

Polling records available snapshots, not a full event audit. Deleted/inaccessible activity, intermediate edits, late indexing beyond the overlap, and review edits without a source timestamp/state change may not be reconstructable. The activity-feed 16 MiB limit may prevent a large catch-up from completing; this remains visibly incomplete without advancing its checkpoint. The original account-verification requirement still prevents automatic cached-account loading during a fully offline startup.

Milestone 2 is preserved in commit `4a22de1`.
