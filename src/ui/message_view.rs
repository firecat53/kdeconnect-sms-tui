use ratatui::layout::{Alignment, Rect};
use ratatui::style::Color;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use ratatui_image::StatefulImage;
use unicode_width::UnicodeWidthChar;

use super::{highlight_matches, sanitize_for_terminal, split_at_cursor, theme};
use crate::app::{App, Focus, ImageState};

/// Maximum height (in terminal rows) for an inline image.
const IMAGE_MAX_ROWS: u16 = 12;

/// Braille spinner frames (each frame is one character).
const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Identity of a selectable item: (message_index, part).
/// part 0 = text body, part 1+ = attachment index + 1.
type SelectionId = (usize, usize);

/// A render element in the message view.
enum RenderItem {
    Text {
        lines: Vec<Line<'static>>,
        sel: SelectionId,
    },
    Image {
        uid: String,
        height: u16,
        sel: SelectionId,
    },
    ImagePlaceholder {
        line: Line<'static>,
        sel: SelectionId,
    },
    /// Non-image attachment label.
    AttachmentLabel {
        line: Line<'static>,
        sel: SelectionId,
    },
    /// A date separator line (not selectable).
    DateSeparator(Line<'static>),
}

impl RenderItem {
    fn height(&self, width: u16) -> u16 {
        match self {
            RenderItem::Text { lines, .. } => {
                if width == 0 {
                    return lines.len() as u16;
                }
                lines
                    .iter()
                    .map(|line| wrapped_line_height(line, width as usize))
                    .sum()
            }
            RenderItem::Image { height, .. } => *height,
            RenderItem::ImagePlaceholder { .. }
            | RenderItem::AttachmentLabel { .. }
            | RenderItem::DateSeparator(_) => 1,
        }
    }

    fn selection_id(&self) -> Option<SelectionId> {
        match self {
            RenderItem::Text { sel, .. }
            | RenderItem::Image { sel, .. }
            | RenderItem::ImagePlaceholder { sel, .. }
            | RenderItem::AttachmentLabel { sel, .. } => Some(*sel),
            RenderItem::DateSeparator(_) => None,
        }
    }
}

/// Calculate the number of terminal rows a Line occupies when word-wrapped
/// to `width` columns, matching ratatui's wrapping behaviour.
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
                rows += 1;
                col = cw;
            } else {
                col += cw;
                if col == width {
                    col = 0;
                    rows += 1;
                }
            }
        }
    }
    if col == 0 && rows > 1 {
        rows -= 1;
    }
    rows.max(1)
}

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let msg_search_active = app.focus == Focus::MessageSearch
        || !app.msg_search_input.is_empty();
    let is_active = app.focus == Focus::MessageView
        || app.focus == Focus::MessageSearch;
    let border_style = if is_active {
        theme::active_border()
    } else {
        theme::inactive_border()
    };

    // Show animated loading indicator in the block title when fetching older messages.
    let loading_title = app
        .selected_conversation_idx
        .and_then(|i| app.conversations.get(i))
        .filter(|c| c.loading_more_messages)
        .map(|_| {
            let frame = SPINNER_FRAMES[app.tick_count as usize % SPINNER_FRAMES.len()];
            Line::from(Span::styled(
                format!(" {} Loading older messages... ", frame),
                theme::help_style(),
            ))
        });

    let mut block = Block::default()
        .borders(Borders::ALL)
        .title(" Messages ")
        .title_style(if is_active {
            theme::title_style()
        } else {
            theme::help_style()
        })
        .border_style(border_style);

    if let Some(loading) = loading_title {
        block = block
            .title_bottom(loading)
            .title_alignment(Alignment::Center);
    }

    // No conversation selected
    let selected = app
        .selected_conversation_idx
        .and_then(|i| app.conversations.get(i));

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
        let placeholder = Paragraph::new(hint).style(theme::help_style()).block(block);
        f.render_widget(placeholder, area);
        return;
    }

    // Build render items from messages
    let mut items: Vec<RenderItem> = Vec::new();

    let mut prev_date: Option<String> = None;

    for (msg_idx, msg) in conv.messages.iter().enumerate() {
        // Insert a date separator when the date changes between messages.
        let msg_date = msg.date_display();
        if prev_date.as_ref() != Some(&msg_date) {
            items.push(RenderItem::DateSeparator(Line::from(Span::styled(
                format!("── {} ──", msg_date),
                theme::timestamp_style(),
            ))));
            prev_date = Some(msg_date);
        }

        let sender = if msg.is_outgoing() {
            "You".to_string()
        } else {
            let addr = msg
                .addresses
                .first()
                .map(|a| a.address.as_str())
                .unwrap_or("?");
            app.contacts.display_name(addr)
        };

        let style = if msg.is_outgoing() {
            theme::outgoing_message()
        } else {
            theme::incoming_message()
        };

        let time = msg.timestamp_display();
        let body = sanitize_for_terminal(&msg.body);

        let mut line_spans = vec![
            Span::styled(format!("[{}] ", time), theme::timestamp_style()),
            Span::styled(format!("{}: ", sender), style),
        ];

        // Highlight search matches in message body
        if !app.msg_search_input.is_empty() && app.msg_search_matches.contains(&msg_idx) {
            let is_selected = app.selected_message_idx == Some(msg_idx)
                && app.selected_message_part == 0;
            let hl = if is_selected {
                theme::search_highlight_selected()
            } else {
                theme::search_highlight()
            };
            line_spans.extend(highlight_matches(
                &body,
                &app.msg_search_input,
                ratatui::style::Style::default(),
                hl,
            ));
        } else {
            line_spans.push(Span::raw(body));
        }

        let text_lines = vec![Line::from(line_spans)];

        items.push(RenderItem::Text {
            lines: text_lines,
            sel: (msg_idx, 0),
        });

        // Add each attachment as a separate selectable item (part 1, 2, ...)
        for (att_idx, att) in msg.attachments.iter().enumerate() {
            let sel = (msg_idx, att_idx + 1);
            if att.is_image() {
                match app.image_states.get(&att.unique_identifier) {
                    Some(ImageState::Loaded(_)) => {
                        items.push(RenderItem::Image {
                            uid: att.unique_identifier.clone(),
                            height: IMAGE_MAX_ROWS,
                            sel,
                        });
                    }
                    Some(ImageState::Downloading) => {
                        items.push(RenderItem::ImagePlaceholder {
                            line: Line::from(Span::styled(
                                format!("  [Downloading {}...]", att.mime_type),
                                theme::help_style(),
                            )),
                            sel,
                        });
                    }
                    Some(ImageState::Failed(reason)) => {
                        items.push(RenderItem::ImagePlaceholder {
                            line: Line::from(Span::styled(
                                format!("  [Image failed: {}]", reason),
                                theme::help_style(),
                            )),
                            sel,
                        });
                    }
                    None => {
                        items.push(RenderItem::ImagePlaceholder {
                            line: Line::from(Span::styled(
                                format!("  [Image: {}]", att.mime_type),
                                theme::help_style(),
                            )),
                            sel,
                        });
                    }
                }
            } else {
                let label = format!("  [Attachment: {}]", att.mime_type);
                items.push(RenderItem::AttachmentLabel {
                    line: Line::from(Span::styled(label, theme::help_style())),
                    sel,
                });
            }
        }
    }

    // Render the block border first, then render content inside
    let full_inner = block.inner(area);
    f.render_widget(block, area);

    // Split inner area for search box at bottom when search is active
    let (inner, search_area) = if msg_search_active {
        use ratatui::layout::{Constraint, Direction, Layout};
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(full_inner);
        (chunks[0], Some(chunks[1]))
    } else {
        (full_inner, None)
    };

    let inner_width = inner.width;
    let inner_height = inner.height;
    app.message_view_height = inner_height;

    // Calculate per-item heights
    let item_heights: Vec<u16> = items.iter().map(|item| item.height(inner_width)).collect();
    let total_height: u16 = item_heights.iter().sum();

    // Compute per-message heights for scroll boundaries
    let mut msg_heights: Vec<u16> = Vec::with_capacity(conv.messages.len());
    {
        let mut item_idx = 0usize;
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
            // Attachment items
            for _ in &msg.attachments {
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
    app.message_max_scroll = max_scroll;

    // Build message_boundaries
    {
        let mut scroll_boundaries: Vec<u16> = Vec::new();
        let mut cum: u16 = 0;
        for &mh in msg_heights.iter().rev() {
            cum += mh;
            if cum > 0 && cum <= max_scroll {
                scroll_boundaries.push(cum);
            }
        }
        if max_scroll > 0 && scroll_boundaries.last() != Some(&max_scroll) {
            scroll_boundaries.push(max_scroll);
        }
        scroll_boundaries.dedup();
        app.message_boundaries = scroll_boundaries;
    }

    // Current selection
    let current_sel: Option<SelectionId> = if is_active {
        app.selected_message_idx
            .map(|idx| (idx, app.selected_message_part))
    } else {
        None
    };

    // Auto-scroll viewport to keep selected item visible.
    // Calculate the selected item's position from bottom.
    if let Some(sel) = current_sel {
        // Find the selected item's cumulative offset from bottom
        let mut offset_from_bottom: u16 = 0;
        let mut found = false;
        // Walk items from bottom (newest) to top (oldest)
        for i in (0..items.len()).rev() {
            let h = item_heights[i];
            if items[i].selection_id() == Some(sel) {
                found = true;
                // offset_from_bottom is the distance from the bottom edge
                // to the BOTTOM of this item. The TOP is offset_from_bottom + h.
                let item_top_from_bottom = offset_from_bottom + h;

                // Ensure the item is visible.
                // message_scroll = offset from bottom edge of content that is
                // hidden below the viewport.
                // Visible range: [message_scroll, message_scroll + inner_height)
                // from the bottom.
                if offset_from_bottom < app.message_scroll {
                    // Item is below viewport — scroll down
                    app.message_scroll = offset_from_bottom;
                } else if item_top_from_bottom > app.message_scroll + inner_height {
                    // Item top is above viewport — scroll up
                    if h >= inner_height {
                        // Item taller than viewport: show top
                        app.message_scroll = item_top_from_bottom.saturating_sub(inner_height);
                    } else {
                        app.message_scroll = item_top_from_bottom.saturating_sub(inner_height);
                    }
                }
                break;
            }
            offset_from_bottom += h;
        }
        if !found {
            // Selection doesn't exist (stale index) — don't adjust scroll
        }
    }

    app.message_scroll = app.message_scroll.min(max_scroll);

    // Compute the pixel offset for rendering.
    let content_start: i32 = if total_height < inner_height {
        (inner_height - total_height) as i32
    } else {
        let scroll_offset = max_scroll.saturating_sub(app.message_scroll);
        -(scroll_offset as i32)
    };

    // Render visible items
    let mut y: i32 = content_start;

    for (i, item) in items.iter().enumerate() {
        let item_height = item_heights[i];

        let item_top = y;
        let item_bottom = y + item_height as i32;

        if item_bottom <= 0 {
            y += item_height as i32;
            continue;
        }
        if item_top >= inner_height as i32 {
            break;
        }

        let render_y = item_top.max(0) as u16;
        let available = inner_height.saturating_sub(render_y);
        let is_selected = current_sel.is_some() && item.selection_id() == current_sel;

        match item {
            RenderItem::Text { lines, .. } => {
                let skip = if item_top < 0 { (-item_top) as u16 } else { 0 };
                let visible_height = item_height.saturating_sub(skip).min(available);
                if visible_height > 0 {
                    let text_area = Rect {
                        x: inner.x,
                        y: inner.y + render_y,
                        width: inner_width,
                        height: visible_height,
                    };
                    if is_selected {
                        // Fill background for highlight
                        for row in text_area.y..text_area.y + text_area.height {
                            for col in text_area.x..text_area.x + text_area.width {
                                if let Some(cell) = f.buffer_mut().cell_mut((col, row)) {
                                    cell.set_bg(Color::DarkGray);
                                }
                            }
                        }
                    }
                    let paragraph = Paragraph::new(lines.clone())
                        .wrap(Wrap { trim: false })
                        .scroll((skip, 0));
                    f.render_widget(paragraph, text_area);
                    if is_selected {
                        // Re-apply background after rendering text (text clears bg)
                        for row in text_area.y..text_area.y + text_area.height {
                            for col in text_area.x..text_area.x + text_area.width {
                                if let Some(cell) = f.buffer_mut().cell_mut((col, row)) {
                                    cell.set_bg(Color::DarkGray);
                                }
                            }
                        }
                    }
                }
            }
            RenderItem::Image { uid, height, .. } => {
                let fits = item_top >= 0 && *height <= available;
                if fits {
                    let img_area = Rect {
                        x: inner.x,
                        y: inner.y + render_y,
                        width: inner_width.min(40),
                        height: *height,
                    };
                    if let Some(ImageState::Loaded(protocol)) = app.image_states.get_mut(uid) {
                        let image_widget =
                            StatefulImage::<ratatui_image::protocol::StatefulProtocol>::default();
                        f.render_stateful_widget(image_widget, img_area, protocol.as_mut());
                    }
                    if is_selected {
                        // Draw a DarkGray vertical bar to the right of the image
                        let bar_x = img_area.x + img_area.width;
                        if bar_x < inner.x + inner_width {
                            for row in img_area.y..img_area.y + img_area.height {
                                if let Some(cell) = f.buffer_mut().cell_mut((bar_x, row)) {
                                    cell.set_symbol("▐");
                                    cell.set_fg(Color::DarkGray);
                                }
                            }
                        }
                    }
                }
            }
            RenderItem::ImagePlaceholder { line, .. }
            | RenderItem::AttachmentLabel { line, .. } => {
                if item_top >= 0 {
                    let text_area = Rect {
                        x: inner.x,
                        y: inner.y + render_y,
                        width: inner_width,
                        height: 1.min(available),
                    };
                    if is_selected {
                        for col in text_area.x..text_area.x + text_area.width {
                            if let Some(cell) = f.buffer_mut().cell_mut((col, text_area.y)) {
                                cell.set_bg(Color::DarkGray);
                            }
                        }
                    }
                    let paragraph = Paragraph::new(line.clone());
                    f.render_widget(paragraph, text_area);
                    if is_selected {
                        // Re-apply bg and override fg for contrast
                        // (help_style uses DarkGray fg which is invisible on DarkGray bg)
                        for col in text_area.x..text_area.x + text_area.width {
                            if let Some(cell) = f.buffer_mut().cell_mut((col, text_area.y)) {
                                cell.set_bg(Color::DarkGray);
                                cell.set_fg(Color::White);
                            }
                        }
                    }
                }
            }
            RenderItem::DateSeparator(line) => {
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

    // Render search box at bottom
    if let Some(sa) = search_area {
        let input = &app.msg_search_input;
        let cursor = app.msg_search_cursor;
        let (before, cursor_char, after) = split_at_cursor(input, cursor);
        let match_info = if app.msg_search_matches.is_empty() && !input.is_empty() {
            Span::styled(" [no match]", theme::help_style())
        } else if let Some(idx) = app.msg_search_match_idx {
            Span::styled(
                format!(" [{}/{}]", idx + 1, app.msg_search_matches.len()),
                theme::help_style(),
            )
        } else {
            Span::raw("")
        };
        let search_line = Line::from(vec![
            Span::styled("/", theme::title_style()),
            Span::raw(before),
            Span::styled(
                cursor_char,
                ratatui::style::Style::default()
                    .add_modifier(ratatui::style::Modifier::REVERSED),
            ),
            Span::raw(after),
            match_info,
        ]);
        f.render_widget(Paragraph::new(search_line), sa);
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
            messages: vec![make_msg("Hey!", true), make_msg("Hi there!", false)],
            is_group: false,
            display_name: None,
            messages_requested: 0,
            total_messages: None,
            loading_more_messages: false,
            loading_started_tick: None,
            alias_thread_ids: Vec::new(),
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
            alias_thread_ids: Vec::new(),
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
            alias_thread_ids: Vec::new(),
        });
        app.selected_conversation_idx = Some(0);

        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &mut app, f.area());
            })
            .unwrap();

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
        assert!(
            last_row_with_msg >= 10,
            "Message at row {} should be near bottom",
            last_row_with_msg
        );
    }

    #[test]
    fn test_message_boundaries_computed() {
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
            alias_thread_ids: Vec::new(),
        });
        app.selected_conversation_idx = Some(0);

        let backend = TestBackend::new(40, 5);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &mut app, f.area());
            })
            .unwrap();

        assert!(
            app.message_max_scroll > 0 || app.message_boundaries.is_empty(),
            "scroll state should be coherent"
        );

        if !app.message_boundaries.is_empty() {
            for w in app.message_boundaries.windows(2) {
                assert!(
                    w[0] < w[1],
                    "boundaries must be sorted: {:?}",
                    app.message_boundaries
                );
            }
            assert!(app.message_boundaries[0] > 0);
            assert!(*app.message_boundaries.last().unwrap() <= app.message_max_scroll);
        }
    }

    #[test]
    fn test_wrapped_line_height_ascii() {
        let line = Line::from("0123456789");
        assert_eq!(wrapped_line_height(&line, 10), 1);
        let line = Line::from("01234567890");
        assert_eq!(wrapped_line_height(&line, 10), 2);
    }

    #[test]
    fn test_wrapped_line_height_emoji_at_boundary() {
        let line = Line::from("12345678\u{1F600}");
        assert_eq!(wrapped_line_height(&line, 10), 1);

        let line = Line::from("123456789\u{1F600}");
        assert_eq!(wrapped_line_height(&line, 10), 2);
    }

    #[test]
    fn test_wrapped_line_height_multiple_emoji() {
        let line = Line::from("\u{1F600}\u{1F601}\u{1F602}\u{1F603}\u{1F604}");
        assert_eq!(wrapped_line_height(&line, 10), 1);

        let line = Line::from("\u{1F600}\u{1F601}\u{1F602}\u{1F603}\u{1F604}\u{1F605}");
        assert_eq!(wrapped_line_height(&line, 10), 2);
    }

    #[test]
    fn test_selected_message_highlight() {
        let mut app = App::new_test();
        app.conversations.push(Conversation {
            thread_id: 1,
            latest_message: Some(make_msg("msg2", false)),
            messages: vec![make_msg("msg1", true), make_msg("msg2", false)],
            is_group: false,
            display_name: None,
            messages_requested: 0,
            total_messages: None,
            loading_more_messages: false,
            loading_started_tick: None,
            alias_thread_ids: Vec::new(),
        });
        app.selected_conversation_idx = Some(0);
        app.selected_message_idx = Some(0);
        app.selected_message_part = 0;
        app.focus = Focus::MessageView;

        let backend = TestBackend::new(60, 15);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &mut app, f.area());
            })
            .unwrap();

        // The selected message should have DarkGray background
        let buf = terminal.backend().buffer();
        let mut found_highlight = false;
        for y in 0..buf.area.height {
            let mut row = String::new();
            for x in 0..buf.area.width {
                if let Some(cell) = buf.cell((x, y)) {
                    row.push_str(cell.symbol());
                }
            }
            if row.contains("msg1") {
                // Check that at least one cell in this row has DarkGray bg
                for x in 0..buf.area.width {
                    if let Some(cell) = buf.cell((x, y)) {
                        if cell.bg == Color::DarkGray {
                            found_highlight = true;
                            break;
                        }
                    }
                }
            }
        }
        assert!(
            found_highlight,
            "Selected message should have highlighted background"
        );
    }
}
