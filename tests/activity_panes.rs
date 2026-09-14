use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gh_wanted::{
    activity::{Activity, ActivityKind},
    app::{Action, App, Pane},
    repositories::Repository,
    store::Store,
    ui,
};
use ratatui::{backend::TestBackend, Terminal};

fn key(app: &mut App, code: KeyCode) -> Action {
    app.key(KeyEvent::new(code, KeyModifiers::NONE)).unwrap()
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

fn app() -> App {
    let mut app = App::new(Store::memory().unwrap(), true);
    app.repositories.push(Repository {
        id: 1,
        full_name: "demo/repo".into(),
        description: None,
        topics: vec![],
        archived: false,
    });
    app.activities = (1..=2)
        .map(|number| Activity {
            repo_id: 1,
            key: format!("event-{number}"),
            source_id: number,
            number,
            kind: ActivityKind::Comment,
            title: format!("Update {number}"),
            body: format!(
                "# Heading\n- Bullet\n> Quote\n```rust\n  let x = 1;\n```\n{}",
                "Long body with wrapping words.\n".repeat(80)
            ),
            actor: "demo".into(),
            association: "NONE".into(),
            url: format!("https://github.com/demo/repo/issues/{number}#issuecomment-{number}"),
            occurred_at: app.now - number as i64,
            fetched_at: app.now,
            acknowledged: false,
        })
        .collect();
    app
}

#[test]
fn focus_scroll_and_sync_overlay_keep_selection() {
    let mut app = app();
    screen(&app, 120, 32);
    key(&mut app, KeyCode::PageDown);
    assert_eq!(app.activity_scroll.get(), 0);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.activity_pane, Pane::Details);
    key(&mut app, KeyCode::Char('j'));
    assert_eq!(app.activity_scroll.get(), 1);
    for _ in 0..100 {
        key(&mut app, KeyCode::PageDown);
    }
    assert_eq!(app.activity_scroll.get(), app.activity_scroll_max.get());
    assert_eq!(app.selected, 0);
    let scroll = app.activity_scroll.get();
    key(&mut app, KeyCode::Char('e'));
    key(&mut app, KeyCode::Char('j'));
    key(&mut app, KeyCode::PageDown);
    assert_eq!(app.detail_scroll, 11);
    assert_eq!(app.activity_scroll.get(), scroll);
    assert_eq!(app.selected, 0);
    key(&mut app, KeyCode::Esc);
    assert!(!app.feed_details);
    assert_eq!(app.activity_pane, Pane::Details);
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.activity_pane, Pane::List);
    key(&mut app, KeyCode::Char('j'));
    screen(&app, 120, 32);
    assert_eq!(app.selected, 1);
    assert_eq!(app.activity_scroll.get(), 0);
}

#[test]
fn narrow_markdown_resize_and_external_url() {
    let mut app = app();
    key(&mut app, KeyCode::Enter);
    let rendered = screen(&app, 70, 40);
    assert!(rendered.contains("Update details"));
    assert!(!rendered.contains("Activity inbox"));
    assert!(rendered.contains("Heading"));
    assert!(!rendered.contains("# Heading"));
    assert!(rendered.contains("• Bullet"));
    assert!(rendered.contains("│ Quote"));
    assert!(rendered.contains("  let x = 1;"));
    let url = app.current_activity().unwrap().url.clone();
    assert!(matches!(key(&mut app, KeyCode::Char('o')), Action::OpenIssue(value) if value == url));
    for _ in 0..100 {
        key(&mut app, KeyCode::PageDown);
    }
    screen(&app, 120, 160);
    assert_eq!(app.activity_scroll.get(), 0);
    app.activities[0].url = "https://example.com".into();
    assert!(app
        .key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE))
        .is_err());
    key(&mut app, KeyCode::Char('b'));
    assert_eq!(app.activity_pane, Pane::List);
}
