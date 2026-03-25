use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::App;

const HELP_TEXT: &str = "Tab:pane  j/k:nav  J/K:page  i:compose  g:group  a/A:archive  s/S:spam  d:devices  r:refresh  q:quit";

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let width = area.width as usize;

    // Build the left side: device name + optional status
    let mut left_spans: Vec<Span> = Vec::new();
    left_spans.push(Span::raw(" "));

    if app.devices.is_empty() {
        left_spans.push(Span::styled(
            "No devices found",
            theme::status_unavailable(),
        ));
        left_spans.push(Span::styled(
            " -- is kdeconnectd running?",
            theme::help_style(),
        ));
    } else if let Some(device) = app.selected_device() {
        let style = if device.is_available() {
            theme::status_available().add_modifier(Modifier::BOLD)
        } else {
            theme::status_unavailable().add_modifier(Modifier::BOLD)
        };
        left_spans.push(Span::styled(&device.name, style));

        if let Some(ref status) = app.status_message {
            left_spans.push(Span::styled(format!("  [{}]", status), theme::help_style()));
        }
    } else {
        left_spans.push(Span::styled(
            "No device selected",
            theme::status_unavailable(),
        ));
    }

    // Calculate left side width
    let left_width: usize = left_spans.iter().map(|s| s.content.width()).sum();
    let help_width = HELP_TEXT.width();

    // If both fit, add spacing + right-justified help text
    let gap = width.saturating_sub(left_width + help_width);
    if gap >= 2 {
        left_spans.push(Span::raw(" ".repeat(gap)));
        left_spans.push(Span::styled(HELP_TEXT, theme::help_style()));
    } else if left_width + 2 + help_width <= width {
        left_spans.push(Span::raw("  "));
        left_spans.push(Span::styled(HELP_TEXT, theme::help_style()));
    }
    // If combined is too wide, just show device info (help omitted)

    f.render_widget(Paragraph::new(Line::from(left_spans)), area);
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
        let backend = TestBackend::new(60, 1);
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

        let backend = TestBackend::new(120, 1);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("Pixel 8"));
        assert!(content.contains("Tab:pane"));
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

        let backend = TestBackend::new(100, 1);
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
    fn test_device_bar_help_on_same_line() {
        let mut app = App::new_test();
        app.devices = vec![Device {
            id: "test".into(),
            name: "Phone".into(),
            reachable: true,
            paired: true,
        }];
        app.selected_device_idx = Some(0);

        let backend = TestBackend::new(120, 1);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        // Both device name and help text on the same line
        assert!(content.contains("Phone"));
        assert!(content.contains("d:devices"));
    }

    #[test]
    fn test_device_bar_narrow_hides_help() {
        let mut app = App::new_test();
        app.devices = vec![Device {
            id: "test".into(),
            name: "My Very Long Device Name".into(),
            reachable: true,
            paired: true,
        }];
        app.selected_device_idx = Some(0);
        app.status_message = Some("65 conversations loaded".into());

        // Very narrow terminal — help text won't fit
        let backend = TestBackend::new(40, 1);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app, f.area());
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("My Very Long Device Name"));
        // Help text should be omitted when it doesn't fit
        assert!(!content.contains("Tab:pane"));
    }
}
