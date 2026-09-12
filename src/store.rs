use crate::repositories::{normalize_tags, Repository};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub struct Store {
    connection: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Self::initialize(Connection::open(path).context("Open repository database")?)
    }

    pub fn memory() -> Result<Self> {
        Self::initialize(Connection::open_in_memory()?)
    }

    fn initialize(mut connection: Connection) -> Result<Self> {
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let check: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if check != "ok" {
            bail!("Repository database failed integrity check: {check}");
        }
        let transaction = connection.transaction()?;
        let version: i64 = transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        match version {
            0 => transaction.execute_batch(
                "CREATE TABLE repositories (
                    host TEXT NOT NULL,
                    account TEXT NOT NULL,
                    repo_id TEXT NOT NULL,
                    repository_json TEXT NOT NULL,
                    PRIMARY KEY (host, account, repo_id)
                );
                CREATE TABLE local_tags (
                    host TEXT NOT NULL,
                    account TEXT NOT NULL,
                    repo_id TEXT NOT NULL,
                    tag TEXT NOT NULL,
                    PRIMARY KEY (host, account, repo_id, tag)
                );
                PRAGMA user_version = 1;",
            )?,
            1..=3 => {}
            _ => bail!("Unsupported repository database schema version {version}"),
        }
        transaction
            .prepare("SELECT host, account, repo_id, repository_json FROM repositories LIMIT 0")?;
        transaction.prepare("SELECT host, account, repo_id, tag FROM local_tags LIMIT 0")?;
        if version < 2 {
            transaction.execute_batch(
                "CREATE TABLE focuses (
                host TEXT NOT NULL, account TEXT NOT NULL, name_key TEXT NOT NULL,
                definition TEXT NOT NULL, PRIMARY KEY(host, account, name_key)
            ); PRAGMA user_version = 2;",
            )?;
        }
        transaction.prepare("SELECT host, account, name_key, definition FROM focuses LIMIT 0")?;
        if version < 3 {
            transaction.execute_batch("CREATE TABLE activity (
                host TEXT NOT NULL, account TEXT NOT NULL, repo_id TEXT NOT NULL,
                event_key TEXT NOT NULL, event_json TEXT NOT NULL, acknowledged INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY(host, account, repo_id, event_key));
                CREATE TABLE activity_feeds (
                host TEXT NOT NULL, account TEXT NOT NULL, repo_id TEXT NOT NULL, feed TEXT NOT NULL,
                initial_since INTEGER NOT NULL, checkpoint INTEGER, error TEXT, retry_at INTEGER,
                paused INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(host, account, repo_id, feed));
                PRAGMA user_version = 3;")?;
        }
        transaction.prepare("SELECT host, account, repo_id, event_key, event_json, acknowledged FROM activity LIMIT 0")?;
        transaction.prepare("SELECT host, account, repo_id, feed, initial_since, checkpoint, error, retry_at, paused FROM activity_feeds LIMIT 0")?;
        transaction.commit()?;
        Ok(Self { connection })
    }

    pub fn save_focus(&mut self, account: u64, focus: &crate::focus::Focus) -> Result<()> {
        focus.validate()?;
        let mut focus = focus.clone();
        focus.name = focus.name.trim().to_owned();
        self.connection.execute(
            "INSERT INTO focuses (host, account, name_key, definition) VALUES ('github.com', ?1, ?2, ?3)",
            params![account.to_string(), focus.name.to_lowercase(), serde_json::to_string(&focus)?],
        ).context("Could not save focus; use a unique name")?;
        Ok(())
    }

    pub fn focuses(&self, account: u64) -> Result<Vec<crate::focus::Focus>> {
        let mut statement = self.connection.prepare("SELECT definition FROM focuses WHERE host = 'github.com' AND account = ?1 ORDER BY name_key")?;
        let rows = statement.query_map([account.to_string()], |row| row.get::<_, String>(0))?;
        let mut focuses = Vec::new();
        for row in rows {
            let focus: crate::focus::Focus =
                serde_json::from_str(&row?).context("Read saved focus")?;
            focus.validate()?;
            focuses.push(focus);
        }
        Ok(focuses)
    }

    pub fn repositories(&self, account: u64) -> Result<Vec<Repository>> {
        let mut statement = self.connection.prepare(
            "SELECT repository_json FROM repositories WHERE host = 'github.com' AND account = ?1 ORDER BY repo_id",
        )?;
        let rows = statement.query_map([account.to_string()], |row| row.get::<_, String>(0))?;
        let mut repositories = Vec::new();
        for row in rows {
            repositories
                .push(serde_json::from_str::<Repository>(&row?).context("Read cached repository")?);
        }
        repositories.sort_by(|a, b| {
            a.full_name
                .to_lowercase()
                .cmp(&b.full_name.to_lowercase())
                .then(a.id.cmp(&b.id))
        });
        Ok(repositories)
    }

    pub fn replace_repositories(&mut self, account: u64, repos: &[Repository]) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let account = account.to_string();
        transaction.execute(
            "DELETE FROM repositories WHERE host = 'github.com' AND account = ?1",
            [&account],
        )?;
        for repo in repos {
            transaction.execute(
                "INSERT INTO repositories (host, account, repo_id, repository_json) VALUES ('github.com', ?1, ?2, ?3)",
                params![account, repo.id.to_string(), serde_json::to_string(repo)?],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn tags(&self, account: u64, repo_id: u64) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT tag FROM local_tags WHERE host = 'github.com' AND account = ?1 AND repo_id = ?2 ORDER BY tag",
        )?;
        let rows = statement
            .query_map(params![account.to_string(), repo_id.to_string()], |row| {
                row.get(0)
            })?;
        Ok(rows.collect::<rusqlite::Result<Vec<String>>>()?)
    }

    pub fn replace_tags(&mut self, account: u64, repo_id: u64, tags: &[String]) -> Result<()> {
        let tags = normalize_tags(tags)?;
        let transaction = self.connection.transaction()?;
        let account = account.to_string();
        let repo_id = repo_id.to_string();
        transaction.execute(
            "DELETE FROM local_tags WHERE host = 'github.com' AND account = ?1 AND repo_id = ?2",
            params![account, repo_id],
        )?;
        for tag in tags {
            transaction.execute(
                "INSERT INTO local_tags (host, account, repo_id, tag) VALUES ('github.com', ?1, ?2, ?3)",
                params![account, repo_id, tag],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

impl Store {
    pub fn ensure_feed(
        &mut self,
        account: u64,
        repo_id: u64,
        feed: &str,
        now: i64,
    ) -> Result<crate::sync::FeedState> {
        anyhow::ensure!(
            feed == "issues"
                || feed == "comments"
                || feed
                    .strip_prefix("reviews:")
                    .is_some_and(|n| n.parse::<u64>().is_ok_and(|n| n > 0)),
            "Invalid activity feed"
        );
        self.connection.execute("INSERT OR IGNORE INTO activity_feeds (host, account, repo_id, feed, initial_since) VALUES ('github.com',?1,?2,?3,?4)", params![account.to_string(), repo_id.to_string(), feed, now.saturating_sub(crate::sync::INITIAL_HISTORY_SECONDS)])?;
        self.feeds(account)?
            .into_iter()
            .find(|f| f.repo_id == repo_id && f.feed == feed)
            .context("Missing activity feed")
    }

    pub fn feeds(&self, account: u64) -> Result<Vec<crate::sync::FeedState>> {
        let mut statement = self.connection.prepare("SELECT repo_id,feed,initial_since,checkpoint,error,retry_at,paused FROM activity_feeds WHERE host='github.com' AND account=?1 ORDER BY repo_id,feed")?;
        let rows = statement.query_map([account.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, bool>(6)?,
            ))
        })?;
        rows.map(|row| {
            let (repo, feed, initial_since, checkpoint, error, retry_at, paused) = row?;
            Ok(crate::sync::FeedState {
                repo_id: repo.parse()?,
                feed,
                initial_since,
                checkpoint,
                error,
                retry_at,
                paused,
            })
        })
        .collect()
    }

    pub fn commit_activity(
        &mut self,
        request: &crate::sync::FeedRequest,
        events: &[crate::activity::Activity],
    ) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let account = request.account.id.to_string();
        let repo = request.repo.id.to_string();
        let current: Option<Option<i64>> = transaction.query_row("SELECT checkpoint FROM activity_feeds WHERE host='github.com' AND account=?1 AND repo_id=?2 AND feed=?3", params![account,repo,request.state.feed], |row| row.get(0)).optional()?;
        anyhow::ensure!(
            current == Some(request.state.checkpoint),
            "Superseded activity refresh; retry"
        );
        anyhow::ensure!(
            request.upper
                >= request
                    .state
                    .checkpoint
                    .unwrap_or(request.state.initial_since),
            "Clock moved backward; checkpoint unchanged"
        );
        for event in events {
            anyhow::ensure!(
                event.repo_id == request.repo.id
                    && event.occurred_at >= request.state.lower()
                    && event.occurred_at <= request.upper,
                "Activity outside requested window"
            );
            transaction.execute("INSERT OR IGNORE INTO activity (host,account,repo_id,event_key,event_json) VALUES ('github.com',?1,?2,?3,?4)", params![account,repo,event.key,serde_json::to_string(event)?])?;
        }
        transaction.execute("UPDATE activity_feeds SET checkpoint=?4,error=NULL,retry_at=NULL,paused=0 WHERE host='github.com' AND account=?1 AND repo_id=?2 AND feed=?3", params![account,repo,request.state.feed,request.upper])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn fail_activity(
        &mut self,
        request: &crate::sync::FeedRequest,
        error: &anyhow::Error,
    ) -> Result<()> {
        let policy = error.downcast_ref::<crate::sync::RequestFailure>();
        self.connection.execute("UPDATE activity_feeds SET error=?4,retry_at=?5,paused=?6 WHERE host='github.com' AND account=?1 AND repo_id=?2 AND feed=?3 AND checkpoint IS ?7", params![request.account.id.to_string(),request.repo.id.to_string(),request.state.feed,error.to_string(),policy.and_then(|p| p.retry_at),policy.is_some_and(|p| p.paused),request.state.checkpoint])?;
        Ok(())
    }

    pub fn activities(&self, account: u64) -> Result<Vec<crate::activity::Activity>> {
        let mut statement = self.connection.prepare(
            "SELECT event_json,acknowledged FROM activity WHERE host='github.com' AND account=?1",
        )?;
        let rows = statement.query_map([account.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
        })?;
        let mut events = Vec::new();
        for row in rows {
            let (json, acknowledged) = row?;
            let mut event: crate::activity::Activity =
                serde_json::from_str(&json).context("Read cached activity")?;
            event.acknowledged = acknowledged;
            events.push(event);
        }
        events.sort_by(|a, b| {
            b.occurred_at
                .cmp(&a.occurred_at)
                .then(a.repo_id.cmp(&b.repo_id))
                .then(a.key.cmp(&b.key))
        });
        Ok(events)
    }

    pub fn acknowledge(
        &mut self,
        account: u64,
        repo_id: u64,
        key: &str,
        acknowledged: bool,
    ) -> Result<()> {
        let changed = self.connection.execute("UPDATE activity SET acknowledged=?4 WHERE host='github.com' AND account=?1 AND repo_id=?2 AND event_key=?3", params![account.to_string(),repo_id.to_string(),key,acknowledged])?;
        anyhow::ensure!(changed == 1, "Activity no longer available");
        Ok(())
    }
}
