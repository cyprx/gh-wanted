use crate::{
    github::{Account, Snapshot},
    repositories::{matches, RepoFilter, Repository},
    store::Store,
};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Browse,
    Search,
    Tags,
    IssueSearch,
    SaveFocus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Repositories,
    Issues,
    Focuses,
    Activity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    List,
    Details,
}

pub struct App {
    pub activities: Vec<crate::activity::Activity>,
    pub feeds: Vec<crate::sync::FeedState>,
    pub activity_pending: Vec<(u64, String)>,
    pub now: i64,
    pub next_refresh: i64,
    pub auto_paused: bool,
    pub network_retry_at: Option<i64>,
    pub unread_only: bool,
    pub feed_details: bool,
    pub view: View,
    pub pane: Pane,
    pub issues: HashMap<u64, Vec<crate::issues::Issue>>,
    pub issue_status: HashMap<u64, String>,
    pub issue_query: String,
    pub focuses: Vec<crate::focus::Focus>,
    pub detail_scroll: u16,
    pub repositories: Vec<Repository>,
    pub tags: HashMap<u64, Vec<String>>,
    pub account: Option<Account>,
    pub selected: usize,
    pub query: String,
    pub input: String,
    pub mode: Mode,
    pub status: String,
    pub busy: bool,
    pub demo: bool,
    pub help: bool,
    pub detail: bool,
    edit_target: Option<(u64, u64)>,
    pub store: Store,
}

pub enum Action {
    None,
    Quit,
    Refresh,
    FetchIssues,
    OpenIssue(String),
    OpenRepository(String),
    FetchActivity,
    FetchReviews(u64, u64),
}

impl App {
    pub fn new(store: Store, demo: bool) -> Self {
        Self {
            activities: vec![],
            feeds: vec![],
            activity_pending: vec![],
            now: chrono::Utc::now().timestamp(),
            next_refresh: 0,
            auto_paused: false,
            network_retry_at: None,
            unread_only: false,
            feed_details: false,
            view: View::Repositories,
            pane: Pane::List,
            issues: HashMap::new(),
            issue_status: HashMap::new(),
            issue_query: String::new(),
            focuses: vec![],
            detail_scroll: 0,
            repositories: vec![],
            tags: HashMap::new(),
            account: None,
            selected: 0,
            query: String::new(),
            input: String::new(),
            mode: Mode::Browse,
            status: "Connecting to GitHub...".into(),
            busy: false,
            demo,
            help: false,
            detail: false,
            edit_target: None,
            store,
        }
    }
    pub fn filter(&self) -> RepoFilter {
        let mut filter = RepoFilter::default();
        for word in query_words(&self.query) {
            if let Some(topic) = word.strip_prefix("topic:") {
                filter.topics.push(topic.to_lowercase());
            } else if let Some(tag) = word.strip_prefix("tag:") {
                filter.local_tags.push(tag.to_lowercase());
            } else {
                if !filter.text.is_empty() {
                    filter.text.push(' ');
                }
                filter.text.push_str(&word);
            }
        }
        filter
    }
    pub fn visible_issues(&self) -> Vec<&crate::issues::Issue> {
        let Ok(filter) = crate::issues::IssueFilter::parse(&self.issue_query) else {
            return vec![];
        };
        let mut issues: Vec<_> = self
            .visible()
            .into_iter()
            .filter_map(|i| self.issues.get(&self.repositories[i].id))
            .flatten()
            .filter(|issue| filter.matches(issue))
            .collect();
        issues.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then(a.repo_id.cmp(&b.repo_id))
                .then(a.number.cmp(&b.number))
        });
        issues
    }
    pub fn current_issue(&self) -> Option<&crate::issues::Issue> {
        self.visible_issues().get(self.selected).copied()
    }
    fn list_len(&self) -> usize {
        match self.view {
            View::Repositories => self.visible().len(),
            View::Issues => self.visible_issues().len(),
            View::Focuses => self.focuses.len(),
            View::Activity => self.visible_activity().len(),
        }
    }
    pub fn apply_issues(
        &mut self,
        account: u64,
        repo_id: u64,
        result: Result<Vec<crate::issues::Issue>>,
    ) {
        if self.account.as_ref().map(|a| a.id) != Some(account) {
            return;
        }
        let selected_issue = if self.view == View::Issues {
            self.current_issue()
                .map(|issue| (issue.repo_id, issue.number))
        } else {
            None
        };
        match result {
            Ok(issues) => {
                self.issues.insert(repo_id, issues);
                self.issue_status.insert(repo_id, "Complete".into());
            }
            Err(error) => {
                self.issue_status.insert(
                    repo_id,
                    format!("Incomplete: {error}; previous results may be stale. r retries"),
                );
            }
        }
        self.selected = self.selected.min(self.list_len().saturating_sub(1));
        if let Some(identity) = selected_issue {
            if let Some(index) = self
                .visible_issues()
                .iter()
                .position(|issue| (issue.repo_id, issue.number) == identity)
            {
                self.selected = index;
            }
        }
    }
    pub fn visible(&self) -> Vec<usize> {
        let filter = self.filter();
        self.repositories
            .iter()
            .enumerate()
            .filter_map(|(index, repo)| {
                let tags = self.tags.get(&repo.id).map(Vec::as_slice).unwrap_or(&[]);
                matches(repo, tags, &filter).then_some(index)
            })
            .collect()
    }
    pub fn current(&self) -> Option<&Repository> {
        self.visible()
            .get(self.selected)
            .and_then(|i| self.repositories.get(*i))
    }
    pub fn identify(&mut self, account: Account) -> Result<()> {
        if self.account.as_ref().map(|a| a.id) != Some(account.id) {
            self.activities.clear();
            self.feeds.clear();
            self.activity_pending.clear();
            self.auto_paused = false;
            self.next_refresh = 0;
            self.issues.clear();
            self.issue_status.clear();
            self.focuses.clear();
            self.view = View::Repositories;
            self.mode = Mode::Browse;
            self.input.clear();
            self.edit_target = None;
        }
        self.repositories.clear();
        self.tags.clear();
        self.selected = 0;
        let id = account.id;
        self.account = Some(account);
        self.focuses = self.store.focuses(id)?;
        self.repositories = self.store.repositories(id)?;
        self.load_tags()?;
        self.load_activity()?;
        self.status = "Refreshing watched repositories (cached data may be stale)...".into();
        Ok(())
    }
    fn load_tags(&mut self) -> Result<()> {
        let Some(account) = &self.account else {
            return Ok(());
        };
        let mut tags = HashMap::new();
        for repo in &self.repositories {
            tags.insert(repo.id, self.store.tags(account.id, repo.id)?);
        }
        self.tags = tags;
        Ok(())
    }
    pub fn apply(&mut self, snapshot: Snapshot) -> Result<()> {
        if self.account.as_ref().map(|a| a.id) != Some(snapshot.account.id) {
            self.activities.clear();
            self.feeds.clear();
            self.issues.clear();
            self.issue_status.clear();
            self.focuses.clear();
            self.repositories.clear();
            self.tags.clear();
            self.account = None;
            anyhow::bail!("GitHub account changed. Refresh to reconnect.");
        }
        let selected_id = self.current().map(|r| r.id);
        self.store
            .replace_repositories(snapshot.account.id, &snapshot.repositories)?;
        self.repositories = snapshot.repositories;
        self.load_tags()?;
        self.selected = selected_id
            .and_then(|id| {
                self.visible()
                    .iter()
                    .position(|i| self.repositories[*i].id == id)
            })
            .unwrap_or(0);
        if self.view != View::Repositories {
            self.selected = 0;
        }
        self.status = format!(
            "{} watched repositories • {}",
            self.repositories.len(),
            if self.demo {
                "Demo: temporary data"
            } else {
                "Synced • local tags stay on this machine"
            }
        );
        Ok(())
    }
    pub fn key(&mut self, key: KeyEvent) -> Result<Action> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(Action::Quit);
        }
        if self.mode != Mode::Browse {
            return self.handle_editor_key(key.code);
        }
        if self.help {
            self.help = false;
            return Ok(Action::None);
        }
        if let Some(action) = self.handle_global_key(key.code) {
            return Ok(action);
        }
        match self.view {
            View::Repositories => self.handle_repo_key(key.code),
            View::Issues => self.handle_issue_key(key.code),
            View::Focuses => self.handle_focus_key(key.code),
            View::Activity => self.handle_activity_key(key.code),
        }
    }

    fn handle_editor_key(&mut self, code: KeyCode) -> Result<Action> {
        match code {
            KeyCode::Esc => {
                self.mode = Mode::Browse;
                self.input.clear();
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) if !c.is_control() => {
                if self.input.len() < 2048 {
                    self.input.push(c);
                }
            }
            KeyCode::Enter => {
                if self.mode == Mode::Search {
                    self.query = self.input.trim().to_owned();
                    self.selected = 0;
                } else if self.mode == Mode::IssueSearch {
                    crate::issues::IssueFilter::parse(&self.input)?;
                    self.issue_query = self.input.trim().to_owned();
                    self.selected = 0;
                    self.detail_scroll = 0;
                } else if self.mode == Mode::SaveFocus {
                    let account = self
                        .account
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Connect before saving a focus"))?;
                    let focus = crate::focus::Focus {
                        name: self.input.clone(),
                        repository_query: self.query.clone(),
                        issue_query: self.issue_query.clone(),
                    };
                    self.store.save_focus(account.id, &focus)?;
                    self.focuses = self.store.focuses(account.id)?;
                    self.status =
                        format!("Focus saved: {}. f opens saved focuses", focus.name.trim());
                } else if let Some((account_id, repo_id)) = self.edit_target {
                    anyhow::ensure!(
                        self.account.as_ref().map(|a| a.id) == Some(account_id),
                        "Account changed; reopen tag editor"
                    );
                    let tags: Vec<String> = if self.input.trim().is_empty() {
                        vec![]
                    } else {
                        self.input.split(',').map(str::to_owned).collect()
                    };
                    self.store.replace_tags(account_id, repo_id, &tags)?;
                    let saved = self.store.tags(account_id, repo_id)?;
                    self.tags.insert(repo_id, saved);
                    self.selected = self.selected.min(self.visible().len().saturating_sub(1));
                    self.status = "Local tags saved".into();
                }
                self.mode = Mode::Browse;
                self.input.clear();
            }
            _ => {}
        }

        Ok(Action::None)
    }

    fn handle_global_key(&mut self, code: KeyCode) -> Option<Action> {
        match code {
            KeyCode::Char('q') => return Some(Action::Quit),
            KeyCode::Char('d') => {
                self.switch_view(View::Activity);
                self.feed_details = false;
                self.detail_scroll = 0;
            }
            KeyCode::Char('i') => {
                self.switch_view(View::Issues);
                return Some(Action::FetchIssues);
            }
            KeyCode::Char('b') => self.switch_view(View::Repositories),
            KeyCode::Char('f') => self.switch_view(View::Focuses),
            KeyCode::PageDown => self.detail_scroll = self.detail_scroll.saturating_add(10),
            KeyCode::PageUp => self.detail_scroll = self.detail_scroll.saturating_sub(10),
            KeyCode::Char('?') => self.help = true,
            KeyCode::Tab => self.detail = !self.detail,
            _ => return None,
        }
        Some(Action::None)
    }

    fn switch_view(&mut self, view: View) {
        self.view = view;
        self.selected = 0;
        self.detail = false;
    }

    fn handle_repo_key(&mut self, code: KeyCode) -> Result<Action> {
        match code {
            KeyCode::Char('r') => return Ok(Action::Refresh),
            KeyCode::Char('s') => self.start_editor(Mode::SaveFocus, String::new()),
            KeyCode::Char('/') => self.start_editor(Mode::Search, self.query.clone()),
            KeyCode::Char('t') => {
                if let Some(repo) = self.current() {
                    let repo_id = repo.id;
                    self.edit_target = self.account.as_ref().map(|a| (a.id, repo_id));
                    self.input = self
                        .tags
                        .get(&repo_id)
                        .map(|t| t.join(", "))
                        .unwrap_or_default();
                    self.mode = Mode::Tags;
                }
            }

            _ => {
                return match self.pane {
                    Pane::List => self.handle_repo_list_key(code),
                    Pane::Details => self.handle_repo_details_key(code),
                }
            }
        }
        Ok(Action::None)
    }

    fn handle_repo_list_key(&mut self, code: KeyCode) -> Result<Action> {
        match code {
            KeyCode::Enter if self.current().is_some() => self.pane = Pane::Details,
            KeyCode::Esc => {
                self.query.clear();
                self.selected = 0;
            }
            _ => self.handle_list_key(code),
        }
        Ok(Action::None)
    }

    fn handle_repo_details_key(&mut self, code: KeyCode) -> Result<Action> {
        match code {
            KeyCode::Esc => self.pane = Pane::List,
            KeyCode::Char('o') => {
                if let Some(repo) = self.current() {
                    anyhow::ensure!(
                        crate::issues::valid_repo_name(&repo.full_name),
                        "Invalid GitHub repository name"
                    );
                    return Ok(Action::OpenRepository(format!(
                        "https://github.com/{}",
                        repo.full_name
                    )));
                }
            }
            _ => {}
        }
        Ok(Action::None)
    }

    fn handle_issue_key(&mut self, code: KeyCode) -> Result<Action> {
        match code {
            KeyCode::Char('r') => return Ok(Action::FetchIssues),
            KeyCode::Char('s') => self.start_editor(Mode::SaveFocus, String::new()),
            KeyCode::Char('/') => self.start_editor(Mode::IssueSearch, self.issue_query.clone()),
            KeyCode::Char('o') => {
                if let Some(issue) = self.current_issue() {
                    anyhow::ensure!(
                        crate::issues::valid_issue_url(&issue.url),
                        "Issue URL is not a valid HTTPS github.com issue URL"
                    );
                    return Ok(Action::OpenIssue(issue.url.clone()));
                }
            }
            KeyCode::Esc => {
                self.issue_query.clear();
                self.selected = 0;
            }
            _ => self.handle_list_key(code),
        }
        Ok(Action::None)
    }

    fn handle_focus_key(&mut self, code: KeyCode) -> Result<Action> {
        match code {
            KeyCode::Char('r') => return Ok(Action::Refresh),
            KeyCode::Enter => {
                if let Some(focus) = self.focuses.get(self.selected) {
                    self.query = focus.repository_query.clone();
                    self.issue_query = focus.issue_query.clone();
                    self.view = View::Issues;
                    self.selected = 0;
                    self.detail = false;
                    self.detail_scroll = 0;
                    return Ok(Action::FetchIssues);
                }
            }
            KeyCode::Esc => {
                self.query.clear();
                self.selected = 0;
            }
            _ => self.handle_list_key(code),
        }
        Ok(Action::None)
    }

    fn handle_activity_key(&mut self, code: KeyCode) -> Result<Action> {
        match code {
            KeyCode::Char('r') => return Ok(Action::FetchActivity),
            KeyCode::Char('a') => {
                if let (Some(account), Some(event)) = (&self.account, self.current_activity()) {
                    let (account_id, repo_id, key, acknowledged) = (
                        account.id,
                        event.repo_id,
                        event.key.clone(),
                        !event.acknowledged,
                    );
                    self.store
                        .acknowledge(account_id, repo_id, &key, acknowledged)?;
                    self.load_activity()?;
                    self.selected = self.selected.min(self.list_len().saturating_sub(1));
                    self.status = if acknowledged {
                        "Acknowledged locally; refresh never changes read state"
                    } else {
                        "Marked unread locally"
                    }
                    .into();
                }
            }
            KeyCode::Char('u') => {
                self.unread_only = !self.unread_only;
                self.selected = 0;
            }
            KeyCode::Char('e') => {
                self.feed_details = !self.feed_details;
                self.detail_scroll = 0;
            }
            KeyCode::Char('v') => {
                if let Some(event) = self.current_activity() {
                    if matches!(
                        event.kind,
                        crate::activity::ActivityKind::PrChange
                            | crate::activity::ActivityKind::Review
                    ) {
                        return Ok(Action::FetchReviews(event.repo_id, event.number));
                    }
                    self.status = "Select a PR change or review to fetch reviews".into();
                }
            }
            KeyCode::Char('o') => {
                if let Some(event) = self.current_activity() {
                    anyhow::ensure!(
                        crate::activity::valid_activity_url(&event.url),
                        "Invalid GitHub activity URL"
                    );
                    return Ok(Action::OpenIssue(event.url.clone()));
                }
            }

            KeyCode::Esc => {
                self.query.clear();
                self.selected = 0;
            }
            _ => self.handle_list_key(code),
        }
        Ok(Action::None)
    }

    fn handle_list_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.list_len().saturating_sub(1));
                self.detail_scroll = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                self.detail_scroll = 0;
            }
            _ => {}
        }
    }

    fn start_editor(&mut self, mode: Mode, input: String) {
        self.mode = mode;
        self.input = input;
    }
}

pub fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect()
}

pub(crate) fn query_words(query: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    for ch in query.chars() {
        if ch == '"' {
            quoted = !quoted;
        } else if ch.is_whitespace() && !quoted {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        } else {
            word.push(ch);
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

impl App {
    pub fn load_activity(&mut self) -> Result<()> {
        if let Some(account) = &self.account {
            self.activities = self.store.activities(account.id)?;
            self.feeds = self.store.feeds(account.id)?;
        }
        Ok(())
    }
    pub fn visible_activity(&self) -> Vec<&crate::activity::Activity> {
        let ids: std::collections::HashSet<_> = self
            .visible()
            .into_iter()
            .map(|i| self.repositories[i].id)
            .collect();
        let mut events: Vec<_> = self
            .activities
            .iter()
            .filter(|event| {
                ids.contains(&event.repo_id)
                    && event.occurred_at <= self.now
                    && (crate::activity::is_today(event.occurred_at, self.now, &chrono::Local)
                        || !event.acknowledged)
                    && (!self.unread_only || !event.acknowledged)
            })
            .collect();
        events.sort_by_key(|event| {
            (
                !crate::activity::is_today(event.occurred_at, self.now, &chrono::Local),
                std::cmp::Reverse(event.occurred_at),
                event.repo_id,
                &event.key,
            )
        });
        events
    }
    pub fn current_activity(&self) -> Option<&crate::activity::Activity> {
        self.visible_activity().get(self.selected).copied()
    }
    pub fn auto_due(&self) -> bool {
        !self.demo
            && !self.busy
            && self.account.is_some()
            && !self.auto_paused
            && self.network_retry_at.is_none_or(|at| self.now >= at)
            && self.now >= self.next_refresh
            && !self.feeds.iter().any(|f| {
                self.repositories.iter().any(|r| r.id == f.repo_id)
                    && (f.paused || f.retry_at.is_some_and(|at| self.now < at))
            })
    }
    pub fn prepare_activity(
        &mut self,
        manual: bool,
        review: Option<(u64, u64)>,
    ) -> Result<Vec<crate::sync::FeedRequest>> {
        self.check_network_window()?;
        anyhow::ensure!(
            !self.busy,
            "Another refresh is running; retry when it finishes"
        );
        let account = self
            .account
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Connect first: b then r"))?;
        let mut requests = Vec::new();
        for repo in &self.repositories {
            let names = match review {
                Some((id, number)) if repo.id == id => vec![format!("reviews:{number}")],
                Some(_) => continue,
                None => vec!["issues".into(), "comments".into()],
            };
            for feed in names {
                let state = self
                    .store
                    .ensure_feed(account.id, repo.id, &feed, self.now)?;
                if state.ready(self.now, manual) {
                    requests.push(crate::sync::FeedRequest {
                        account: account.clone(),
                        repo: repo.clone(),
                        state,
                        upper: self.now,
                    });
                }
            }
        }
        self.load_activity()?;
        self.next_refresh = self.now.saturating_add(crate::sync::REFRESH_SECONDS);
        self.auto_paused = false;
        self.activity_pending = requests
            .iter()
            .map(|r| (r.repo.id, r.state.feed.clone()))
            .collect();
        self.busy = !requests.is_empty();
        self.status = "Loading activity; first sync covers the last 24 hours, later syncs catch up from saved checkpoints".into();
        Ok(requests)
    }
    pub fn apply_activity(
        &mut self,
        request: &crate::sync::FeedRequest,
        result: Result<Vec<crate::activity::Activity>>,
    ) -> Result<()> {
        if self.account.as_ref().map(|a| a.id) != Some(request.account.id) {
            return Ok(());
        }
        let selected = if self.view == View::Activity {
            self.current_activity().map(|a| (a.repo_id, a.key.clone()))
        } else {
            None
        };
        self.activity_pending
            .retain(|(id, feed)| *id != request.repo.id || *feed != request.state.feed);
        let result = result.and_then(|events| self.store.commit_activity(request, &events));
        if let Err(error) = result {
            self.observe_failure(&error);
            self.store.fail_activity(request, &error)?;
            self.status = format!("Activity incomplete: {error}");
        }
        self.load_activity()?;
        if let Some((repo, key)) = selected {
            if let Some(index) = self
                .visible_activity()
                .iter()
                .position(|a| a.repo_id == repo && a.key == key)
            {
                self.selected = index;
            }
        }
        self.selected = self.selected.min(self.list_len().saturating_sub(1));
        Ok(())
    }

    pub fn observe_failure(&mut self, error: &anyhow::Error) {
        if let Some(policy) = error.downcast_ref::<crate::sync::RequestFailure>() {
            self.auto_paused |= policy.paused;
            if let Some(at) = policy.retry_at {
                self.network_retry_at = Some(self.network_retry_at.unwrap_or(at).max(at));
            }
        }
    }
    pub fn check_network_window(&self) -> Result<()> {
        let retry_at = self
            .feeds
            .iter()
            .filter_map(|f| f.retry_at)
            .chain(self.network_retry_at)
            .max();
        if let Some(at) = retry_at.filter(|at| *at > self.now) {
            anyhow::bail!(
                "GitHub rate limit; retry after {}",
                crate::activity::iso(at)?
            );
        }
        Ok(())
    }
}
