use gh_wanted::{focus::Focus, store::Store};
use rusqlite::Connection;

#[test]
fn focus_roundtrip_unique_names_and_account_isolation() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("focus.sqlite3");
    let focus = Focus {
        name: "Rust starters".into(),
        repository_query: "topic:rust tag:priority".into(),
        issue_query: "label:\"good first issue\" unassigned".into(),
    };
    {
        let mut store = Store::open(&path)?;
        store.save_focus(1, &focus)?;
        let mut duplicate = focus.clone();
        duplicate.name = " RUST STARTERS ".into();
        assert!(store.save_focus(1, &duplicate).is_err());
        store.save_focus(2, &focus)?;
        duplicate.name = " \n".into();
        assert!(store.save_focus(1, &duplicate).is_err());
    }
    let store = Store::open(&path)?;
    assert_eq!(store.focuses(1)?, vec![focus.clone()]);
    assert_eq!(store.focuses(2)?, vec![focus]);
    assert!(store.focuses(3)?.is_empty());
    Ok(())
}
#[test]
fn migrates_v1_preserving_tags_and_rolls_back_conflicts() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    for conflict in [false, true] {
        let path = dir.path().join(format!("{conflict}.sqlite3"));
        let db = Connection::open(&path)?;
        db.execute_batch("CREATE TABLE repositories (host TEXT, account TEXT, repo_id TEXT, repository_json TEXT); CREATE TABLE local_tags (host TEXT, account TEXT, repo_id TEXT, tag TEXT); INSERT INTO local_tags VALUES ('github.com','1','7','priority'); PRAGMA user_version=1;")?;
        if conflict {
            db.execute_batch("CREATE TABLE focuses (sentinel TEXT);")?;
        }
        drop(db);
        let store = Store::open(&path);
        if conflict {
            assert!(store.is_err());
            let db = Connection::open(&path)?;
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
                1
            );
        } else {
            assert_eq!(store?.tags(1, 7)?, vec!["priority"]);
        }
    }
    Ok(())
}
