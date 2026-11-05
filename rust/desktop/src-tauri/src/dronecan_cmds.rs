//! Tauri commands for DroneCAN. Thin wrappers over `DroneCanService`.
use base64::{engine::general_purpose::STANDARD, Engine};
use control_station_link::{
    DroneCanService, DroneCanState, NodeImageInfo, NodeParam, NodeParamValue,
};
use tauri::State;

fn text<T>(result: Result<T, control_station_link::LinkError>) -> Result<T, String> {
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn dronecan_start(bus: u8, service: State<'_, DroneCanService>) -> Result<String, String> {
    text(service.start(bus))
}

#[tauri::command]
pub fn dronecan_stop(service: State<'_, DroneCanService>) -> Result<String, String> {
    text(service.stop())
}

#[tauri::command]
pub fn dronecan_status(service: State<'_, DroneCanService>) -> DroneCanState {
    service.status()
}

#[tauri::command]
pub async fn dronecan_params(
    node: u8,
    service: State<'_, DroneCanService>,
) -> Result<Vec<NodeParam>, String> {
    text(service.list_params(node).await)
}

#[tauri::command]
pub async fn dronecan_set_param(
    node: u8,
    name: String,
    value: NodeParamValue,
    service: State<'_, DroneCanService>,
) -> Result<NodeParam, String> {
    text(service.set_param(node, &name, value).await)
}

#[tauri::command]
pub async fn dronecan_save_params(
    node: u8,
    service: State<'_, DroneCanService>,
) -> Result<String, String> {
    text(service.save_params(node).await)
}

#[tauri::command]
pub async fn dronecan_restart(
    node: u8,
    service: State<'_, DroneCanService>,
) -> Result<String, String> {
    text(service.restart_node(node).await)
}

/// Describes a firmware file (base64 of an `.apj` or a raw `.bin`) before it is sent.
#[tauri::command]
pub fn dronecan_inspect_firmware(data: String) -> Result<NodeImageInfo, String> {
    let bytes = STANDARD
        .decode(data.trim())
        .map_err(|e| format!("the file could not be read: {e}"))?;
    DroneCanService::inspect_image(&bytes).map_err(|e| e.to_string())
}

/// Serves a firmware file to a node; progress arrives in the `dronecan` events.
#[tauri::command]
pub async fn dronecan_update_firmware(
    node: u8,
    data: String,
    force: bool,
    service: State<'_, DroneCanService>,
) -> Result<String, String> {
    let bytes = STANDARD
        .decode(data.trim())
        .map_err(|e| format!("the file could not be read: {e}"))?;
    text(service.start_update(node, bytes, force).await)
}

#[tauri::command]
pub fn dronecan_cancel_update(service: State<'_, DroneCanService>) -> Result<String, String> {
    text(service.cancel_update())
}

#[tauri::command]
pub fn dronecan_set_allocation(
    enabled: bool,
    service: State<'_, DroneCanService>,
) -> Result<String, String> {
    text(service.set_allocation(enabled))
}
