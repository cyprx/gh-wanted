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
    Issues(u64, u64, u16, Result<Vec<gh_wanted::issues::Issue>>),
    IssuesDone,
    Activity(
        gh_wanted::sync::FeedRequest,
        Result<Vec<gh_wanted::activity::Activity>>,
    ),
    ActivityDone,
}

fn metrics_path() -> Result<std::path::PathBuf> {
    let dirs =
        ProjectDirs::from("", "", "gh-wanted").context("Cannot determine local data directory")?;
    Ok(dirs.data_local_dir().join("refresh-metrics.jsonl"))
}

fn refresh_client(cancellation: Arc<AtomicBool>, kind: &str) -> GhClient {
    let client = GhClient::default().with_cancellation(cancellation);
    match metrics_path() {
        Ok(path) => client.with_metrics(path, kind),
        Err(_) => client,
    }
}

fn binding_failure(error: &anyhow::Error) -> anyhow::Error {
    match error.downcast_ref::<gh_wanted::sync::RequestFailure>() {
        Some(policy) => policy.clone().into(),
        None => anyhow::anyhow!("Could not bind refresh credentials: {error}"),
    }
}

fn start_activity(
    sender: SyncSender<Update>,
    cancellation: Arc<AtomicBool>,
    requests: Vec<gh_wanted::sync::FeedRequest>,
    trigger: &'static str,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let client = refresh_client(cancellation.clone(), "activity");
        client.record_refresh_plan(
            trigger,
            requests.first().map(|r| r.account.id),
            requests.len(),
        );
        let expected = requests.first().map(|r| &r.account);
        let client = client.bind(expected);
        gh_wanted::sync::run_bounded(
            requests,
            &cancellation,
            |request| match &client {
                Ok(client) => {
                    if let Err(error) = client.check_refresh() {
                        client.record_skipped_feed(request.repo.id, &request.state.feed);
                        return Err(error);
                    }
                    client.activity(request)
                }
                Err(error) => Err(binding_failure(error)),
            },
            |request, result| sender.send(Update::Activity(request, result)).is_ok(),
        );
        let _ = sender.send(Update::ActivityDone);
    })
}

fn demo_activity(app: &mut App, requests: Vec<gh_wanted::sync::FeedRequest>) -> Result<()> {
    use gh_wanted::activity::{Activity, ActivityKind};
    for request in requests {
        let anchor = request.state.initial_since + gh_wanted::sync::INITIAL_HISTORY_SECONDS;
        let kind = match request.state.feed.as_str() {
            "issues" => ActivityKind::NewIssue,
            "comments" => ActivityKind::Comment,
            _ => ActivityKind::Review,
        };
        let occurred_at = if kind == ActivityKind::Comment {
            anchor - 86399
        } else {
            anchor
        };
        let number = request
            .state
            .feed
            .strip_prefix("reviews:")
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(1);
        let mut events = vec![Activity { repo_id: request.repo.id, key: format!("demo-{:?}",kind), source_id: 1, number: 1, kind: kind.clone(), title: if kind == ActivityKind::Comment { "Contributor shared a reproduction" } else if kind == ActivityKind::Review { "Review: APPROVED" } else { "Improve keyboard navigation" }.into(), body: "Try the keyboard flow and acknowledge this update locally. Refresh keeps your read state.".into(), actor: "demo-contributor".into(), association: "CONTRIBUTOR".into(), url: format!("https://github.com/{}/issues/1",request.repo.full_name), occurred_at, fetched_at: app.now, acknowledged: false }];
        if kind == ActivityKind::Review {
            events[0].number = number;
            events[0].key = format!("demo-review-{number}");
            events[0].url = format!(
                "https://github.com/{}/pull/{number}",
                request.repo.full_name
            );
        }
        if request.state.feed == "issues" {
            events.push(Activity {
                kind: ActivityKind::PrChange,
                key: "demo-pr".into(),
                source_id: 2,
                number: 2,
                title: "PR: Improve empty-list navigation".into(),
                url: format!("https://github.com/{}/pull/2", request.repo.full_name),
                ..events[0].clone()
            });
        }
        events.retain(|event| {
            event.occurred_at >= request.state.lower() && event.occurred_at <= request.upper
        });
        app.apply_activity(&request, Ok(events))?;
    }
    app.busy = false;
    app.status = "Demo activity ready. d opens Today; a acknowledges; v loads PR reviews".into();
    Ok(())
}

fn start_issues(
    sender: SyncSender<Update>,
    cancellation: Arc<AtomicBool>,
    account: Account,
    repos: Vec<Repository>,
    days: u16,
    now: i64,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let client = refresh_client(cancellation.clone(), "issues");
        client.record_refresh_plan("manual", Some(account.id), repos.len());
        let client = client
            .with_issue_window(days, now)
            .and_then(|client| client.bind(Some(&account)));
        gh_wanted::sync::run_bounded(
            repos,
            &cancellation,
            |repo| match &client {
                Ok(client) => {
                    if let Err(error) = client.check_refresh() {
                        client.record_skipped_feed(repo.id, "issues");
                        return Err(error);
                    }
                    client.issues(&account, repo)
                }
                Err(error) => Err(binding_failure(error)),
            },
            |repo, result| {
                sender
                    .send(Update::Issues(account.id, repo.id, days, result))
                    .is_ok()
            },
        );
        let _ = sender.send(Update::IssuesDone);
    })
}

fn demo_issues(app: &mut App, repos: &[Repository]) {
    let current_date = chrono::Utc::now().to_rfc3339();
    for repo in repos {
        app.apply_issues(1, repo.id, Ok(vec![gh_wanted::issues::Issue {
            repo_id: repo.id, number: 1, title: "Improve keyboard navigation".into(),
            body: Some("Help new contributors navigate the list with the keyboard.\n\nAdd a regression test for moving through an empty list, and document the shortcuts.".into()),
            state: "open".into(), created_at: current_date.clone(), updated_at: current_date.clone(),
            labels: vec!["good first issue".into(), "enhancement".into()], assignees: vec![],
            url: format!("https://github.com/{}/issues/1", repo.full_name),
        }]));
    }
}

fn open_url(url: &str) -> Result<()> {
    anyhow::ensure!(
        gh_wanted::activity::valid_activity_url(url)
            || url
                .strip_prefix("https://github.com/")
                .is_some_and(gh_wanted::issues::valid_repo_name),
        "Invalid GitHub URL"
    );
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
        let client = refresh_client(cancellation, "repositories");
        let result = (|| {
            let client = client.bind(None)?;
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
            Update::Activity(request, result) => app.apply_activity(&request, result),
            Update::ActivityDone => {
                app.busy = false;
                app.activity_pending.clear();
                app.next_refresh = app.now.saturating_add(gh_wanted::sync::REFRESH_SECONDS);
                if app.feeds.iter().all(|f| f.error.is_none()) {
                    app.status = "Activity saved. d Today/catch-up; a acknowledge; refresh every 15 minutes while running".into();
                }
                Ok(())
            }
            Update::Issues(account, repo, days, result) => {
                if let Err(error) = &result {
                    app.observe_failure(error);
                }
                app.apply_issues_for_window(account, repo, days, result);
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
            app.observe_failure(&error);
            app.status = format!("Refresh failed: {error}. Cached data may be stale. r retries.");
        }
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--refresh-metrics"] {
        let path = metrics_path()?;
        eprintln!("Refresh metrics: {}", path.display());
        println!("{}", gh_wanted::metrics::report(&path)?);
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("gh-wanted [--demo | --refresh-metrics]\n\nWatch GitHub repositories and organize local tags.\nAuthenticate with gh auth login before normal use.\n--demo uses fictional repositories and temporary in-memory storage.\n--refresh-metrics summarizes local refresh measurements and prints their file path.");
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
        let requests = app.prepare_activity(true, None)?;
        demo_activity(&mut app, requests)?;
    } else {
        app.busy = true;
        worker = Some(start_sync(sender.clone(), cancellation.clone()));
    }
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        loop {
            app.now = chrono::Utc::now().timestamp();
            updates(&mut app, &receiver);
            if app.auto_due() {
                match app.prepare_activity(false, None) {
                    Ok(requests) if !requests.is_empty() => {
                        worker = Some(start_activity(
                            sender.clone(),
                            cancellation.clone(),
                            requests,
                            "automatic",
                        ))
                    }
                    Err(error) => {
                        app.status = format!("Automatic activity refresh failed: {error}");
                        app.next_refresh = app.now.saturating_add(gh_wanted::sync::REFRESH_SECONDS);
                    }
                    _ => {}
                }
            }
            terminal.draw(|frame| ui::draw(frame, &app))?;
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    let action = app.key(key);
                    if matches!(
                        &action,
                        Ok(Action::Refresh
                            | Action::FetchIssues
                            | Action::FetchActivity
                            | Action::FetchReviews(..))
                    ) {
                        if let Err(error) = app.check_network_window() {
                            app.status = error.to_string();
                            continue;
                        }
                    }
                    match action {
                        Ok(action @ (Action::FetchActivity | Action::FetchReviews(..))) => {
                            let review = match action {
                                Action::FetchReviews(repo, number) => Some((repo, number)),
                                _ => None,
                            };
                            match app.prepare_activity(true, review) {
                                Ok(requests) if is_demo => demo_activity(&mut app, requests)?,
                                Ok(requests) if !requests.is_empty() => {
                                    worker = Some(start_activity(
                                        sender.clone(),
                                        cancellation.clone(),
                                        requests,
                                        "manual",
                                    ))
                                }
                                Ok(_) => app.status =
                                    "No watched repositories to refresh. b returns to repositories"
                                        .into(),
                                Err(error) => app.status = error.to_string(),
                            }
                        }
                        Ok(Action::OpenIssue(url)) => {
                            if is_demo {
                                app.status =
                                    "Demo issues are fictional; browser opening is disabled".into();
                            } else if let Err(error) = open_url(&url) {
                                app.status = error.to_string();
                            } else {
                                app.status = "Opening issue in browser".into();
                            }
                        }
                        Ok(Action::OpenRepository(url)) => {
                            if is_demo {
                                app.status =
                                    "Demo repository are fictional; browser opening is disabled"
                                        .into();
                            } else if let Err(error) = open_url(&url) {
                                app.status = error.to_string();
                            } else {
                                app.status = "Opening repository in browser".into();
                            }
                        }
                        Ok(Action::FetchIssues) if !app.busy => {
                            if let Some(account) = app.account.clone() {
                                let mut repos: Vec<_> = app
                                    .visible()
                                    .into_iter()
                                    .map(|i| app.repositories[i].clone())
                                    .collect();
                                repos.sort_by_key(|repo| app.refresh_priority(repo.id));
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
                                        app.issue_window_days(),
                                        app.now,
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
