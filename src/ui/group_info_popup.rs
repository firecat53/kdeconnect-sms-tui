use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use super::theme;
use crate::app::App;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let members = app.group_members();

    // Height: border(2) + members + 1 blank + 1 label + 1 input
    let content_lines = members.len() as u16 + 3;
    let popup_width = 50.min(area.width * 80 / 100).max(24);
    let popup_height = (content_lines + 2).min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let is_group = app
        .selected_conversation_idx
        .and_then(|i| app.conversations.get(i))
        .is_some_and(|c| c.is_group);
    let title = if is_group {
        " Group Info "
    } else {
        " Contact Info "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(theme::title_style())
        .border_style(theme::active_border())
        .style(ratatui::style::Style::default().bg(theme::background()));

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Member list
    for (name, phone) in &members {
        let display = if name == phone {
            name.clone()
        } else {
            format!("{} ({})", name, phone)
        };
        lines.push(Line::from(Span::styled(display, theme::title_style())));
    }

    // Blank separator
    lines.push(Line::from(""));

    // Label
    lines.push(Line::from(Span::styled("Name:", theme::help_style())));

    // Input line with cursor
    let input = &app.group_name_input;
    let cursor_byte = app.group_name_cursor;
    let before = &input[..cursor_byte];
    let cursor_char = input[cursor_byte..].chars().next().unwrap_or(' ');
    let after_len = cursor_byte + cursor_char.len_utf8().min(input.len() - cursor_byte);
    let after = &input[after_len.min(input.len())..];

    lines.push(Line::from(vec![
        Span::styled(before.to_string(), theme::title_style()),
        Span::styled(cursor_char.to_string(), theme::selected_style()),
        Span::styled(after.to_string(), theme::title_style()),
    ]));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, inner);
}
