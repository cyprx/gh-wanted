use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Focus {
    pub name: String,
    pub repository_query: String,
    pub issue_query: String,
}

impl Focus {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty()
                && self.name.trim().len() <= 128
                && !self.name.chars().any(char::is_control),
            "Focus names must be 1-128 bytes without control characters"
        );
        ensure!(
            self.repository_query.len() <= 2048 && self.issue_query.len() <= 2048,
            "Focus filters are too long"
        );
        crate::issues::IssueFilter::parse(&self.issue_query)?;
        Ok(())
    }
}
