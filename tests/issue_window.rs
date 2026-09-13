use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gh_wanted::{
    app::{Action, App, Mode, View},
    github::{Account, Snapshot},
    issues::{Issue, IssueFilter},
    repositories::Repository,
    store::Store,
};
fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn window_filter_defaults_validates_and_survives_saved_focus() {
    assert_eq!(IssueFilter::parse("").unwrap().days, 7);
    assert_eq!(IssueFilter::parse("days:30 label:bug").unwrap().days, 30);
    assert!(IssueFilter::parse("days:90").is_err());
    let mut app = App::new(Store::memory().unwrap(), true);
    app.identify(Account {
        id: 1,
        login: "demo".into(),
    })
    .unwrap();
    app.view = View::Issues;
    app.key(key(KeyCode::Char('/'))).unwrap();
    app.input = "days:30 label:bug".into();
    assert!(matches!(
        app.key(key(KeyCode::Enter)).unwrap(),
        Action::FetchIssues
    ));
    assert_eq!(app.mode, Mode::Browse);
    app.key(key(KeyCode::Char('s'))).unwrap();
    app.input = "Monthly bugs".into();
    app.key(key(KeyCode::Enter)).unwrap();
    app.issue_query.clear();
    app.key(key(KeyCode::Char('f'))).unwrap();
    app.key(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.issue_window_days(), 30);
    assert!(matches!(
        app.key(key(KeyCode::Esc)).unwrap(),
        Action::FetchIssues
    ));
    assert_eq!(app.issue_window_days(), 7);
}

#[test]
fn narrowing_filters_cached_issues_by_updated_time_and_failure_keeps_cache() {
    let mut app = App::new(Store::memory().unwrap(), true);
    let account = Account {
        id: 1,
        login: "demo".into(),
    };
    app.identify(account.clone()).unwrap();
    app.apply(Snapshot {
        account,
        repositories: vec![Repository {
            id: 1,
            full_name: "demo/repo".into(),
            description: None,
            topics: vec![],
            archived: false,
        }],
    })
    .unwrap();
    app.now = gh_wanted::activity::timestamp("2026-09-13T12:00:00Z").unwrap();
    let recent = Issue {
        repo_id: 1,
        number: 1,
        title: "Old issue, new activity".into(),
        body: None,
        state: "open".into(),
        created_at: "2020-01-01T00:00:00Z".into(),
        updated_at: "2026-09-12T00:00:00Z".into(),
        labels: vec![],
        assignees: vec![],
        url: "https://github.com/demo/repo/issues/1".into(),
    };
    let older = Issue {
        number: 2,
        updated_at: "2026-08-20T00:00:00Z".into(),
        ..recent.clone()
    };
    app.apply_issues(1, 1, Ok(vec![recent, older]));
    assert_eq!(app.visible_issues().len(), 1);
    app.issue_query = "days:30".into();
    assert_eq!(app.visible_issues().len(), 2);
    app.apply_issues(1, 1, Err(anyhow::anyhow!("16 MiB limit")));
    assert_eq!(app.visible_issues().len(), 2);
    assert!(app.issue_status[&1].contains("Incomplete"));
    app.apply_issues_for_window(1, 1, 7, Ok(vec![]));
    assert_eq!(
        app.visible_issues().len(),
        2,
        "Old-window response replaced current cache"
    );
}
