use crate::app::{clean, App, Mode, View};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    if area.width < 42 || area.height < 10 {
        frame.render_widget(
            Paragraph::new("gh-wanted: enlarge terminal to 42 × 10. q quits."),
            area,
        );
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(3),
        Constraint::Length(2),
    ])
    .split(area);
    let who = app
        .account
        .as_ref()
        .map(|a| clean(&a.login))
        .unwrap_or_else(|| "not connected".into());
    let title = format!(
        " gh-wanted  /  {}{}",
        who,
        if app.demo { "  [DEMO]" } else { "" }
    );
    frame.render_widget(
        Paragraph::new("/ filter  i issues  b repos  f focuses  s save  r refresh  ? help")
            .block(Block::default().title(title).borders(Borders::BOTTOM))
            .style(Style::default().fg(Color::Cyan)),
        rows[0],
    );
    if app.help {
        frame.render_widget(Paragraph::new("j/k or arrows move. Tab toggles details. PgUp/PgDn scroll issue details.\n/ edits the current filter; Enter applies; Esc cancels or clears.\nRepositories: topic:rust tag:priority. All topic/tag filters must match.\nt edits comma-separated local tags; empty input removes tags.\ni browses issues from ALL matching repositories. b returns to repositories.\nIssues: label:\"good first issue\" state:open unassigned keyword\nLabels use AND. State defaults to open; closed and all are supported.\nKeywords match title/body, ignoring case. o opens the selected issue in a browser.\ns saves both filters under a unique name. f lists focuses; Enter reopens one.\nFocus membership follows current topics/tags. Focuses stay on this machine.\nr refreshes the current feed. Failed issue fetches remain incomplete.\nq or Ctrl-C quits. Any key closes help.").wrap(Wrap { trim: false }).block(Block::bordered().title(" Help ")), rows[1]);
    } else if app.view != View::Repositories {
        draw_discovery(frame, app, rows[1]);
    } else {
        let columns = if area.width >= 90 && !app.detail {
            Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)])
                .split(rows[1])
                .to_vec()
        } else {
            vec![rows[1]]
        };
        if !app.detail {
            let visible = app.visible();
            let items: Vec<ListItem> = visible
                .iter()
                .map(|index| {
                    let repo = &app.repositories[*index];
                    let local = app
                        .tags
                        .get(&repo.id)
                        .map(|t| {
                            t.iter()
                                .map(|v| format!("+{}", clean(v)))
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default();
                    ListItem::new(vec![
                        Line::from(clean(&repo.full_name)),
                        Line::from(Span::styled(
                            format!(
                                "  {} {}",
                                repo.topics
                                    .iter()
                                    .map(|t| format!("#{}", clean(t)))
                                    .collect::<Vec<_>>()
                                    .join(" "),
                                local
                            ),
                            Style::default().fg(Color::Cyan),
                        )),
                    ])
                })
                .collect();
            let block = Block::bordered().title(format!(
                " Watched repositories ({}/{}) ",
                visible.len(),
                app.repositories.len()
            ));
            if items.is_empty() {
                frame.render_widget(
                    Paragraph::new(if app.busy {
                        "Loading..."
                    } else {
                        "No repositories match. Press / to change filters or r to refresh."
                    })
                    .block(block)
                    .wrap(Wrap { trim: true }),
                    columns[0],
                );
            } else {
                let mut state = ListState::default().with_selected(Some(app.selected));
                frame.render_stateful_widget(
                    List::new(items)
                        .block(block)
                        .highlight_style(
                            Style::default()
                                .bg(Color::DarkGray)
                                .add_modifier(Modifier::BOLD),
                        )
                        .highlight_symbol("› "),
                    columns[0],
                    &mut state,
                );
            }
        }
        if app.detail || columns.len() > 1 {
            let pane = *columns.last().unwrap_or(&rows[1]);
            let body = app.current().map(|repo| format!("{}{}\n\n{}\n\nGitHub topics\n{}\n\nLocal tags\n{}\n\nPress t to edit local tags.", clean(&repo.full_name), if repo.archived { " [archived]" } else { "" }, clean(repo.description.as_deref().unwrap_or("No description")), repo.topics.iter().map(|t| format!("#{}", clean(t))).collect::<Vec<_>>().join(" "), app.tags.get(&repo.id).map(|t| t.iter().map(|v| format!("+{}", clean(v))).collect::<Vec<_>>().join(" ")).unwrap_or_default())).unwrap_or_else(|| "Select a repository".into());
            frame.render_widget(
                Paragraph::new(body)
                    .block(Block::bordered().title(" Repository "))
                    .wrap(Wrap { trim: false }),
                pane,
            );
        }
    }
    let (label, input) = match app.mode {
        Mode::Browse if app.view == View::Issues => (
            " Issues: label:NAME state:open|closed|all unassigned text ",
            app.issue_query.as_str(),
        ),
        Mode::Browse if app.view == View::Focuses => (" Enter opens focus • b repositories ", ""),
        Mode::IssueSearch => (
            " Issue filter • Enter applies • Esc cancels ",
            app.input.as_str(),
        ),
        Mode::SaveFocus => (
            " Save focus • unique name • Enter saves • Esc cancels ",
            app.input.as_str(),
        ),
        Mode::Browse => (" Filter: topic:NAME tag:NAME text ", app.query.as_str()),
        Mode::Search => (
            " Edit filter • Enter applies • Esc cancels ",
            app.input.as_str(),
        ),
        Mode::Tags => (
            " Edit local tags • comma-separated • Enter saves ",
            app.input.as_str(),
        ),
    };
    let safe_input = clean(input);
    let width = ratatui::text::Line::from(safe_input.as_str()).width();
    let available = rows[2].width.saturating_sub(3) as usize;
    let scroll = if app.mode == Mode::Browse {
        0
    } else {
        width.saturating_sub(available)
    };
    frame.render_widget(
        Paragraph::new(safe_input)
            .scroll((0, scroll.min(u16::MAX as usize) as u16))
            .block(Block::bordered().title(label)),
        rows[2],
    );
    if app.mode != Mode::Browse {
        frame.set_cursor_position((rows[2].x + 1 + width.min(available) as u16, rows[2].y + 1));
    }
    frame.render_widget(
        Paragraph::new(clean(&app.status))
            .style(Style::default().fg(if app.busy { Color::Yellow } else { Color::Gray }))
            .wrap(Wrap { trim: true }),
        rows[3],
    );
}

fn draw_discovery(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if app.view == View::Focuses {
        if app.focuses.is_empty() {
            frame.render_widget(Paragraph::new("No saved focuses yet. Press b, filter repositories, then i to browse issues. Set an issue filter with / and press s to save both filters.").wrap(Wrap { trim: true }).block(Block::bordered().title(" Saved focuses ")), area);
        } else {
            let items: Vec<_> = app
                .focuses
                .iter()
                .map(|f| {
                    ListItem::new(vec![
                        Line::from(clean(&f.name)),
                        Line::from(format!(
                            "  Repos: {} | Issues: {}",
                            clean(&f.repository_query),
                            clean(&f.issue_query)
                        )),
                    ])
                })
                .collect();
            frame.render_stateful_widget(
                List::new(items)
                    .block(Block::bordered().title(" Saved focuses • Enter opens "))
                    .highlight_style(Style::default().bg(Color::DarkGray))
                    .highlight_symbol("› "),
                area,
                &mut ListState::default().with_selected(Some(app.selected)),
            );
        }
        return;
    }
    let visible = app.visible();
    let complete = visible
        .iter()
        .filter(|i| {
            app.issue_status
                .get(&app.repositories[**i].id)
                .is_some_and(|s| s == "Complete")
        })
        .count();
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
    let failures: Vec<_> = visible
        .iter()
        .filter_map(|i| {
            let repo = &app.repositories[*i];
            let status = app
                .issue_status
                .get(&repo.id)
                .map(String::as_str)
                .unwrap_or("Not fetched; r loads issues");
            (status != "Complete").then(|| format!("{}: {}", clean(&repo.full_name), clean(status)))
        })
        .collect();
    frame.render_widget(
        Paragraph::new(format!(
            "Repositories: {} | {}/{} complete{}\n{}",
            clean(&app.query),
            complete,
            visible.len(),
            if complete < visible.len() {
                " • INCOMPLETE"
            } else {
                ""
            },
            failures.join(" | ")
        ))
        .wrap(Wrap { trim: true }),
        rows[0],
    );
    let columns = if area.width >= 90 && !app.detail {
        Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(rows[1])
            .to_vec()
    } else {
        vec![rows[1]]
    };
    let issues = app.visible_issues();
    if !app.detail {
        let block = Block::bordered().title(format!(" Issues ({}) • Tab details ", issues.len()));
        if issues.is_empty() {
            let text = if visible.is_empty() {
                "No repositories match. Press b and change the repository filter with /."
            } else if complete < visible.len() {
                "Issue results are incomplete. Wait for loading, or press r to retry. b returns to repositories."
            } else {
                "No issues match. Press / to change labels, keywords, state, or unassigned. Default state is open."
            };
            frame.render_widget(
                Paragraph::new(text).wrap(Wrap { trim: true }).block(block),
                columns[0],
            );
        } else {
            let items: Vec<_> = issues
                .iter()
                .map(|issue| {
                    let name = app
                        .repositories
                        .iter()
                        .find(|r| r.id == issue.repo_id)
                        .map(|r| r.full_name.as_str())
                        .unwrap_or("unknown");
                    ListItem::new(vec![
                        Line::from(format!(
                            "{} #{} [{}]",
                            clean(name),
                            issue.number,
                            clean(&issue.state)
                        )),
                        Line::from(clean(&issue.title)),
                    ])
                })
                .collect();
            frame.render_stateful_widget(
                List::new(items)
                    .block(block)
                    .highlight_style(Style::default().bg(Color::DarkGray))
                    .highlight_symbol("› "),
                columns[0],
                &mut ListState::default().with_selected(Some(app.selected)),
            );
        }
    }
    if app.detail || columns.len() > 1 {
        let body = app.current_issue().map(|i| format!("#{} {}\nState: {}\nLabels: {}\nAssignees: {}\nCreated: {}\nUpdated: {}\n{}\n\n{}", i.number, clean(&i.title), clean(&i.state), clean(&i.labels.join(", ")), if i.assignees.is_empty() { "unassigned".into() } else { clean(&i.assignees.join(", ")) }, clean(&i.created_at), clean(&i.updated_at), clean(&i.url), i.body.as_deref().unwrap_or("No description").lines().map(clean).collect::<Vec<_>>().join("\n"))).unwrap_or_else(|| "Select an issue".into());
        frame.render_widget(
            Paragraph::new(body)
                .wrap(Wrap { trim: false })
                .scroll((app.detail_scroll, 0))
                .block(Block::bordered().title(" Issue • PgUp/PgDn scroll • o browser ")),
            *columns.last().unwrap(),
        );
    }
}
