use std::fs;
use std::path::{Path, PathBuf};

use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;

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
            // Skip hidden files/dirs unless show_hidden is enabled
            if !app.file_picker_show_hidden
                && path
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

/// Abbreviate a path by replacing the home directory prefix with `~`.
pub fn abbreviate_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = path.strip_prefix(&home) {
            if rest.as_os_str().is_empty() {
                return "~".to_string();
            }
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
}

/// Expand a path string, replacing a leading `~` with the home directory.
pub fn expand_path(input: &str) -> PathBuf {
    let trimmed = input.trim();
    if trimmed == "~" {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/"))
            .join(rest)
    } else {
        PathBuf::from(trimmed)
    }
}

/// Sync the file picker input box to show the currently highlighted entry path.
pub fn sync_file_picker_input(app: &mut App) {
    let path = if app.file_picker_idx == 0 {
        // "../" entry — show the parent dir path
        app.file_picker_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| app.file_picker_dir.clone())
    } else {
        let entry_idx = app.file_picker_idx - 1;
        app.file_picker_entries
            .get(entry_idx)
            .cloned()
            .unwrap_or_else(|| app.file_picker_dir.clone())
    };
    app.file_picker_input = abbreviate_path(&path);
    app.file_picker_input_cursor = app.file_picker_input.len();
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
        .border_style(theme::active_border())
        .style(ratatui::style::Style::default().bg(theme::background()));

    let inner = block.inner(popup_area);

    // Layout: 1 line for input box, remaining for list, 1 line for hints
    let input_area = Rect::new(inner.x, inner.y, inner.width, 1);
    let list_height = inner.height.saturating_sub(2); // minus input and hint
    let list_area = Rect::new(inner.x, inner.y + 1, inner.width, list_height);
    let hint_area = Rect::new(inner.x, inner.y + 1 + list_height, inner.width, 1);

    // Render block first
    f.render_widget(block, popup_area);

    // Render input box
    let input = &app.file_picker_input;
    let cursor_byte = app.file_picker_input_cursor;

    // Calculate horizontal scroll so cursor is always visible
    let input_width = input_area.width as usize;
    let before_cursor = &input[..cursor_byte];
    let before_width: usize = before_cursor.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum();
    let scroll_offset = if before_width >= input_width {
        before_width - input_width + 1
    } else {
        0
    };

    // Build the visible portion of the input with cursor
    if app.file_picker_input_focused {
        // Focused: show cursor highlight
        let before = &input[..cursor_byte];
        let cursor_char = input[cursor_byte..].chars().next().unwrap_or(' ');
        let after_start = cursor_byte + cursor_char.len_utf8().min(input.len() - cursor_byte);
        let after = &input[after_start.min(input.len())..];

        // Apply scroll offset by skipping characters
        let mut skip_width = scroll_offset;
        let mut visible_before = String::new();
        for ch in before.chars() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(0);
            if skip_width > 0 {
                skip_width = skip_width.saturating_sub(w);
            } else {
                visible_before.push(ch);
            }
        }

        let input_line = Line::from(vec![
            Span::styled(visible_before, theme::title_style()),
            Span::styled(cursor_char.to_string(), theme::selected_style()),
            Span::styled(after.to_string(), theme::title_style()),
        ]);
        f.render_widget(Paragraph::new(input_line), input_area);
    } else {
        // Not focused: show plain text, dimmer style
        let mut skip_width = scroll_offset;
        let mut visible = String::new();
        for ch in input.chars() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(0);
            if skip_width > 0 {
                skip_width = skip_width.saturating_sub(w);
            } else {
                visible.push(ch);
            }
        }
        let input_line = Line::from(Span::styled(visible, theme::help_style()));
        f.render_widget(Paragraph::new(input_line), input_area);
    }

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

    // Render list
    let list = List::new(items).highlight_style(theme::selected_style());
    let mut state = ListState::default();
    state.select(Some(app.file_picker_idx));
    f.render_stateful_widget(list, list_area, &mut state);

    // Render hint line
    let hidden_indicator = if app.file_picker_show_hidden { "on" } else { "off" };
    let hint_text = format!(
        "Esc:cancel  Tab:input  .:hidden({})",
        hidden_indicator
    );
    let hint = Paragraph::new(Span::styled(hint_text, theme::help_style()));
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
    fn test_abbreviate_path() {
        if let Some(home) = dirs::home_dir() {
            assert_eq!(abbreviate_path(&home), "~");
            assert_eq!(
                abbreviate_path(&home.join("Documents")),
                "~/Documents"
            );
        }
        assert_eq!(abbreviate_path(&std::path::PathBuf::from("/tmp")), "/tmp");
    }

    #[test]
    fn test_expand_path() {
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expand_path("~"), home);
            assert_eq!(expand_path("~/Documents"), home.join("Documents"));
        }
        assert_eq!(expand_path("/tmp"), std::path::PathBuf::from("/tmp"));
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
