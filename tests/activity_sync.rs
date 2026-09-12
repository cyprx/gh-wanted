use chrono::FixedOffset;
use gh_wanted::{
    activity::{self, Activity, ActivityKind},
    github::Account,
    repositories::Repository,
    store::Store,
    sync::{FeedRequest, RequestFailure},
};
use serde_json::json;

fn repo(id: u64) -> Repository {
    Repository {
        id,
        full_name: format!("demo/repo{id}"),
        description: None,
        topics: vec![],
        archived: false,
    }
}
fn request(store: &mut Store, repo_id: u64, feed: &str, now: i64) -> FeedRequest {
    FeedRequest {
        account: Account {
            id: 1,
            login: "demo".into(),
        },
        repo: repo(repo_id),
        state: store.ensure_feed(1, repo_id, feed, now).unwrap(),
        upper: now,
    }
}
fn event(repo_id: u64, key: &str, at: i64) -> Activity {
    Activity {
        repo_id,
        key: key.into(),
        source_id: 1,
        number: 1,
        kind: ActivityKind::Comment,
        title: "A reply".into(),
        body: "Details".into(),
        actor: "contributor".into(),
        association: "NONE".into(),
        url: "https://github.com/demo/repo1/issues/1#issuecomment-1".into(),
        occurred_at: at,
        fetched_at: at,
        acknowledged: false,
    }
}
#[test]
fn two_sessions_catch_up_partial_failure_replay_and_acknowledgment() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("activity.sqlite3");
    let start = activity::timestamp("2026-09-10T10:00:00Z")?;
    {
        let mut store = Store::open(&path)?;
        let a = request(&mut store, 1, "issues", start);
        store.commit_activity(&a, &[event(1, "new", start)])?;
        store.acknowledge(1, 1, "new", true)?;
        let b = request(&mut store, 2, "comments", start);
        store.fail_activity(&b, &anyhow::anyhow!("missing page"))?;
        assert_eq!(
            store
                .feeds(1)?
                .iter()
                .find(|f| f.repo_id == 2)
                .unwrap()
                .checkpoint,
            None
        );
    }
    let mut store = Store::open(&path)?;
    assert!(store.activities(1)?[0].acknowledged);
    assert!(store.activities(2)?.is_empty());
    let next = start + 2 * 86400;
    let a = request(&mut store, 1, "issues", next);
    assert_eq!(a.state.lower(), start - 60);
    store.commit_activity(
        &a,
        &[event(1, "new", start), event(1, "changed", start + 3600)],
    )?;
    let b = request(&mut store, 2, "comments", next);
    assert_eq!(b.state.lower(), start - 86400);
    store.commit_activity(&b, &[event(2, "missed reply", start + 1800)])?;
    let replay = request(&mut store, 1, "issues", next + 1);
    store.commit_activity(&replay, &[])?;
    let events = store.activities(1)?;
    assert_eq!(events.len(), 3);
    assert!(events.iter().find(|e| e.key == "new").unwrap().acknowledged);
    assert!(store.feeds(1)?.iter().all(|f| f.error.is_none()));
    Ok(())
}
#[test]
fn activity_and_checkpoint_roll_back_together_and_reject_stale_results() {
    let mut store = Store::memory().unwrap();
    let r = request(&mut store, 1, "issues", 100000);
    assert!(store
        .commit_activity(
            &r,
            &[
                event(1, "valid", 100000),
                event(2, "wrong repository", 100000)
            ]
        )
        .is_err());
    assert!(store.activities(1).unwrap().is_empty());
    assert_eq!(store.feeds(1).unwrap()[0].checkpoint, None);
    store
        .commit_activity(&r, &[event(1, "valid", 100000)])
        .unwrap();
    assert!(store.commit_activity(&r, &[]).is_err());
    let backward = request(&mut store, 1, "issues", 99999);
    assert!(store.commit_activity(&backward, &[]).is_err());
    assert_eq!(store.feeds(1).unwrap()[0].checkpoint, Some(100000));
}
#[test]
fn midnight_timezone_changes_and_equal_timestamp_identities() {
    let now = activity::timestamp("2026-09-11T00:30:00Z").unwrap();
    let before = activity::timestamp("2026-09-10T23:30:00Z").unwrap();
    assert!(!activity::is_today(
        before,
        now,
        &FixedOffset::east_opt(0).unwrap()
    ));
    assert!(activity::is_today(
        before,
        now,
        &FixedOffset::east_opt(7 * 3600).unwrap()
    ));
    assert!(activity::is_today(
        before,
        now,
        &FixedOffset::west_opt(5 * 3600).unwrap()
    ));
    let issue = json!({"id":10,"number":1,"title":"New", "body":"Body", "created_at":"2026-09-11T00:30:00Z", "updated_at":"2026-09-11T00:30:00Z", "html_url":"https://github.com/demo/repo1/issues/1"});
    let mut second = issue.clone();
    second["id"] = json!(11);
    second["number"] = json!(2);
    let entries = activity::parse_records(
        1,
        "issues",
        vec![issue.clone(), issue, second],
        now - 60,
        now,
    )
    .unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|e| e.kind == ActivityKind::NewIssue));
}
#[test]
fn source_kinds_and_future_boundaries_are_explicit() {
    let time = "2026-09-11T12:00:00Z";
    let upper = activity::timestamp(time).unwrap();
    let issue = json!({"id":10,"number":1,"title":"Changed", "created_at":"2026-09-01T12:00:00Z", "updated_at":time,"html_url":"https://github.com/demo/repo1/issues/1"});
    let mut pr = issue.clone();
    pr["id"] = json!(11);
    pr["pull_request"] = json!({});
    let entries =
        activity::parse_records(1, "issues", vec![issue, pr.clone()], upper - 60, upper).unwrap();
    assert!(entries.iter().any(|e| e.kind == ActivityKind::IssueChange));
    assert!(entries.iter().any(|e| e.kind == ActivityKind::PrChange));
    assert!(entries.iter().all(|e| e.association == "NONE"));
    assert!(
        activity::parse_records(1, "issues", vec![pr], upper - 60, upper - 1)
            .unwrap()
            .is_empty()
    );
    let comment = json!({"id":42,"issue_url":"https://api.github.com/repos/demo/repo1/issues/1","updated_at":time,"html_url":"https://github.com/demo/repo1/issues/1#issuecomment-42","body":"A comment","user":{"login":"sam"},"author_association":"MEMBER"});
    let entries = activity::parse_records(1, "comments", vec![comment], upper - 1, upper).unwrap();
    assert_eq!(entries[0].kind, ActivityKind::Comment);
    assert_eq!(entries[0].association, "MEMBER");
    let review = json!({"id":7,"submitted_at":time,"state":"APPROVED","html_url":"https://github.com/demo/repo1/pull/1#pullrequestreview-7"});
    assert_eq!(
        activity::parse_records(1, "reviews:1", vec![review], upper - 1, upper).unwrap()[0].kind,
        ActivityKind::Review
    );
}
#[test]
fn retries_respect_headers_and_persist_pause() {
    let now = 100000;
    let failure = gh_wanted::sync::request_failure(b"HTTP/2.0 429 Too Many Requests\r\nRetry-After: 3600\r\nX-RateLimit-Remaining: 0\r\nX-RateLimit-Reset: 107200\r\n\r\n{}",b"",now);
    assert_eq!(failure.retry_at, Some(107200));
    assert!(!failure.paused);
    let auth = gh_wanted::sync::request_failure(
        b"",
        b"gh: Bad credentials (HTTP 401) private-payload",
        now,
    );
    assert!(auth.paused);
    assert!(!auth.to_string().contains("private-payload"));
    let mut store = Store::memory().unwrap();
    let r = request(&mut store, 1, "issues", now);
    store
        .fail_activity(&r, &anyhow::Error::new(failure))
        .unwrap();
    let state = store.feeds(1).unwrap().remove(0);
    assert!(!state.ready(now, true));
    assert!(state.ready(107200, false));
    store
        .fail_activity(
            &r,
            &anyhow::Error::new(RequestFailure {
                paused: true,
                retry_at: None,
            }),
        )
        .unwrap();
    let state = store.feeds(1).unwrap().remove(0);
    assert!(!state.ready(now, false));
    assert!(state.ready(now, true));
}
#[test]
fn schema_two_migration_preserves_focus_and_rollback_is_atomic() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    for conflict in [false, true] {
        let path = dir.path().join(format!("{conflict}.sqlite3"));
        let db = rusqlite::Connection::open(&path)?;
        db.execute_batch("CREATE TABLE repositories(host TEXT,account TEXT,repo_id TEXT,repository_json TEXT); CREATE TABLE local_tags(host TEXT,account TEXT,repo_id TEXT,tag TEXT); CREATE TABLE focuses(host TEXT,account TEXT,name_key TEXT,definition TEXT); PRAGMA user_version=2;")?;
        db.execute(
            "INSERT INTO focuses VALUES ('github.com','1','saved',?1)",
            [r#"{"name":"Saved","repository_query":"topic:rust","issue_query":""}"#],
        )?;
        if conflict {
            db.execute_batch("CREATE TABLE activity_feeds(sentinel TEXT);")?;
        }
        drop(db);
        if conflict {
            assert!(Store::open(&path).is_err());
            let db = rusqlite::Connection::open(&path)?;
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
                2
            );
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name='activity'",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                0
            );
        } else {
            assert_eq!(Store::open(&path)?.focuses(1)?[0].name, "Saved");
        }
    }
    Ok(())
}

fn app(now: i64) -> gh_wanted::app::App {
    let mut app = gh_wanted::app::App::new(Store::memory().unwrap(), false);
    let account = Account {
        id: 1,
        login: "demo".into(),
    };
    app.identify(account.clone()).unwrap();
    app.apply(gh_wanted::github::Snapshot {
        account,
        repositories: vec![repo(1)],
    })
    .unwrap();
    app.now = now;
    app
}
fn key(c: char) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char(c),
        crossterm::event::KeyModifiers::NONE,
    )
}
#[test]
fn today_and_read_state_are_independent_and_account_switch_clears_activity() {
    let now = activity::timestamp("2026-09-11T12:00:00Z").unwrap();
    let mut app = app(now);
    let requests = app.prepare_activity(true, None).unwrap();
    for r in &requests {
        let events = if r.state.feed == "issues" {
            vec![event(1, "today", now)]
        } else {
            vec![event(1, "yesterday", now - 86400)]
        };
        app.apply_activity(r, Ok(events)).unwrap();
    }
    app.busy = false;
    app.key(key('d')).unwrap();
    assert_eq!(app.visible_activity().len(), 2);
    assert!(app.activities.iter().all(|e| !e.acknowledged));
    app.key(key('a')).unwrap();
    assert!(app.current_activity().unwrap().acknowledged);
    assert_eq!(app.visible_activity().len(), 2);
    app.key(key('u')).unwrap();
    assert_eq!(app.visible_activity().len(), 1);
    app.key(key('u')).unwrap();
    app.now += 86400;
    assert_eq!(app.visible_activity().len(), 1);
    assert_eq!(app.visible_activity()[0].key, "yesterday");
    app.identify(Account {
        id: 2,
        login: "other".into(),
    })
    .unwrap();
    assert!(app.activities.is_empty());
    assert!(app.feeds.is_empty());
    app.apply_activity(&requests[0], Ok(vec![event(1, "late old account", now)]))
        .unwrap();
    assert!(app.activities.is_empty());
}
#[test]
fn scheduler_is_bounded_pauses_and_honors_manual_rate_limit() {
    let mut app = app(100000);
    assert!(app.auto_due());
    let requests = app.prepare_activity(false, None).unwrap();
    app.now += 900;
    assert!(!app.auto_due());
    assert!(app.prepare_activity(true, None).is_err());
    app.busy = false;
    assert!(app.auto_due());
    let error = anyhow::Error::new(RequestFailure {
        paused: true,
        retry_at: None,
    });
    app.apply_activity(&requests[0], Err(error)).unwrap();
    assert!(!app.auto_due());
    let retry = app.prepare_activity(true, None).unwrap();
    for r in &retry {
        app.apply_activity(r, Ok(vec![])).unwrap();
    }
    app.busy = false;
    app.now += 900;
    assert!(app.auto_due());
    app.observe_failure(&anyhow::Error::new(RequestFailure {
        paused: false,
        retry_at: Some(app.now + 7200),
    }));
    assert!(!app.auto_due());
    assert!(app.prepare_activity(true, None).is_err());
    app.now += 7200;
    assert!(app.auto_due());
}
#[test]
fn activity_ui_renders_sections_and_failed_feed_status() {
    use ratatui::{backend::TestBackend, Terminal};
    let now = activity::timestamp("2026-09-11T12:00:00Z").unwrap();
    let mut app = app(now);
    let requests = app.prepare_activity(true, None).unwrap();
    app.apply_activity(
        &requests[0],
        Ok(vec![event(1, "today", now), event(1, "old", now - 86400)]),
    )
    .unwrap();
    app.apply_activity(&requests[1], Err(anyhow::anyhow!("Page failed")))
        .unwrap();
    app.busy = false;
    app.key(key('d')).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(120, 32)).unwrap();
    terminal.draw(|f| gh_wanted::ui::draw(f, &app)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(screen.contains("Today (local time)"));
    assert!(screen.contains("Catch-up (earlier unread)"));
    assert!(screen.contains("INCOMPLETE"));
    app.key(key('e')).unwrap();
    terminal.draw(|f| gh_wanted::ui::draw(f, &app)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(screen.contains("Page failed"));
    for (w, h) in [(42, 10), (20, 5)] {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| gh_wanted::ui::draw(f, &app)).unwrap();
    }
}

#[test]
fn review_state_versions_and_pending_reviews() {
    let now = activity::timestamp("2026-09-11T12:00:00Z").unwrap();
    let review = json!({"id":7,"submitted_at":"2026-09-11T11:00:00Z","state":"APPROVED","html_url":"https://github.com/demo/repo1/pull/1#pullrequestreview-7"});
    let mut dismissed = review.clone();
    dismissed["state"] = json!("DISMISSED");
    let values = activity::parse_records(
        1,
        "reviews:1",
        vec![
            review,
            dismissed,
            json!({"submitted_at":null,"html_url":null}),
        ],
        now - 86400,
        now,
    )
    .unwrap();
    assert_eq!(values.len(), 2);
    assert_ne!(values[0].key, values[1].key);
    assert!(activity::valid_activity_url(&values[0].url));
    assert!(!activity::valid_activity_url(
        "https://github.com.evil/demo/repo/pull/1"
    ));
}
