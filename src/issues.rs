use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub repo_id: u64,
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueFilter {
    pub labels: Vec<String>,
    pub keyword: String,
    pub state: String,
    pub unassigned: bool,
}

impl IssueFilter {
    pub fn parse(query: &str) -> Result<Self> {
        ensure!(
            query.chars().filter(|c| *c == '"').count() % 2 == 0,
            "Close the quoted filter value"
        );
        let mut filter = Self {
            labels: vec![],
            keyword: String::new(),
            state: "open".into(),
            unassigned: false,
        };
        let mut keywords = Vec::new();
        for word in crate::app::query_words(query) {
            if let Some(label) = word.strip_prefix("label:") {
                ensure!(!label.is_empty(), "Provide a label after label:");
                filter.labels.push(label.to_lowercase());
            } else if let Some(state) = word.strip_prefix("state:") {
                ensure!(
                    matches!(state, "open" | "closed" | "all"),
                    "State must be open, closed, or all"
                );
                filter.state = state.into();
            } else if word == "unassigned" {
                filter.unassigned = true;
            } else {
                keywords.push(word);
            }
        }
        filter.keyword = keywords.join(" ").to_lowercase();
        Ok(filter)
    }

    pub fn matches(&self, issue: &Issue) -> bool {
        (self.state == "all" || self.state == issue.state)
            && (!self.unassigned || issue.assignees.is_empty())
            && self
                .labels
                .iter()
                .all(|label| issue.labels.iter().any(|v| v.to_lowercase() == *label))
            && (issue.title.to_lowercase().contains(&self.keyword)
                || issue
                    .body
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&self.keyword))
    }
}

pub fn valid_repo_name(name: &str) -> bool {
    let parts: Vec<_> = name.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && *part != "."
                && *part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
}

pub fn valid_issue_url(url: &str) -> bool {
    let Some(path) = url.strip_prefix("https://github.com/") else {
        return false;
    };
    let parts: Vec<_> = path.split('/').collect();
    parts.len() == 4
        && valid_repo_name(&format!("{}/{}", parts[0], parts[1]))
        && parts[2] == "issues"
        && parts[3].bytes().all(|b| b.is_ascii_digit())
        && parts[3].parse::<u64>().is_ok_and(|n| n > 0)
}
