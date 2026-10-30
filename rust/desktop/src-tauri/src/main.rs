#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use control_station_commands::{
    CalibrationKind, CalibrationSession, Parameter, ParameterValue, SetParameter,
};
use control_station_map_cache::{validate_download, OfflineRegion, TileProvider};
use control_station_vehicle_state::{VehicleId, VehicleSnapshot};
use std::sync::Mutex;
use tauri::State;

struct AppState {
    selected: Mutex<VehicleId>,
    parameters: Mutex<Vec<Parameter>>,
}
impl Default for AppState {
    fn default() -> Self {
        Self {
            selected: Mutex::new(VehicleId {
                system_id: 1,
                component_id: 1,
            }),
            parameters: Mutex::new(vec![
                Parameter {
                    name: "ARMING_CHECK".into(),
                    value: ParameterValue::Int(1),
                    component_id: 1,
                },
                Parameter {
                    name: "BATT_MONITOR".into(),
                    value: ParameterValue::Int(4),
                    component_id: 1,
                },
            ]),
        }
    }
}

#[tauri::command]
fn telemetry(state: State<'_, AppState>) -> VehicleSnapshot {
    VehicleSnapshot::discovered(*state.selected.lock().expect("state lock"))
}

#[tauri::command]
fn parameters(state: State<'_, AppState>) -> Vec<Parameter> {
    state.parameters.lock().expect("state lock").clone()
}

#[tauri::command]
fn set_parameter(request: SetParameter, state: State<'_, AppState>) -> Result<Parameter, String> {
    let target = *state.selected.lock().map_err(|_| "state unavailable")?;
    if request.target != target {
        return Err("target does not match selected vehicle".into());
    }
    let mut params = state.parameters.lock().map_err(|_| "state unavailable")?;
    if let Some(existing) = params.iter_mut().find(|p| {
        p.name == request.parameter.name && p.component_id == request.parameter.component_id
    }) {
        *existing = request.parameter.clone();
    } else {
        params.push(request.parameter.clone());
    }
    Ok(request.parameter)
}

#[tauri::command]
fn start_calibration(kind: CalibrationKind, state: State<'_, AppState>) -> CalibrationSession {
    CalibrationSession::begin(*state.selected.lock().expect("state lock"), kind)
}

#[tauri::command]
fn validate_offline_region(provider: TileProvider, region: OfflineRegion) -> Result<(), String> {
    validate_download(&provider, &region).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            telemetry,
            parameters,
            set_parameter,
            start_calibration,
            validate_offline_region
        ])
        .run(tauri::generate_context!())
        .expect("failed to run desktop application");
}
