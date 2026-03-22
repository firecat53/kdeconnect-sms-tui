use ratatui::layout::Rect;
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Focus};
use super::theme;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focus == Focus::Compose;

    let title = if is_focused {
        " Compose (Esc: back, Enter: send) "
    } else {
        " Compose "
    };

    let border_style = if is_focused {
        theme::active_border()
    } else {
        theme::inactive_border()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(if is_focused { theme::title_style() } else { theme::help_style() })
        .border_style(border_style);

    if app.compose_input.is_empty() && !is_focused {
        let placeholder = Paragraph::new(Span::styled(
            "Type a message...",
            theme::help_style(),
        ))
        .block(block);
        f.render_widget(placeholder, area);
        return;
    }

    let text = &app.compose_input;
    let paragraph = Paragraph::new(text.as_str())
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);

    // Show cursor when focused
    if is_focused {
        let inner = inner_rect(area);
        let (cx, cy) = cursor_position(text, app.compose_cursor, inner.width as usize);
        f.set_cursor_position((inner.x + cx as u16, inner.y + cy as u16));
    }
}

/// Get the inner rect (accounting for border)
fn inner_rect(area: Rect) -> Rect {
    Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

/// Calculate cursor (x, y) position within the compose area, accounting for wrapping.
fn cursor_position(text: &str, byte_offset: usize, width: usize) -> (usize, usize) {
    if width == 0 {
        return (0, 0);
    }

    let text_before_cursor = &text[..byte_offset];
    let mut x = 0usize;
    let mut y = 0usize;

    for ch in text_before_cursor.chars() {
        if ch == '\n' {
            x = 0;
            y += 1;
        } else {
            x += 1;
            if x >= width {
                x = 0;
                y += 1;
            }
        }
    }

    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_position_empty() {
        assert_eq!(cursor_position("", 0, 40), (0, 0));
    }

    #[test]
    fn test_cursor_position_simple() {
        assert_eq!(cursor_position("hello", 5, 40), (5, 0));
    }

    #[test]
    fn test_cursor_position_newline() {
        assert_eq!(cursor_position("hi\nworld", 3, 40), (0, 1));
        assert_eq!(cursor_position("hi\nworld", 8, 40), (5, 1));
    }

    #[test]
    fn test_cursor_position_wrap() {
        // Width 5, "abcde" wraps after 5 chars
        assert_eq!(cursor_position("abcdef", 5, 5), (0, 1));
        assert_eq!(cursor_position("abcdef", 6, 5), (1, 1));
    }

    #[test]
    fn test_cursor_position_zero_width() {
        assert_eq!(cursor_position("hello", 3, 0), (0, 0));
    }

    #[test]
    fn test_compose_renders_placeholder() {
        let app = App::new_test();
        let backend = ratatui::backend::TestBackend::new(40, 5);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("Type a message"));
    }

    #[test]
    fn test_compose_renders_text() {
        let mut app = App::new_test();
        app.compose_input = "Hello world".into();
        app.compose_cursor = 11;

        let backend = ratatui::backend::TestBackend::new(40, 5);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("Hello world"));
    }

    #[test]
    fn test_compose_focused_shows_title() {
        let mut app = App::new_test();
        app.focus = Focus::Compose;

        let backend = ratatui::backend::TestBackend::new(50, 5);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("Esc: back"));
    }
}
