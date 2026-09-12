#![cfg(unix)]
use gh_wanted::{
    github::{Account, GhClient},
    repositories::Repository,
    sync::run_bounded,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{atomic::AtomicBool, Mutex},
    time::{Duration, Instant},
};

fn fake(script: &str) -> (tempfile::TempDir, GhClient) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gh");
    fs::write(&path, format!("#!/bin/sh\n{script}")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let client = GhClient::new(path, Duration::from_secs(3));
    (dir, client)
}
fn account() -> Account {
    Account {
        id: 7,
        login: "alice".into(),
    }
}
fn repo(id: u64) -> Repository {
    Repository {
        id,
        full_name: format!("demo/repo{id}"),
        description: None,
        topics: vec![],
        archived: false,
    }
}

const SCRIPT: &str = r#"
if [ "$1" = auth ]; then printf '%s' 'synthetic-test-credential'; exit 0; fi
sleep 0.03
if [ "$4" = user ]; then
  echo user >> "$0.calls"
  if [ -f "$0.switched" ] && [ "$GH_TOKEN" != synthetic-test-credential ]; then
    printf '%s' '{"id":8,"login":"bob"}'
  else printf '%s' '{"id":7,"login":"alice"}'; fi
  exit 0
fi
echo data >> "$0.calls"
if [ "$5" = --include ]; then
  [ "$GH_TOKEN" = synthetic-test-credential ] || exit 9
  printf 'HTTP/2.0 200 OK\r\n\r\n[]'
else printf '%s' '[[]]'; fi
"#;

#[test]
fn pinned_credentials_survive_active_account_switch_and_reject_wrong_expected_account() {
    let (dir, client) = fake(SCRIPT);
    let metrics = dir.path().join("metrics.jsonl");
    let client = client
        .with_metrics(metrics.clone(), "issues")
        .bind(Some(&account()))
        .unwrap();
    fs::write(dir.path().join("gh.switched"), "").unwrap();
    client.issues(&account(), &repo(1)).unwrap();
    client.issues(&account(), &repo(2)).unwrap();
    let calls = fs::read_to_string(dir.path().join("gh.calls")).unwrap();
    assert_eq!(calls.lines().filter(|s| *s == "user").count(), 1);
    assert_eq!(calls.lines().filter(|s| *s == "data").count(), 2);
    let wrong = Account {
        id: 8,
        login: "bob".into(),
    };
    assert!(client.issues(&wrong, &repo(1)).is_err());
    assert!(!fs::read_to_string(metrics)
        .unwrap()
        .contains("synthetic-test-credential"));
    let (_dir, client) = fake(SCRIPT);
    assert!(client.bind(Some(&wrong)).is_err());
}

#[test]
fn shared_rate_failure_prevents_subsequent_dispatch() {
    let script = SCRIPT.replace(
        "printf 'HTTP/2.0 200 OK\\r\\n\\r\\n[]'",
        "printf 'HTTP/2.0 429 Too Many Requests\\r\\nRetry-After: 60\\r\\n\\r\\n{}'; exit 1",
    );
    let (dir, client) = fake(&script);
    let client = client.bind(Some(&account())).unwrap();
    assert!(client.issues(&account(), &repo(1)).is_err());
    assert!(client.issues(&account(), &repo(2)).is_err());
    let calls = fs::read_to_string(dir.path().join("gh.calls")).unwrap();
    assert_eq!(calls.lines().filter(|s| *s == "data").count(), 1);
}

#[test]
fn bound_concurrent_refresh_reduces_calls_in_controlled_baseline() {
    let (old_dir, old) = fake(SCRIPT);
    let start = Instant::now();
    for id in 0..8 {
        old.issues(&account(), &repo(id)).unwrap();
    }
    let old_time = start.elapsed();
    let old_calls = fs::read_to_string(old_dir.path().join("gh.calls")).unwrap();
    assert_eq!(old_calls.lines().count(), 24);
    let (dir, client) = fake(SCRIPT);
    let start = Instant::now();
    let client = client.bind(Some(&account())).unwrap();
    let results = Mutex::new(Vec::new());
    run_bounded(
        (0..8).map(repo).collect(),
        &AtomicBool::new(false),
        |repo| client.issues(&account(), repo),
        |repo, result| {
            results.lock().unwrap().push((repo.id, result.unwrap()));
            true
        },
    );
    let new_time = start.elapsed();
    assert_eq!(results.into_inner().unwrap().len(), 8);
    let calls = fs::read_to_string(dir.path().join("gh.calls")).unwrap();
    assert_eq!(calls.lines().count(), 9);
    println!("Controlled 8-repo refresh: sequential={old_time:?}, bound/3-workers={new_time:?}; API calls 24 -> 9");
}
