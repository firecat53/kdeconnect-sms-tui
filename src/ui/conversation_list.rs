use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::{App, Focus, LoadingState};
use super::theme;

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
        .title_style(if is_active { theme::title_style() } else { theme::help_style() })
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

    let items: Vec<ListItem> = app
        .conversations
        .iter()
        .map(|conv| {
            let name = conv
                .display_name
                .as_deref()
                .or_else(|| {
                    app.config
                        .group_names
                        .get(&conv.thread_id.to_string())
                        .map(|s| s.as_str())
                })
                .or_else(|| {
                    conv.primary_address()
                        .and_then(|addr| app.contacts.lookup(addr))
                })
                .unwrap_or_else(|| conv.primary_address().unwrap_or("Unknown"));

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

            // Preview text — truncate on a char boundary to avoid panics
            // with multi-byte characters (emoji, accented chars, etc.)
            let preview = conv.preview_text();
            let max_chars = area.width.saturating_sub(4) as usize;
            let preview_truncated: String = if preview.chars().count() > max_chars {
                let truncated: String = preview.chars().take(max_chars.saturating_sub(3)).collect();
                format!("{}...", truncated)
            } else {
                preview.to_string()
            };

            let unread_marker = if is_unread { " *" } else { "" };

            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(name_display, name_style),
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
    state.select(app.selected_conversation_idx);
    f.render_stateful_widget(list, area, &mut state);
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
    let hours = ((secs % 86400) + 86400) % 86400 / 3600;
    let mins = ((secs % 3600) + 3600) % 3600 / 60;
    format!("{:02}:{:02}", hours, mins)
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
        assert_eq!(format_timestamp(now_millis - 7200_000), "2h");
        assert_eq!(format_timestamp(now_millis - 172800_000), "2d");
    }
}
