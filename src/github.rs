use crate::repositories::Repository;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
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
            program: program.into(),
            timeout,
            cancellation: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn with_cancellation(mut self, cancellation: Arc<AtomicBool>) -> Self {
        self.cancellation = cancellation;
        self
    }

    pub fn account(&self) -> Result<Account> {
        serde_json::from_slice(&self.api("user", false)?)
            .map_err(|_| anyhow!("GitHub returned an invalid account response"))
    }

    pub fn sync(&self) -> Result<Snapshot> {
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
        let bytes = self.api(&endpoint, true)?;
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
        command
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
        let result = collect(&mut child, receiver, self.timeout, &self.cancellation);
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
