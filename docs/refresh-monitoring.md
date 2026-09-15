# Refresh monitoring

`scheduler_wait` events record milliseconds spent waiting for the active tab or a free request slot. Feed and refresh durations include this wait, so a long elapsed refresh can reflect time spent on another tab rather than slow GitHub responses. Scheduling pauses between CLI calls and shares a three-request limit across views.

Normal runs automatically append local diagnostics beside `state.sqlite3`.
Run `cargo run -- --refresh-metrics` (or `gh-wanted --refresh-metrics`) to
print that path and JSON summaries. No GitHub requests are made by this command.
Demo mode does not produce refresh measurements.

The files are `refresh-metrics.jsonl` and `refresh-metrics.previous.jsonl`.
The current file rotates at 5 MiB, keeping one previous file (roughly 10 MiB
total). Delete both while the app is closed to reset the baseline. Diagnostics
are best effort: denied writes or a full disk do not interrupt refresh. If the
report is empty after a normal refresh, check write access to the printed path.

## What is recorded

Issue runs record `issue_window` events with the repository ID, window length and
UTC cutoff. Summaries expose `issue_window_days`; compare runs with the same
window. The default is seven days, with `days:30` available in the Issues filter.

- Run ID, refresh kind, UTC timestamps, monotonic elapsed time, start and close.
- Operation start/end, numeric repository ID, activity feed, duration, result
  count and success/failure. Activity windows distinguish initial from incremental
  fetching. The `worker` field correlates operations and requests when they overlap.
- CLI call start/end, endpoint category, duration, bytes, observed pages and raw
  record counts. Account checks are included and separately summarized.
- Available HTTP status, rate quota remaining/reset, retry deadline and failure
  category. Failed responses retain only allowlisted numeric header fields.

No credentials, response bodies, issue titles, repository names, raw errors or
query strings are logged. Numeric repository IDs, account IDs and PR numbers can be present.
Nothing is uploaded. Checkpoint and acknowledgment semantics are unchanged.

## Account-bound concurrent refreshes

New runs include `account_bound: true` in summaries; issue/activity plans also
include `worker_limit: 3`. Credentials are captured once from the environment
or `gh auth token`, verified through `/user`, then held in memory and supplied
to each CLI child through `GH_TOKEN`. A CLI account switch cannot change the
credential of an in-flight refresh. A new refresh verifies its credential again;
an account mismatch rejects issue/activity work until repositories reconnect.
Credential capture timing is recorded separately, with no token content.

Three workers process a finite, stable priority queue. Persisted activity and
in-session issue results prioritize known-active repositories. Quiet repositories
remain in the queue. Completed results reach the UI independently; SQLite writes
remain on the UI thread. No new schema or persistent priority cache is added.

Rate/auth failures pause new requests across the refresh. Up to three requests
may already be in flight; these may finish. Both issue and activity pagination
check the shared pause before fetching another page. A failed multi-page feed
still produces no partial result and never advances its checkpoint.

## Interpretation

`cli_calls` counts completed CLI invocations, **not HTTP requests**. Successful
`--paginate --slurp` output supplies observed pages, but internal retries and
partial pages on failure are not observable. `unknown_page_calls` makes missing
counts explicit; `observed_pages` is a lower bound. Calls include subprocess
startup and network time, not pure server latency. Successful account and bulk
issue/subscription calls do not expose HTTP headers in the existing CLI mode;
missing quota/status values mean unknown, not zero.

`first_nonempty_result_ms` is the first parsed nonempty operation result, not
the first newly inserted event or first rendered frame. Returned items may
overlap cached data. `elapsed_ms` spans the worker client's lifetime, including
UI-channel waiting, but excludes final UI persistence after the client closes.
`closed` indicates client shutdown, not a successful refresh; inspect failure
counts. A killed process may leave starts without finishes. Rotation can remove
the beginning of older runs, so retained summaries may be partial.

Use the raw operation durations to find slow feeds and the request durations to
separate account overhead from data retrieval. Compare first sync, ordinary
incremental sync and a no-change repeat against the same watch list. Look at
page counts before considering batching; look at account duration and total
CLI duration when assessing concurrency; inspect rate-limit events before
increasing dispatch. Run one app instance when collecting a comparison baseline.
With concurrency, summed call duration can exceed wall time; use `elapsed_ms`
for user wait time. Bound issue refreshes now fetch explicit pages with headers,
so their page counts and quota measurements are more complete than older runs.

This slice measures retrieval. Scheduling/queue latency, cache-hit rate, actual
new-event counts, and time to visible UI update need separate instrumentation
before making claims about end-to-end freshness.
