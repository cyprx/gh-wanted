use crate::{github::Account, repositories::Repository};

pub const REFRESH_SECONDS: i64 = 15 * 60;
pub const OVERLAP_SECONDS: i64 = 60;
pub const INITIAL_HISTORY_SECONDS: i64 = 24 * 60 * 60;

#[derive(Clone, Debug)]
pub struct FeedState {
    pub repo_id: u64,
    pub feed: String,
    pub initial_since: i64,
    pub checkpoint: Option<i64>,
    pub error: Option<String>,
    pub retry_at: Option<i64>,
    pub paused: bool,
}

impl FeedState {
    pub fn lower(&self) -> i64 {
        if self.feed.starts_with("reviews:") {
            return self.initial_since;
        }
        self.checkpoint
            .map(|v| v.saturating_sub(OVERLAP_SECONDS))
            .unwrap_or(self.initial_since)
            .max(self.initial_since)
    }
    pub fn ready(&self, now: i64, manual: bool) -> bool {
        (manual || !self.paused) && self.retry_at.is_none_or(|at| now >= at)
    }
}

#[derive(Clone, Debug)]
pub struct FeedRequest {
    pub account: Account,
    pub repo: Repository,
    pub state: FeedState,
    pub upper: i64,
}

#[derive(Debug)]
pub struct RequestFailure {
    pub paused: bool,
    pub retry_at: Option<i64>,
}
impl std::fmt::Display for RequestFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.paused {
            write!(
                f,
                "GitHub authentication or permission failed; automatic refresh paused, r retries"
            )
        } else if let Some(at) = self.retry_at {
            write!(
                f,
                "GitHub rate limit; retry after {}",
                crate::activity::iso(at).unwrap_or_else(|_| at.to_string())
            )
        } else {
            write!(
                f,
                "GitHub CLI request failed; check authentication and network access"
            )
        }
    }
}
impl std::error::Error for RequestFailure {}

pub fn request_failure(stdout: &[u8], stderr: &[u8], now: i64) -> RequestFailure {
    let out = String::from_utf8_lossy(stdout);
    let err = String::from_utf8_lossy(stderr).to_lowercase();
    let headers = out.split("\r\n\r\n").next().unwrap_or_default();
    let mut retry_at = None;
    let mut exhausted = false;
    for line in headers.lines() {
        if let Some((name, value)) = line.split_once(':') {
            let value = value.trim();
            match name.to_ascii_lowercase().as_str() {
                "retry-after" => {
                    retry_at = value
                        .parse::<i64>()
                        .ok()
                        .map(|v| now.saturating_add(v.max(0)))
                        .or_else(|| {
                            chrono::DateTime::parse_from_rfc2822(value)
                                .ok()
                                .map(|v| v.timestamp())
                        });
                }
                "x-ratelimit-remaining" => exhausted = value == "0",
                _ => {}
            }
        }
    }
    if exhausted {
        for line in headers.lines() {
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("x-ratelimit-reset") {
                    if let Ok(reset) = value.trim().parse::<i64>() {
                        retry_at = Some(retry_at.unwrap_or(reset).max(reset));
                    }
                }
            }
        }
    }
    let rate = exhausted
        || retry_at.is_some()
        || err.contains("rate limit")
        || err.contains("http 429")
        || headers.contains(" 429");
    RequestFailure {
        paused: !rate
            && (err.contains("http 401")
                || err.contains("http 403")
                || err.contains("gh auth login")
                || headers.contains(" 401")
                || headers.contains(" 403")),
        retry_at: rate.then(|| {
            retry_at
                .unwrap_or(now.saturating_add(60))
                .max(now.saturating_add(60))
        }),
    }
}
