use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use ratatui_image::StatefulImage;
use unicode_width::UnicodeWidthChar;

use crate::app::{App, Focus, ImageState};
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
                lines.iter().map(|line| wrapped_line_height(line, width as usize)).sum()
            }
            RenderItem::Image { height, .. } => *height,
            RenderItem::ImagePlaceholder(_) => 1,
        }
    }
}

/// Calculate the number of terminal rows a Line occupies when word-wrapped
/// to `width` columns, matching ratatui's wrapping behaviour.  A wide
/// character (e.g. emoji, CJK) that would start in the last column is
/// wrapped to the next row.
fn wrapped_line_height(line: &Line<'_>, width: usize) -> u16 {
    let mut rows: u16 = 1;
    let mut col: usize = 0;
    for span in &line.spans {
        for ch in span.content.chars() {
            let cw = ch.width().unwrap_or(0);
            if cw == 0 {
                continue;
            }
            if col + cw > width {
                // Character doesn't fit on this row – wrap.
                rows += 1;
                col = cw;
            } else {
                col += cw;
                if col == width {
                    // Exactly filled the row.  The *next* character (if any)
                    // will start a new row; but we don't bump rows here
                    // because ratatui only creates a new row when there is
                    // actually a character to place.
                    //
                    // Reset col so the next char starts a fresh row.
                    col = 0;
                    rows += 1;
                }
            }
        }
    }
    // If we just bumped rows at the exact boundary but there were no more
    // characters after it, we over-counted by 1.
    if col == 0 && rows > 1 {
        rows -= 1;
    }
    rows.max(1)
}

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let is_active = app.focus == Focus::MessageView;
    let border_style = if is_active {
        theme::active_border()
    } else {
        theme::inactive_border()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Messages ")
        .title_style(if is_active { theme::title_style() } else { theme::help_style() })
        .border_style(border_style);

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
    app.message_view_height = inner_height;

    // Calculate total content height and message boundaries.
    // We group render items back into per-message groups so that
    // message-by-message scrolling snaps to message boundaries.
    // Each message produces a Text item + optional attachment items.
    // We track the cumulative height from the bottom for each message boundary.
    let item_heights: Vec<u16> = items.iter().map(|item| item.height(inner_width)).collect();
    let total_height: u16 = item_heights.iter().sum();

    // Build message boundaries: cumulative height offsets from bottom.
    // Each message in conv.messages produced a contiguous block of render items.
    // Recompute which items belong to each message.
    {
        let mut boundaries = Vec::new();
        let mut item_idx = 0;
        let mut cumulative_from_bottom: u16 = 0;

        // Walk messages in order (oldest first), items correspond 1:1 with message groups
        for msg in &conv.messages {
            // Each message produces: 1 Text item + N image/placeholder items (one per image attachment)
            let mut msg_height: u16 = 0;

            // Text item
            if item_idx < item_heights.len() {
                msg_height += item_heights[item_idx];
                item_idx += 1;
            }

            // Image attachment items
            let image_count = msg.attachments.iter().filter(|a| a.is_image()).count();
            for _ in 0..image_count {
                if item_idx < item_heights.len() {
                    msg_height += item_heights[item_idx];
                    item_idx += 1;
                }
            }

            cumulative_from_bottom += msg_height;
            // The boundary represents the scroll offset where the top of this
            // message would be at the bottom of the viewport.
            // We store: total_height - position_of_message_top = distance from bottom
            boundaries.push(total_height.saturating_sub(cumulative_from_bottom.saturating_sub(msg_height)));
        }

        // Convert to offsets from bottom (for scroll comparison).
        // boundaries[i] = how far from the bottom the top of message i is.
        // For scrolling, we want the offset where the bottom of a message aligns
        // with the bottom of viewport. That's: boundary - inner_height (but >= 0).
        let mut scroll_boundaries: Vec<u16> = Vec::new();
        let mut cum = 0u16;
        // Walk from newest (last) to oldest (first) message
        for msg_idx in (0..conv.messages.len()).rev() {
            // Calculate this message's height from its render items
            let mut msg_item_start = 0usize;
            for i in 0..msg_idx {
                msg_item_start += 1; // text
                msg_item_start += conv.messages[i].attachments.iter().filter(|a| a.is_image()).count();
            }
            let mut msg_h = 0u16;
            let items_for_msg = 1 + conv.messages[msg_idx].attachments.iter().filter(|a| a.is_image()).count();
            for j in 0..items_for_msg {
                if msg_item_start + j < item_heights.len() {
                    msg_h += item_heights[msg_item_start + j];
                }
            }
            cum += msg_h;
            // Scroll offset to have the top of this message visible at viewport bottom
            if cum > inner_height {
                scroll_boundaries.push(cum.saturating_sub(inner_height));
            }
        }
        scroll_boundaries.sort();
        scroll_boundaries.dedup();
        app.message_boundaries = scroll_boundaries;
    }

    // message_scroll is an offset FROM the bottom (0 = newest visible)
    let max_scroll = total_height.saturating_sub(inner_height);
    // Clamp so the user can't scroll past the oldest message.
    app.message_scroll = app.message_scroll.min(max_scroll);
    app.message_max_scroll = max_scroll;
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
                // Only render when fully visible.  Protocol-based images
                // (Kitty, Sixel) use escape sequences positioned by the
                // terminal — they cannot be clipped by ratatui's Buffer,
                // so we must not extend the Rect beyond the viewport.
                let fits = item_top >= 0 && *height <= available;
                if fits {
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
            messages_requested: 0,
            total_messages: None,
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
            messages_requested: 0,
            total_messages: None,
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

    #[test]
    fn test_wrapped_line_height_ascii() {
        // 10 chars in 10 cols = 1 row
        let line = Line::from("0123456789");
        assert_eq!(wrapped_line_height(&line, 10), 1);
        // 11 chars in 10 cols = 2 rows
        let line = Line::from("01234567890");
        assert_eq!(wrapped_line_height(&line, 10), 2);
    }

    #[test]
    fn test_wrapped_line_height_emoji_at_boundary() {
        // "12345678" is 8 cols, then a 2-wide emoji fits exactly at cols 9-10
        let line = Line::from("12345678\u{1F600}"); // 😀 is 2 cols wide
        assert_eq!(wrapped_line_height(&line, 10), 1);

        // "123456789" is 9 cols, emoji needs 2 cols but only 1 remains → wraps
        let line = Line::from("123456789\u{1F600}");
        assert_eq!(wrapped_line_height(&line, 10), 2);
    }

    #[test]
    fn test_wrapped_line_height_multiple_emoji() {
        // 5 emoji × 2 cols each = 10 cols = 1 row in width 10
        let line = Line::from("\u{1F600}\u{1F601}\u{1F602}\u{1F603}\u{1F604}");
        assert_eq!(wrapped_line_height(&line, 10), 1);

        // 6 emoji × 2 cols = 12 cols → 2 rows in width 10
        let line = Line::from("\u{1F600}\u{1F601}\u{1F602}\u{1F603}\u{1F604}\u{1F605}");
        assert_eq!(wrapped_line_height(&line, 10), 2);
    }
}
