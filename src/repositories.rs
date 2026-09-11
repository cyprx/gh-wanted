use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub id: u64,
    pub full_name: String,
    pub description: Option<String>,
    pub topics: Vec<String>,
    pub archived: bool,
}

#[derive(Clone, Debug, Default)]
pub struct RepoFilter {
    pub text: String,
    pub topics: Vec<String>,
    pub local_tags: Vec<String>,
}

pub fn matches(repo: &Repository, tags: &[String], filter: &RepoFilter) -> bool {
    let text = filter.text.trim().to_lowercase();
    let text_matches = text.is_empty()
        || repo.full_name.to_lowercase().contains(&text)
        || repo
            .description
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains(&text);
    text_matches
        && filter.topics.iter().all(|wanted| {
            repo.topics
                .iter()
                .any(|topic| topic.to_lowercase() == wanted.trim().to_lowercase())
        })
        && filter.local_tags.iter().all(|wanted| {
            tags.iter()
                .any(|tag| tag.to_lowercase() == wanted.trim().to_lowercase())
        })
}

pub fn normalize_tags(tags: &[String]) -> Result<Vec<String>> {
    let mut normalized = BTreeSet::new();
    for tag in tags {
        if tag.chars().any(char::is_control) {
            bail!("Tags must not contain control characters");
        }
        let tag = tag.trim().to_lowercase();
        if tag.is_empty() || tag.len() > 128 {
            bail!("Tags must contain between 1 and 128 bytes after normalization");
        }
        normalized.insert(tag);
    }
    Ok(normalized.into_iter().collect())
}
