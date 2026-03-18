use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use ratatui_image::StatefulImage;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, ImageState};
use super::theme;

/// Maximum height (in terminal rows) for an inline image.
const IMAGE_MAX_ROWS: u16 = 12;

/// A render element in the message view: either text lines or an image.
enum RenderItem {
    Text(Vec<Line<'static>>),
    Image {
        uid: String,
        height: u16,
    },
    ImagePlaceholder(Line<'static>),
}

impl RenderItem {
    fn height(&self, width: u16) -> u16 {
        match self {
            RenderItem::Text(lines) => {
                if width == 0 {
                    return lines.len() as u16;
                }
                // Estimate wrapped height using display width (not byte length)
                // so that emoji and wide characters are measured correctly.
                lines.iter().map(|line| {
                    let w: usize = line.spans.iter().map(|s| s.content.width()).sum();
                    1.max(w.div_ceil(width as usize)) as u16
                }).sum()
            }
            RenderItem::Image { height, .. } => *height,
            RenderItem::ImagePlaceholder(_) => 1,
        }
    }
}

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
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

    // Build render items from messages
    let mut items: Vec<RenderItem> = Vec::new();

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
        let mut text_lines = vec![Line::from(vec![
            Span::styled(format!("[{}] ", time), theme::timestamp_style()),
            Span::styled(format!("{}: ", sender), style),
            Span::raw(msg.body.clone()),
        ])];

        // Add non-image attachment labels to text
        for att in &msg.attachments {
            if !att.is_image() {
                let label = format!("  [Attachment: {}]", att.mime_type);
                text_lines.push(Line::from(Span::styled(label, theme::help_style())));
            }
        }

        if !text_lines.is_empty() {
            items.push(RenderItem::Text(text_lines));
        }

        // Add image attachments
        for att in &msg.attachments {
            if att.is_image() {
                match app.image_states.get(&att.unique_identifier) {
                    Some(ImageState::Loaded(_)) => {
                        items.push(RenderItem::Image {
                            uid: att.unique_identifier.clone(),
                            height: IMAGE_MAX_ROWS,
                        });
                    }
                    Some(ImageState::Downloading) => {
                        items.push(RenderItem::ImagePlaceholder(
                            Line::from(Span::styled(
                                format!("  [Downloading {}...]", att.mime_type),
                                theme::help_style(),
                            ))
                        ));
                    }
                    Some(ImageState::Failed(reason)) => {
                        items.push(RenderItem::ImagePlaceholder(
                            Line::from(Span::styled(
                                format!("  [Image failed: {}]", reason),
                                theme::help_style(),
                            ))
                        ));
                    }
                    None => {
                        items.push(RenderItem::ImagePlaceholder(
                            Line::from(Span::styled(
                                format!("  [Image: {}]", att.mime_type),
                                theme::help_style(),
                            ))
                        ));
                    }
                }
            }
        }
    }

    // Render the block border first, then render content inside
    let inner = block.inner(area);
    f.render_widget(block, area);

    let inner_width = inner.width;
    let inner_height = inner.height;

    // Calculate total content height
    let total_height: u16 = items.iter().map(|item| item.height(inner_width)).sum();

    // message_scroll is an offset FROM the bottom (0 = newest visible)
    let max_scroll = total_height.saturating_sub(inner_height);
    let scroll_offset = max_scroll.saturating_sub(app.message_scroll) as i32;

    // Render visible items
    let mut y: i32 = -(scroll_offset as i32);

    for item in &items {
        let item_height = item.height(inner_width);

        // Check if item is visible (even partially)
        let item_top = y;
        let item_bottom = y + item_height as i32;

        if item_bottom <= 0 {
            // Entirely above the visible area
            y += item_height as i32;
            continue;
        }
        if item_top >= inner_height as i32 {
            // Entirely below the visible area
            break;
        }

        // Clamp to visible area
        let render_y = item_top.max(0) as u16;
        let available = inner_height.saturating_sub(render_y);

        match item {
            RenderItem::Text(lines) => {
                let skip = if item_top < 0 { (-item_top) as u16 } else { 0 };
                let visible_height = item_height.saturating_sub(skip).min(available);
                if visible_height > 0 {
                    let text_area = Rect {
                        x: inner.x,
                        y: inner.y + render_y,
                        width: inner_width,
                        height: visible_height,
                    };
                    let paragraph = Paragraph::new(lines.clone())
                        .wrap(Wrap { trim: false })
                        .scroll((skip, 0));
                    f.render_widget(paragraph, text_area);
                }
            }
            RenderItem::Image { uid, height } => {
                // Only render images when fully visible.  When partially
                // clipped at the viewport edge, `available` changes each
                // frame, which causes StatefulProtocol to re-encode and
                // produces visible flickering.
                if item_top >= 0 && *height <= available {
                    let img_area = Rect {
                        x: inner.x,
                        y: inner.y + render_y,
                        width: inner_width.min(40),
                        height: *height,
                    };
                    if let Some(ImageState::Loaded(protocol)) = app.image_states.get_mut(uid) {
                        let image_widget = StatefulImage::<ratatui_image::protocol::StatefulProtocol>::default();
                        f.render_stateful_widget(image_widget, img_area, protocol.as_mut());
                    }
                }
            }
            RenderItem::ImagePlaceholder(line) => {
                if item_top >= 0 {
                    let text_area = Rect {
                        x: inner.x,
                        y: inner.y + render_y,
                        width: inner_width,
                        height: 1.min(available),
                    };
                    let paragraph = Paragraph::new(line.clone());
                    f.render_widget(paragraph, text_area);
                }
            }
        }

        y += item_height as i32;
    }
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
        let mut app = App::new_test();
        let backend = TestBackend::new(50, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &mut app, f.area());
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
                draw(f, &mut app, f.area());
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
                draw(f, &mut app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("Loading messages"));
    }
}
