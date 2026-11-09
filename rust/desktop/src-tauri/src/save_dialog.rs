//! Native "Save as" dialog shared by the commands that write a file chosen by the user.
//! The dialog is opened here, in Rust, so the webview never gets a generic "write any path"
//! command: each command decides what it writes and only the destination comes from the user.
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

/// One file-type filter of the dialog: a label and its extensions (without the dot).
pub struct Filter {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
}

/// Asks the user where to save; `None` when the dialog was cancelled.
pub async fn ask_save_path(
    app: &AppHandle,
    title: &str,
    default_name: &str,
    filter: Option<Filter>,
) -> Result<Option<PathBuf>, String> {
    let mut dialog = app
        .dialog()
        .file()
        .set_title(title)
        .set_file_name(safe_file_name(default_name));
    if let Some(filter) = filter {
        dialog = dialog.add_filter(filter.name, filter.extensions);
    }
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    // The blocking variant must not run on the main thread.
    let chosen = tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file())
        .await
        .map_err(|e| format!("save dialog failed: {e}"))?;
    match chosen {
        None => Ok(None),
        Some(path) => path
            .into_path()
            .map(Some)
            .map_err(|e| format!("invalid destination: {e}")),
    }
}

/// Writes `bytes` to `path`, reporting the path in the error.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

/// The last path component, without anything that could make it a path of its own.
fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    match base {
        "" | "." | ".." => "file".to_owned(),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::safe_file_name;

    #[test]
    fn default_names_keep_only_the_file_component() {
        assert_eq!(safe_file_name("/APM/LOGS/00000001.BIN"), "00000001.BIN");
        assert_eq!(safe_file_name("copter.param"), "copter.param");
        assert_eq!(safe_file_name("a\\b.txt"), "b.txt");
        assert_eq!(safe_file_name(".."), "file");
        assert_eq!(safe_file_name("/APM/"), "file");
    }
}
