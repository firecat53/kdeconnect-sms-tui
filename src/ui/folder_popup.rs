use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;

use super::theme;
use crate::app::{App, FolderKind};

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    let title = match app.folder_popup_kind {
        FolderKind::Archive => " Archive ",
        FolderKind::Spam => " Spam ",
        FolderKind::Trash => " Trash ",
    };

    let thread_ids = app.folder_thread_ids();

    let popup_width = 50.min(area.width * 80 / 100).max(24);
    // Each list item renders two lines (name + preview).
    let content_height = (thread_ids.len() * 2).max(1) as u16;
    let popup_height = (content_height + 2).min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(theme::title_style())
        .border_style(theme::active_border())
        .style(ratatui::style::Style::default().bg(theme::background()));

    if thread_ids.is_empty() {
        let empty_msg = match app.folder_popup_kind {
            FolderKind::Archive => "No archived conversations",
            FolderKind::Spam => "No spam conversations",
            FolderKind::Trash => "No trashed conversations",
        };
        let inner = block.inner(popup_area);
        f.render_widget(block, popup_area);
        let paragraph = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
            empty_msg,
            theme::help_style(),
        )));
        f.render_widget(paragraph, inner);
        return;
    }

    let items: Vec<ListItem> = thread_ids
        .iter()
        .map(|&tid| {
            let conv = app.conversations.iter().find(|c| c.thread_id == tid);
            let name = conv
                .and_then(|c| c.display_name.as_deref())
                .map(|s| s.to_string())
                .or_else(|| app.state.group_names.get(&tid.to_string()).cloned())
                .or_else(|| {
                    conv.and_then(|c| c.primary_address())
                        .and_then(|addr| app.contacts.lookup(addr))
                })
                .or_else(|| {
                    conv.and_then(|c| c.primary_address())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| format!("Thread {}", tid));

            let preview = conv.map(|c| c.preview_text()).unwrap_or("");
            let max_w = popup_width.saturating_sub(4) as usize;
            let preview_trunc: String = preview.chars().take(max_w).collect();

            ListItem::new(vec![
                Line::from(Span::styled(name, theme::title_style())),
                Line::from(Span::styled(preview_trunc, theme::help_style())),
            ])
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected_style());

    let mut state = ListState::default();
    state.select(Some(app.folder_popup_idx));
    f.render_stateful_widget(list, popup_area, &mut state);
}
