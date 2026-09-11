use crate::repositories::{normalize_tags, Repository};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
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
            1 => {}
            _ => bail!("Unsupported repository database schema version {version}"),
        }
        transaction
            .prepare("SELECT host, account, repo_id, repository_json FROM repositories LIMIT 0")?;
        transaction.prepare("SELECT host, account, repo_id, tag FROM local_tags LIMIT 0")?;
        transaction.commit()?;
        Ok(Self { connection })
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
