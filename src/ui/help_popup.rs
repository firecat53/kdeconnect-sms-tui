use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::theme;

/// Key binding entries: (key, description)
const HELP_ENTRIES: &[(&str, &str)] = &[
    ("j/k, Up/Down", "Navigate conversations, messages, or popup lists"),
    ("J/K, PgDn/PgUp", "Page through conversations or messages"),
    ("l, Tab", "Move from conversations to messages"),
    ("h, Tab", "Move from messages to conversations"),
    ("Enter / i", "Compose from the selected conversation"),
    ("Enter (messages)", "Open selected attachment"),
    ("D (messages)", "Download selected image to downloads folder"),
    ("c", "Copy message text or attachment"),
    ("Esc", "Cancel / back to previous panel"),
    ("", ""),
    ("d", "Open device selector"),
    ("r", "Refresh conversations / reconnect"),
    ("g", "Edit group conversation name"),
    ("", ""),
    ("a", "Archive selected conversation"),
    ("s", "Mark selected conversation as spam"),
    ("A", "Browse archived conversations"),
    ("S", "Browse spam conversations"),
    ("", ""),
    ("Shift+Enter / Alt+Enter / Ctrl+j", "Insert newline while composing"),
    ("Alt+A / Alt+X", "Add or remove a compose attachment"),
    ("Backspace (picker)", "Move to parent directory"),
    ("/", "Search conversations or messages"),
    ("n/p", "Next/previous search result"),
    ("Esc (search)", "Clear search"),
    ("", ""),
    ("t / T", "Cycle dark / light themes"),
    ("Ctrl+C", "Quit"),
    ("?", "Show this help"),
];

pub fn draw(f: &mut Frame) {
    let area = f.area();

    // Size the popup to fit content: key column is padded to 20 chars
    let key_col_width = 20;
    let content_width = HELP_ENTRIES
        .iter()
        .map(|(_, d)| key_col_width + d.len())
        .max()
        .unwrap_or(40) as u16;
    let popup_width = (content_width + 4)
        .min(area.width.saturating_sub(4))
        .max(30);
    let popup_height = (HELP_ENTRIES.len() as u16 + 3).min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help ")
        .title_style(theme::title_style())
        .border_style(theme::active_border())
        .style(ratatui::style::Style::default().bg(theme::background()));

    let lines: Vec<Line> = HELP_ENTRIES
        .iter()
        .map(|(key, desc)| {
            if key.is_empty() {
                Line::raw("")
            } else {
                Line::from(vec![
                    Span::styled(
                        format!("{:<20}", key),
                        theme::help_style().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(*desc),
                ])
            }
        })
        .chain(std::iter::once(Line::raw("")))
        .chain(std::iter::once(Line::styled(
            "Press any key to close",
            theme::help_style(),
        )))
        .collect();

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, popup_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn test_help_popup_renders() {
        let backend = TestBackend::new(80, 40);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f);
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("Help"));
        assert!(content.contains("Navigate conversations"));
        assert!(content.contains("Ctrl+C"));
        assert!(content.contains("Archive"));
    }
}
