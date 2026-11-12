//! Tauri commands for the Calibration screen (accelerometer, compass, ...). Each one calls
//! `CalibrationService`; progress reaches the UI through the `calibration` event from `main()`.

use control_station_link::{CalibrationKind, CalibrationService, CalibrationState};
use tauri::State;

fn text<T>(result: Result<T, control_station_link::LinkError>) -> Result<T, String> {
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn calibration_start(
    kind: CalibrationKind,
    service: State<'_, CalibrationService>,
) -> Result<String, String> {
    text(service.start(kind).await)
}

#[tauri::command]
pub fn calibration_accel_confirm(service: State<'_, CalibrationService>) -> Result<String, String> {
    text(service.accel_confirm())
}

#[tauri::command]
pub async fn calibration_compass_accept(
    service: State<'_, CalibrationService>,
) -> Result<String, String> {
    text(service.compass_accept().await)
}

#[tauri::command]
pub async fn calibration_compass_cancel(
    service: State<'_, CalibrationService>,
) -> Result<String, String> {
    text(service.compass_cancel().await)
}

#[tauri::command]
pub async fn calibration_cancel(service: State<'_, CalibrationService>) -> Result<String, String> {
    text(service.cancel().await)
}

#[tauri::command]
pub async fn radio_calibration_finish(
    service: State<'_, CalibrationService>,
) -> Result<String, String> {
    text(service.radio_finish().await)
}

#[tauri::command]
pub fn calibration_status(service: State<'_, CalibrationService>) -> CalibrationState {
    service.status()
}
