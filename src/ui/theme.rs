use ratatui::style::{Color, Modifier, Style};

pub fn title_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub fn selected_style() -> Style {
    Style::default()
        .bg(Color::DarkGray)
        .fg(Color::White)
}

pub fn status_available() -> Style {
    Style::default().fg(Color::Green)
}

pub fn status_unavailable() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub fn incoming_message() -> Style {
    Style::default().fg(Color::White)
}

pub fn outgoing_message() -> Style {
    Style::default().fg(Color::Cyan)
}

pub fn timestamp_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub fn help_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub fn active_border() -> Style {
    Style::default().fg(Color::Cyan)
}

pub fn inactive_border() -> Style {
    Style::default().fg(Color::DarkGray)
}
