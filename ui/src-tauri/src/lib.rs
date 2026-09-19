//! otelview desktop shell.
//!
//! The desktop app is the same React UI pointed at a remote otelview API —
//! configure the base URL and token in the in-app Settings view (persisted in
//! the webview's localStorage).

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .run(tauri::generate_context!())
        .expect("error while running otelview desktop");
}
