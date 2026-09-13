use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gh_wanted::{
    app::{App, Pane, View},
    github::{Account, Snapshot},
    issues::Issue,
    repositories::Repository,
    store::Store,
    ui,
};
use ratatui::{backend::TestBackend, Terminal};

fn key(app: &mut App, code: KeyCode) {
    app.key(KeyEvent::new(code, KeyModifiers::NONE)).unwrap();
}
fn screen(app: &App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::draw(frame, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}
fn app(body: &str) -> App {
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
    let issue = Issue {
        repo_id: 1,
        number: 1,
        title: "First issue".into(),
        body: Some(body.into()),
        state: "open".into(),
        created_at: "2026-09-12T12:00:00Z".into(),
        updated_at: "2026-09-13T12:00:00Z".into(),
        labels: vec!["bug".into()],
        assignees: vec!["demo".into()],
        url: "https://github.com/demo/repo/issues/1".into(),
    };
    app.apply_issues(
        1,
        1,
        Ok(vec![
            issue.clone(),
            Issue {
                number: 2,
                title: "Second issue".into(),
                ..issue
            },
        ]),
    );
    app.view = View::Issues;
    app
}

#[test]
fn detail_scroll_is_bounded_and_does_not_move_selection() {
    let body = format!(
        "{}\nEND OF DESCRIPTION",
        "A long line with unicode 日本語 and wrapping words. ".repeat(200)
    );
    let mut app = app(&body);
    screen(&app, 120, 32);
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Char('j'));
    assert_eq!(app.issue_scroll.get(), 1);
    assert_eq!(app.selected, 0);
    for _ in 0..300 {
        key(&mut app, KeyCode::PageDown);
    }
    assert_eq!(app.issue_scroll.get(), app.issue_scroll_max.get());
    assert!(screen(&app, 120, 32).contains("END OF DESCRIPTION"));
    for _ in 0..300 {
        key(&mut app, KeyCode::PageUp);
    }
    assert_eq!(app.issue_scroll.get(), 0);
    key(&mut app, KeyCode::Char('j'));
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('j'));
    assert_eq!(app.selected, 1);
    assert_eq!(app.issue_scroll.get(), 0);
    assert_eq!(app.issue_pane, Pane::List);
}

#[test]
fn narrow_details_render_markdown_and_resize_clamps_scroll() {
    let mut app = app("## Heading\n- Item\n> Quoted text\n```rust\nlet x = 1;\n```\nLast line");
    let list = screen(&app, 60, 30);
    assert!(list.contains(" Issues ("));
    key(&mut app, KeyCode::Enter);
    let details = screen(&app, 60, 40);
    assert!(details.contains(" Issue details "));
    assert!(!details.contains(" Issues ("));
    assert!(details.contains("Heading") && !details.contains("## Heading"));
    assert!(details.contains("• Item") && details.contains("│ Quoted text"));
    assert!(details.contains("let x = 1;") && !details.contains("```"));
    assert!(details.contains("demo/repo") && details.contains("Created"));
    screen(&app, 45, 16);
    key(&mut app, KeyCode::PageDown);
    screen(&app, 120, 60);
    assert_eq!(app.issue_scroll.get(), 0);
    key(&mut app, KeyCode::Char('/'));
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.issue_pane, Pane::Details);
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.issue_pane, Pane::List);
}
