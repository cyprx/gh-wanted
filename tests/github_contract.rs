#![cfg(unix)]

use gh_wanted::github::GhClient;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

fn fake(body: &str, timeout: Duration) -> (tempfile::TempDir, GhClient) {
    let directory = tempfile::tempdir().unwrap();
    let program = directory.path().join("gh");
    fs::write(&program, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    let client = GhClient::new(program.into_os_string(), timeout);
    (directory, client)
}

fn response(pages: &str) -> String {
    format!(
        r#"
[ "$1" = api ] && [ "$2" = --hostname ] && [ "$3" = github.com ] || exit 2
[ "$GH_PROMPT_DISABLED" = 1 ] || exit 3
if [ "$4" = user ]; then
  [ "$#" = 4 ] || exit 4
  printf '%s' '{{"id":7,"login":"alice"}}'
else
  [ "$4" = 'user/subscriptions?per_page=100' ] && [ "$5" = --paginate ] && [ "$6" = --slurp ] && [ "$#" = 6 ] || exit 5
  printf '%s' '{pages}'
fi
"#
    )
}

#[test]
fn paginated_subscriptions_are_deduplicated_by_id() {
    let pages = r#"[[{"id":1,"full_name":"a/one","description":null,"topics":["rust"],"archived":false}],[{"id":1,"full_name":"a/one","description":null,"topics":[],"archived":false},{"id":2,"full_name":"b/two","description":"two","topics":[],"archived":true}]]"#;
    let (_dir, client) = fake(&response(pages), Duration::from_secs(2));
    let snapshot = client.sync().unwrap();
    assert_eq!(snapshot.account.login, "alice");
    assert_eq!(snapshot.repositories.len(), 2);
    assert_eq!(snapshot.repositories[0].topics, ["rust"]);
    assert!(snapshot.repositories[1].archived);
}

#[test]
fn empty_subscriptions_are_valid() {
    let (_dir, client) = fake(&response("[[]]"), Duration::from_secs(2));
    assert!(client.sync().unwrap().repositories.is_empty());
}

#[test]
fn refresh_metrics_capture_pagination_account_overhead_and_safe_results() {
    let (dir, client) = fake(&response("[[],[]]"), Duration::from_secs(2));
    let path = dir.path().join("metrics.jsonl");
    let client = client.with_metrics(path.clone(), "repositories");
    client.sync().unwrap();
    drop(client);
    let raw = fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("alice"));
    let summary: Vec<serde_json::Value> =
        serde_json::from_str(&gh_wanted::metrics::report(&path).unwrap()).unwrap();
    assert_eq!(summary[0]["cli_calls"], 3);
    assert_eq!(summary[0]["account_calls"], 2);
    assert_eq!(summary[0]["observed_pages"], 4);
    assert_eq!(summary[0]["items_returned"], 0);
    assert!(summary[0].get("first_nonempty_result_ms").is_none());
}

#[test]
fn account_can_be_checked_without_reading_subscriptions() {
    let (_dir, client) = fake(&response("invalid-unused-data"), Duration::from_secs(2));
    assert_eq!(client.account().unwrap().id, 7);
}

#[test]
fn malformed_account_is_rejected() {
    let (_dir, client) = fake("printf '%s' '{}'", Duration::from_secs(2));
    assert!(client
        .account()
        .unwrap_err()
        .to_string()
        .contains("invalid account response"));
}

#[test]
fn malformed_response_is_rejected_without_echoing_content() {
    let (_dir, client) = fake(&response("private-invalid-payload"), Duration::from_secs(2));
    let error = client.sync().unwrap_err().to_string();
    assert!(error.contains("invalid subscription response"));
    assert!(!error.contains("private-invalid-payload"));
}

#[test]
fn command_failure_does_not_expose_stderr() {
    let (_dir, client) = fake("echo private-stderr >&2; exit 1", Duration::from_secs(2));
    let error = client.sync().unwrap_err().to_string();
    assert!(error.contains("request failed"));
    assert!(!error.contains("private-stderr"));
}

#[test]
fn hanging_command_is_killed_and_reaped() {
    let (_dir, client) = fake("exec sleep 10", Duration::from_millis(50));
    let started = Instant::now();
    assert!(client.sync().unwrap_err().to_string().contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn missing_executable_returns_actionable_error() {
    let directory = tempfile::tempdir().unwrap();
    let client = GhClient::new(
        directory.path().join("missing").into_os_string(),
        Duration::from_secs(1),
    );
    assert!(client
        .sync()
        .unwrap_err()
        .to_string()
        .contains("Could not start GitHub CLI"));
}

#[test]
fn changed_account_rejects_snapshot() {
    let script = r#"
counter="$0.count"
if [ "$4" = user ]; then
  if [ -f "$counter" ]; then
    printf '%s' '{"id":8,"login":"bob"}'
  else
    touch "$counter"
    printf '%s' '{"id":7,"login":"alice"}'
  fi
else
  printf '%s' '[[]]'
fi
"#;
    let (_dir, client) = fake(script, Duration::from_secs(2));
    assert!(client
        .sync()
        .unwrap_err()
        .to_string()
        .contains("account changed"));
}

#[test]
fn excessive_output_is_rejected() {
    let (_dir, client) = fake("head -c 16777217 /dev/zero", Duration::from_secs(5));
    assert!(client
        .sync()
        .unwrap_err()
        .to_string()
        .contains("16 MiB limit"));
}

#[test]
fn cancellation_stops_inflight_process() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let (dir, client) = fake("exec sleep 10", Duration::from_secs(20));
    let cancelled = Arc::new(AtomicBool::new(false));
    let client = client.with_cancellation(cancelled.clone());
    let started = Instant::now();
    let worker = std::thread::spawn(move || client.account());
    std::thread::sleep(Duration::from_millis(50));
    cancelled.store(true, Ordering::Relaxed);
    assert!(worker
        .join()
        .unwrap()
        .unwrap_err()
        .to_string()
        .contains("cancelled"));
    assert!(started.elapsed() < Duration::from_secs(2));
    drop(dir);
}

#[test]
fn issue_fetch_has_explicit_parameters_and_rejects_failed_pages() {
    let script = r#"
if [ "$4" = user ]; then
  printf '%s' '{"id":7,"login":"alice"}'
else
  case "$4" in 'repos/demo/repo/issues?state=all&sort=updated&direction=desc&per_page=100&since='*) ;; *) exit 5;; esac
  [ "$5" = --paginate ] && [ "$6" = --slurp ] && [ "$#" = 6 ] || exit 5
  printf '%s' '[[]]'
fi
"#;
    let repo = gh_wanted::repositories::Repository {
        id: 1,
        full_name: "demo/repo".into(),
        description: None,
        topics: vec![],
        archived: false,
    };
    let account = gh_wanted::github::Account {
        id: 7,
        login: "alice".into(),
    };
    let (_dir, client) = fake(script, Duration::from_secs(2));
    assert!(client.issues(&account, &repo).unwrap().is_empty());
    let failed = script.replace("printf '%s' '[[]]'", "printf '%s' '[[]'; exit 1");
    let (_dir, client) = fake(&failed, Duration::from_secs(2));
    assert!(client.issues(&account, &repo).is_err());
    let (_dir, client) = fake(script, Duration::from_secs(2));
    let wrong = gh_wanted::github::Account {
        id: 8,
        login: "bob".into(),
    };
    assert!(client.issues(&wrong, &repo).is_err());
}
