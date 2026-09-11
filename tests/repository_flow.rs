use gh_wanted::repositories::{matches, normalize_tags, RepoFilter, Repository};

#[test]
fn filters_combine_text_topics_and_local_tags() {
    let repo = Repository {
        id: 1,
        full_name: "Owner/Tool".into(),
        description: Some("A Rust terminal utility".into()),
        topics: vec!["rust".into(), "cli".into()],
        archived: false,
    };
    let tags = vec!["favorite".into(), "work".into()];
    assert!(matches(&repo, &tags, &RepoFilter::default()));
    let mut filter = RepoFilter {
        text: "TERMINAL".into(),
        topics: vec!["RUST".into(), "cli".into()],
        local_tags: vec!["Favorite".into(), "work".into()],
    };
    assert!(matches(&repo, &tags, &filter));
    filter.topics.push("missing".into());
    assert!(!matches(&repo, &tags, &filter));
    filter.topics.pop();
    filter.local_tags.push("missing".into());
    assert!(!matches(&repo, &tags, &filter));
    filter.local_tags.pop();
    filter.text = "owner/tool".into();
    assert!(matches(&repo, &tags, &filter));
    filter.text = "absent".into();
    assert!(!matches(&repo, &tags, &filter));
}

#[test]
fn normalizes_tags_and_rejects_invalid_input() -> anyhow::Result<()> {
    assert_eq!(
        normalize_tags(&[" Rust ".into(), "RUST".into(), "CLI".into()])?,
        vec!["cli", "rust"]
    );
    assert!(normalize_tags(&[])?.is_empty());
    for bad in [
        "".to_string(),
        "   ".into(),
        "a\nb".into(),
        "\tvalid".into(),
        "x".repeat(129),
    ] {
        assert!(normalize_tags(&[bad]).is_err());
    }
    Ok(())
}
