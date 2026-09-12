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
        Paragraph::new("d Today  i issues  b repos  f focuses  / filter  r refresh  ? help")
            .block(Block::default().title(title).borders(Borders::BOTTOM))
            .style(Style::default().fg(Color::Cyan)),
        rows[0],
    );
    if app.help {
        frame.render_widget(Paragraph::new("j/k or arrows move. Tab details. PgUp/PgDn scroll. q/Ctrl-C quit.\nb repositories; / filters topic:rust tag:priority; t edits local tags.\ni issues; / filters label:\"good first issue\" state:open unassigned keyword.\nLabels use AND; state is open/closed/all; keywords search title/body.\ns saves both filters; f lists focuses; Enter reopens one.\nd Today and catch-up across the current repository filter.\na toggles acknowledgment locally. Opening/refreshing never marks read.\nu toggles unread-only. Older unread items remain in catch-up.\ne shows per-feed errors/checkpoints; PgUp/PgDn scroll; e returns.\nv loads reviews for the selected PR change/review on demand.\no opens a validated GitHub URL. r refreshes the current view.\nActivity refreshes every 15 minutes while running, without overlapping.\nAuth failures pause automatic refresh; rate-limit retry times are honored.\nFirst activity sync covers 24h. Checkpoints survive restart.\nLocal tags, focuses, and read state stay on this machine.\nAny key closes help.").wrap(Wrap { trim: false }).block(Block::bordered().title(" Help ")), rows[1]);
    } else if app.view == View::Activity {
        draw_activity(frame, app, rows[1]);
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
        Mode::Browse if app.view == View::Activity => (
            " a acknowledge • u unread filter • e feed status • v PR reviews • Tab details ",
            if app.unread_only {
                "Unread only. b returns to repository filters"
            } else {
                "Today and earlier unread. b returns to repository filters"
            },
        ),
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

fn draw_activity(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let visible = app.visible();
    let ids: std::collections::HashSet<_> =
        visible.iter().map(|i| app.repositories[*i].id).collect();
    let feeds: Vec<_> = app
        .feeds
        .iter()
        .filter(|f| ids.contains(&f.repo_id))
        .collect();
    let complete = feeds
        .iter()
        .filter(|f| {
            matches!(f.feed.as_str(), "issues" | "comments")
                && f.checkpoint.is_some()
                && f.error.is_none()
                && !app.activity_pending.contains(&(f.repo_id, f.feed.clone()))
        })
        .count();
    let expected = ids.len() * 2;
    let incomplete = complete < expected
        || feeds.iter().any(|f| f.error.is_some())
        || app.activity_pending.iter().any(|(id, _)| ids.contains(id));
    let events = app.visible_activity();
    let today = events
        .iter()
        .filter(|e| crate::activity::is_today(e.occurred_at, app.now, &chrono::Local))
        .count();
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
    frame.render_widget(Paragraph::new(format!("Today ({today}) | Catch-up ({}) | {complete}/{expected} feeds checked{}\nInitial history: last 24h. Reviews on demand. e shows checkpoints/errors.",events.len()-today, if incomplete { " • INCOMPLETE" } else { "" })).wrap(Wrap { trim: true }),rows[0]);
    if app.feed_details {
        let mut lines = vec!["Feed status • PgUp/PgDn scroll • e returns".to_owned()];
        for i in visible {
            let repo = &app.repositories[i];
            let matching: Vec<_> = feeds.iter().filter(|f| f.repo_id == repo.id).collect();
            if matching.is_empty() {
                lines.push(format!(
                    "{}: not fetched; r starts last 24h",
                    clean(&repo.full_name)
                ));
            }
            for feed in matching {
                let checkpoint = feed
                    .checkpoint
                    .and_then(|v| crate::activity::iso(v).ok())
                    .unwrap_or_else(|| "none (initial last 24h)".into());
                let status = if app
                    .activity_pending
                    .contains(&(feed.repo_id, feed.feed.clone()))
                {
                    "Loading"
                } else {
                    feed.error.as_deref().unwrap_or("Saved")
                };
                lines.push(format!(
                    "{} / {}\n  Checkpoint: {}\n  {}",
                    clean(&repo.full_name),
                    clean(&feed.feed),
                    checkpoint,
                    clean(status)
                ));
            }
        }
        frame.render_widget(
            Paragraph::new(lines.join("\n"))
                .wrap(Wrap { trim: false })
                .scroll((app.detail_scroll, 0))
                .block(Block::bordered().title(" Activity feeds ")),
            rows[1],
        );
        return;
    }
    let columns = if area.width >= 90 && !app.detail {
        Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(rows[1])
            .to_vec()
    } else {
        vec![rows[1]]
    };
    if !app.detail {
        let block = Block::bordered().title(" Today / Catch-up • a acknowledges locally ");
        if events.is_empty() {
            let message = if ids.is_empty() {
                "No repositories match. Press b to change repository filters."
            } else if incomplete {
                "Activity is incomplete. Press r to fetch or retry; e shows feed errors. Cached unread activity is retained."
            } else {
                "You're caught up. No matching activity to show. u toggles unread-only; r refreshes. New activity is checked every 15 minutes while running."
            };
            frame.render_widget(
                Paragraph::new(message)
                    .wrap(Wrap { trim: true })
                    .block(block),
                columns[0],
            );
        } else {
            let mut previous_today = None;
            let items: Vec<_> = events
                .iter()
                .map(|event| {
                    let is_today =
                        crate::activity::is_today(event.occurred_at, app.now, &chrono::Local);
                    let mut lines = Vec::new();
                    if previous_today != Some(is_today) {
                        lines.push(Line::styled(
                            if is_today {
                                "Today (local time)"
                            } else {
                                "Catch-up (earlier unread)"
                            },
                            Style::default().fg(Color::Cyan),
                        ));
                        previous_today = Some(is_today);
                    }
                    let repo = app
                        .repositories
                        .iter()
                        .find(|r| r.id == event.repo_id)
                        .map(|r| r.full_name.as_str())
                        .unwrap_or("unknown");
                    lines.push(Line::from(format!(
                        "[{}] {} • {} #{}",
                        if event.acknowledged { "read" } else { "unread" },
                        event.kind.label(),
                        clean(repo),
                        event.number
                    )));
                    lines.push(Line::from(clean(&event.title)));
                    ListItem::new(lines)
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
        let body = app.current_activity().map(|event| {
            let local = chrono::DateTime::from_timestamp(event.occurred_at,0).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S %:z").to_string()).unwrap_or_default();
            format!("{}\n{} • {}\n{}\nBy {} (association: {})\nFetched: {}\n{}\n\n{}\n\nSnapshot activity is not a full audit. Issue timestamps do not establish replies or CI results.",clean(&event.title),event.kind.label(),if event.acknowledged { "read" } else { "unread" },local,clean(&event.actor),clean(&event.association),crate::activity::iso(event.fetched_at).unwrap_or_default(),clean(&event.url),event.body.lines().map(clean).collect::<Vec<_>>().join("\n"))
        }).unwrap_or_else(|| "Select activity. a changes read state locally; v fetches reviews for a selected PR.".into());
        frame.render_widget(
            Paragraph::new(body)
                .wrap(Wrap { trim: false })
                .scroll((app.detail_scroll, 0))
                .block(Block::bordered().title(" Activity • o browser • PgUp/PgDn scroll ")),
            *columns.last().unwrap(),
        );
    }
}
