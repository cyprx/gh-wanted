use crate::app::{clean, App, Mode};
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
        Paragraph::new("Find where you're needed.  / search   t local tags   r refresh   ? help")
            .block(Block::default().title(title).borders(Borders::BOTTOM))
            .style(Style::default().fg(Color::Cyan)),
        rows[0],
    );
    if app.help {
        frame.render_widget(Paragraph::new("Repositories: j/k or arrows to move. Tab toggles detail view.\n\n/ edits filters; Enter applies; Esc cancels or clears.\nExample: topic:rust tag:priority\nAll topic: and tag: filters must match. Other words search names/descriptions.\n\nt edits comma-separated local tags. Empty input removes all local tags.\nGitHub topics are read-only. Local tags never leave this machine.\n\nr refreshes. q or Ctrl-C quits. Any key closes help.").wrap(Wrap { trim: false }).block(Block::bordered().title(" Help ")), rows[1]);
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
