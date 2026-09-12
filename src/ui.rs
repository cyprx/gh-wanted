use crate::app::{clean, App, Mode, View};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, List, ListItem, ListState, Padding, Paragraph, Wrap},
    Frame,
};

const BACKGROUND: Color = Color::Rgb(20, 24, 29);
const SURFACE: Color = Color::Rgb(27, 33, 40);
const TEXT: Color = Color::Rgb(230, 228, 220);
const MUTED: Color = Color::Rgb(154, 166, 178);
const BORDER: Color = Color::Rgb(65, 78, 89);
const ACCENT: Color = Color::Rgb(235, 184, 108);
const TOPIC: Color = Color::Rgb(139, 201, 188);
const SELECTED: Color = Color::Rgb(48, 62, 65);
const WARNING: Color = Color::Rgb(241, 157, 126);

fn panel(title: impl Into<String>) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title_style(Style::default().fg(TEXT).add_modifier(Modifier::BOLD))
        .style(Style::default().fg(TEXT).bg(SURFACE))
        .title(Line::styled(
            title.into(),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ))
        .padding(Padding::new(1, 1, 1, 0))
}

fn selected() -> Style {
    Style::default()
        .bg(SELECTED)
        .fg(TEXT)
        .add_modifier(Modifier::BOLD)
}

fn heading(text: impl Into<String>) -> Line<'static> {
    Line::styled(
        text.into(),
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )
}

fn metadata(text: impl Into<String>) -> Line<'static> {
    Line::styled(text.into(), Style::default().fg(MUTED))
}

fn body_lines(body: &str) -> Vec<Line<'static>> {
    body.lines().map(|line| Line::from(clean(line))).collect()
}

fn panes(area: Rect, detail: bool) -> Vec<Rect> {
    if area.width >= 88 && !detail {
        let columns = Layout::horizontal([
            Constraint::Percentage(45),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);
        vec![columns[0], columns[2]]
    } else {
        vec![area]
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);
    let brand = Line::from(vec![
        Span::styled(
            " gh-wanted ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" / find where you're needed", Style::default().fg(MUTED)),
    ]);
    let who = format!(
        "{}{}  ",
        app.account
            .as_ref()
            .map(|a| clean(&a.login))
            .unwrap_or_else(|| "not connected".into()),
        if app.demo { "  [DEMO]" } else { "" }
    );
    let columns = Layout::horizontal([
        Constraint::Min(1),
        Constraint::Length(if area.width >= 80 { 28 } else { 15 }),
    ])
    .split(rows[0]);
    frame.render_widget(Paragraph::new(brand), columns[0]);
    frame.render_widget(
        Paragraph::new(who)
            .alignment(Alignment::Right)
            .style(Style::default().fg(MUTED)),
        columns[1],
    );
    let mut tabs = Vec::new();
    for (view, key, label) in [
        (View::Repositories, "b", "Repos"),
        (View::Issues, "i", "Issues"),
        (View::Activity, "d", "Today"),
        (View::Focuses, "f", "Focuses"),
    ] {
        let active = app.view == view;
        let style = if active {
            Style::default()
                .fg(BACKGROUND)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        tabs.push(Span::styled(format!(" {key} {label} "), style));
        tabs.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(tabs)), rows[2]);
}

fn draw_shortcuts(frame: &mut Frame, app: &App, area: Rect) {
    let pairs = match app.view {
        View::Repositories => vec![
            ("j/k", "move"),
            ("/", "filter"),
            ("t", "tags"),
            ("i", "issues"),
            ("s", "save focus"),
        ],
        View::Issues => vec![
            ("j/k", "move"),
            ("/", "filter"),
            ("Tab", "details"),
            ("o", "open"),
            ("s", "save focus"),
        ],
        View::Activity => vec![
            ("a", "read/unread"),
            ("u", "unread"),
            ("e", "sync"),
            ("v", "reviews"),
            ("Tab", "details"),
        ],
        View::Focuses => vec![
            ("j/k", "move"),
            ("Enter", "open focus"),
            ("b", "repositories"),
        ],
    };
    let mut spans = Vec::new();
    for (key, label) in pairs.into_iter().chain([("?", "help"), ("q", "quit")]) {
        spans.push(Span::styled(
            format!(" {key} "),
            Style::default().fg(ACCENT),
        ));
        spans.push(Span::styled(
            format!("{label}  "),
            Style::default().fg(MUTED),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(BACKGROUND).fg(TEXT)),
        area,
    );
    if area.width < 42 || area.height < 14 {
        frame.render_widget(
            Paragraph::new("gh-wanted\nEnlarge terminal to 42 × 14.\nq quits.")
                .style(Style::default().fg(ACCENT)),
            area,
        );
        return;
    }
    let area = Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2);
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(3),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .split(area);
    draw_header(frame, app, rows[0]);
    draw_shortcuts(frame, app, rows[4]);
    if app.help {
        frame.render_widget(Paragraph::new("j/k or arrows move. Tab details. PgUp/PgDn scroll. q/Ctrl-C quit.\nb repositories; / filters topic:rust tag:priority; t edits local tags.\ni issues; / filters label:\"good first issue\" state:open unassigned keyword.\nLabels use AND; state is open/closed/all; keywords search title/body.\ns saves both filters; f lists focuses; Enter reopens one.\nd Today and catch-up across the current repository filter.\na toggles acknowledgment locally. Opening/refreshing never marks read.\nu toggles unread-only. Older unread items remain in catch-up.\ne shows per-feed errors/checkpoints; PgUp/PgDn scroll; e returns.\nv loads reviews for the selected PR change/review on demand.\no opens a validated GitHub URL. r refreshes the current view.\nActivity refreshes every 15 minutes while running, without overlapping.\nAuth failures pause automatic refresh; rate-limit retry times are honored.\nFirst activity sync covers 24h. Checkpoints survive restart.\nLocal tags, focuses, and read state stay on this machine.\nAny key closes help.").wrap(Wrap { trim: false }).block(panel("").title(" Help ")), rows[1]);
    } else if app.view == View::Activity {
        draw_activity(frame, app, rows[1]);
    } else if app.view != View::Repositories {
        draw_discovery(frame, app, rows[1]);
    } else {
        let columns = panes(rows[1], app.detail);
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
                        Line::styled(
                            clean(&repo.full_name),
                            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                        ),
                        Line::from(vec![
                            Span::styled(
                                format!(
                                    "{} ",
                                    repo.topics
                                        .iter()
                                        .map(|t| format!("#{}", clean(t)))
                                        .collect::<Vec<_>>()
                                        .join(" "),
                                ),
                                Style::default().fg(TOPIC),
                            ),
                            Span::styled(local, Style::default().fg(ACCENT)),
                        ]),
                        Line::from(""),
                    ])
                })
                .collect();
            let block = panel("").title(format!(
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
                        .highlight_style(selected())
                        .highlight_symbol("› "),
                    columns[0],
                    &mut state,
                );
            }
        }
        if app.detail || columns.len() > 1 {
            let pane = *columns.last().unwrap_or(&rows[1]);
            let body = app
                .current()
                .map(|repo| {
                    let mut lines = vec![
                        heading(clean(&repo.full_name)),
                        metadata(if repo.archived {
                            "Archived repository"
                        } else {
                            "Watched repository"
                        }),
                        Line::from(""),
                    ];
                    lines.extend(body_lines(
                        repo.description
                            .as_deref()
                            .unwrap_or("No description provided."),
                    ));
                    lines.extend([
                        Line::from(""),
                        metadata("GITHUB TOPICS"),
                        Line::styled(
                            repo.topics
                                .iter()
                                .map(|t| format!("#{}", clean(t)))
                                .collect::<Vec<_>>()
                                .join("  "),
                            Style::default().fg(TOPIC),
                        ),
                        Line::from(""),
                        metadata("LOCAL TAGS"),
                    ]);
                    let tags = app
                        .tags
                        .get(&repo.id)
                        .filter(|t| !t.is_empty())
                        .map(|t| {
                            t.iter()
                                .map(|v| format!("+{}", clean(v)))
                                .collect::<Vec<_>>()
                                .join("  ")
                        })
                        .unwrap_or_else(|| "No local tags yet".into());
                    lines.extend([
                        Line::styled(tags, Style::default().fg(ACCENT)),
                        Line::from(""),
                        metadata("t  edit local tags"),
                    ]);
                    lines
                })
                .unwrap_or_else(|| vec![metadata("Select a repository to explore.")]);
            frame.render_widget(
                Paragraph::new(body)
                    .block(panel("").title(" Repository "))
                    .wrap(Wrap { trim: false }),
                pane,
            );
        }
    }
    let (label, input) = match app.mode {
        Mode::Browse if app.view == View::Activity => (
            " Activity view ",
            if app.unread_only {
                "Unread only. b returns to repository filters"
            } else {
                "Today and earlier unread. b returns to repository filters"
            },
        ),
        Mode::Browse if app.view == View::Issues => (" / Issue filter ", app.issue_query.as_str()),
        Mode::Browse if app.view == View::Focuses => (
            " Saved views ",
            "Enter opens a focus with its saved filters",
        ),
        Mode::IssueSearch => (
            " Issue filter • Enter applies • Esc cancels ",
            app.input.as_str(),
        ),
        Mode::SaveFocus => (
            " Save focus • unique name • Enter saves • Esc cancels ",
            app.input.as_str(),
        ),
        Mode::Browse => (" / Repository filter ", app.query.as_str()),
        Mode::Search => (
            " Edit filter • Enter applies • Esc cancels ",
            app.input.as_str(),
        ),
        Mode::Tags => (
            " Edit local tags • comma-separated • Enter saves ",
            app.input.as_str(),
        ),
    };
    let safe_input = if input.is_empty() && app.mode == Mode::Browse {
        if app.view == View::Issues {
            "All open issues  /  label:\"good first issue\" unassigned".into()
        } else {
            "All watched repositories  /  topic:rust tag:priority".into()
        }
    } else {
        clean(input)
    };
    let width = ratatui::text::Line::from(safe_input.as_str()).width();
    let available = rows[2].width.saturating_sub(5) as usize;
    let scroll = if app.mode == Mode::Browse {
        0
    } else {
        width.saturating_sub(available)
    };
    frame.render_widget(
        Paragraph::new(safe_input)
            .scroll((0, scroll.min(u16::MAX as usize) as u16))
            .style(Style::default().fg(if app.mode == Mode::Browse {
                MUTED
            } else {
                TEXT
            }))
            .block(
                panel("")
                    .title(label)
                    .padding(Padding::horizontal(1))
                    .border_style(Style::default().fg(if app.mode == Mode::Browse {
                        BORDER
                    } else {
                        ACCENT
                    })),
            ),
        rows[2],
    );
    if app.mode != Mode::Browse {
        frame.set_cursor_position((rows[2].x + 2 + width.min(available) as u16, rows[2].y + 1));
    }
    frame.render_widget(
        Paragraph::new(clean(&app.status))
            .style(Style::default().fg(if app.busy {
                ACCENT
            } else if app.status.to_lowercase().contains("fail")
                || app.status.contains("incomplete")
            {
                WARNING
            } else {
                MUTED
            }))
            .wrap(Wrap { trim: true }),
        rows[3],
    );
}

fn draw_discovery(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if app.view == View::Focuses {
        let columns = panes(area, false);
        if app.focuses.is_empty() {
            frame.render_widget(Paragraph::new("No saved focuses yet. Press b, filter repositories, then i to browse issues. Set an issue filter with / and press s to save both filters.").wrap(Wrap { trim: true }).block(panel("").title(" Saved focuses ")), area);
        } else {
            let items: Vec<_> = app
                .focuses
                .iter()
                .map(|f| {
                    ListItem::new(vec![
                        heading(clean(&f.name)),
                        metadata(format!(
                            "{} / {}",
                            clean(&f.repository_query),
                            clean(&f.issue_query)
                        )),
                        Line::from(""),
                    ])
                })
                .collect();
            frame.render_stateful_widget(
                List::new(items)
                    .block(panel(format!(" Saved focuses ({}) ", app.focuses.len())))
                    .highlight_style(selected())
                    .highlight_symbol("› "),
                columns[0],
                &mut ListState::default().with_selected(Some(app.selected)),
            );
            if columns.len() > 1 {
                let lines = app
                    .focuses
                    .get(app.selected)
                    .map(|focus| {
                        vec![
                            heading(clean(&focus.name)),
                            metadata("A saved view of where you can contribute."),
                            Line::from(""),
                            metadata("REPOSITORIES"),
                            Line::from(if focus.repository_query.is_empty() {
                                "All watched repositories".into()
                            } else {
                                clean(&focus.repository_query)
                            }),
                            Line::from(""),
                            metadata("ISSUES"),
                            Line::from(if focus.issue_query.is_empty() {
                                "All open issues".into()
                            } else {
                                clean(&focus.issue_query)
                            }),
                            Line::from(""),
                            metadata("Enter  open this focus"),
                            Line::from(""),
                            metadata("Membership follows your current topics and local tags."),
                        ]
                    })
                    .unwrap_or_default();
                frame.render_widget(
                    Paragraph::new(lines)
                        .wrap(Wrap { trim: false })
                        .block(panel(" Focus preview ")),
                    columns[1],
                );
            }
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
        Paragraph::new(vec![
            heading(format!(
                "Issue discovery  /  {}/{} repositories loaded{}",
                complete,
                visible.len(),
                if complete < visible.len() {
                    " • INCOMPLETE"
                } else {
                    ""
                }
            )),
            metadata(if failures.is_empty() {
                if app.query.is_empty() {
                    "All watched repositories".into()
                } else {
                    format!("Repositories: {}", clean(&app.query))
                }
            } else {
                failures.join(" | ")
            }),
        ])
        .wrap(Wrap { trim: true }),
        rows[0],
    );
    let columns = panes(rows[1], app.detail);
    let issues = app.visible_issues();
    if !app.detail {
        let block = panel(format!(" Issues ({}) ", issues.len()));
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
                        Line::styled(
                            clean(&issue.title),
                            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                        ),
                        metadata(format!(
                            "{} #{} [{}]",
                            clean(name),
                            issue.number,
                            clean(&issue.state)
                        )),
                        Line::from(""),
                    ])
                })
                .collect();
            frame.render_stateful_widget(
                List::new(items)
                    .block(block)
                    .highlight_style(selected())
                    .highlight_symbol("› "),
                columns[0],
                &mut ListState::default().with_selected(Some(app.selected)),
            );
        }
    }
    if app.detail || columns.len() > 1 {
        let body = app
            .current_issue()
            .map(|i| {
                let mut lines = vec![
                    heading(clean(&i.title)),
                    metadata(format!(
                        "#{}  •  {}  •  {}",
                        i.number,
                        clean(&i.state),
                        if i.assignees.is_empty() {
                            "unassigned".into()
                        } else {
                            clean(&i.assignees.join(", "))
                        }
                    )),
                    Line::styled(clean(&i.labels.join("  /  ")), Style::default().fg(TOPIC)),
                    Line::from(""),
                ];
                lines.extend(body_lines(
                    i.body.as_deref().unwrap_or("No description provided."),
                ));
                lines.extend([
                    Line::from(""),
                    metadata(format!("Created  {}", clean(&i.created_at))),
                    metadata(format!("Updated  {}", clean(&i.updated_at))),
                    Line::from(""),
                    metadata(clean(&i.url)),
                ]);
                lines
            })
            .unwrap_or_else(|| vec![metadata("Select an issue to read its description.")]);
        frame.render_widget(
            Paragraph::new(body)
                .wrap(Wrap { trim: false })
                .scroll((app.detail_scroll, 0))
                .block(panel(" Issue details ")),
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
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    format!("Today ({today})  /  Catch-up ({})  ", events.len() - today),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        "{complete}/{expected} feeds{}",
                        if incomplete {
                            " • INCOMPLETE"
                        } else {
                            " checked"
                        }
                    ),
                    Style::default().fg(if incomplete { WARNING } else { TOPIC }),
                ),
            ]),
            metadata("Initial history: last 24h  •  Reviews on demand  •  e sync details"),
        ])
        .wrap(Wrap { trim: true }),
        rows[0],
    );
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
                .block(panel("").title(" Activity feeds ")),
            rows[1],
        );
        return;
    }
    let columns = panes(rows[1], app.detail);
    if !app.detail {
        let block = panel(" Activity inbox ");
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
                            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                        ));
                        previous_today = Some(is_today);
                    }
                    let repo = app
                        .repositories
                        .iter()
                        .find(|r| r.id == event.repo_id)
                        .map(|r| r.full_name.as_str())
                        .unwrap_or("unknown");
                    lines.push(Line::styled(
                        clean(&event.title),
                        Style::default()
                            .fg(if event.acknowledged { MUTED } else { TEXT })
                            .add_modifier(Modifier::BOLD),
                    ));
                    lines.push(metadata(format!(
                        "[{}] {} • {} #{}",
                        if event.acknowledged { "read" } else { "unread" },
                        event.kind.label(),
                        clean(repo),
                        event.number
                    )));
                    lines.push(Line::from(""));
                    ListItem::new(lines)
                })
                .collect();
            frame.render_stateful_widget(
                List::new(items)
                    .block(block)
                    .highlight_style(selected())
                    .highlight_symbol("› "),
                columns[0],
                &mut ListState::default().with_selected(Some(app.selected)),
            );
        }
    }
    if app.detail || columns.len() > 1 {
        let body = app
            .current_activity()
            .map(|event| {
                let local = chrono::DateTime::from_timestamp(event.occurred_at, 0)
                    .map(|d| {
                        d.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M:%S %:z")
                            .to_string()
                    })
                    .unwrap_or_default();
                let mut lines = vec![
                    heading(clean(&event.title)),
                    Line::styled(
                        format!(
                            "{}  •  {}",
                            event.kind.label(),
                            if event.acknowledged { "read" } else { "unread" }
                        ),
                        Style::default().fg(TOPIC),
                    ),
                    metadata(local),
                    Line::from(""),
                    metadata(format!(
                        "{}  /  {}",
                        clean(&event.actor),
                        clean(&event.association)
                    )),
                    Line::from(""),
                ];
                lines.extend(body_lines(&event.body));
                lines.extend([
                    Line::from(""),
                    metadata(clean(&event.url)),
                    metadata(format!(
                        "Fetched {}",
                        crate::activity::iso(event.fetched_at).unwrap_or_default()
                    )),
                    Line::from(""),
                    metadata("Snapshot activity, not a complete event audit."),
                ]);
                lines
            })
            .unwrap_or_else(|| vec![metadata("Select activity to read the update.")]);
        frame.render_widget(
            Paragraph::new(body)
                .wrap(Wrap { trim: false })
                .scroll((app.detail_scroll, 0))
                .block(panel(" Update details ")),
            *columns.last().unwrap(),
        );
    }
}
