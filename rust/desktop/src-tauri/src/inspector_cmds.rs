//! Tauri commands for the MAVLink inspector. Thin wrappers over `InspectorService`.
use control_station_link::InspectorService;
use tauri::State;

/// Starts or stops the inspector; enabling resets its counters.
#[tauri::command]
pub fn inspector_enable(enabled: bool, service: State<'_, InspectorService>) {
    service.set_enabled(enabled);
}

/// Starts recording a numeric field: `systemId/componentId/messageId/field` or `field[3]`.
#[tauri::command]
pub fn inspector_watch(key: String, service: State<'_, InspectorService>) -> Result<(), String> {
    service.watch(&key)
}

#[tauri::command]
pub fn inspector_unwatch(key: String, service: State<'_, InspectorService>) {
    service.unwatch(&key);
}

#[tauri::command]
pub fn inspector_clear(service: State<'_, InspectorService>) {
    service.clear();
}
