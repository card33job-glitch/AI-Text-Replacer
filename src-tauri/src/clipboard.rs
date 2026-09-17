use tauri::{AppHandle, ClipboardManager};

pub fn get_clipboard(app: &AppHandle) -> Result<String, String> {
    app.clipboard_manager()
        .read_text()
        .map(|text| text.unwrap_or_default())
        .map_err(|_| "Failed to read clipboard".to_string())
}

pub fn set_clipboard(app: &AppHandle, text: String) -> Result<(), String> {
    app.clipboard_manager()
        .write_text(text)
        .map_err(|_| "Failed to write to clipboard".to_string())
}
