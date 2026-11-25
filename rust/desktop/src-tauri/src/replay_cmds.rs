//! Tauri commands for the "Replay (log)" connection: the playback controls (play, pause, seek,
//! speed, the path flown so far) and the native dialog that picks the log to replay. Thin wrappers
//! over `LinkManager::replay_*`; the position goes to the UI as `replay` events (see `main.rs`).
use control_station_link::ReplayStatus;
use tauri::{AppHandle, State};

use crate::save_dialog::{ask_open_path, Filter};
use crate::{text, AppState};

/// Where the replay is, or `None` when the connection is not a log replay.
#[tauri::command]
pub fn replay_status(state: State<'_, AppState>) -> Option<ReplayStatus> {
    state.link.replay_status()
}

#[tauri::command]
pub fn replay_play(state: State<'_, AppState>) -> Result<(), String> {
    text(state.link.replay_play())
}

#[tauri::command]
pub fn replay_pause(state: State<'_, AppState>) -> Result<(), String> {
    text(state.link.replay_pause())
}

/// `settle` is false while the user is still dragging the timeline and true for the final position.
#[tauri::command]
pub fn replay_seek(seconds: f64, settle: bool, state: State<'_, AppState>) -> Result<(), String> {
    text(state.link.replay_seek(seconds, settle))
}

#[tauri::command]
pub fn replay_set_speed(speed: f64, state: State<'_, AppState>) -> Result<(), String> {
    text(state.link.replay_set_speed(speed))
}

/// The path flown up to `until_s` seconds into the log, as `[lat, lon]` pairs.
#[tauri::command]
pub fn replay_track(until_s: f64, state: State<'_, AppState>) -> Result<Vec<(f64, f64)>, String> {
    text(state.link.replay_track(until_s))
}

/// Asks the user which flight log to replay; `None` when the dialog was cancelled. The path goes
/// back to the webview, which sends it to `connect` in a `replay` config. That does not widen what
/// the webview can do: `connect` already took a typed path for replays before this dialog existed.
#[tauri::command]
pub async fn pick_replay_log(app: AppHandle) -> Result<Option<String>, String> {
    let filter = Filter {
        name: "DataFlash log",
        extensions: &["bin", "BIN"],
    };
    let path = ask_open_path(&app, "Choose flight log", Some(filter)).await?;
    Ok(path.map(|p| p.to_string_lossy().into_owned()))
}
