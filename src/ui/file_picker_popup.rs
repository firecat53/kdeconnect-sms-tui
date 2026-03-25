use std::fs;
use std::path::Path;

use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use super::theme;
use crate::app::App;

/// Image file extensions we allow in the picker.
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "bmp", "webp", "heic", "heif"];

fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Populate `app.file_picker_entries` from the current `app.file_picker_dir`.
/// Entries are sorted: directories first (alphabetical), then image files (alphabetical).
pub fn refresh_file_picker_entries(app: &mut App) {
    let mut dirs = Vec::new();
    let mut files = Vec::new();

    if let Ok(entries) = fs::read_dir(&app.file_picker_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            // Skip hidden files/dirs
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with('.'))
                .unwrap_or(false)
            {
                continue;
            }
            if path.is_dir() {
                dirs.push(path);
            } else if is_image_file(&path) {
                files.push(path);
            }
        }
    }

    dirs.sort_by(|a, b| {
        a.file_name()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .cmp(&b.file_name().unwrap_or_default().to_ascii_lowercase())
    });
    files.sort_by(|a, b| {
        a.file_name()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .cmp(&b.file_name().unwrap_or_default().to_ascii_lowercase())
    });

    app.file_picker_entries = dirs;
    app.file_picker_entries.extend(files);
    app.file_picker_idx = 0;
}

/// Detect MIME type from file extension.
pub fn mime_from_path(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
    {
        Some(ref ext) if ext == "jpg" || ext == "jpeg" => "image/jpeg".into(),
        Some(ref ext) if ext == "png" => "image/png".into(),
        Some(ref ext) if ext == "gif" => "image/gif".into(),
        Some(ref ext) if ext == "webp" => "image/webp".into(),
        Some(ref ext) if ext == "bmp" => "image/bmp".into(),
        Some(ref ext) if ext == "heic" || ext == "heif" => "image/heif".into(),
        _ => "application/octet-stream".into(),
    }
}

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    let popup_width = 60.min(area.width * 90 / 100).max(30);
    let popup_height = 20.min(area.height * 80 / 100).max(6);
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let dir_name = app
        .file_picker_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("/");
    let title = format!(" Select Image ({}) ", dir_name);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title.as_str())
        .title_style(theme::title_style())
        .border_style(theme::active_border());

    // Reserve 1 line at the bottom for the hint
    let inner = block.inner(popup_area);
    let list_height = inner.height.saturating_sub(1);

    // Build list items: first "../", then entries
    let mut items: Vec<ListItem> = Vec::with_capacity(app.file_picker_entries.len() + 1);

    // Parent directory entry
    items.push(ListItem::new(Line::from(Span::styled(
        "../",
        theme::help_style().add_modifier(Modifier::BOLD),
    ))));

    for entry in &app.file_picker_entries {
        let name = entry.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        if entry.is_dir() {
            items.push(ListItem::new(Line::from(Span::styled(
                format!("{}/", name),
                theme::title_style().add_modifier(Modifier::BOLD),
            ))));
        } else {
            items.push(ListItem::new(Line::from(Span::raw(name.to_string()))));
        }
    }

    let list_area = Rect::new(inner.x, inner.y, inner.width, list_height);
    let hint_area = Rect::new(inner.x, inner.y + list_height, inner.width, 1);

    // Render block first
    f.render_widget(block, popup_area);

    // Render list
    let list = List::new(items).highlight_style(theme::selected_style());
    let mut state = ListState::default();
    state.select(Some(app.file_picker_idx));
    f.render_stateful_widget(list, list_area, &mut state);

    // Render hint line
    let hint = Paragraph::new(Span::styled(
        "Enter:select  Esc:cancel  Backspace:up",
        theme::help_style(),
    ));
    f.render_widget(hint, hint_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;

    #[test]
    fn test_mime_from_path() {
        assert_eq!(
            mime_from_path(&std::path::PathBuf::from("photo.jpg")),
            "image/jpeg"
        );
        assert_eq!(
            mime_from_path(&std::path::PathBuf::from("photo.PNG")),
            "image/png"
        );
        assert_eq!(
            mime_from_path(&std::path::PathBuf::from("file.txt")),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_file_picker_popup_renders() {
        let mut app = App::new_test();
        app.file_picker_dir = std::path::PathBuf::from("/tmp");
        app.file_picker_entries = vec![
            std::path::PathBuf::from("/tmp/photos"),
            std::path::PathBuf::from("/tmp/image.jpg"),
        ];
        app.file_picker_idx = 0;

        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                draw(f, &app);
            })
            .unwrap();

        let content = crate::ui::test_helpers::buffer_to_string(terminal.backend().buffer());
        assert!(content.contains("Select Image"));
        assert!(content.contains("../"));
    }
}
