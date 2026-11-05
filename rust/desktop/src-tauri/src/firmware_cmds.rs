//! Tauri commands for the Firmware screen. Thin wrappers over `FirmwareService`. The decoded
//! image stays on the Rust side (`FirmwareState`) so megabytes never cross the IPC boundary.
use std::sync::Mutex;

use base64::{engine::general_purpose::STANDARD, Engine};
use control_station_firmware::{boards, parse_apj, select, BoardSummary, Query};
use control_station_link::{BoardInfo, Firmware, FirmwareService, ManifestEntry};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Default)]
pub struct FirmwareState {
    catalog: Mutex<Vec<ManifestEntry>>,
    loaded: Mutex<Option<Firmware>>,
}

/// What the UI needs to show about the image that is ready to flash.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedFirmware {
    board_id: u32,
    description: String,
    version: String,
    git_hash: String,
    size: usize,
}

impl From<&Firmware> for LoadedFirmware {
    fn from(f: &Firmware) -> Self {
        Self {
            board_id: f.board_id,
            description: f.description.clone(),
            version: f.version.clone(),
            git_hash: f.git_hash.clone(),
            size: f.image.len(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildQuery {
    board_id: Option<u32>,
    vehicle_type: Option<String>,
    version_type: Option<String>,
    #[serde(default)]
    latest_only: bool,
}

/// Downloads the manifest and returns the boards that have builds.
#[tauri::command]
pub async fn firmware_catalog(
    firmware: State<'_, FirmwareService>,
    state: State<'_, FirmwareState>,
) -> Result<Vec<BoardSummary>, String> {
    let entries = firmware.catalog().await.map_err(|e| e.to_string())?;
    let list = boards(&entries);
    *state.catalog.lock().unwrap() = entries;
    Ok(list)
}

/// Builds from the downloaded manifest (call `firmware_catalog` first).
#[tauri::command]
pub fn firmware_builds(query: BuildQuery, state: State<'_, FirmwareState>) -> Vec<ManifestEntry> {
    let catalog = state.catalog.lock().unwrap();
    let query = Query {
        board_id: query.board_id,
        vehicle_type: query.vehicle_type,
        version_type: query.version_type,
        latest_only: query.latest_only,
    };
    select(&catalog, &query).into_iter().cloned().collect()
}

/// Downloads the `.apj` at `url` (official server only) and keeps it ready to flash.
#[tauri::command]
pub async fn firmware_load_url(
    url: String,
    firmware: State<'_, FirmwareService>,
    state: State<'_, FirmwareState>,
) -> Result<LoadedFirmware, String> {
    let loaded = firmware.download(&url).await.map_err(|e| e.to_string())?;
    let summary = LoadedFirmware::from(&loaded);
    *state.loaded.lock().unwrap() = Some(loaded);
    Ok(summary)
}

/// Uses a local `.apj` (base64) the user picked.
#[tauri::command]
pub fn firmware_load_file(
    data: String,
    state: State<'_, FirmwareState>,
) -> Result<LoadedFirmware, String> {
    let bytes = STANDARD
        .decode(data.as_bytes())
        .map_err(|e| format!("invalid file data: {e}"))?;
    let loaded = parse_apj(&bytes).map_err(|e| e.to_string())?;
    let summary = LoadedFirmware::from(&loaded);
    *state.loaded.lock().unwrap() = Some(loaded);
    Ok(summary)
}

/// Reboots the connected vehicle into its bootloader and disconnects.
#[tauri::command]
pub async fn firmware_reboot_bootloader(
    firmware: State<'_, FirmwareService>,
) -> Result<(), String> {
    firmware
        .reboot_to_bootloader()
        .await
        .map_err(|e| e.to_string())
}

/// Reads the board id and flash size from the bootloader on `port`; changes nothing.
#[tauri::command]
pub async fn firmware_identify(
    port: String,
    firmware: State<'_, FirmwareService>,
) -> Result<BoardInfo, String> {
    firmware.identify(&port).await.map_err(|e| e.to_string())
}

/// Flashes the loaded image. The service refuses a board id mismatch before erasing.
#[tauri::command]
pub async fn firmware_flash(
    port: String,
    firmware: State<'_, FirmwareService>,
    state: State<'_, FirmwareState>,
) -> Result<BoardInfo, String> {
    let image = state
        .loaded
        .lock()
        .unwrap()
        .clone()
        .ok_or("no firmware loaded")?;
    firmware
        .flash(&port, &image)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn firmware_cancel(firmware: State<'_, FirmwareService>) {
    firmware.cancel();
}
