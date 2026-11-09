//! Tauri commands for the Flight Plan screen. Thin wrappers over `MissionService` and the
//! `.waypoints` / command-catalog helpers.
use control_station_ardupilot_meta::{mission_commands as catalog, CommandInfo, VehicleType};
use control_station_link::{LinkManager, MissionItem, MissionKind, MissionService};
use tauri::{AppHandle, State};

use crate::save_dialog::{ask_save_path, write_file, Filter};

pub struct MissionState {
    pub service: MissionService,
    pub link: LinkManager,
}

/// Downloads a mission, fence or rally list; for a mission, item 0 is home.
#[tauri::command]
pub async fn mission_download(
    kind: MissionKind,
    state: State<'_, MissionState>,
) -> Result<Vec<MissionItem>, String> {
    state
        .service
        .download(kind)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mission_upload(
    kind: MissionKind,
    items: Vec<MissionItem>,
    state: State<'_, MissionState>,
) -> Result<(), String> {
    state
        .service
        .upload(kind, &items)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mission_clear(
    kind: MissionKind,
    state: State<'_, MissionState>,
) -> Result<(), String> {
    state.service.clear(kind).await.map_err(|e| e.to_string())
}

/// The commands for the selected vehicle's family (Copter's list when no vehicle is alive).
#[tauri::command]
pub fn mission_commands(state: State<'_, MissionState>) -> Vec<CommandInfo> {
    let vehicle = state
        .link
        .selected_vehicle()
        .map_or(VehicleType::Copter, |(_, vehicle_type)| vehicle_type);
    catalog(vehicle)
}

#[tauri::command]
pub fn mission_parse_file(text: String) -> Result<Vec<MissionItem>, String> {
    control_station_mission::parse_waypoints(&text).map_err(|e| e.to_string())
}

/// Asks where to save, then writes `items` as a QGC WPL 110 `.waypoints` file. Resolves to the
/// saved path, or `None` when the user cancelled.
#[tauri::command]
pub async fn mission_save_file(
    default_name: String,
    items: Vec<MissionItem>,
    app: AppHandle,
) -> Result<Option<String>, String> {
    let filter = Filter {
        name: "Waypoint files",
        extensions: &["waypoints", "txt"],
    };
    let Some(dest) = ask_save_path(&app, "Save mission", &default_name, Some(filter)).await? else {
        return Ok(None);
    };
    write_file(
        &dest,
        control_station_mission::format_waypoints(&items).as_bytes(),
    )?;
    Ok(Some(dest.display().to_string()))
}
