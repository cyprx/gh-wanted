use gh_wanted::{
    github::parse_issues,
    issues::{valid_issue_url, IssueFilter},
};

fn pages() -> Vec<u8> {
    let issue = serde_json::json!({"number": 1, "title": "Keyboard navigation", "body": "Handle EMPTY lists", "state": "open", "created_at":"2026-09-10T00:00:00Z", "updated_at":"2026-09-11T00:00:00Z", "labels":[{"name":"good first issue"},{"name":"bug"}], "html_url":"https://github.com/demo/repo/issues/1"});
    serde_json::to_vec(&serde_json::json!([[issue.clone(), {"pull_request": {}}], [issue]]))
        .unwrap()
}
#[test]
fn excludes_prs_deduplicates_and_filters_issue_fields() {
    let issues = parse_issues(42, &pages()).unwrap();
    assert_eq!(issues.len(), 1);
    let issue = &issues[0];
    assert_eq!((issue.repo_id, issue.number), (42, 1));
    assert!(issue.assignees.is_empty());
    assert!(
        IssueFilter::parse("label:\"good first issue\" label:bug unassigned empty")
            .unwrap()
            .matches(issue)
    );
    assert!(!IssueFilter::parse("label:rust").unwrap().matches(issue));
    assert!(!IssueFilter::parse("state:closed").unwrap().matches(issue));
    assert!(IssueFilter::parse("state:all KEYBOARD")
        .unwrap()
        .matches(issue));
    let mut assigned = issue.clone();
    assigned.assignees.push("contributor".into());
    assert!(!IssueFilter::parse("unassigned").unwrap().matches(&assigned));
    assert!(IssueFilter::parse("state:typo").is_err());
    assert!(IssueFilter::parse("label:\"unclosed").is_err());
    assert!(parse_issues(42, b"[[").is_err());
    assert!(parse_issues(42, b"[[]]").unwrap().is_empty());
}
#[test]
fn browser_url_must_be_an_exact_https_github_issue() {
    assert!(valid_issue_url("https://github.com/owner/repo/issues/123"));
    for url in [
        "http://github.com/o/r/issues/1",
        "https://github.com.evil/o/r/issues/1",
        "https://github.com@evil/o/r/issues/1",
        "https://github.com/o/r/pull/1",
        "https://github.com/o/r/issues/1?redirect=evil",
        "https://github.com/o/r/issues/0",
        "https://github.com/o/r/issues/1\n",
        "https://github.com/../r/issues/1",
    ] {
        assert!(!valid_issue_url(url), "{url}");
    }
}
