//! Tauri commands for the parameter screens. Thin wrappers over `ParamService`.
use std::collections::HashMap;

use control_station_ardupilot_meta::ParamMeta;
use control_station_link::ParamService;
use control_station_params::{Param, ParsedFile};
use tauri::State;

pub struct ParamsState {
    pub service: ParamService,
}

/// Downloads every parameter of the selected vehicle; resolves to how many arrived.
#[tauri::command]
pub async fn params_refresh(state: State<'_, ParamsState>) -> Result<usize, String> {
    state.service.refresh().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn params_list(state: State<'_, ParamsState>) -> Vec<Param> {
    state.service.list()
}

/// Writes one parameter; resolves to the value the vehicle confirmed.
#[tauri::command]
pub async fn param_set(
    name: String,
    value: f64,
    state: State<'_, ParamsState>,
) -> Result<f64, String> {
    state
        .service
        .set(&name, value)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn params_metadata(
    state: State<'_, ParamsState>,
) -> Result<HashMap<String, ParamMeta>, String> {
    Ok(state.service.metadata().await.as_ref().clone())
}

#[tauri::command]
pub fn params_parse_file(text: String, state: State<'_, ParamsState>) -> ParsedFile {
    state.service.parse_file(&text)
}

#[tauri::command]
pub fn params_format_file(state: State<'_, ParamsState>) -> String {
    state.service.format_file()
}
