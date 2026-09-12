use anyhow::{ensure, Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityKind {
    NewIssue,
    IssueChange,
    Comment,
    PrChange,
    Review,
}

impl ActivityKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::NewIssue => "New issue",
            Self::IssueChange => "Issue changed",
            Self::Comment => "Comment",
            Self::PrChange => "PR changed",
            Self::Review => "Review",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub repo_id: u64,
    pub key: String,
    pub source_id: u64,
    pub number: u64,
    pub kind: ActivityKind,
    pub title: String,
    pub body: String,
    pub actor: String,
    pub association: String,
    pub url: String,
    pub occurred_at: i64,
    pub fetched_at: i64,
    #[serde(default)]
    pub acknowledged: bool,
}

pub fn timestamp(value: &str) -> Result<i64> {
    Ok(DateTime::parse_from_rfc3339(value)
        .context("Invalid GitHub activity timestamp")?
        .timestamp())
}

pub fn iso(value: i64) -> Result<String> {
    Ok(DateTime::<Utc>::from_timestamp(value, 0)
        .context("Invalid sync timestamp")?
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

pub fn is_today<T: TimeZone>(occurred: i64, now: i64, timezone: &T) -> bool {
    let (Some(event), Some(now)) = (
        DateTime::<Utc>::from_timestamp(occurred, 0),
        DateTime::<Utc>::from_timestamp(now, 0),
    ) else {
        return false;
    };
    event.with_timezone(timezone).date_naive() == now.with_timezone(timezone).date_naive()
}

pub fn valid_activity_url(url: &str) -> bool {
    let (base, fragment) = url
        .split_once('#')
        .map_or((url, None), |(a, b)| (a, Some(b)));
    if fragment
        .is_some_and(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
    {
        return false;
    }
    let Some(path) = base.strip_prefix("https://github.com/") else {
        return false;
    };
    let parts: Vec<_> = path.split('/').collect();
    parts.len() == 4
        && crate::issues::valid_repo_name(&format!("{}/{}", parts[0], parts[1]))
        && matches!(parts[2], "issues" | "pull")
        && !parts[3].is_empty()
        && parts[3].bytes().all(|b| b.is_ascii_digit())
        && parts[3].parse::<u64>().is_ok_and(|n| n > 0)
}

pub fn parse_records(
    repo_id: u64,
    feed: &str,
    values: Vec<serde_json::Value>,
    lower: i64,
    upper: i64,
) -> Result<Vec<Activity>> {
    let mut entries = std::collections::BTreeMap::new();
    for v in values {
        if feed.starts_with("reviews:") && v.get("submitted_at").is_none_or(|v| v.is_null()) {
            continue;
        }
        let field = |key: &str| {
            v.get(key)
                .and_then(|v| v.as_str())
                .context("Invalid GitHub activity response")
        };
        let source_id = v
            .get("id")
            .and_then(|v| v.as_u64())
            .context("Missing activity source ID")?;
        let actor = v
            .pointer("/user/login")
            .and_then(|v| v.as_str())
            .unwrap_or("deleted user")
            .to_owned();
        let association = v
            .get("author_association")
            .and_then(|v| v.as_str())
            .unwrap_or("NONE")
            .to_owned();
        let url = field("html_url")?.to_owned();
        let body = v
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned();
        let mut candidates = vec![];
        let number;
        if feed == "issues" {
            number = v
                .get("number")
                .and_then(|v| v.as_u64())
                .context("Missing issue number")?;
            let created = timestamp(field("created_at")?)?;
            let updated = timestamp(field("updated_at")?)?;
            let title = field("title")?.to_owned();
            if v.get("pull_request").is_some() {
                candidates.push((ActivityKind::PrChange, updated, title, String::new()));
            } else {
                candidates.push((
                    ActivityKind::NewIssue,
                    created,
                    title.clone(),
                    String::new(),
                ));
                if updated > created {
                    candidates.push((ActivityKind::IssueChange, updated, title, String::new()));
                }
            }
        } else if feed == "comments" {
            number = field("issue_url")?
                .rsplit('/')
                .next()
                .context("Invalid comment issue URL")?
                .parse::<u64>()?;
            candidates.push((
                ActivityKind::Comment,
                timestamp(field("updated_at")?)?,
                format!("Comment on #{number}"),
                String::new(),
            ));
        } else if let Some(pr) = feed.strip_prefix("reviews:") {
            number = pr.parse::<u64>()?;
            if v.get("submitted_at").is_none_or(|v| v.is_null()) {
                continue;
            }
            let state = field("state")?.to_owned();
            candidates.push((
                ActivityKind::Review,
                timestamp(field("submitted_at")?)?,
                format!("Review on #{number}: {state}"),
                state,
            ));
        } else {
            anyhow::bail!("Unknown activity feed");
        }
        ensure!(number > 0, "Invalid activity number");
        for (kind, occurred_at, title, version) in candidates {
            if occurred_at < lower || occurred_at > upper {
                continue;
            }
            let key = format!("{kind:?}:{source_id}:{occurred_at}:{version}");
            entries.insert(
                key.clone(),
                Activity {
                    repo_id,
                    key,
                    source_id,
                    number,
                    kind,
                    title,
                    body: body.clone(),
                    actor: actor.clone(),
                    association: association.clone(),
                    url: url.clone(),
                    occurred_at,
                    fetched_at: upper,
                    acknowledged: false,
                },
            );
        }
    }
    Ok(entries.into_values().collect())
}
