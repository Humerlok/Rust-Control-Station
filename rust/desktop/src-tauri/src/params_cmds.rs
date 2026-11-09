//! Tauri commands for the parameter screens. Thin wrappers over `ParamService`.
use std::collections::HashMap;

use control_station_ardupilot_meta::ParamMeta;
use control_station_link::ParamService;
use control_station_params::{Param, ParsedFile};
use tauri::{AppHandle, State};

use crate::save_dialog::{ask_save_path, write_file, Filter};

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

/// Asks where to save, then writes every parameter in `.param` format. Resolves to the saved
/// path, or `None` when the user cancelled.
#[tauri::command]
pub async fn params_save_file(
    default_name: String,
    app: AppHandle,
    state: State<'_, ParamsState>,
) -> Result<Option<String>, String> {
    let filter = Filter {
        name: "Parameter files",
        extensions: &["param", "parm"],
    };
    let Some(dest) = ask_save_path(&app, "Save parameters", &default_name, Some(filter)).await?
    else {
        return Ok(None);
    };
    write_file(&dest, state.service.format_file().as_bytes())?;
    Ok(Some(dest.display().to_string()))
}
