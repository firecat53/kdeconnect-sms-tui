use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;

use super::{sanitize_for_terminal, theme};
use crate::app::{App, Focus, LoadingState};

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let title = match app.loading {
        LoadingState::Loading => " Conversations (loading...) ",
        _ => " Conversations ",
    };

    let is_active = app.focus == Focus::ConversationList;
    let border_style = if is_active {
        theme::active_border()
    } else {
        theme::inactive_border()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_style(if is_active {
            theme::title_style()
        } else {
            theme::help_style()
        })
        .border_style(border_style);

    if app.conversations.is_empty() {
        let msg = match &app.loading {
            LoadingState::Loading => "Loading...",
            LoadingState::Error(e) => e.as_str(),
            LoadingState::Idle => {
                if app.selected_device().is_some_and(|d| d.is_available()) {
                    "No conversations (press r to refresh)"
                } else {
                    "No device connected"
                }
            }
        };
        let placeholder = ratatui::widgets::Paragraph::new(msg)
            .style(theme::help_style())
            .block(block);
        f.render_widget(placeholder, area);
        return;
    }

    // Build (filtered_index, original_index, conversation) tuples,
    // skipping archived/spam conversations.
    let visible: Vec<(usize, &crate::models::conversation::Conversation)> = app
        .conversations
        .iter()
        .enumerate()
        .filter(|(_, conv)| !app.state.is_hidden(conv.thread_id))
        .collect();

    let items: Vec<ListItem> = visible
        .iter()
        .map(|(_, conv)| {
            let name: String = if let Some(n) = conv.display_name.as_deref() {
                n.to_string()
            } else if let Some(n) = app.state.group_names.get(&conv.thread_id.to_string()) {
                n.clone()
            } else if conv.is_group {
                app.generate_group_initials(conv)
            } else {
                let addr = conv.primary_address().unwrap_or("Unknown");
                app.contacts
                    .lookup(addr)
                    .unwrap_or_else(|| addr.to_string())
            };

            let is_unread = conv
                .latest_message
                .as_ref()
                .is_some_and(|m| !m.read && m.is_incoming());

            let name_style = if is_unread {
                theme::title_style().add_modifier(Modifier::BOLD)
            } else {
                theme::title_style()
            };

            // Group indicator
            let name_display = if conv.is_group {
                format!("[G] {}", name)
            } else {
                name.to_string()
            };

            // Timestamp
            let time_str = conv
                .latest_message
                .as_ref()
                .map(|m| format_timestamp(m.date))
                .unwrap_or_default();

            // Preview text — sanitize emoji sequences then truncate by
            // display width to handle wide chars (emoji, CJK).
            let preview = sanitize_for_terminal(conv.preview_text());
            let max_width = area.width.saturating_sub(4) as usize;
            let preview_truncated = truncate_to_width(&preview, max_width);

            let unread_marker = if is_unread { " *" } else { "" };

            // Truncate the name line too (name + marker + time could overflow)
            let name_sanitized = sanitize_for_terminal(&name_display);
            let time_width = time_str.chars().map(safe_char_width).sum::<usize>();
            let marker_width = unread_marker.len(); // ASCII only
            let name_budget = max_width.saturating_sub(time_width + marker_width + 1);
            let name_truncated = truncate_to_width(&name_sanitized, name_budget);

            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(name_truncated, name_style),
                    Span::styled(unread_marker.to_string(), theme::status_available()),
                    Span::raw(" "),
                    Span::styled(time_str, theme::timestamp_style()),
                ]),
                Line::from(Span::styled(preview_truncated, theme::help_style())),
            ])
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected_style());

    let mut state = ListState::default();
    // Map the original selected index to the filtered list position.
    let filtered_idx = app
        .selected_conversation_idx
        .and_then(|sel| visible.iter().position(|(orig_idx, _)| *orig_idx == sel));
    state.select(filtered_idx);
    f.render_stateful_widget(list, area, &mut state);
}

/// Terminal-safe character width.  `unicode-width` follows UAX #11 which
/// classifies many emoji as Neutral (width 1), but modern terminals render
/// them as 2 columns.  This function bumps such characters to width 2.
fn safe_char_width(ch: char) -> usize {
    let w = ch.width().unwrap_or(0);
    if w == 0 {
        return 0;
    }
    // Characters that unicode-width reports as 1 but terminals render as 2:
    // emoji and symbol characters in ranges above U+2000.  We intentionally
    // exclude General Punctuation (U+2000–U+206F) and similar text-like
    // ranges, targeting symbol/emoji blocks.
    if w == 1 {
        let cp = ch as u32;
        if matches!(cp,
            0x00A9 | 0x00AE |                     // ©, ®
            0x203C | 0x2049 |                     // ‼, ⁉
            0x2122 | 0x2139 |                     // ™, ℹ
            0x2194..=0x2199 |                     // ↔–↙
            0x21A9..=0x21AA |                     // ↩, ↪
            0x231A..=0x231B |                     // ⌚, ⌛
            0x2328 | 0x23CF |                     // ⌨, ⏏
            0x23E9..=0x23F3 |                     // ⏩–⏳
            0x23F8..=0x23FA |                     // ⏸–⏺
            0x25AA..=0x25AB |                     // ▪, ▫
            0x25B6 | 0x25C0 |                     // ▶, ◀
            0x25FB..=0x25FE |                     // ◻–◾
            0x2600..=0x27BF |                     // ☀–➿ (Misc Symbols, Dingbats)
            0x2934..=0x2935 |                     // ⤴, ⤵
            0x2B05..=0x2B07 |                     // ⬅–⬇
            0x2B1B..=0x2B1C |                     // ⬛, ⬜
            0x2B50 | 0x2B55 |                     // ⭐, ⭕
            0x3030 | 0x303D | 0x3297 | 0x3299 |  // 〰, 〽, ㊗, ㊙
            0xFE0F                                 // Variation Selector 16 (emoji pres.)
        ) {
            return 2;
        }
    }
    w
}

/// Truncate a string to fit within `max_width` display columns,
/// accounting for wide characters (emoji, CJK). Appends "..." if truncated.
/// Strips newlines and control characters from the output.
fn truncate_to_width(s: &str, max_width: usize) -> String {
    let mut width = 0usize;
    let mut result = String::new();
    let ellipsis_width = 3; // "..."

    for ch in s.chars() {
        // Replace newlines/control chars with space for single-line preview.
        let ch = if ch.is_control() { ' ' } else { ch };

        let cw = safe_char_width(ch);
        if width + cw > max_width {
            // Won't fit — truncate with ellipsis if there's room
            if max_width >= ellipsis_width {
                // Trim back to make room for "..."
                while !result.is_empty() {
                    let last_w = result.chars().next_back().map(safe_char_width).unwrap_or(0);
                    if width + ellipsis_width <= max_width {
                        break;
                    }
                    result.pop();
                    width -= last_w;
                }
                result.push_str("...");
            }
            return result;
        }
        width += cw;
        result.push(ch);
    }
    result
}

/// Format a Unix millisecond timestamp into a human-readable relative time.
fn format_timestamp(millis: i64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let diff_secs = (now - millis) / 1000;

    if diff_secs < 0 {
        return format_time_of_day(millis);
    }

    if diff_secs < 60 {
        return "now".into();
    }
    if diff_secs < 3600 {
        return format!("{}m", diff_secs / 60);
    }
    if diff_secs < 86400 {
        return format!("{}h", diff_secs / 3600);
    }
    if diff_secs < 604800 {
        return format!("{}d", diff_secs / 86400);
    }

    // Older than a week: show date-ish
    format_time_of_day(millis)
}

/// Simple time-of-day display from epoch millis.
fn format_time_of_day(millis: i64) -> String {
    let secs = millis / 1000;
    let time_t = secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&time_t, &mut tm);
    }
    format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::models::conversation::Conversation;
    use crate::models::message::{Address, Message, MessageType};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn make_msg(body: &str, read: bool, incoming: bool) -> Message {
        Message {
            event: 0x1,
            body: body.into(),
            addresses: vec![Address {
                address: "+15551234".into(),
            }],
            date: 1700000000000,
            message_type: if incoming {
                MessageType::Inbox
            } else {
                MessageType::Sent
            },
            read,
            thread_id: 1,
            uid: 1,
            sub_id: -1,
            attachments: vec![],
        }
    }

    #[test]
    fn test_conversation_list_empty() {
        let app = App::new_test();
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("No device connected"));
    }

    #[test]
    fn test_conversation_list_with_items() {
        let mut app = App::new_test();
        app.conversations.push(Conversation {
            thread_id: 1,
            latest_message: Some(make_msg("Hey there!", false, true)),
            messages: vec![],
            is_group: false,
            display_name: None,
            messages_requested: 0,
            total_messages: None,
            loading_more_messages: false,
            loading_started_tick: None,
        });

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("Hey there!"));
    }

    #[test]
    fn test_group_indicator() {
        let mut app = App::new_test();
        app.conversations.push(Conversation {
            thread_id: 1,
            latest_message: Some(make_msg("group msg", true, true)),
            messages: vec![],
            is_group: true,
            display_name: Some("Family".into()),
            messages_requested: 0,
            total_messages: None,
            loading_more_messages: false,
            loading_started_tick: None,
        });

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("[G] Family"));
    }

    #[test]
    fn test_format_timestamp_recent() {
        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        assert_eq!(format_timestamp(now_millis), "now");
        assert_eq!(format_timestamp(now_millis - 120_000), "2m");
        assert_eq!(format_timestamp(now_millis - 7_200_000), "2h");
        assert_eq!(format_timestamp(now_millis - 172_800_000), "2d");
    }

    #[test]
    fn test_truncate_to_width_ascii() {
        assert_eq!(truncate_to_width("hello", 10), "hello");
        assert_eq!(truncate_to_width("hello world!", 8), "hello...");
    }

    #[test]
    fn test_truncate_to_width_emoji() {
        // 😀 is a single codepoint, 2 columns wide
        let s = "Which is fine \u{1F600}";
        // "Which is fine " = 14 cols, emoji = 2, total = 16
        assert_eq!(truncate_to_width(s, 16), s);
        // In width 15, emoji doesn't fit, truncate
        let truncated = truncate_to_width(s, 15);
        assert!(!truncated.contains('\u{1F600}'));
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_truncate_to_width_all_emoji() {
        // 5 emoji × 2 cols = 10 cols
        let s = "\u{1F600}\u{1F601}\u{1F602}\u{1F603}\u{1F604}";
        assert_eq!(truncate_to_width(s, 10), s);
        // Only room for 3 emoji (6 cols) + "..." (3 cols) = 9 cols in width 9
        let truncated = truncate_to_width(s, 9);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_safe_char_width_emoji_symbols() {
        // These emoji symbols are often width 1 in unicode-width but 2 in terminals.
        // safe_char_width should return 2 for them.
        assert_eq!(safe_char_width('❤'), 2); // U+2764 (Heavy Black Heart)
        assert_eq!(safe_char_width('✅'), 2); // U+2705
        assert_eq!(safe_char_width('⭐'), 2); // U+2B50
        assert_eq!(safe_char_width('☀'), 2); // U+2600
                                             // Regular ASCII should be 1
        assert_eq!(safe_char_width('A'), 1);
        assert_eq!(safe_char_width(' '), 1);
    }

    #[test]
    fn test_truncate_newlines() {
        // Newlines in preview text should be replaced with spaces
        let s = "Hello\nworld";
        let truncated = truncate_to_width(s, 20);
        assert!(!truncated.contains('\n'));
        assert!(truncated.contains("Hello world"));
    }

    #[test]
    fn test_sanitize_zjw_emoji() {
        // 🤷‍♂️ = U+1F937 U+200D U+2642 U+FE0F — should collapse to base emoji
        let zjw = "\u{1F937}\u{200D}\u{2642}\u{FE0F}";
        let sanitized = sanitize_for_terminal(zjw);
        assert_eq!(sanitized, "\u{1F937}");

        // VS16 only (e.g. ❤️ = U+2764 U+FE0F) — strip VS16
        let vs16 = "\u{2764}\u{FE0F}";
        let sanitized = sanitize_for_terminal(vs16);
        assert_eq!(sanitized, "\u{2764}");

        // Plain text is unchanged
        assert_eq!(sanitize_for_terminal("Hello world"), "Hello world");

        // Simple emoji (no ZWJ/VS16) is unchanged
        assert_eq!(sanitize_for_terminal("\u{1F600}"), "\u{1F600}");
    }
}
