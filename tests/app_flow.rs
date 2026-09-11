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
    app
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
