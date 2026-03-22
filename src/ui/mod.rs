pub mod device_bar;
pub mod conversation_list;
pub mod message_view;
pub mod compose;
pub mod device_popup;
pub mod theme;

#[cfg(test)]
pub mod test_helpers;

use ratatui::Frame;

use crate::app::{App, Focus};

/// Render the full application UI.
pub fn draw(f: &mut Frame, app: &mut App) {
    use ratatui::layout::{Constraint, Direction, Layout};

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // device bar + help line
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

    // Device popup overlay (rendered last, on top)
    if app.focus == Focus::DevicePopup {
        device_popup::draw(f, app);
    }
}
