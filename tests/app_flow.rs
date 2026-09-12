use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gh_wanted::{
    app::{clean, Action, App, Mode},
    github::{Account, Snapshot},
    repositories::Repository,
    store::Store,
    ui,
};
use ratatui::{backend::TestBackend, Terminal};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn repo_pane_focus_and_editor_escape_are_independent() {
    use gh_wanted::app::Pane;
    let mut app = app();
    let mut other = app.repositories[0].clone();
    other.id = 2;
    other.full_name = "demo/other".into();
    app.repositories.push(other);
    app.key(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.pane, Pane::Details);
    app.key(key(KeyCode::Char('j'))).unwrap();
    assert_eq!(app.selected, 0);
    app.key(key(KeyCode::Char('t'))).unwrap();
    app.key(key(KeyCode::Char('b'))).unwrap();
    assert_eq!(app.input, "b");
    app.key(key(KeyCode::Esc)).unwrap();
    assert_eq!(app.mode, Mode::Browse);
    assert_eq!(app.pane, Pane::Details);
    app.key(key(KeyCode::Esc)).unwrap();
    assert_eq!(app.pane, Pane::List);
    app.key(key(KeyCode::Char('j'))).unwrap();
    assert_eq!(app.selected, 1);
    app.query = "no matching repository".into();
    app.key(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.pane, Pane::List);
}

#[test]
fn repo_focus_does_not_capture_keys_in_other_views() {
    use gh_wanted::{app::Pane, focus::Focus};
    let mut app = app();
    app.key(key(KeyCode::Enter)).unwrap();
    app.focuses = ["one", "two"]
        .into_iter()
        .map(|name| Focus {
            name: name.into(),
            repository_query: String::new(),
            issue_query: String::new(),
        })
        .collect();
    app.key(key(KeyCode::Char('f'))).unwrap();
    app.key(key(KeyCode::Char('j'))).unwrap();
    assert_eq!(app.selected, 1);
    app.key(key(KeyCode::Char('i'))).unwrap();
    app.issue_query = "some query".into();
    app.key(key(KeyCode::Esc)).unwrap();
    assert!(app.issue_query.is_empty());
    assert_eq!(app.pane, Pane::Details);
}

#[test]
fn repository_open_is_scoped_to_details_and_validates_name() {
    let mut app = app();
    assert!(matches!(
        app.key(key(KeyCode::Char('o'))).unwrap(),
        Action::None
    ));
    app.key(key(KeyCode::Enter)).unwrap();
    assert!(
        matches!(app.key(key(KeyCode::Char('o'))).unwrap(), Action::OpenRepository(url) if url == "https://github.com/demo/rust")
    );
    app.repositories[0].full_name = "demo/rust?redirect=elsewhere".into();
    assert!(app.key(key(KeyCode::Char('o'))).is_err());
}
fn app() -> App {
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
            full_name: "demo/rust".into(),
            description: None,
            topics: vec!["rust".into()],
            archived: false,
        }],
    })
    .unwrap();
    app.key(key(KeyCode::Char('b'))).unwrap();
    app
}

#[test]
fn today_is_default_and_account_loading_preserves_chosen_view() {
    use gh_wanted::app::View;
    let mut app = App::new(Store::memory().unwrap(), true);
    assert_eq!(app.view, View::Activity);
    app.identify(Account {
        id: 1,
        login: "demo".into(),
    })
    .unwrap();
    assert_eq!(app.view, View::Activity);
    app.key(key(KeyCode::Char('b'))).unwrap();
    app.identify(Account {
        id: 2,
        login: "other".into(),
    })
    .unwrap();
    assert_eq!(app.view, View::Repositories);
    assert_eq!(
        App::new(Store::memory().unwrap(), true).view,
        View::Activity
    );
}

#[test]
fn today_landing_distinguishes_connection_empty_loading_and_failure() {
    fn screen(app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 32)).unwrap();
        terminal.draw(|frame| ui::draw(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>()
    }
    let mut app = App::new(Store::memory().unwrap(), true);
    app.busy = true;
    assert!(screen(&app).contains("Connecting to GitHub"));
    app.identify(Account {
        id: 1,
        login: "demo".into(),
    })
    .unwrap();
    app.busy = false;
    let empty = screen(&app);
    assert!(empty.contains("No watched repositories yet"));
    assert!(empty.find("d Today").unwrap() < empty.find("i Issues").unwrap());
    assert!(empty.find("i Issues").unwrap() < empty.find("b Repos").unwrap());
    app.repositories.push(Repository {
        id: 1,
        full_name: "demo/repo".into(),
        description: None,
        topics: vec![],
        archived: false,
    });
    app.busy = true;
    assert!(screen(&app).contains("Still checking for updates"));
    app.busy = false;
    assert!(screen(&app).contains("Activity is incomplete"));
}
#[test]
fn editing_tags_and_combining_filter() {
    let mut app = app();
    app.key(key(KeyCode::Char('t'))).unwrap();
    for ch in "Priority".chars() {
        app.key(key(KeyCode::Char(ch))).unwrap();
    }
    app.key(key(KeyCode::Enter)).unwrap();
    app.query = "topic:rust tag:priority".into();
    assert_eq!(app.visible(), vec![0]);
    app.query = "topic:rust tag:missing".into();
    assert!(app.visible().is_empty());
    app.key(key(KeyCode::Down)).unwrap();
    assert_eq!(app.selected, 0);
}
#[test]
fn text_input_does_not_trigger_commands_and_invalid_save_keeps_tags() {
    let mut app = app();
    app.key(key(KeyCode::Char('t'))).unwrap();
    assert!(matches!(
        app.key(key(KeyCode::Char('q'))).unwrap(),
        Action::None
    ));
    assert_eq!(app.input, "q");
    app.key(key(KeyCode::Enter)).unwrap();
    app.key(key(KeyCode::Char('t'))).unwrap();
    app.input = "priority,,bug".into();
    assert!(app.key(key(KeyCode::Enter)).is_err());
    assert_eq!(app.mode, Mode::Tags);
    assert_eq!(app.tags[&1], vec!["q"]);
    app.key(key(KeyCode::Esc)).unwrap();
    assert_eq!(app.tags[&1], vec!["q"]);
}
#[test]
fn renders_unicode_empty_small_and_help() {
    let mut app = app();
    app.repositories[0].full_name = "demo/日本語🦀\u{1b}[31m".into();
    for (width, height) in [(100, 30), (50, 15), (20, 5)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| ui::draw(frame, &app)).unwrap();
        app.query = "missing".into();
        terminal.draw(|frame| ui::draw(frame, &app)).unwrap();
        app.help = true;
        terminal.draw(|frame| ui::draw(frame, &app)).unwrap();
    }
    assert_eq!(clean("a\u{1b}\n\u{202e}b"), "ab");
}
#[test]
fn changing_accounts_clears_cached_rows_before_loading() {
    let mut app = app();
    app.identify(Account {
        id: 2,
        login: "other".into(),
    })
    .unwrap();
    assert!(app.repositories.is_empty());
    assert!(app.tags.is_empty());
}
#[test]
fn refresh_does_not_redirect_tag_edit() {
    let mut app = app();
    app.key(key(KeyCode::Char('t'))).unwrap();
    app.input = "needs review".into();
    app.apply(Snapshot {
        account: Account {
            id: 1,
            login: "demo".into(),
        },
        repositories: vec![Repository {
            id: 2,
            full_name: "demo/other".into(),
            description: None,
            topics: vec![],
            archived: false,
        }],
    })
    .unwrap();
    app.key(key(KeyCode::Enter)).unwrap();
    assert_eq!(app.store.tags(1, 1).unwrap(), vec!["needs review"]);
    assert!(app.store.tags(1, 2).unwrap().is_empty());
}
#[test]
fn quoted_tag_filter_matches_spaces() {
    let mut app = app();
    app.tags.insert(1, vec!["needs review".into()]);
    app.query = "topic:rust tag:\"needs review\"".into();
    assert_eq!(app.visible(), vec![0]);
}

#[test]
fn refresh_priority_prefers_known_activity_without_dropping_quiet_repos() {
    let mut app = app();
    app.repositories.push(Repository {
        id: 2,
        full_name: "demo/active".into(),
        description: None,
        topics: vec![],
        archived: false,
    });
    app.issues.insert(
        2,
        vec![gh_wanted::issues::Issue {
            repo_id: 2,
            number: 1,
            title: "Known issue".into(),
            body: None,
            state: "open".into(),
            created_at: "2026-09-11".into(),
            updated_at: "2026-09-11".into(),
            labels: vec![],
            assignees: vec![],
            url: "https://github.com/demo/active/issues/1".into(),
        }],
    );
    let jobs = app.prepare_activity(true, None).unwrap();
    assert_eq!(jobs.len(), 4);
    assert_eq!(
        jobs.iter().map(|r| r.repo.id).collect::<Vec<_>>(),
        vec![2, 2, 1, 1]
    );
}

#[test]
fn saved_focus_recomputes_membership_and_failed_fetch_preserves_results() {
    use gh_wanted::app::View;
    let mut app = app();
    app.tags.insert(1, vec!["priority".into()]);
    app.query = "topic:rust tag:priority".into();
    assert!(matches!(
        app.key(key(KeyCode::Char('i'))).unwrap(),
        Action::FetchIssues
    ));
    app.key(key(KeyCode::Char('/'))).unwrap();
    app.input = "label:\"good first issue\"".into();
    app.key(key(KeyCode::Enter)).unwrap();
    app.key(key(KeyCode::Char('s'))).unwrap();
    app.input = "Rust starters".into();
    app.key(key(KeyCode::Enter)).unwrap();
    app.query.clear();
    app.issue_query.clear();
    app.key(key(KeyCode::Char('f'))).unwrap();
    assert_eq!(app.view, View::Focuses);
    assert!(matches!(
        app.key(key(KeyCode::Enter)).unwrap(),
        Action::FetchIssues
    ));
    assert_eq!(app.query, "topic:rust tag:priority");
    assert_eq!(app.issue_query, "label:\"good first issue\"");
    assert_eq!(app.visible(), vec![0]);
    let issue = gh_wanted::issues::Issue {
        repo_id: 1,
        number: 1,
        title: "Help keyboard users".into(),
        body: None,
        state: "open".into(),
        created_at: "2026-09-10".into(),
        updated_at: "2026-09-11".into(),
        labels: vec!["good first issue".into()],
        assignees: vec![],
        url: "https://github.com/demo/rust/issues/1".into(),
    };
    app.apply_issues(1, 1, Ok(vec![issue.clone()]));
    assert_eq!(app.visible_issues().len(), 1);
    app.apply_issues(1, 1, Err(anyhow::anyhow!("Page 2 failed")));
    assert_eq!(app.visible_issues(), vec![&issue]);
    assert!(app.issue_status[&1].contains("Incomplete"));
    app.apply_issues(2, 1, Ok(vec![]));
    assert_eq!(app.visible_issues().len(), 1);
    for (width, height) in [(100, 30), (50, 15), (20, 5)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| ui::draw(f, &app)).unwrap();
        app.detail = true;
        terminal.draw(|f| ui::draw(f, &app)).unwrap();
        app.detail = false;
    }
    app.tags.insert(1, vec![]);
    app.key(key(KeyCode::Char('f'))).unwrap();
    app.key(key(KeyCode::Enter)).unwrap();
    assert!(app.visible().is_empty());
    assert!(app.visible_issues().is_empty());
    app.identify(Account {
        id: 2,
        login: "other".into(),
    })
    .unwrap();
    assert!(app.issues.is_empty());
    assert!(app.focuses.is_empty());
}

#[test]
fn issue_selection_survives_other_repository_finishing() {
    let mut app = app();
    app.view = gh_wanted::app::View::Issues;
    app.repositories.push(Repository {
        id: 2,
        full_name: "demo/other".into(),
        description: None,
        topics: vec![],
        archived: false,
    });
    let mut issue = gh_wanted::issues::Issue {
        repo_id: 1,
        number: 1,
        title: "Selected".into(),
        body: None,
        state: "open".into(),
        created_at: "2026-09-10".into(),
        updated_at: "2026-09-10".into(),
        labels: vec![],
        assignees: vec![],
        url: "https://github.com/demo/rust/issues/1".into(),
    };
    app.apply_issues(1, 1, Ok(vec![issue.clone()]));
    issue.repo_id = 2;
    issue.updated_at = "2026-09-11".into();
    app.apply_issues(1, 2, Ok(vec![issue]));
    assert_eq!(app.current_issue().unwrap().repo_id, 1);
    assert_eq!(app.selected, 1);
}
