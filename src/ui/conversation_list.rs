use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::App;
use super::theme;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Conversations ")
        .title_style(theme::title_style());

    if app.conversations.is_empty() {
        let placeholder = ratatui::widgets::Paragraph::new("No conversations")
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
                    conv.primary_address()
                        .and_then(|addr| app.contacts.lookup(addr))
                })
                .unwrap_or_else(|| {
                    conv.primary_address().unwrap_or("Unknown")
                });

            let preview = conv.preview_text();
            let preview_truncated = if preview.len() > 30 {
                format!("{}...", &preview[..27])
            } else {
                preview.to_string()
            };

            ListItem::new(vec![
                Line::from(Span::styled(name.to_string(), theme::title_style())),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::models::conversation::Conversation;
    use crate::models::message::{Address, Message, MessageType};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

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
        assert!(content.contains("No conversations"));
    }

    #[test]
    fn test_conversation_list_with_items() {
        let mut app = App::new_test();
        let msg = Message {
            event: 0x1,
            body: "Hey there!".into(),
            addresses: vec![Address {
                address: "+15551234".into(),
            }],
            date: 1700000000000,
            message_type: MessageType::Inbox,
            read: false,
            thread_id: 1,
            uid: 1,
            sub_id: -1,
            attachments: vec![],
        };
        app.conversations.push(Conversation {
            thread_id: 1,
            latest_message: Some(msg),
            messages: vec![],
            is_group: false,
            display_name: None,
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
}
