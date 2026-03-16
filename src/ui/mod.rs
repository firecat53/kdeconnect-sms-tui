pub mod device_bar;
pub mod conversation_list;
pub mod message_view;
pub mod theme;

#[cfg(test)]
pub mod test_helpers;

use ratatui::Frame;

use crate::app::App;

/// Render the full application UI.
pub fn draw(f: &mut Frame, app: &App) {
    use ratatui::layout::{Constraint, Direction, Layout};

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // device bar
            Constraint::Min(1),   // main content
        ])
        .split(f.area());

    device_bar::draw(f, app, chunks[0]);

    // Split main content: conversation list (left) | message view (right)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30), // conversation list
            Constraint::Percentage(70), // message view
        ])
        .split(chunks[1]);

    conversation_list::draw(f, app, main_chunks[0]);
    message_view::draw(f, app, main_chunks[1]);
}
