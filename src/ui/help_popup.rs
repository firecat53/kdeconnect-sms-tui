use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::theme;

/// Key binding entries: (key, description)
const HELP_ENTRIES: &[(&str, &str)] = &[
    ("j/k, Up/Down", "Navigate items"),
    ("J/K, PgDn/PgUp", "Page down / page up"),
    ("h/l, Tab", "Switch panel (conversations / messages)"),
    ("Enter, i", "Compose message"),
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
    ("Shift+Enter", "Insert newline while composing"),
    ("Ctrl+C", "Quit"),
    ("?", "Show this help"),
];

pub fn draw(f: &mut Frame) {
    let area = f.area();

    // Size the popup to fit content
    let content_width = HELP_ENTRIES
        .iter()
        .map(|(k, d)| k.len() + d.len() + 4) // key + separator + desc
        .max()
        .unwrap_or(40) as u16;
    let popup_width = (content_width + 4).min(area.width.saturating_sub(4)).max(30);
    let popup_height = (HELP_ENTRIES.len() as u16 + 3).min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help ")
        .title_style(theme::title_style())
        .border_style(theme::active_border());

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
        assert!(content.contains("Navigate items"));
        assert!(content.contains("Ctrl+C"));
        assert!(content.contains("Archive"));
    }
}
