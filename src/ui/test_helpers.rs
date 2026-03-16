use ratatui::buffer::Buffer;

/// Extract all text content from a ratatui Buffer as a single string.
pub fn buffer_to_string(buf: &Buffer) -> String {
    let mut result = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            if let Some(cell) = buf.cell((x, y)) {
                result.push_str(cell.symbol());
            }
        }
    }
    result
}
