pub mod device_bar;
pub mod conversation_list;
pub mod message_view;
pub mod compose;
pub mod device_popup;
pub mod group_info_popup;
pub mod folder_popup;
pub mod help_popup;
pub mod theme;

#[cfg(test)]
pub mod test_helpers;

use ratatui::Frame;
use unicode_segmentation::UnicodeSegmentation;

use crate::app::{App, Focus};

/// Sanitize a string for terminal rendering by collapsing multi-codepoint
/// emoji sequences (ZWJ, variation selectors) into simpler forms that
/// `unicode-width` measures correctly.
///
/// Without this, ratatui's internal width calculation (based on `unicode-width`)
/// disagrees with the terminal's actual rendering for ZWJ emoji like 🤷‍♂️,
/// which causes buffer misalignment and breaks box borders.
pub(crate) fn sanitize_for_terminal(s: &str) -> String {
    let mut result = String::new();
    for grapheme in s.graphemes(true) {
        if grapheme.contains('\u{200D}') {
            // ZWJ sequence (e.g. 🤷‍♂️) — keep only the base emoji.
            // Terminal renders the whole sequence as 2 columns, but
            // unicode-width sums all codepoint widths (> 2).
            if let Some(ch) = grapheme.chars().next() {
                result.push(ch);
            }
        } else if grapheme.contains('\u{FE0F}') {
            // VS16 (emoji presentation) — strip it so unicode-width
            // sees the base character only.
            for ch in grapheme.chars() {
                if ch != '\u{FE0F}' {
                    result.push(ch);
                }
            }
        } else {
            result.push_str(grapheme);
        }
    }
    result
}

/// Render the full application UI.
pub fn draw(f: &mut Frame, app: &mut App) {
    use ratatui::layout::{Constraint, Direction, Layout};

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // device bar
            Constraint::Min(1),   // main content
        ])
        .split(f.area());

    device_bar::draw(f, app, chunks[0]);

    // Split main content: conversation list (left) | message + compose (right)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30), // conversation list
            Constraint::Percentage(70), // message view + compose
        ])
        .split(chunks[1]);

    conversation_list::draw(f, app, main_chunks[0]);

    // Split right panel: messages (top) | compose (bottom)
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // message view
            Constraint::Length(4), // compose input
        ])
        .split(main_chunks[1]);

    message_view::draw(f, app, right_chunks[0]);
    compose::draw(f, app, right_chunks[1]);

    // Popup overlays (rendered last, on top)
    match app.focus {
        Focus::DevicePopup => device_popup::draw(f, app),
        Focus::GroupInfoPopup => group_info_popup::draw(f, app),
        Focus::FolderPopup => folder_popup::draw(f, app),
        Focus::HelpPopup => help_popup::draw(f),
        _ => {}
    }
}
