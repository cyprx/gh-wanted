//! Local, bounded refresh diagnostics. Never persist response bodies or raw errors.
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::Instant,
};

const MAX_BYTES: u64 = 5 * 1024 * 1024;
static NEXT_RUN: AtomicU64 = AtomicU64::new(0);

pub struct Metrics {
    path: PathBuf,
    run: String,
    kind: String,
    started: Instant,
    lock: Mutex<()>,
}

impl Metrics {
    pub fn record_http_failure(&self, output: &[u8]) {
        let text = String::from_utf8_lossy(output);
        let head = text
            .split("\r\n\r\n")
            .next()
            .unwrap_or_default()
            .split("\n\n")
            .next()
            .unwrap_or_default();
        let mut value = json!({"event":"http_failure"});
        value["http_status"] = json!(head
            .lines()
            .next()
            .filter(|s| s.starts_with("HTTP/"))
            .and_then(|s| s.split_whitespace().nth(1))
            .and_then(|s| s.parse::<u16>().ok()));
        for line in head.lines() {
            if let Some((name, val)) = line.split_once(':') {
                let field = match name.to_ascii_lowercase().as_str() {
                    "x-ratelimit-remaining" => "rate_remaining",
                    "x-ratelimit-reset" => "rate_reset",
                    "retry-after" => "retry_after_seconds",
                    _ => continue,
                };
                value[field] = json!(val.trim().parse::<u64>().ok());
            }
        }
        self.record(value);
    }
    pub fn new(path: PathBuf, kind: &str) -> Self {
        let result = Self {
            path,
            run: format!(
                "{}-{}-{}",
                chrono::Utc::now().timestamp_micros(),
                std::process::id(),
                NEXT_RUN.fetch_add(1, Ordering::Relaxed)
            ),
            kind: kind.into(),
            started: Instant::now(),
            lock: Mutex::new(()),
        };
        result.record(json!({"event": "run_started"}));
        result
    }

    pub fn record(&self, mut value: Value) {
        // Requests belonging to each worker remain correlated when operations overlap.
        value["worker"] = json!(format!("{:?}", std::thread::current().id()));
        value["schema"] = json!(1);
        value["run"] = json!(self.run);
        value["kind"] = json!(self.kind);
        value["at"] = json!(chrono::Utc::now().timestamp_millis());
        value["elapsed_ms"] = json!(self.started.elapsed().as_millis() as u64);
        // Diagnostics must never break a refresh, including full disks or denied access.
        let _ = self.append(&value);
    }

    fn append(&self, value: &Value) -> std::io::Result<()> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| std::io::Error::other("metrics lock"))?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        if fs::metadata(&self.path).is_ok_and(|m| m.len() >= MAX_BYTES) {
            let previous = self.path.with_extension("previous.jsonl");
            if previous.exists() {
                fs::remove_file(&previous)?;
            }
            fs::rename(&self.path, previous)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        serde_json::to_writer(&mut file, value)?;
        file.write_all(b"\n")
    }

    pub fn operation<T>(
        &self,
        name: &str,
        repo: Option<u64>,
        feed: Option<&str>,
        work: impl FnOnce() -> anyhow::Result<T>,
        count: impl FnOnce(&T) -> usize,
    ) -> anyhow::Result<T> {
        let started = Instant::now();
        self.record(
            json!({"event":"operation_started", "operation":name, "repo_id":repo, "feed":feed}),
        );
        let result = work();
        self.record(
            json!({"event":"operation_finished", "operation":name, "repo_id":repo, "feed":feed,
            "duration_ms": started.elapsed().as_millis() as u64,
            "outcome": outcome(&result), "items": result.as_ref().ok().map(count)}),
        );
        result
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        self.record(json!({"event":"run_closed"}));
    }
}

pub fn outcome<T>(result: &anyhow::Result<T>) -> &'static str {
    match result {
        Ok(_) => "ok",
        Err(e) => {
            if let Some(policy) = e.downcast_ref::<crate::sync::RequestFailure>() {
                if policy.retry_at.is_some() {
                    return "rate_limited";
                }
                if policy.paused {
                    return "auth_or_permission";
                }
            }
            match e.to_string().as_str() {
                "GitHub sync cancelled" => "cancelled",
                "GitHub CLI timed out" => "timeout",
                _ => "error",
            }
        }
    }
}

/// Summarize retained records; never confuse a CLI invocation with an HTTP request.
pub fn report(path: &Path) -> anyhow::Result<String> {
    let mut runs = std::collections::BTreeMap::<String, Value>::new();
    for path in [path.with_extension("previous.jsonl"), path.to_path_buf()] {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        for line in BufReader::new(file).lines() {
            let line = line?;
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(id) = v["run"].as_str() else {
                continue;
            };
            let summary = runs.entry(id.into()).or_insert_with(|| json!({"run":id,"kind":v["kind"],"closed":false,"cli_calls":0,"observed_pages":0,"failed_calls":0,"account_calls":0,"operations":0,"items_returned":0}));
            summary["elapsed_ms"] = v["elapsed_ms"].clone();
            match v["event"].as_str() {
                Some("issue_window") => summary["issue_window_days"] = v["days"].clone(),
                Some("account_bound") => {
                    summary["account_bound"] = json!(true);
                    summary["account_id"] = v["account_id"].clone();
                }
                Some("refresh_plan") => {
                    summary["worker_limit"] = v["worker_limit"].clone();
                    summary["trigger"] = v["trigger"].clone();
                    summary["account_id"] = v["account_id"].clone();
                    summary["planned_operations"] = v["planned_operations"].clone();
                }
                Some("operation_skipped") => {
                    summary["skipped_operations"] =
                        json!(summary["skipped_operations"].as_u64().unwrap_or(0) + 1)
                }
                Some("run_closed") => summary["closed"] = json!(true),
                Some("request_finished") => {
                    for (field, increment) in [
                        ("cli_calls", 1),
                        ("observed_pages", v["pages"].as_u64().unwrap_or(0)),
                        ("failed_calls", u64::from(v["outcome"] != "ok")),
                        ("account_calls", u64::from(v["endpoint"] == "user")),
                        ("cli_duration_ms", v["duration_ms"].as_u64().unwrap_or(0)),
                        (
                            "account_duration_ms",
                            if v["endpoint"] == "user" {
                                v["duration_ms"].as_u64().unwrap_or(0)
                            } else {
                                0
                            },
                        ),
                        ("unknown_page_calls", u64::from(v["pages"].is_null())),
                        (
                            "rate_limited_calls",
                            u64::from(v["outcome"] == "rate_limited"),
                        ),
                    ] {
                        summary[field] = json!(summary[field].as_u64().unwrap_or(0) + increment);
                    }
                }
                Some("operation_finished") => {
                    summary["operations"] = json!(summary["operations"].as_u64().unwrap_or(0) + 1);
                    summary["failed_operations"] = json!(
                        summary["failed_operations"].as_u64().unwrap_or(0)
                            + u64::from(v["outcome"] != "ok")
                    );
                    let items = v["items"].as_u64().unwrap_or(0);
                    summary["items_returned"] =
                        json!(summary["items_returned"].as_u64().unwrap_or(0) + items);
                    if items > 0 && summary.get("first_nonempty_result_ms").is_none() {
                        summary["first_nonempty_result_ms"] = v["elapsed_ms"].clone();
                    }
                }
                _ => {}
            }
        }
    }
    Ok(serde_json::to_string_pretty(
        &runs.into_values().collect::<Vec<_>>(),
    )?)
}
