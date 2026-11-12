//! Tauri commands for the vehicle's onboard logs (SpeedLOG "Vehicle logs" dialog). Thin wrappers
//! over `LogService`: list, download into the app cache and open in SpeedLOG, or save a copy where
//! the user chooses. Progress goes to the UI as `logs-progress` events (see `main.rs`).
use std::path::PathBuf;

use control_station_link::{LogEntry, LogService};
use control_station_speedlog::LogInfo;
use tauri::{AppHandle, Manager, State};

use crate::save_dialog::{ask_save_path, write_file, Filter};
use crate::speedlog_cmds::SpeedLogState;

#[tauri::command]
pub async fn logs_list(logs: State<'_, LogService>) -> Result<Vec<LogEntry>, String> {
    logs.list().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn logs_cancel(logs: State<'_, LogService>) {
    logs.cancel();
}

/// Where a downloaded log is kept, so opening it again (or saving it) does not download it twice.
fn cache_path(app: &AppHandle, entry: &LogEntry) -> PathBuf {
    let safe: String = entry
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let stem = safe.trim_end_matches(".BIN").trim_end_matches(".bin");
    app.path()
        .app_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("vehicle-logs")
        .join(format!("{stem}-{}.BIN", entry.size))
}

/// The log's bytes: from the cache when it holds this exact entry, else downloaded (and cached).
async fn fetch(app: &AppHandle, logs: &LogService, entry: &LogEntry) -> Result<Vec<u8>, String> {
    let path = cache_path(app, entry);
    if let Ok(bytes) = std::fs::read(&path) {
        if !bytes.is_empty() && bytes.len() as u64 == entry.size {
            return Ok(bytes);
        }
    }
    let bytes = logs.download(entry).await.map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        // The cache is a convenience: a failure to write it does not fail the download.
        if std::fs::create_dir_all(dir).is_ok() {
            let _ = std::fs::write(&path, &bytes);
        }
    }
    Ok(bytes)
}

/// Downloads a log from the vehicle and opens it in SpeedLOG.
#[tauri::command]
pub async fn logs_open(
    entry: LogEntry,
    app: AppHandle,
    logs: State<'_, LogService>,
    speedlog: State<'_, SpeedLogState>,
) -> Result<LogInfo, String> {
    let bytes = fetch(&app, &logs, &entry).await?;
    let name = if entry.name.to_ascii_lowercase().ends_with(".bin") {
        entry.name.clone()
    } else {
        format!("{}.BIN", entry.name)
    };
    speedlog.load_bytes(name, bytes).await
}

/// Asks where to save, then downloads the log there; the path, or `None` when cancelled.
#[tauri::command]
pub async fn logs_save(
    entry: LogEntry,
    app: AppHandle,
    logs: State<'_, LogService>,
) -> Result<Option<String>, String> {
    let default_name = if entry.name.to_ascii_lowercase().ends_with(".bin") {
        entry.name.clone()
    } else {
        format!("{}.BIN", entry.name.replace(' ', "_"))
    };
    let filter = Filter {
        name: "DataFlash log",
        extensions: &["BIN", "bin"],
    };
    let Some(path) = ask_save_path(&app, "Save log", &default_name, Some(filter)).await? else {
        return Ok(None);
    };
    let bytes = fetch(&app, &logs, &entry).await?;
    write_file(&path, &bytes)?;
    Ok(Some(path.display().to_string()))
}
