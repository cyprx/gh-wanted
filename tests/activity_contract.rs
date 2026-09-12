#![cfg(unix)]
use gh_wanted::{
    github::{Account, GhClient},
    repositories::Repository,
    store::Store,
    sync::FeedRequest,
};
use std::{fs, os::unix::fs::PermissionsExt, time::Duration};

fn fake(body: &str) -> (tempfile::TempDir, GhClient) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gh");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let client = GhClient::new(path, Duration::from_secs(2));
    (dir, client)
}
fn request(feed: &str) -> FeedRequest {
    let mut store = Store::memory().unwrap();
    let now = gh_wanted::activity::timestamp("2026-09-11T12:00:00Z").unwrap();
    FeedRequest {
        account: Account {
            id: 1,
            login: "demo".into(),
        },
        repo: Repository {
            id: 1,
            full_name: "demo/repo".into(),
            description: None,
            topics: vec![],
            archived: false,
        },
        state: store.ensure_feed(1, 1, feed, now).unwrap(),
        upper: now,
    }
}
#[test]
fn follows_pages_and_failed_second_page_returns_no_snapshot() {
    let script = r#"
[ "$1" = api ] && [ "$2" = --hostname ] && [ "$3" = github.com ] || exit 9
if [ "$4" = user ]; then printf '%s' '{"id":1,"login":"demo"}'; exit 0; fi
[ "$5" = --include ] && [ "$#" = 5 ] || exit 9
case "$4" in
  'repos/demo/repo/issues?state=all&sort=updated&direction=asc&since=2026-09-10T12:00:00Z&per_page=100&page=1')
    printf 'HTTP/2.0 200 OK\r\nLink: <https://api.github.com/unused>; rel="next"\r\n\r\n'
    printf '%s' '[{"id":1,"number":1,"title":"New issue","created_at":"2026-09-11T12:00:00Z","updated_at":"2026-09-11T12:00:00Z","html_url":"https://github.com/demo/repo/issues/1"}]' ;;
  'repos/demo/repo/issues?state=all&sort=updated&direction=asc&since=2026-09-10T12:00:00Z&per_page=100&page=2')
    printf 'HTTP/2.0 200 OK\r\n\r\n[]' ;;
  *) exit 9 ;;
esac
"#;
    let (_dir, client) = fake(script);
    assert_eq!(client.activity(&request("issues")).unwrap().len(), 1);
    let failing = script.replace(
        "printf 'HTTP/2.0 200 OK\\r\\n\\r\\n[]'",
        "echo private-error >&2; exit 1",
    );
    let (_dir, client) = fake(&failing);
    let error = client.activity(&request("issues")).unwrap_err();
    assert!(!error.to_string().contains("private-error"));
}
#[test]
fn comment_and_review_endpoints_are_paginated_and_read_only() {
    for (feed,endpoint) in [("comments","repos/demo/repo/issues/comments?sort=updated&direction=asc&since=2026-09-10T12:00:00Z&per_page=100&page=1"),("reviews:7","repos/demo/repo/pulls/7/reviews?per_page=100&page=1")] {
        let script=format!(r#"
if [ "$4" = user ]; then printf '%s' '{{"id":1,"login":"demo"}}'; exit 0; fi
[ "$4" = '{endpoint}' ] && [ "$5" = --include ] && [ "$#" = 5 ] || exit 9
printf 'HTTP/2.0 200 OK\r\n\r\n[]'
"#);
        let (_dir,client)=fake(&script);
        assert!(client.activity(&request(feed)).unwrap().is_empty());
    }
}
#[test]
fn rate_limit_headers_survive_cli_failure_and_account_changes_reject_feed() {
    let script = r#"
if [ "$4" = user ]; then printf '%s' '{"id":1,"login":"demo"}'; exit 0; fi
printf 'HTTP/2.0 429 Too Many Requests\r\nRetry-After: 7200\r\n\r\n{}'
exit 1
"#;
    let (_dir, client) = fake(script);
    let now = chrono::Utc::now().timestamp();
    let error = client.activity(&request("comments")).unwrap_err();
    let failure = error
        .downcast_ref::<gh_wanted::sync::RequestFailure>()
        .unwrap();
    assert!(failure.retry_at.unwrap() >= now + 7200);
    let changed = r#"
if [ "$4" = user ]; then
  if [ -f "$0.count" ]; then printf '%s' '{"id":2,"login":"other"}'; else touch "$0.count"; printf '%s' '{"id":1,"login":"demo"}'; fi
else printf 'HTTP/2.0 200 OK\r\n\r\n[]'; fi
"#;
    let (_dir, client) = fake(changed);
    assert!(
        client
            .activity(&request("issues"))
            .unwrap_err()
            .downcast_ref::<gh_wanted::sync::RequestFailure>()
            .unwrap()
            .paused
    );
}
