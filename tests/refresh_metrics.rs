use gh_wanted::metrics::{report, Metrics};
use serde_json::{json, Value};

#[test]
fn metrics_persist_across_sessions_and_report_failures_without_raw_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("refresh-metrics.jsonl");
    {
        let metrics = Metrics::new(path.clone(), "activity");
        metrics
            .record(json!({"event":"request_finished","endpoint":"user","outcome":"ok","pages":1}));
        let result: anyhow::Result<()> = metrics.operation(
            "activity",
            Some(7),
            Some("issues"),
            || Err(anyhow::anyhow!("secret response body")),
            |_| 0,
        );
        assert!(result.is_err());
    }
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("secret response body"));
    let summaries: Vec<Value> = serde_json::from_str(&report(&path).unwrap()).unwrap();
    assert_eq!(summaries[0]["closed"], true);
    assert_eq!(summaries[0]["account_calls"], 1);
    assert_eq!(summaries[0]["operations"], 1);
}

#[test]
fn rotation_is_bounded_and_partial_lines_do_not_break_report() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("refresh-metrics.jsonl");
    std::fs::write(&path, vec![b' '; 5 * 1024 * 1024]).unwrap();
    {
        let metrics = Metrics::new(path.clone(), "issues");
        metrics.record(json!({"event":"request_finished","outcome":"rate_limited","retry_at":100}));
    }
    assert!(path.with_extension("previous.jsonl").exists());
    assert!(std::fs::metadata(&path).unwrap().len() < 4096);
    let summaries: Vec<Value> = serde_json::from_str(&report(&path).unwrap()).unwrap();
    assert_eq!(summaries[0]["failed_calls"], 1);
    assert_eq!(summaries[0]["observed_pages"], 0);
}

#[test]
fn unavailable_metrics_storage_does_not_fail_work() {
    let dir = tempfile::tempdir().unwrap();
    let metrics = Metrics::new(dir.path().to_path_buf(), "activity");
    assert_eq!(
        metrics
            .operation("activity", None, None, || Ok(vec![1]), Vec::len)
            .unwrap(),
        vec![1]
    );
}
