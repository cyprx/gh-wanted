use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gh_wanted::{
    app::{Action, App, Mode, Pane, View},
    github::{Account, Snapshot},
    repositories::{parse_repository, Repository},
    store::Store,
};

fn repo() -> Repository {
    Repository {
        id: 7,
        full_name: "nats-io/nats-server".into(),
        description: None,
        topics: vec!["rust".into()],
        archived: false,
    }
}
fn key(app: &mut App, code: KeyCode) -> anyhow::Result<Action> {
    app.key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn input_accepts_only_repository_names_or_github_https_urls() {
    for input in [
        "nats-io/nats-server",
        " https://github.com/nats-io/nats-server/ ",
    ] {
        assert_eq!(parse_repository(input).unwrap(), "nats-io/nats-server");
    }
    for input in [
        "",
        "../repo",
        "https://evil.com/a/b",
        "https://github.com/a/b/issues",
        "a/b?token=x",
        "a/b#x",
        "http://github.com/a/b",
        "a/b\nc",
    ] {
        assert!(parse_repository(input).is_err(), "{input}");
    }
}

#[test]
fn tracking_persists_merges_and_removes_without_unwatching() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.sqlite3");
    let mut store = Store::open(&path)?;
    store.track_repository(1, &repo())?;
    store.track_repository(1, &repo())?;
    store.replace_tags(1, 7, &["priority".into()])?;
    store.replace_repositories(1, &[])?;
    drop(store);
    let mut store = Store::open(&path)?;
    assert_eq!(store.repositories(1)?, vec![repo()]);
    assert!(store.repositories(2)?.is_empty());
    store.replace_repositories(1, &[repo()])?;
    assert_eq!(store.repositories(1)?.len(), 1);
    store.untrack_repository(1, 7)?;
    assert_eq!(store.repositories(1)?, vec![repo()]);
    store.replace_repositories(1, &[])?;
    assert!(store.repositories(1)?.is_empty());
    assert_eq!(store.tags(1, 7)?, ["priority"]);
    Ok(())
}

#[test]
fn add_editor_account_guard_and_local_remove() -> anyhow::Result<()> {
    let mut app = App::new(Store::memory()?, true);
    let account = Account {
        id: 1,
        login: "demo".into(),
    };
    app.identify(account.clone())?;
    app.apply(Snapshot {
        account: account.clone(),
        repositories: vec![],
    })?;
    app.view = View::Repositories;
    key(&mut app, KeyCode::Char('+'))?;
    assert_eq!(app.mode, Mode::TrackRepository);
    app.input = "invalid".into();
    assert!(key(&mut app, KeyCode::Enter).is_err());
    assert_eq!(app.mode, Mode::TrackRepository);
    app.input = "https://github.com/nats-io/nats-server".into();
    assert!(
        matches!(key(&mut app, KeyCode::Enter)?, Action::TrackRepository(name) if name == repo().full_name)
    );
    assert!(app.apply_tracked(2, repo()).is_err());
    app.apply_tracked(1, repo())?;
    app.apply(Snapshot {
        account,
        repositories: vec![],
    })?;
    assert_eq!(app.repositories, vec![repo()]);
    assert_eq!(app.prepare_activity(true, None)?.len(), 2);
    app.busy = false;
    key(&mut app, KeyCode::Enter)?;
    assert_eq!(app.pane, Pane::Details);
    key(&mut app, KeyCode::Char('x'))?;
    assert!(app.repositories.is_empty());
    assert!(app.tracked.is_empty());
    Ok(())
}

#[test]
fn schema_three_upgrade_preserves_cache_and_tags() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.sqlite3");
    let mut store = Store::open(&path)?;
    store.replace_repositories(1, &[repo()])?;
    store.replace_tags(1, 7, &["keep".into()])?;
    drop(store);
    let db = rusqlite::Connection::open(&path)?;
    db.execute_batch("DROP TABLE tracked_repositories; PRAGMA user_version=3;")?;
    drop(db);
    let mut store = Store::open(&path)?;
    assert_eq!(store.repositories(1)?, vec![repo()]);
    assert_eq!(store.tags(1, 7)?, ["keep"]);
    assert!(store.tracked_ids(1)?.is_empty());
    store.track_repository(1, &repo())?;
    drop(store);
    let db = rusqlite::Connection::open(&path)?;
    // Simulate a conflicting table during an upgrade; failure must not change version or data.
    db.execute_batch("PRAGMA user_version=3;")?;
    drop(db);
    assert!(Store::open(&path).is_err());
    let db = rusqlite::Connection::open(&path)?;
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
        3
    );
    assert_eq!(
        db.query_row("SELECT tag FROM local_tags", [], |r| r.get::<_, String>(0))?,
        "keep"
    );
    Ok(())
}
