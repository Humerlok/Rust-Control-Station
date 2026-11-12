//! Tauri commands for the SpeedLOG screen (Tools). Thin wrappers over `control_station_speedlog`.
//! The opened log stays on the Rust side (`SpeedLogState`); the UI asks for the field tree and for
//! each trace reduced to the visible window, so megabytes never cross the IPC boundary.
use std::sync::{Arc, Mutex};

use control_station_speedlog::{
    Lang, LogInfo, LogMessage, ModeSpan, ParamRow, Preset, SeriesView, SpeedLog, TypeNode, BUCKETS,
};
use serde::Deserialize;
use tauri::{AppHandle, State};

use crate::save_dialog::{ask_open_path, Filter};

#[derive(Default)]
pub struct SpeedLogState {
    log: Mutex<Option<Arc<SpeedLog>>>,
}

impl SpeedLogState {
    fn current(&self) -> Result<Arc<SpeedLog>, String> {
        self.log
            .lock()
            .map_err(|_| "log state poisoned".to_string())?
            .clone()
            .ok_or_else(|| "no log open".to_string())
    }

    /// Parses `data` (off the async runtime) and makes it the open log.
    pub async fn load_bytes(&self, name: String, data: Vec<u8>) -> Result<LogInfo, String> {
        let log = tauri::async_runtime::spawn_blocking(move || SpeedLog::from_bytes(name, data))
            .await
            .map_err(|e| format!("failed to read the log: {e}"))?;
        self.set(log)
    }

    fn set(&self, log: SpeedLog) -> Result<LogInfo, String> {
        let info = log.info();
        *self
            .log
            .lock()
            .map_err(|_| "log state poisoned".to_string())? = Some(Arc::new(log));
        Ok(info)
    }
}

/// Runs a read on the open log off the async runtime (series can take a moment on big logs).
async fn with_log<T: Send + 'static>(
    state: &SpeedLogState,
    f: impl FnOnce(&SpeedLog) -> T + Send + 'static,
) -> Result<T, String> {
    let log = state.current()?;
    tauri::async_runtime::spawn_blocking(move || f(&log))
        .await
        .map_err(|e| e.to_string())
}

/// Asks for a `.bin` log with the native dialog and opens it; `None` when cancelled.
#[tauri::command]
pub async fn speedlog_open(
    app: AppHandle,
    state: State<'_, SpeedLogState>,
) -> Result<Option<LogInfo>, String> {
    let filter = Filter {
        name: "DataFlash log",
        extensions: &["bin", "BIN"],
    };
    let Some(path) = ask_open_path(&app, "Open log", Some(filter)).await? else {
        return Ok(None);
    };
    let log = tauri::async_runtime::spawn_blocking(move || SpeedLog::open(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("failed to read the log: {e}"))?;
    state.set(log).map(Some)
}

#[tauri::command]
pub fn speedlog_close(state: State<'_, SpeedLogState>) {
    if let Ok(mut log) = state.log.lock() {
        *log = None;
    }
}

#[tauri::command]
pub fn speedlog_info(state: State<'_, SpeedLogState>) -> Result<Option<LogInfo>, String> {
    Ok(state.current().ok().map(|l| l.info()))
}

#[tauri::command]
pub async fn speedlog_tree(
    lang: Lang,
    state: State<'_, SpeedLogState>,
) -> Result<Vec<TypeNode>, String> {
    with_log(&state, move |l| l.tree(lang)).await
}

#[derive(Deserialize)]
pub struct SeriesRequest {
    #[serde(rename = "type")]
    type_name: String,
    field: String,
}

/// Several traces at once, each reduced to `range` (seconds; whole log when absent). An entry is
/// `null` when the field cannot be plotted.
#[tauri::command]
pub async fn speedlog_series(
    requests: Vec<SeriesRequest>,
    range: Option<(f64, f64)>,
    buckets: Option<usize>,
    state: State<'_, SpeedLogState>,
) -> Result<Vec<Option<SeriesView>>, String> {
    let buckets = buckets.unwrap_or(BUCKETS).clamp(16, 20_000);
    with_log(&state, move |l| {
        requests
            .iter()
            .map(|r| l.series_view(&r.type_name, &r.field, range, buckets))
            .collect()
    })
    .await
}

#[tauri::command]
pub async fn speedlog_modes(state: State<'_, SpeedLogState>) -> Result<Vec<ModeSpan>, String> {
    with_log(&state, |l| l.modes()).await
}

#[tauri::command]
pub async fn speedlog_params(
    lang: Lang,
    state: State<'_, SpeedLogState>,
) -> Result<Vec<ParamRow>, String> {
    with_log(&state, move |l| l.params(lang)).await
}

#[tauri::command]
pub async fn speedlog_messages(
    lang: Lang,
    state: State<'_, SpeedLogState>,
) -> Result<Vec<LogMessage>, String> {
    with_log(&state, move |l| l.messages(lang)).await
}

#[tauri::command]
pub async fn speedlog_presets(state: State<'_, SpeedLogState>) -> Result<Vec<Preset>, String> {
    with_log(&state, |l| l.presets()).await
}
