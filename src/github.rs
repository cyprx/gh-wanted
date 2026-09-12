use crate::repositories::Repository;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Account {
    pub id: u64,
    pub login: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub account: Account,
    pub repositories: Vec<Repository>,
}

pub struct GhClient {
    token: Option<OsString>,
    bound_account: Option<Account>,
    failure: Mutex<Option<crate::sync::RequestFailure>>,
    metrics: Option<crate::metrics::Metrics>,
    program: OsString,
    timeout: Duration,
    cancellation: Arc<AtomicBool>,
}

impl Default for GhClient {
    fn default() -> Self {
        Self::new("gh", Duration::from_secs(30))
    }
}

impl GhClient {
    pub fn new(program: impl Into<OsString>, timeout: Duration) -> Self {
        Self {
            token: None,
            bound_account: None,
            failure: Mutex::new(None),
            metrics: None,
            program: program.into(),
            timeout,
            cancellation: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn with_cancellation(mut self, cancellation: Arc<AtomicBool>) -> Self {
        self.cancellation = cancellation;
        self
    }

    pub fn with_metrics(mut self, path: std::path::PathBuf, kind: &str) -> Self {
        self.metrics = Some(crate::metrics::Metrics::new(path, kind));
        self
    }

    /// Freeze credentials for this refresh before verifying their account identity.
    /// Tokens never enter command arguments, metrics, or error messages.
    pub fn bind(mut self, expected: Option<&Account>) -> Result<Self> {
        let token = std::env::var_os("GH_TOKEN")
            .filter(|v| !v.is_empty())
            .or_else(|| std::env::var_os("GITHUB_TOKEN").filter(|v| !v.is_empty()));
        self.token = Some(match token {
            Some(token) => token,
            None => {
                let started = Instant::now();
                let mut command = Command::new(&self.program);
                command.args(["auth", "token", "--hostname", "github.com"]);
                let result = self.execute(command, false);
                if let Some(metrics) = &self.metrics {
                    metrics.record(serde_json::json!({"event":"credential_capture", "duration_ms":started.elapsed().as_millis() as u64, "outcome":crate::metrics::outcome(&result)}));
                }
                let bytes = result?;
                let token = std::str::from_utf8(&bytes)
                    .map_err(|_| anyhow!("Invalid GitHub credential response"))?
                    .trim();
                anyhow::ensure!(
                    !token.is_empty() && !token.chars().any(char::is_whitespace),
                    "Invalid GitHub credential response"
                );
                OsString::from(token)
            }
        });
        let account = self.account()?;
        if expected.is_some_and(|expected| expected.id != account.id) {
            return Err(crate::sync::RequestFailure {
                paused: true,
                retry_at: None,
            }
            .into());
        }
        self.bound_account = Some(account);
        if let Some(metrics) = &self.metrics {
            metrics.record(serde_json::json!({"event":"account_bound", "account_id":self.bound_account.as_ref().map(|a| a.id)}));
        }
        Ok(self)
    }

    pub fn check_refresh(&self) -> Result<()> {
        anyhow::ensure!(
            !self.cancellation.load(Ordering::Relaxed),
            "GitHub sync cancelled"
        );
        if let Some(failure) = self.failure.lock().expect("refresh failure lock").clone() {
            return Err(failure.into());
        }
        Ok(())
    }

    pub fn record_refresh_plan(&self, trigger: &str, account: Option<u64>, feeds: usize) {
        if let Some(metrics) = &self.metrics {
            metrics.record(serde_json::json!({"event":"refresh_plan", "trigger":trigger, "account_id":account, "planned_operations":feeds, "worker_limit":crate::sync::REFRESH_WORKERS}));
        }
    }

    pub fn record_skipped_feed(&self, repo: u64, feed: &str) {
        if let Some(metrics) = &self.metrics {
            metrics.record(serde_json::json!({"event":"operation_skipped", "repo_id":repo, "feed":feed, "reason":"shared_request_failure"}));
        }
    }

    pub fn account(&self) -> Result<Account> {
        if let Some(account) = &self.bound_account {
            return Ok(account.clone());
        }
        serde_json::from_slice(&self.api("user", false)?)
            .map_err(|_| anyhow!("GitHub returned an invalid account response"))
    }

    pub fn sync(&self) -> Result<Snapshot> {
        match &self.metrics {
            Some(metrics) => metrics.operation(
                "repositories",
                None,
                None,
                || self.sync_inner(),
                |s| s.repositories.len(),
            ),
            None => self.sync_inner(),
        }
    }

    fn sync_inner(&self) -> Result<Snapshot> {
        let account = self.account()?;
        let pages: Vec<Vec<Repository>> =
            serde_json::from_slice(&self.api("user/subscriptions?per_page=100", true)?)
                .map_err(|_| anyhow!("GitHub returned an invalid subscription response"))?;
        let final_account = self.account()?;
        if account != final_account {
            bail!("GitHub account changed during synchronization; try again");
        }
        let mut seen = std::collections::HashSet::new();
        let repositories = pages
            .into_iter()
            .flatten()
            .filter(|repo| seen.insert(repo.id))
            .collect();
        Ok(Snapshot {
            account,
            repositories,
        })
    }

    pub fn issues(
        &self,
        account: &Account,
        repo: &Repository,
    ) -> Result<Vec<crate::issues::Issue>> {
        match &self.metrics {
            Some(metrics) => metrics.operation(
                "issues",
                Some(repo.id),
                None,
                || self.issues_inner(account, repo),
                Vec::len,
            ),
            None => self.issues_inner(account, repo),
        }
    }

    fn issues_inner(
        &self,
        account: &Account,
        repo: &Repository,
    ) -> Result<Vec<crate::issues::Issue>> {
        anyhow::ensure!(
            crate::issues::valid_repo_name(&repo.full_name),
            "Invalid repository name"
        );
        anyhow::ensure!(
            self.account()? == *account,
            "GitHub account changed; refresh repositories"
        );
        let endpoint = format!(
            "repos/{}/issues?state=all&sort=updated&direction=desc&per_page=100",
            repo.full_name
        );
        let bytes = if self.bound_account.is_some() {
            // Own pagination so a shared pause is checked before every next page.
            let mut pages = Vec::new();
            let mut total = 0;
            let mut page = 1_u64;
            loop {
                let response = self.api_options(&format!("{endpoint}&page={page}"), false, true)?;
                total += response.len();
                anyhow::ensure!(
                    total <= OUTPUT_LIMIT,
                    "GitHub CLI output exceeded the 16 MiB limit"
                );
                let (headers, body) = split_response(&response)?;
                let records: Vec<serde_json::Value> = serde_json::from_slice(body)
                    .map_err(|_| anyhow!("GitHub returned an invalid issue response"))?;
                pages.push(records);
                if !headers.lines().any(|line| {
                    line.split_once(':').is_some_and(|(name, value)| {
                        name.eq_ignore_ascii_case("link") && value.contains("rel=\"next\"")
                    })
                }) {
                    break;
                }
                page += 1;
            }
            serde_json::to_vec(&pages)?
        } else {
            self.api(&endpoint, true)?
        };
        anyhow::ensure!(
            self.account()? == *account,
            "GitHub account changed during issue refresh"
        );
        parse_issues(repo.id, &bytes)
    }

    fn api(&self, endpoint: &str, paginate: bool) -> Result<Vec<u8>> {
        self.api_options(endpoint, paginate, false)
    }

    pub fn activity(
        &self,
        request: &crate::sync::FeedRequest,
    ) -> Result<Vec<crate::activity::Activity>> {
        match &self.metrics {
            Some(metrics) => {
                metrics.record(serde_json::json!({"event":"feed_window", "repo_id":request.repo.id, "feed":request.state.feed,
                    "initial":request.state.checkpoint.is_none(), "lower":request.state.lower(), "upper":request.upper}));
                metrics.operation(
                    "activity",
                    Some(request.repo.id),
                    Some(&request.state.feed),
                    || self.activity_inner(request),
                    Vec::len,
                )
            }
            None => self.activity_inner(request),
        }
    }

    fn activity_inner(
        &self,
        request: &crate::sync::FeedRequest,
    ) -> Result<Vec<crate::activity::Activity>> {
        anyhow::ensure!(
            crate::issues::valid_repo_name(&request.repo.full_name),
            "Invalid repository name"
        );
        if self.account()? != request.account {
            return Err(crate::sync::RequestFailure {
                paused: true,
                retry_at: None,
            }
            .into());
        }
        let name = &request.repo.full_name;
        let since = crate::activity::iso(request.state.lower())?;
        let endpoint = match request.state.feed.as_str() {
            "issues" => format!("repos/{name}/issues?state=all&sort=updated&direction=asc&since={since}&per_page=100"),
            "comments" => format!("repos/{name}/issues/comments?sort=updated&direction=asc&since={since}&per_page=100"),
            feed => {
                let pr = feed.strip_prefix("reviews:").context("Unknown activity feed")?.parse::<u64>()?;
                anyhow::ensure!(pr > 0, "Invalid PR number");
                format!("repos/{name}/pulls/{pr}/reviews?per_page=100")
            }
        };
        let mut page = 1_u64;
        let mut total = 0;
        let mut values = Vec::new();
        loop {
            let response = self.api_options(&format!("{endpoint}&page={page}"), false, true)?;
            total += response.len();
            anyhow::ensure!(
                total <= OUTPUT_LIMIT,
                "Activity feed exceeded the 16 MiB limit; checkpoint unchanged"
            );
            let (headers, body) = split_response(&response)?;
            let records: Vec<serde_json::Value> = serde_json::from_slice(body)
                .map_err(|_| anyhow!("GitHub returned an invalid activity page"))?;
            values.extend(records);
            if !headers.lines().any(|line| {
                line.split_once(':').is_some_and(|(name, value)| {
                    name.eq_ignore_ascii_case("link") && value.contains("rel=\"next\"")
                })
            }) {
                break;
            }
            page += 1;
        }
        if self.account()? != request.account {
            return Err(crate::sync::RequestFailure {
                paused: true,
                retry_at: None,
            }
            .into());
        }
        let mut events = crate::activity::parse_records(
            request.repo.id,
            &request.state.feed,
            values,
            request.state.lower(),
            request.upper,
        )?;
        let fetched_at = chrono::Utc::now().timestamp();
        for event in &mut events {
            event.fetched_at = fetched_at;
        }
        Ok(events)
    }

    fn api_options(&self, endpoint: &str, paginate: bool, headers: bool) -> Result<Vec<u8>> {
        self.check_refresh()?;
        let started = Instant::now();
        let path = endpoint.split('?').next().unwrap_or(endpoint);
        let parts: Vec<_> = path.split('/').collect();
        let endpoint_kind = if parts.first() == Some(&"repos") && parts.len() >= 4 {
            format!("repos/:owner/:repo/{}", parts[3..].join("/"))
        } else {
            path.to_owned()
        };
        if let Some(metrics) = &self.metrics {
            metrics
                .record(serde_json::json!({"event":"request_started", "endpoint":endpoint_kind}));
        }
        let result = self.api_options_inner(endpoint, paginate, headers);
        if let Some(policy) = result
            .as_ref()
            .err()
            .and_then(|e| e.downcast_ref::<crate::sync::RequestFailure>())
        {
            if policy.paused || policy.retry_at.is_some() {
                let mut failure = self.failure.lock().expect("refresh failure lock");
                let previous = failure.get_or_insert_with(|| policy.clone());
                previous.paused |= policy.paused;
                previous.retry_at = previous.retry_at.into_iter().chain(policy.retry_at).max();
            }
        }
        if let Some(metrics) = &self.metrics {
            let mut pages = None;
            let mut records = None;
            let mut status = None;
            let mut remaining = None;
            let mut reset = None;
            if let Ok(bytes) = &result {
                let body = if headers {
                    if let Ok((head, body)) = split_response(bytes) {
                        status = Some(200_u16);
                        for line in head.lines() {
                            if let Some((name, value)) = line.split_once(':') {
                                match name.to_ascii_lowercase().as_str() {
                                    "x-ratelimit-remaining" => {
                                        remaining = value.trim().parse::<u64>().ok()
                                    }
                                    "x-ratelimit-reset" => reset = value.trim().parse::<i64>().ok(),
                                    _ => {}
                                }
                            }
                        }
                        body
                    } else {
                        bytes.as_slice()
                    }
                } else {
                    bytes.as_slice()
                };
                if let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) {
                    if paginate {
                        if let Some(batch) = value.as_array() {
                            pages = Some(batch.len());
                            records = Some(
                                batch
                                    .iter()
                                    .filter_map(|p| p.as_array())
                                    .map(Vec::len)
                                    .sum::<usize>(),
                            );
                        }
                    } else {
                        pages = Some(1);
                        records = value.as_array().map(Vec::len);
                    }
                }
            }
            let retry_at = result
                .as_ref()
                .err()
                .and_then(|e| e.downcast_ref::<crate::sync::RequestFailure>())
                .and_then(|p| p.retry_at);
            metrics.record(
                serde_json::json!({"event":"request_finished", "endpoint":endpoint_kind,
                "duration_ms":started.elapsed().as_millis() as u64, "paginated":paginate,
                "outcome":crate::metrics::outcome(&result), "pages":pages, "records":records,
                "bytes":result.as_ref().ok().map(Vec::len), "http_status":status,
                "rate_remaining":remaining,"rate_reset":reset,"retry_at":retry_at}),
            );
        }
        result
    }

    fn api_options_inner(&self, endpoint: &str, paginate: bool, headers: bool) -> Result<Vec<u8>> {
        anyhow::ensure!(
            !self.cancellation.load(Ordering::Relaxed),
            "GitHub sync cancelled"
        );
        let mut command = Command::new(&self.program);
        command.args(["api", "--hostname", "github.com", endpoint]);
        if headers {
            command.arg("--include");
        }
        if paginate {
            command.args(["--paginate", "--slurp"]);
        }
        self.execute(command, true)
    }

    fn execute(&self, mut command: Command, record_http_failure: bool) -> Result<Vec<u8>> {
        anyhow::ensure!(
            !self.cancellation.load(Ordering::Relaxed),
            "GitHub sync cancelled"
        );
        if let Some(token) = &self.token {
            command.env("GH_TOKEN", token).env_remove("GITHUB_TOKEN");
        }
        command
            .env_remove("GH_DEBUG")
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_PAGER", "cat")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .context("Could not start GitHub CLI; install gh and authenticate with github.com")?;
        let (sender, receiver) = mpsc::sync_channel(8);
        read_pipe(
            child.stdout.take().expect("piped stdout"),
            true,
            sender.clone(),
        );
        read_pipe(child.stderr.take().expect("piped stderr"), false, sender);
        let result = collect(
            &mut child,
            receiver,
            self.timeout,
            &self.cancellation,
            if record_http_failure {
                self.metrics.as_ref()
            } else {
                None
            },
        );
        if result.is_err() {
            let _ = child.kill();
            child
                .wait()
                .context("Could not reap GitHub CLI after failure")?;
        }
        result
    }
}

enum PipeEvent {
    Data(bool, Vec<u8>),
    End,
    Failed,
}

fn read_pipe(mut pipe: impl Read + Send + 'static, stdout: bool, sender: SyncSender<PipeEvent>) {
    std::thread::spawn(move || {
        let mut buffer = [0; 8192];
        loop {
            let event = match pipe.read(&mut buffer) {
                Ok(0) => PipeEvent::End,
                Ok(n) => PipeEvent::Data(stdout, buffer[..n].to_vec()),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => PipeEvent::Failed,
            };
            let finished = !matches!(event, PipeEvent::Data(..));
            if sender.send(event).is_err() || finished {
                break;
            }
        }
    });
}

fn collect(
    child: &mut std::process::Child,
    receiver: Receiver<PipeEvent>,
    timeout: Duration,
    cancellation: &AtomicBool,
    metrics: Option<&crate::metrics::Metrics>,
) -> Result<Vec<u8>> {
    let started = Instant::now();
    let mut output = Vec::new();
    let mut stderr = Vec::new();
    let mut total = 0usize;
    let mut ended = 0;
    let mut status = None;
    loop {
        anyhow::ensure!(
            !cancellation.load(Ordering::Relaxed),
            "GitHub sync cancelled"
        );
        if started.elapsed() >= timeout {
            bail!("GitHub CLI timed out");
        }
        if status.is_none() {
            status = child
                .try_wait()
                .context("Could not inspect GitHub CLI status")?;
        }
        if ended == 2 {
            if let Some(status) = status {
                if !status.success() {
                    if let Some(metrics) = metrics {
                        metrics.record_http_failure(&output);
                    }
                    return Err(crate::sync::request_failure(
                        &output,
                        &stderr,
                        chrono::Utc::now().timestamp(),
                    )
                    .into());
                }
                return Ok(output);
            }
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        match receiver.recv_timeout(Duration::from_millis(5)) {
            Ok(PipeEvent::Data(stdout, bytes)) => {
                total += bytes.len();
                if total > OUTPUT_LIMIT {
                    bail!("GitHub CLI output exceeded the 16 MiB limit");
                }
                if stdout {
                    output.extend_from_slice(&bytes);
                } else {
                    stderr.extend_from_slice(&bytes);
                }
            }
            Ok(PipeEvent::End) => ended += 1,
            Ok(PipeEvent::Failed) => bail!("Could not read GitHub CLI output"),
            Err(mpsc::RecvTimeoutError::Timeout) => (),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                bail!("GitHub CLI output stream closed unexpectedly")
            }
        }
    }
}

pub fn parse_issues(repo_id: u64, bytes: &[u8]) -> Result<Vec<crate::issues::Issue>> {
    #[derive(Deserialize)]
    struct Label {
        name: String,
    }
    #[derive(Deserialize)]
    struct Assignee {
        login: String,
    }
    #[derive(Deserialize)]
    struct Record {
        number: u64,
        title: String,
        body: Option<String>,
        state: String,
        created_at: String,
        updated_at: String,
        labels: Vec<Label>,
        #[serde(default)]
        assignees: Vec<Assignee>,
        html_url: String,
    }
    let invalid = || anyhow!("GitHub returned an invalid issue response");
    let pages: Vec<Vec<serde_json::Value>> =
        serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let mut issues = std::collections::BTreeMap::new();
    for value in pages.into_iter().flatten() {
        if value.get("pull_request").is_some() {
            continue;
        }
        let record: Record = serde_json::from_value(value).map_err(|_| invalid())?;
        anyhow::ensure!(
            record.number > 0 && matches!(record.state.as_str(), "open" | "closed"),
            "GitHub returned an invalid issue response"
        );
        let issue = crate::issues::Issue {
            repo_id,
            number: record.number,
            title: record.title,
            body: record.body,
            state: record.state,
            created_at: record.created_at,
            updated_at: record.updated_at,
            labels: record.labels.into_iter().map(|l| l.name).collect(),
            assignees: record.assignees.into_iter().map(|a| a.login).collect(),
            url: record.html_url,
        };
        let previous: Option<&crate::issues::Issue> = issues.get(&issue.number);
        if previous.is_none_or(|old| old.updated_at <= issue.updated_at) {
            issues.insert(issue.number, issue);
        }
    }
    Ok(issues.into_values().collect())
}

fn split_response(response: &[u8]) -> Result<(&str, &[u8])> {
    let split = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| (i, 4))
        .or_else(|| {
            response
                .windows(2)
                .position(|w| w == b"\n\n")
                .map(|i| (i, 2))
        })
        .context("GitHub response missing HTTP headers; checkpoint unchanged")?;
    let headers = std::str::from_utf8(&response[..split.0]).context("Invalid HTTP headers")?;
    let status = headers
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .context("Missing HTTP status")?;
    anyhow::ensure!(
        status == "200",
        "Unexpected GitHub response status; checkpoint unchanged"
    );
    Ok((headers, &response[split.0 + split.1..]))
}
