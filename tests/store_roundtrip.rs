use gh_wanted::{repositories::Repository, store::Store};
use rusqlite::Connection;

fn repo(id: u64, name: &str) -> Repository {
    Repository {
        id,
        full_name: name.into(),
        description: None,
        topics: vec![],
        archived: false,
    }
}

#[test]
fn persists_across_restart_rename_unwatch_and_accounts() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("cache.sqlite3");
    {
        let mut store = Store::open(&path)?;
        store.replace_repositories(1, &[repo(u64::MAX, "old/name")])?;
        store.replace_tags(1, u64::MAX, &[" Rust ".into(), "rust".into()])?;
        store.replace_repositories(2, &[repo(u64::MAX, "other/name")])?;
        store.replace_tags(2, u64::MAX, &["Other".into()])?;
    }
    let mut store = Store::open(&path)?;
    assert_eq!(store.repositories(1)?, vec![repo(u64::MAX, "old/name")]);
    assert_eq!(store.tags(1, u64::MAX)?, vec!["rust"]);
    store.replace_repositories(1, &[repo(u64::MAX, "new/name")])?;
    assert_eq!(store.tags(1, u64::MAX)?, vec!["rust"]);
    store.replace_repositories(1, &[])?;
    assert!(store.repositories(1)?.is_empty());
    assert_eq!(store.tags(1, u64::MAX)?, vec!["rust"]);
    assert_eq!(store.repositories(2)?, vec![repo(u64::MAX, "other/name")]);
    assert_eq!(store.tags(2, u64::MAX)?, vec!["other"]);
    Ok(())
}

#[test]
fn failed_replacements_keep_previous_data() -> anyhow::Result<()> {
    let mut store = Store::memory()?;
    store.replace_repositories(1, &[repo(1, "original")])?;
    store.replace_tags(1, 1, &["keep".into()])?;
    assert!(store
        .replace_repositories(1, &[repo(2, "first"), repo(2, "duplicate")])
        .is_err());
    assert_eq!(store.repositories(1)?, vec![repo(1, "original")]);
    assert!(store
        .replace_tags(1, 1, &["valid".into(), " ".into()])
        .is_err());
    assert_eq!(store.tags(1, 1)?, vec!["keep"]);
    Ok(())
}

#[test]
fn rejects_corrupt_and_future_databases_without_reset() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let corrupt = directory.path().join("corrupt.sqlite3");
    let bytes = b"This is not a SQLite database";
    std::fs::write(&corrupt, bytes)?;
    assert!(Store::open(&corrupt).is_err());
    assert_eq!(std::fs::read(&corrupt)?, bytes);
    let future = directory.path().join("future.sqlite3");
    {
        let connection = Connection::open(&future)?;
        connection.execute_batch("CREATE TABLE sentinel (value TEXT); INSERT INTO sentinel VALUES ('keep'); PRAGMA user_version = 4;")?;
    }
    assert!(Store::open(&future).is_err());
    let connection = Connection::open(&future)?;
    assert_eq!(
        connection.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
        4
    );
    assert_eq!(
        connection.query_row("SELECT value FROM sentinel", [], |r| r.get::<_, String>(0))?,
        "keep"
    );
    Ok(())
}

#[test]
fn failed_migration_is_atomic() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("conflict.sqlite3");
    {
        let connection = Connection::open(&path)?;
        connection.execute_batch("CREATE TABLE local_tags (sentinel TEXT);")?;
    }
    assert!(Store::open(&path).is_err());
    let connection = Connection::open(&path)?;
    assert_eq!(
        connection.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
        0
    );
    assert_eq!(
        connection.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = 'repositories'",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        0
    );
    Ok(())
}
