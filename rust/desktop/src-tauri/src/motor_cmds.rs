use control_station_link::{
    MotorFrame, MotorTestRequest, MotorTestService, MotorTestState, SpinParam,
};
use tauri::State;

fn text<T>(result: Result<T, control_station_link::LinkError>) -> Result<T, String> {
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn motor_frame(
    class: Option<u32>,
    frame_type: Option<u32>,
    generic_count: Option<u8>,
    service: State<'_, MotorTestService>,
) -> Result<MotorFrame, String> {
    text(service.frame(class, frame_type, generic_count))
}

#[tauri::command]
pub async fn motor_start(
    request: MotorTestRequest,
    service: State<'_, MotorTestService>,
) -> Result<String, String> {
    text(service.start(request).await)
}

#[tauri::command]
pub async fn motor_start_all(
    request: MotorTestRequest,
    count: u8,
    service: State<'_, MotorTestService>,
) -> Result<String, String> {
    text(service.start_all(request, count).await)
}

#[tauri::command]
pub async fn motor_start_sequence(
    request: MotorTestRequest,
    count: u8,
    service: State<'_, MotorTestService>,
) -> Result<String, String> {
    text(service.start_sequence(request, count).await)
}

#[tauri::command]
pub async fn motor_stop_all(
    count: u8,
    service: State<'_, MotorTestService>,
) -> Result<String, String> {
    text(service.stop_all(count).await)
}

#[tauri::command]
pub async fn motor_set_spin(
    which: SpinParam,
    percent: f32,
    service: State<'_, MotorTestService>,
) -> Result<String, String> {
    text(service.set_spin(which, percent).await)
}

#[tauri::command]
pub fn motor_watch_esc(enabled: bool, service: State<'_, MotorTestService>) -> Result<(), String> {
    text(service.watch_esc(enabled))
}

#[tauri::command]
pub fn motor_status(service: State<'_, MotorTestService>) -> MotorTestState {
    service.status()
}
