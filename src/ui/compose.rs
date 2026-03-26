use ratatui::layout::Rect;
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::theme;
use crate::app::{App, Focus};

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.focus == Focus::Compose;

    let title = if is_focused {
        " Compose (Esc: back, Enter: send, Alt+A: attach) "
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
        .title_style(if is_focused {
            theme::title_style()
        } else {
            theme::help_style()
        })
        .border_style(border_style);

    let has_attachment = app.pending_attachment.is_some();

    if app.compose_input.is_empty() && !is_focused && !has_attachment {
        let placeholder =
            Paragraph::new(Span::styled("Type a message...", theme::help_style())).block(block);
        f.render_widget(placeholder, area);
        return;
    }

    // Render the block first
    f.render_widget(block, area);
    let inner = inner_rect(area);

    // Render attachment indicator if present (takes one line)
    let text_y_offset: u16 = if let Some((ref path, _)) = app.pending_attachment {
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let label = format!("[Attached: {}] (Alt+X: remove)", filename);
        let attach_area = Rect::new(inner.x, inner.y, inner.width, 1);
        let attach_line = Paragraph::new(Span::styled(label, theme::help_style()));
        f.render_widget(attach_line, attach_area);
        1
    } else {
        0
    };

    // Render message text below the attachment line
    let text_area = Rect::new(
        inner.x,
        inner.y + text_y_offset,
        inner.width,
        inner.height.saturating_sub(text_y_offset),
    );
    let text = &app.compose_input;
    app.compose_width = text_area.width;

    if is_focused {
        let (cx, cy) = cursor_position(text, app.compose_cursor, text_area.width as usize);
        let visible_rows = text_area.height as u16;

        // Adjust scroll so cursor stays visible
        if (cy as u16) < app.compose_scroll {
            app.compose_scroll = cy as u16;
        } else if (cy as u16) >= app.compose_scroll + visible_rows {
            app.compose_scroll = (cy as u16) - visible_rows + 1;
        }

        let paragraph = Paragraph::new(text.as_str())
            .wrap(Wrap { trim: false })
            .scroll((app.compose_scroll, 0));
        f.render_widget(paragraph, text_area);
        f.set_cursor_position((
            text_area.x + cx as u16,
            text_area.y + (cy as u16) - app.compose_scroll,
        ));
    } else {
        // When not focused, show from the top without scrolling
        let paragraph = Paragraph::new(text.as_str()).wrap(Wrap { trim: false });
        f.render_widget(paragraph, text_area);
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

/// Split text into segments separated by '\n', yielding (byte_offset, &str) pairs.
fn split_newlines(text: &str) -> Vec<(usize, &str)> {
    let mut result = Vec::new();
    let mut start = 0;
    for (i, ch) in text.char_indices() {
        if ch == '\n' {
            result.push((start, &text[start..i]));
            start = i + 1;
        }
    }
    result.push((start, &text[start..]));
    result
}

/// Compute wrapped lines the same way ratatui does with `Wrap { trim: false }`.
/// Returns a Vec of (byte_start, byte_end) for each visual line.
pub(crate) fn wrap_lines(text: &str, width: usize) -> Vec<(usize, usize)> {
    if width == 0 {
        return vec![(0, text.len())];
    }

    let mut lines: Vec<(usize, usize)> = Vec::new();
    for (seg_start, segment) in split_newlines(text) {
        if segment.is_empty() {
            lines.push((seg_start, seg_start));
            continue;
        }
        let mut line_start = seg_start;
        let mut col = 0usize;
        let mut last_break: Option<usize> = None;

        for (i, ch) in segment.char_indices() {
            let byte_pos = seg_start + i;
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);

            if ch == ' ' {
                last_break = Some(byte_pos + 1);
            }

            if col + cw > width && col > 0 {
                if let Some(brk) = last_break {
                    if brk > line_start {
                        lines.push((line_start, brk));
                        line_start = brk;
                        // Recompute col for chars already on the new line.
                        // When the break is past the current char (space triggered
                        // the wrap), there are no chars on the new line yet.
                        col = if line_start <= byte_pos {
                            text[line_start..byte_pos]
                                .chars()
                                .map(|c| {
                                    unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)
                                })
                                .sum()
                        } else {
                            0
                        };
                    } else {
                        lines.push((line_start, byte_pos));
                        line_start = byte_pos;
                        col = 0;
                    }
                } else {
                    lines.push((line_start, byte_pos));
                    line_start = byte_pos;
                    col = 0;
                }
                last_break = None;
            }

            col += cw;
        }
        lines.push((line_start, seg_start + segment.len()));
    }

    if lines.is_empty() {
        lines.push((0, 0));
    }
    lines
}

/// Calculate cursor (x, y) position within the compose area, accounting for word wrapping.
pub(crate) fn cursor_position(text: &str, byte_offset: usize, width: usize) -> (usize, usize) {
    if width == 0 {
        return (0, 0);
    }

    let lines = wrap_lines(text, width);
    // Find the last line whose start <= byte_offset.
    // For wrapped continuations (start == previous end), this picks the new line.
    // For newline gaps (start > previous end), the cursor on the \n stays on the earlier line.
    let mut best_y = 0;
    for (y, &(start, _)) in lines.iter().enumerate() {
        if start <= byte_offset {
            best_y = y;
        }
    }
    let (start, _) = lines[best_y];
    let x: usize = text[start..byte_offset.min(text.len())]
        .chars()
        .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
        .sum();
    (x, best_y)
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
        // Width 5, "abcdef" wraps after 5 chars (no spaces = char wrap)
        assert_eq!(cursor_position("abcdef", 5, 5), (0, 1));
        assert_eq!(cursor_position("abcdef", 6, 5), (1, 1));
    }

    #[test]
    fn test_cursor_position_word_wrap() {
        // "aaa bbb" with width 5: "aaa " fits (4 cols), "bb" would make 6 > 5
        // So it wraps at space: line1="aaa " (0..4), line2="bbb" (4..7)
        assert_eq!(cursor_position("aaa bbb", 4, 5), (0, 1)); // 'b' starts on line 2
        assert_eq!(cursor_position("aaa bbb", 7, 5), (3, 1)); // end of "bbb"
    }

    #[test]
    fn test_cursor_position_zero_width() {
        assert_eq!(cursor_position("hello", 3, 0), (0, 0));
    }

    #[test]
    fn test_wrap_lines_basic() {
        let lines = wrap_lines("hello", 40);
        assert_eq!(lines, vec![(0, 5)]);
    }

    #[test]
    fn test_wrap_lines_newline() {
        let lines = wrap_lines("hi\nworld", 40);
        assert_eq!(lines, vec![(0, 2), (3, 8)]);
    }

    #[test]
    fn test_wrap_lines_word_wrap() {
        // "aaa bbb" width 5: "aaa " = 4 cols, "bb" would be 6 > 5
        // word wrap at space: line1 = "aaa " (0..4), line2 = "bbb" (4..7)
        let lines = wrap_lines("aaa bbb", 5);
        assert_eq!(lines, vec![(0, 4), (4, 7)]);
    }

    #[test]
    fn test_wrap_lines_char_wrap_no_spaces() {
        let lines = wrap_lines("abcdefgh", 5);
        assert_eq!(lines, vec![(0, 5), (5, 8)]);
    }

    #[test]
    fn test_compose_renders_placeholder() {
        let mut app = App::new_test();
        let backend = ratatui::backend::TestBackend::new(40, 5);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &mut app, f.area());
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
                draw(f, &mut app, f.area());
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
                draw(f, &mut app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("Esc: back"));
    }
}
