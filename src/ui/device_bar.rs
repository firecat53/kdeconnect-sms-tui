use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use super::theme;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" KDE Connect SMS ");

    let content = if app.devices.is_empty() {
        Line::from(vec![
            Span::styled("No devices found", theme::status_unavailable()),
            Span::styled(
                " -- is kdeconnectd running?",
                theme::help_style(),
            ),
        ])
    } else {
        let mut spans = Vec::new();

        for (i, device) in app.devices.iter().enumerate() {
            let is_selected = app.selected_device_idx == Some(i);

            let style = if is_selected {
                if device.is_available() {
                    theme::status_available().add_modifier(Modifier::BOLD)
                } else {
                    theme::status_unavailable().add_modifier(Modifier::BOLD)
                }
            } else if device.is_available() {
                theme::status_available()
            } else {
                theme::status_unavailable()
            };

            if i > 0 {
                spans.push(Span::raw(" | "));
            }

            let prefix = if is_selected { "> " } else { "  " };
            spans.push(Span::styled(
                format!("{}{}", prefix, device.name),
                style,
            ));
        }

        // Show status message if present
        if let Some(ref status) = app.status_message {
            spans.push(Span::styled(
                format!("  [{}]", status),
                theme::help_style(),
            ));
        } else {
            spans.push(Span::styled(
                "  [Tab: switch | r: refresh | q: quit]",
                theme::help_style(),
            ));
        }

        Line::from(spans)
    };

    let paragraph = Paragraph::new(content).block(block);
    f.render_widget(paragraph, area);
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
        let backend = TestBackend::new(60, 5);
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

        let backend = TestBackend::new(80, 5);
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

        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("5 conversations loaded"));
    }
}
