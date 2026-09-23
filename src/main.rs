use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyEventKind};
use directories::ProjectDirs;
use gh_wanted::{
    app::{Action, App, View},
    github::{Account, GhClient, Snapshot},
    repositories::Repository,
    scheduler::{Lane, Scheduler},
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
    Tracked(u64, Result<Repository>),
    Activity(
        gh_wanted::sync::FeedRequest,
        Result<Vec<gh_wanted::activity::Activity>>,
    ),
    ActivityDone,
}

type Message = (Lane, u64, Update);
struct WorkerSender {
    sender: SyncSender<Message>,
    lane: Lane,
    generation: u64,
    scheduler: Arc<Scheduler>,
}
impl WorkerSender {
    fn send(&self, update: Update) -> Result<()> {
        self.sender
            .send((self.lane, self.generation, update))
            .map_err(|_| anyhow::anyhow!("UI closed"))
    }
}

#[derive(Default)]
struct Refreshes {
    scheduler: Arc<Scheduler>,
    generations: std::collections::HashMap<Lane, u64>,
    running: std::collections::HashMap<Lane, Arc<AtomicBool>>,
    workers: Vec<thread::JoinHandle<()>>,
}
impl Refreshes {
    fn begin(
        &mut self,
        lane: Lane,
        sender: &SyncSender<Message>,
    ) -> (WorkerSender, Arc<AtomicBool>) {
        self.cancel(lane);
        let generation = self.generations.entry(lane).or_default();
        *generation += 1;
        let cancellation = Arc::new(AtomicBool::new(false));
        self.running.insert(lane, cancellation.clone());
        (
            WorkerSender {
                sender: sender.clone(),
                lane,
                generation: *generation,
                scheduler: self.scheduler.clone(),
            },
            cancellation,
        )
    }
    fn cancel(&mut self, lane: Lane) {
        if let Some(token) = self.running.remove(&lane) {
            token.store(true, Ordering::Relaxed);
        }
        *self.generations.entry(lane).or_default() += 1;
    }
    fn focus(&self, app: &mut App) {
        let lane = match app.view {
            View::Activity => Some(Lane::Activity),
            View::Issues => Some(Lane::Issues),
            View::Repositories => Some(Lane::Repositories),
            View::Focuses => None,
        };
        self.scheduler.activate(lane);
        app.busy = self.running.contains_key(&Lane::Repositories)
            || lane.is_some_and(|lane| self.running.contains_key(&lane));
    }
    fn activity(
        &mut self,
        sender: &SyncSender<Message>,
        requests: Vec<gh_wanted::sync::FeedRequest>,
        trigger: &'static str,
    ) {
        let (sender, cancellation) = self.begin(Lane::Activity, sender);
        self.workers
            .push(start_activity(sender, cancellation, requests, trigger));
    }
    fn issues(
        &mut self,
        sender: &SyncSender<Message>,
        account: Account,
        repos: Vec<Repository>,
        days: u16,
        now: i64,
    ) {
        let (sender, cancellation) = self.begin(Lane::Issues, sender);
        self.workers.push(start_issues(
            sender,
            cancellation,
            account,
            repos,
            days,
            now,
        ));
    }
    fn repositories(&mut self, sender: &SyncSender<Message>) {
        self.cancel(Lane::Activity);
        self.cancel(Lane::Issues);
        let (sender, cancellation) = self.begin(Lane::Repositories, sender);
        self.workers.push(start_sync(sender, cancellation));
    }
}

fn metrics_path() -> Result<std::path::PathBuf> {
    let dirs =
        ProjectDirs::from("", "", "gh-wanted").context("Cannot determine local data directory")?;
    Ok(dirs.data_local_dir().join("refresh-metrics.jsonl"))
}

fn refresh_client(cancellation: Arc<AtomicBool>, kind: &str, sender: &WorkerSender) -> GhClient {
    let client = GhClient::default()
        .with_cancellation(cancellation)
        .with_scheduler(sender.scheduler.clone(), sender.lane);
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
    sender: WorkerSender,
    cancellation: Arc<AtomicBool>,
    requests: Vec<gh_wanted::sync::FeedRequest>,
    trigger: &'static str,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let client = refresh_client(cancellation.clone(), "activity", &sender);
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
    sender: WorkerSender,
    cancellation: Arc<AtomicBool>,
    account: Account,
    repos: Vec<Repository>,
    days: u16,
    now: i64,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let client = refresh_client(cancellation.clone(), "issues", &sender);
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

fn start_sync(sender: WorkerSender, cancellation: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let client = refresh_client(cancellation, "repositories", &sender);
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

fn updates(app: &mut App, receiver: &Receiver<Message>, refreshes: &mut Refreshes) {
    while let Ok((lane, generation, update)) = receiver.try_recv() {
        if refreshes.generations.get(&lane) != Some(&generation) {
            continue;
        }
        if matches!(
            &update,
            Update::Done(_) | Update::ActivityDone | Update::IssuesDone | Update::Tracked(..)
        ) {
            refreshes.running.remove(&lane);
        }
        let result = match update {
            Update::Tracked(account, result) => {
                result.and_then(|repo| app.apply_tracked(account, repo))
            }
            Update::Activity(request, result) => app.apply_activity(&request, result),
            Update::ActivityDone => {
                app.busy = false;
                app.activity_pending.clear();
                app.next_refresh = app.now.saturating_add(gh_wanted::sync::REFRESH_SECONDS);
                if app.view == View::Activity && app.feeds.iter().all(|f| f.error.is_none()) {
                    app.status = "Activity updated".into();
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
                if app.view == View::Issues {
                    app.status = "Issue refresh finished".into();
                }
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
    let mut refreshes = Refreshes::default();
    if is_demo {
        demo(&mut app)?;
        let requests = app.prepare_activity(true, None)?;
        demo_activity(&mut app, requests)?;
    } else {
        app.busy = true;
        refreshes.repositories(&sender);
    }
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        loop {
            app.now = chrono::Utc::now().timestamp();
            updates(&mut app, &receiver, &mut refreshes);
            refreshes.focus(&mut app);
            refreshes.workers.retain(|worker| !worker.is_finished());
            // Navigation during initial discovery should load once repositories arrive.
            if !is_demo
                && app.view == View::Issues
                && !app.busy
                && !app.auto_paused
                && app.check_network_window().is_ok()
                && app
                    .visible()
                    .iter()
                    .any(|i| !app.issue_status.contains_key(&app.repositories[*i].id))
            {
                if let Some(account) = app.account.clone() {
                    refreshes.scheduler.retry(app.now);
                    let mut repos: Vec<_> = app
                        .visible()
                        .into_iter()
                        .filter(|i| !app.issue_status.contains_key(&app.repositories[*i].id))
                        .map(|i| app.repositories[i].clone())
                        .collect();
                    repos.sort_by_key(|repo| app.refresh_priority(repo.id));
                    for repo in &repos {
                        app.issue_status.insert(repo.id, "Loading".into());
                    }
                    refreshes.issues(&sender, account, repos, app.issue_window_days(), app.now);
                    refreshes.focus(&mut app);
                }
            }
            if app.view == View::Activity
                && !refreshes.running.contains_key(&Lane::Activity)
                && app.auto_due()
            {
                refreshes.scheduler.retry(app.now);
                match app.prepare_activity(false, None) {
                    Ok(requests) if !requests.is_empty() => {
                        refreshes.activity(&sender, requests, "automatic")
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
                    let previous_view = app.view;
                    let previous_days = app.issue_window_days();
                    let action = app.key(key);
                    if previous_days != app.issue_window_days() {
                        refreshes.cancel(Lane::Issues);
                    }
                    refreshes.focus(&mut app);
                    // Returning to an unfinished refresh resumes its retained page state.
                    if matches!(&action, Ok(Action::FetchIssues))
                        && refreshes.running.contains_key(&Lane::Issues)
                    {
                        app.status = "Resuming issues…".into();
                        continue;
                    }
                    if previous_view != app.view && app.busy {
                        app.status = "Resuming refresh…".into();
                    }
                    if matches!(
                        &action,
                        Ok(Action::Refresh
                            | Action::TrackRepository(_)
                            | Action::FetchIssues
                            | Action::FetchActivity
                            | Action::FetchReviews(..))
                    ) {
                        if let Err(error) = app.check_network_window() {
                            app.status = error.to_string();
                            continue;
                        }
                        refreshes.scheduler.retry(app.now);
                    }
                    match action {
                        Ok(Action::TrackRepository(name)) => {
                            if is_demo {
                                app.status = "Tracking requires a real GitHub connection; disabled in demo mode".into();
                            } else if let Some(account) = app.account.clone() {
                                let (worker_sender, cancellation) =
                                    refreshes.begin(Lane::Repositories, &sender);
                                app.status = format!("Checking {name}…");
                                refreshes.workers.push(thread::spawn(move || {
                                    let result =
                                        refresh_client(cancellation, "track", &worker_sender)
                                            .bind(Some(&account))
                                            .and_then(|client| client.repository(&name));
                                    let _ = worker_sender.send(Update::Tracked(account.id, result));
                                }));
                            }
                        }
                        Ok(action @ (Action::FetchActivity | Action::FetchReviews(..))) => {
                            let review = match action {
                                Action::FetchReviews(repo, number) => Some((repo, number)),
                                _ => None,
                            };
                            match app.prepare_activity(true, review) {
                                Ok(requests) if is_demo => demo_activity(&mut app, requests)?,
                                Ok(requests) if !requests.is_empty() => {
                                    refreshes.activity(&sender, requests, "manual")
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
                                    app.status = "Refreshing issues…".into();
                                    refreshes.issues(
                                        &sender,
                                        account,
                                        repos,
                                        app.issue_window_days(),
                                        app.now,
                                    );
                                }
                            } else {
                                app.status = "Connect first: b returns to repositories, r retries connection".into();
                            }
                        }
                        Ok(Action::FetchIssues) => {
                            app.status = "Issues will load after repository discovery".into()
                        }
                        Ok(Action::Quit) => break,
                        Ok(Action::Refresh)
                            if !refreshes.running.contains_key(&Lane::Repositories) =>
                        {
                            if is_demo {
                                demo(&mut app)?;
                            } else {
                                app.busy = true;
                                app.activity_pending.clear();
                                app.next_refresh = 0;
                                app.issue_status.clear();
                                app.status = "Refreshing repositories…".into();
                                refreshes.repositories(&sender);
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
    for cancellation in refreshes.running.values() {
        cancellation.store(true, Ordering::Relaxed);
    }
    drop(receiver);
    ratatui::restore();
    for worker in refreshes.workers {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_views_changes_busy_without_cancelling_refreshes() {
        let (sender, _) = mpsc::sync_channel(4);
        let mut refreshes = Refreshes::default();
        let mut app = App::new(Store::memory().unwrap(), true);
        let (_, activity) = refreshes.begin(Lane::Activity, &sender);
        refreshes.focus(&mut app);
        assert!(app.busy);
        app.view = View::Issues;
        refreshes.focus(&mut app);
        assert!(!app.busy);
        assert!(!activity.load(Ordering::Relaxed));
        let (_, issues) = refreshes.begin(Lane::Issues, &sender);
        refreshes.focus(&mut app);
        assert!(app.busy);
        app.view = View::Focuses;
        refreshes.focus(&mut app);
        assert!(!app.busy);
        assert!(!issues.load(Ordering::Relaxed));
        app.view = View::Activity;
        refreshes.focus(&mut app);
        assert!(app.busy);
    }

    #[test]
    fn obsolete_completion_cannot_finish_a_replacement_refresh() {
        let (sender, receiver) = mpsc::sync_channel(4);
        let mut refreshes = Refreshes::default();
        let mut app = App::new(Store::memory().unwrap(), true);
        let (old, cancellation) = refreshes.begin(Lane::Issues, &sender);
        let (new, _) = refreshes.begin(Lane::Issues, &sender);
        assert!(cancellation.load(Ordering::Relaxed));
        old.send(Update::IssuesDone).unwrap();
        updates(&mut app, &receiver, &mut refreshes);
        assert!(refreshes.running.contains_key(&Lane::Issues));
        new.send(Update::IssuesDone).unwrap();
        updates(&mut app, &receiver, &mut refreshes);
        assert!(!refreshes.running.contains_key(&Lane::Issues));
    }
}
