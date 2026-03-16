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
        let placeholder = Paragraph::new("No messages loaded")
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
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

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
}
