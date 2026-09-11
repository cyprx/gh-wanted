use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyEventKind};
use directories::ProjectDirs;
use gh_wanted::{
    app::{Action, App},
    github::{Account, GhClient, Snapshot},
    repositories::Repository,
    store::Store,
    ui,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread,
    time::Duration,
};

enum Update {
    Account(Account),
    Done(Result<Snapshot>),
    Issues(u64, u64, Result<Vec<gh_wanted::issues::Issue>>),
    IssuesDone,
}

fn start_issues(
    sender: SyncSender<Update>,
    cancellation: Arc<AtomicBool>,
    account: Account,
    repos: Vec<Repository>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let client = GhClient::default().with_cancellation(cancellation.clone());
        for repo in repos {
            if cancellation.load(Ordering::Relaxed) {
                break;
            }
            let result = client.issues(&account, &repo);
            if sender
                .send(Update::Issues(account.id, repo.id, result))
                .is_err()
            {
                return;
            }
        }
        let _ = sender.send(Update::IssuesDone);
    })
}

fn demo_issues(app: &mut App, repos: &[Repository]) {
    for repo in repos {
        app.apply_issues(1, repo.id, Ok(vec![gh_wanted::issues::Issue {
            repo_id: repo.id, number: 1, title: "Improve keyboard navigation".into(),
            body: Some("Help new contributors navigate the list with the keyboard.\n\nAdd a regression test for moving through an empty list, and document the shortcuts.".into()),
            state: "open".into(), created_at: "2026-09-10T09:00:00Z".into(), updated_at: "2026-09-11T09:00:00Z".into(),
            labels: vec!["good first issue".into(), "enhancement".into()], assignees: vec![],
            url: format!("https://github.com/{}/issues/1", repo.full_name),
        }]));
    }
}

fn open_issue(url: &str) -> Result<()> {
    anyhow::ensure!(gh_wanted::issues::valid_issue_url(url), "Invalid issue URL");
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = std::process::Command::new("rundll32.exe");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = command.spawn().context("Could not launch browser")?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn start_sync(sender: SyncSender<Update>, cancellation: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let client = GhClient::default().with_cancellation(cancellation);
        let result = (|| {
            let account = client.account()?;
            sender
                .send(Update::Account(account.clone()))
                .context("UI closed")?;
            let snapshot = client.sync()?;
            anyhow::ensure!(
                snapshot.account.id == account.id,
                "GitHub account changed during refresh"
            );
            Ok(snapshot)
        })();
        let _ = sender.send(Update::Done(result));
    })
}

fn demo(app: &mut App) -> Result<()> {
    let account = Account {
        id: 1,
        login: "demo".into(),
    };
    app.identify(account.clone())?;
    let repos = vec![
        Repository {
            id: 1,
            full_name: "demo/terminal-kit".into(),
            description: Some("Build thoughtful terminal interfaces in Rust.".into()),
            topics: vec!["rust".into(), "tui".into()],
            archived: false,
        },
        Repository {
            id: 2,
            full_name: "demo/issue-garden".into(),
            description: Some("Tools for open-source contributors.".into()),
            topics: vec!["typescript".into(), "open-source".into()],
            archived: false,
        },
        Repository {
            id: 3,
            full_name: "demo/fast-search".into(),
            description: Some("A small and fast search engine.".into()),
            topics: vec!["rust".into(), "search".into()],
            archived: false,
        },
    ];
    app.apply(Snapshot {
        account,
        repositories: repos,
    })
}

fn updates(app: &mut App, receiver: &Receiver<Update>) {
    while let Ok(update) = receiver.try_recv() {
        let result = match update {
            Update::Issues(account, repo, result) => {
                app.apply_issues(account, repo, result);
                Ok(())
            }
            Update::IssuesDone => {
                app.busy = false;
                app.status = "Issue refresh finished. Repository completion is shown above. r retries; s saves focus".into();
                Ok(())
            }
            Update::Account(account) => app.identify(account),
            Update::Done(result) => {
                app.busy = false;
                result.and_then(|snapshot| app.apply(snapshot))
            }
        };
        if let Err(error) = result {
            app.status = format!("Refresh failed: {error}. Cached data may be stale. r retries.");
        }
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("gh-wanted [--demo]\n\nWatch GitHub repositories and organize local tags.\nAuthenticate with gh auth login before normal use.\n--demo uses fictional repositories and temporary in-memory storage.");
        return Ok(());
    }
    anyhow::ensure!(
        args.is_empty() || args == ["--demo"],
        "Unknown arguments; use --help"
    );
    let is_demo = args == ["--demo"];
    let store = if is_demo {
        Store::memory()?
    } else {
        let dirs = ProjectDirs::from("", "", "gh-wanted")
            .context("Cannot determine local data directory")?;
        std::fs::create_dir_all(dirs.data_local_dir())
            .context("Cannot create local data directory")?;
        Store::open(&dirs.data_local_dir().join("state.sqlite3"))?
    };
    let mut app = App::new(store, is_demo);
    let (sender, receiver) = mpsc::sync_channel(2);
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut worker = None;
    if is_demo {
        demo(&mut app)?;
    } else {
        app.busy = true;
        worker = Some(start_sync(sender.clone(), cancellation.clone()));
    }
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        loop {
            updates(&mut app, &receiver);
            terminal.draw(|frame| ui::draw(frame, &app))?;
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    match app.key(key) {
                        Ok(Action::OpenIssue(url)) => {
                            if is_demo {
                                app.status =
                                    "Demo issues are fictional; browser opening is disabled".into();
                            } else if let Err(error) = open_issue(&url) {
                                app.status = error.to_string();
                            } else {
                                app.status = "Opening issue in browser".into();
                            }
                        }
                        Ok(Action::FetchIssues) if !app.busy => {
                            if let Some(account) = app.account.clone() {
                                let repos: Vec<_> = app
                                    .visible()
                                    .into_iter()
                                    .map(|i| app.repositories[i].clone())
                                    .collect();
                                for repo in &repos {
                                    app.issue_status.insert(
                                        repo.id,
                                        "Loading; previous results may be stale".into(),
                                    );
                                }
                                if is_demo {
                                    demo_issues(&mut app, &repos);
                                } else if !repos.is_empty() {
                                    app.busy = true;
                                    app.status = "Loading all issue pages; results remain incomplete until each repository finishes".into();
                                    worker = Some(start_issues(
                                        sender.clone(),
                                        cancellation.clone(),
                                        account,
                                        repos,
                                    ));
                                }
                            } else {
                                app.status = "Connect first: b returns to repositories, r retries connection".into();
                            }
                        }
                        Ok(Action::FetchIssues) => {
                            app.status =
                                "Another refresh is running. Press r in Issues when it finishes"
                                    .into()
                        }
                        Ok(Action::Quit) => break,
                        Ok(Action::Refresh) if !app.busy => {
                            if is_demo {
                                demo(&mut app)?;
                            } else {
                                app.busy = true;
                                app.status = "Refreshing...".into();
                                worker = Some(start_sync(sender.clone(), cancellation.clone()));
                            }
                        }
                        Err(error) => app.status = format!("Action failed: {error}"),
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    })();
    cancellation.store(true, Ordering::Relaxed);
    drop(receiver);
    ratatui::restore();
    if let Some(worker) = worker {
        worker
            .join()
            .map_err(|_| anyhow::anyhow!("Sync worker panicked"))?;
    }
    result
}
fn main() {
    if let Err(error) = run() {
        eprintln!("gh-wanted: {error:#}");
        std::process::exit(1);
    }
}
