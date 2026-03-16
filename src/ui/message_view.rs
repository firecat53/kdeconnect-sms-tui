use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use super::theme;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Messages ")
        .title_style(theme::title_style());

    // No conversation selected
    let selected = app.selected_conversation_idx.and_then(|i| app.conversations.get(i));

    if selected.is_none() {
        let placeholder = Paragraph::new("Select a conversation")
            .style(theme::help_style())
            .block(block);
        f.render_widget(placeholder, area);
        return;
    }

    let conv = selected.unwrap();

    if conv.messages.is_empty() {
        let hint = if conv.latest_message.is_some() {
            "Loading messages... (press Enter to focus compose)"
        } else {
            "No messages loaded"
        };
        let placeholder = Paragraph::new(hint)
            .style(theme::help_style())
            .block(block);
        f.render_widget(placeholder, area);
        return;
    }

    let mut lines = Vec::new();

    for msg in &conv.messages {
        let sender = if msg.is_incoming() {
            let addr = msg.addresses.first().map(|a| a.address.as_str()).unwrap_or("?");
            app.contacts.display_name(addr)
        } else {
            "You".to_string()
        };

        let style = if msg.is_incoming() {
            theme::incoming_message()
        } else {
            theme::outgoing_message()
        };

        let time = msg.timestamp_display();
        lines.push(Line::from(vec![
            Span::styled(format!("[{}] ", time), theme::timestamp_style()),
            Span::styled(format!("{}: ", sender), style),
            Span::raw(&msg.body),
        ]));

        if msg.has_attachments() {
            for att in &msg.attachments {
                let label = if att.is_image() {
                    format!("  [Image: {}]", att.mime_type)
                } else {
                    format!("  [Attachment: {}]", att.mime_type)
                };
                lines.push(Line::from(Span::styled(label, theme::help_style())));
            }
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.message_scroll, 0));

    f.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::models::conversation::Conversation;
    use crate::models::message::{Address, Message, MessageType};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn make_msg(body: &str, incoming: bool) -> Message {
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
            read: true,
            thread_id: 1,
            uid: 1,
            sub_id: -1,
            attachments: vec![],
        }
    }

    #[test]
    fn test_message_view_no_selection() {
        let app = App::new_test();
        let backend = TestBackend::new(50, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("Select a conversation"));
    }

    #[test]
    fn test_message_view_with_messages() {
        let mut app = App::new_test();
        app.conversations.push(Conversation {
            thread_id: 1,
            latest_message: Some(make_msg("Hey!", true)),
            messages: vec![
                make_msg("Hey!", true),
                make_msg("Hi there!", false),
            ],
            is_group: false,
            display_name: None,
        });
        app.selected_conversation_idx = Some(0);

        let backend = TestBackend::new(60, 15);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("Hey!"));
        assert!(content.contains("Hi there!"));
        assert!(content.contains("You"));
    }

    #[test]
    fn test_message_view_empty_messages_with_latest() {
        let mut app = App::new_test();
        app.conversations.push(Conversation {
            thread_id: 1,
            latest_message: Some(make_msg("preview", true)),
            messages: vec![], // messages not yet loaded
            is_group: false,
            display_name: None,
        });
        app.selected_conversation_idx = Some(0);

        let backend = TestBackend::new(60, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("Loading messages"));
    }
}
