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
}

pub struct App {
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
}

impl App {
    pub fn new(store: Store, demo: bool) -> Self {
        Self {
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
            self.mode = Mode::Browse;
            self.input.clear();
            self.edit_target = None;
        }
        self.repositories.clear();
        self.tags.clear();
        self.selected = 0;
        let id = account.id;
        self.account = Some(account);
        self.repositories = self.store.repositories(id)?;
        self.load_tags()?;
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
            match key.code {
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
            return Ok(Action::None);
        }
        if self.help {
            self.help = false;
            return Ok(Action::None);
        }
        match key.code {
            KeyCode::Char('q') => return Ok(Action::Quit),
            KeyCode::Char('r') => return Ok(Action::Refresh),
            KeyCode::Char('?') => self.help = true,
            KeyCode::Tab => self.detail = !self.detail,
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.visible().len().saturating_sub(1))
            }
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Char('/') => {
                self.input = self.query.clone();
                self.mode = Mode::Search;
            }
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
            KeyCode::Esc => {
                self.query.clear();
                self.selected = 0;
            }
            _ => {}
        }
        Ok(Action::None)
    }
}

pub fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect()
}

fn query_words(query: &str) -> Vec<String> {
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
