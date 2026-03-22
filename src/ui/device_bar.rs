use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::App;
use super::theme;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    // Split into two rows: device info (1 line) and help bar (1 line)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // device line
            Constraint::Length(1), // help line
        ])
        .split(area);

    // Device line
    let device_line = if app.devices.is_empty() {
        Line::from(vec![
            Span::styled(" No devices found", theme::status_unavailable()),
            Span::styled(
                " -- is kdeconnectd running?",
                theme::help_style(),
            ),
        ])
    } else {
        let mut spans = Vec::new();
        spans.push(Span::raw(" "));

        if let Some(device) = app.selected_device() {
            let style = if device.is_available() {
                theme::status_available().add_modifier(Modifier::BOLD)
            } else {
                theme::status_unavailable().add_modifier(Modifier::BOLD)
            };
            spans.push(Span::styled(&device.name, style));
        } else {
            spans.push(Span::styled("No device selected", theme::status_unavailable()));
        }

        if let Some(ref status) = app.status_message {
            spans.push(Span::styled(
                format!("  [{}]", status),
                theme::help_style(),
            ));
        }

        Line::from(spans)
    };

    f.render_widget(Paragraph::new(device_line), chunks[0]);

    // Help line
    let help = Line::from(Span::styled(
        " Tab:pane  j/k:nav  J/K:page  i:compose  d:devices  r:refresh  q:quit",
        theme::help_style(),
    ));
    f.render_widget(Paragraph::new(help), chunks[1]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::models::device::Device;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn test_device_bar_renders_no_devices() {
        let app = App::new_test();
        let backend = TestBackend::new(60, 3);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("No devices found"));
    }

    #[test]
    fn test_device_bar_renders_with_device() {
        let mut app = App::new_test();
        app.devices = vec![Device {
            id: "test".into(),
            name: "Pixel 8".into(),
            reachable: true,
            paired: true,
        }];
        app.selected_device_idx = Some(0);

        let backend = TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("Pixel 8"));
    }

    #[test]
    fn test_device_bar_shows_status() {
        let mut app = App::new_test();
        app.devices = vec![Device {
            id: "test".into(),
            name: "Phone".into(),
            reachable: true,
            paired: true,
        }];
        app.selected_device_idx = Some(0);
        app.status_message = Some("5 conversations loaded".into());

        let backend = TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("5 conversations loaded"));
    }

    #[test]
    fn test_device_bar_shows_help_line() {
        let app = App::new_test();
        let backend = TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("Tab:pane"));
        assert!(content.contains("d:devices"));
    }
}
