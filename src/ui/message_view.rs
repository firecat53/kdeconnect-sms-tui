use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use ratatui_image::StatefulImage;
use unicode_width::UnicodeWidthChar;

use crate::app::{App, Focus, ImageState};
use super::{sanitize_for_terminal, theme};

/// Maximum height (in terminal rows) for an inline image.
const IMAGE_MAX_ROWS: u16 = 12;

/// Braille spinner frames (each frame is one character).
const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// A render element in the message view: either text lines or an image.
enum RenderItem {
    Text(Vec<Line<'static>>),
    Image {
        uid: String,
        height: u16,
    },
    ImagePlaceholder(Line<'static>),
    /// A date separator line (not counted as a message for scroll boundaries).
    DateSeparator(Line<'static>),
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
            RenderItem::DateSeparator(_) => 1,
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

    // Show animated spinner at the top when loading older messages
    if conv.loading_more_messages {
        let frame = SPINNER_FRAMES[app.tick_count as usize % SPINNER_FRAMES.len()];
        items.push(RenderItem::Text(vec![Line::from(Span::styled(
            format!(" {} Loading older messages...", frame),
            theme::help_style(),
        ))]));
    }

    let mut prev_date: Option<String> = None;

    for msg in &conv.messages {
        // Insert a date separator when the date changes between messages.
        let msg_date = msg.date_display();
        if prev_date.as_ref() != Some(&msg_date) {
            items.push(RenderItem::DateSeparator(Line::from(Span::styled(
                format!("── {} ──", msg_date),
                theme::timestamp_style(),
            ))));
            prev_date = Some(msg_date);
        }

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
        let body = sanitize_for_terminal(&msg.body);
        let mut text_lines = vec![Line::from(vec![
            Span::styled(format!("[{}] ", time), theme::timestamp_style()),
            Span::styled(format!("{}: ", sender), style),
            Span::raw(body),
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

    // Calculate per-item heights and per-message heights.
    let item_heights: Vec<u16> = items.iter().map(|item| item.height(inner_width)).collect();
    let total_height: u16 = item_heights.iter().sum();

    // Compute per-message heights by grouping render items back to messages.
    // Each message produces: optional DateSeparator + 1 Text item + N image/placeholder items.
    // Date separators are folded into the following message's height (they
    // don't count as a separate message for scroll-boundary purposes).
    // Skip the spinner item at the front if present.
    let mut msg_heights: Vec<u16> = Vec::with_capacity(conv.messages.len());
    {
        let mut item_idx = if conv.loading_more_messages { 1usize } else { 0usize };
        for msg in &conv.messages {
            let mut h: u16 = 0;
            // Date separator (if present before this message)
            if item_idx < items.len() && matches!(&items[item_idx], RenderItem::DateSeparator(_)) {
                h += item_heights[item_idx];
                item_idx += 1;
            }
            // Text item
            if item_idx < item_heights.len() {
                h += item_heights[item_idx];
                item_idx += 1;
            }
            // Image attachment items
            for _ in msg.attachments.iter().filter(|a| a.is_image()) {
                if item_idx < item_heights.len() {
                    h += item_heights[item_idx];
                    item_idx += 1;
                }
            }
            msg_heights.push(h);
        }
    }

    // message_scroll is an offset FROM the bottom (0 = newest visible)
    let max_scroll = total_height.saturating_sub(inner_height);
    app.message_scroll = app.message_scroll.min(max_scroll);
    app.message_max_scroll = max_scroll;

    // Build message_boundaries: sorted ascending scroll offsets that snap
    // to message-by-message positions.
    //
    // message_scroll is an offset FROM the bottom: 0 = newest visible,
    // increasing values scroll toward older messages.  Each boundary is the
    // cumulative height of the N newest messages — scrolling to that value
    // hides those N messages below the viewport.
    {
        let mut scroll_boundaries: Vec<u16> = Vec::new();
        let mut cum: u16 = 0;
        for &mh in msg_heights.iter().rev() {
            cum += mh;
            if cum > 0 && cum <= max_scroll {
                scroll_boundaries.push(cum);
            }
        }
        // Ensure max_scroll is reachable as the final boundary.
        if max_scroll > 0 && scroll_boundaries.last() != Some(&max_scroll) {
            scroll_boundaries.push(max_scroll);
        }
        // Already sorted (cum is monotonically increasing), but dedup for safety.
        scroll_boundaries.dedup();
        app.message_boundaries = scroll_boundaries;
    }

    // Compute the pixel offset for rendering.
    // When content is shorter than viewport, bottom-align it.
    let content_start: i32 = if total_height < inner_height {
        (inner_height - total_height) as i32
    } else {
        let scroll_offset = max_scroll.saturating_sub(app.message_scroll);
        -(scroll_offset as i32)
    };

    // Render visible items
    let mut y: i32 = content_start;

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
            RenderItem::ImagePlaceholder(line) | RenderItem::DateSeparator(line) => {
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
            loading_more_messages: false,
            loading_started_tick: None,
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
            loading_more_messages: false,
            loading_started_tick: None,
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
    fn test_message_view_bottom_aligned() {
        // With a tall viewport and few messages, content should be at the bottom.
        let mut app = App::new_test();
        app.conversations.push(Conversation {
            thread_id: 1,
            latest_message: Some(make_msg("Only msg", true)),
            messages: vec![make_msg("Only msg", true)],
            is_group: false,
            display_name: None,
            messages_requested: 0,
            total_messages: None,
            loading_more_messages: false,
            loading_started_tick: None,
        });
        app.selected_conversation_idx = Some(0);

        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &mut app, f.area());
            })
            .unwrap();

        // The message should appear near the bottom, not at the top.
        // Check that the last row(s) contain the message text.
        let buf = terminal.backend().buffer();
        let mut last_row_with_msg = 0;
        for y in 0..buf.area.height {
            let mut row = String::new();
            for x in 0..buf.area.width {
                if let Some(cell) = buf.cell((x, y)) {
                    row.push_str(cell.symbol());
                }
            }
            if row.contains("Only msg") {
                last_row_with_msg = y;
            }
        }
        // Message should be in the bottom half of the viewport (row 10+)
        assert!(last_row_with_msg >= 10, "Message at row {} should be near bottom", last_row_with_msg);
    }

    #[test]
    fn test_message_boundaries_computed() {
        // Verify that message boundaries are set during render.
        let mut app = App::new_test();
        app.conversations.push(Conversation {
            thread_id: 1,
            latest_message: Some(make_msg("msg3", true)),
            messages: vec![
                make_msg("msg1", true),
                make_msg("msg2", false),
                make_msg("msg3", true),
            ],
            is_group: false,
            display_name: None,
            messages_requested: 0,
            total_messages: None,
            loading_more_messages: false,
            loading_started_tick: None,
        });
        app.selected_conversation_idx = Some(0);

        // Use a small viewport so messages exceed it
        let backend = TestBackend::new(40, 5);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &mut app, f.area());
            })
            .unwrap();

        // With 3 messages in a 3-row inner area (5 - 2 border),
        // boundaries should be non-empty since messages likely exceed viewport.
        // At minimum, message_max_scroll should reflect that content overflows.
        assert!(app.message_max_scroll > 0 || app.message_boundaries.is_empty(),
            "scroll state should be coherent");

        // Boundaries should be cumulative heights from the newest message.
        // Each boundary hides N messages below the viewport.
        if !app.message_boundaries.is_empty() {
            // Boundaries must be sorted ascending
            for w in app.message_boundaries.windows(2) {
                assert!(w[0] < w[1], "boundaries must be sorted: {:?}", app.message_boundaries);
            }
            // First boundary > 0 (hides at least the newest message)
            assert!(app.message_boundaries[0] > 0);
            // Last boundary <= max_scroll
            assert!(*app.message_boundaries.last().unwrap() <= app.message_max_scroll);
        }
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
