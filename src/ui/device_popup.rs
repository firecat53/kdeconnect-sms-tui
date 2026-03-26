use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;

use super::theme;
use crate::app::App;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    // Center a popup: width = min(40, 80% of screen), height = devices + 2 (border)
    let popup_width = 40.min(area.width * 80 / 100).max(20);
    let popup_height = (app.devices.len() as u16 + 2).min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    // Clear the area behind the popup
    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Select Device ")
        .title_style(theme::title_style())
        .border_style(theme::active_border());

    let items: Vec<ListItem> = app
        .devices
        .iter()
        .map(|device| {
            let style = if device.is_available() {
                theme::status_available()
            } else {
                theme::status_unavailable()
            };
            let marker = if app.selected_device().is_some_and(|d| d.id == device.id) {
                Span::styled("* ", style.add_modifier(Modifier::BOLD))
            } else {
                Span::raw("  ")
            };
            ListItem::new(Line::from(vec![
                marker,
                Span::styled(&device.name, style),
                if !device.is_available() {
                    Span::styled(" (offline)", theme::help_style())
                } else {
                    Span::raw("")
                },
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected_style());

    let mut state = ListState::default();
    state.select(Some(app.device_popup_idx));
    f.render_stateful_widget(list, popup_area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, Focus};
    use crate::models::device::Device;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn test_device_popup_renders() {
        let mut app = App::new_test();
        app.devices = vec![
            Device {
                id: "a".into(),
                name: "Phone A".into(),
                reachable: true,
                paired: true,
            },
            Device {
                id: "b".into(),
                name: "Phone B".into(),
                reachable: false,
                paired: true,
            },
        ];
        app.selected_device_idx = Some(0);
        app.selected_device_id = Some("a".into());
        app.device_popup_idx = 0;
        app.focus = Focus::DevicePopup;

        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app);
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("Select Device"));
        assert!(content.contains("Phone A"));
        assert!(content.contains("Phone B"));
        assert!(content.contains("offline"));
    }
}
