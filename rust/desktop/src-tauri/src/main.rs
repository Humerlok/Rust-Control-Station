#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod calibration_cmds;
mod dronecan_cmds;
mod firmware_cmds;
mod ftp_cmds;
mod inspector_cmds;
mod mission_cmds;
mod motor_cmds;
mod params_cmds;

use std::sync::Arc;

use control_station_commands::{ActionInfo, ACTIONS};
use control_station_link::{
    CalibrationService, ConnectionConfig, DroneCanService, FirmwareService, FtpService,
    InspectorService, LinkEvent, LinkInfo, LinkManager, MissionService, MotorTestService,
    ParamService, ParamWriter, TelemetrySnapshot,
};
use control_station_transport as transport;
use control_station_vehicle_state::{FieldInfo, VehicleId, FIELDS};
use serde::Serialize;
use tauri::{Emitter, Manager, State};
use tokio::sync::broadcast::error::RecvError;

struct AppState {
    link: LinkManager,
}

#[derive(Serialize)]
struct SerialPortDto {
    name: String,
    description: String,
    vid: Option<u16>,
    pid: Option<u16>,
}

fn text<T>(result: Result<T, control_station_link::LinkError>) -> Result<T, String> {
    result.map_err(|e| e.to_string())
}

#[tauri::command]
fn list_serial_ports() -> Vec<SerialPortDto> {
    transport::list_serial_ports()
        .into_iter()
        .map(|p| SerialPortDto {
            name: p.name,
            description: p.description,
            vid: p.vid,
            pid: p.pid,
        })
        .collect()
}

#[tauri::command]
async fn connect(config: ConnectionConfig, state: State<'_, AppState>) -> Result<(), String> {
    text(state.link.connect(config).await)
}

#[tauri::command]
fn disconnect(state: State<'_, AppState>) {
    state.link.disconnect();
}

#[tauri::command]
fn link_status(state: State<'_, AppState>) -> LinkInfo {
    state.link.link_info()
}

#[tauri::command]
fn telemetry_snapshot(state: State<'_, AppState>) -> TelemetrySnapshot {
    state.link.telemetry()
}

#[tauri::command]
fn select_vehicle(system_id: u8, component_id: u8, state: State<'_, AppState>) {
    state.link.select_vehicle(VehicleId {
        system_id,
        component_id,
    });
}

#[tauri::command]
fn quick_fields() -> &'static [FieldInfo] {
    FIELDS
}

#[tauri::command]
fn list_actions() -> &'static [ActionInfo] {
    ACTIONS
}

#[tauri::command]
async fn arm(force: bool, state: State<'_, AppState>) -> Result<String, String> {
    text(state.link.arm(force).await)
}

#[tauri::command]
async fn disarm(force: bool, state: State<'_, AppState>) -> Result<String, String> {
    text(state.link.disarm(force).await)
}

#[tauri::command]
async fn set_mode(mode: String, state: State<'_, AppState>) -> Result<String, String> {
    text(state.link.set_mode(&mode).await)
}

#[tauri::command]
async fn set_waypoint(seq: u16, state: State<'_, AppState>) -> Result<String, String> {
    text(state.link.set_waypoint(seq).await)
}

#[tauri::command]
async fn restart_mission(state: State<'_, AppState>) -> Result<String, String> {
    text(state.link.set_waypoint(0).await)
}

#[tauri::command]
async fn do_action(name: String, state: State<'_, AppState>) -> Result<String, String> {
    text(state.link.do_action(&name).await)
}

#[tauri::command]
async fn reboot(state: State<'_, AppState>) -> Result<String, String> {
    text(state.link.reboot().await)
}

#[tauri::command]
async fn takeoff(altitude_m: f32, state: State<'_, AppState>) -> Result<String, String> {
    text(state.link.takeoff(altitude_m).await)
}

#[tauri::command]
async fn fly_to(
    lat: f64,
    lon: f64,
    altitude_m: f32,
    state: State<'_, AppState>,
) -> Result<String, String> {
    text(state.link.fly_to(lat, lon, altitude_m).await)
}

#[tauri::command]
async fn set_home_here(
    lat: f64,
    lon: f64,
    altitude_m: f32,
    state: State<'_, AppState>,
) -> Result<String, String> {
    text(state.link.set_home(lat, lon, altitude_m).await)
}

fn main() {
    let link = LinkManager::new();
    let mut events = link.subscribe();
    let params_link = link.clone();
    let calibration_link = link.clone();
    let inspector_link = link.clone();
    let mission_link = link.clone();
    let ftp_link = link.clone();
    let firmware_link = link.clone();
    let motor_link = link.clone();
    let dronecan_link = link.clone();
    tauri::Builder::default()
        .manage(AppState { link })
        .setup(move |app| {
            let handle = app.handle().clone();

            let cache_dir = app
                .path()
                .app_cache_dir()
                .unwrap_or_else(|_| std::env::temp_dir())
                .join("param-metadata");
            // MAVFTP: the file screen, and the fast parameter download.
            let ftp = FtpService::new(ftp_link);
            let mut ftp_progress = ftp.subscribe_progress();
            app.manage(ftp.clone());
            let ftp_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match ftp_progress.recv().await {
                        Ok(p) => {
                            if let Err(error) = ftp_handle.emit("ftp-progress", p) {
                                eprintln!("failed to emit ftp-progress: {error}");
                            }
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            });

            // Firmware: manifest, `.apj` download and bootloader flashing.
            let firmware = FirmwareService::new(firmware_link);
            let mut flash_progress = firmware.subscribe_progress();
            app.manage(firmware);
            app.manage(firmware_cmds::FirmwareState::default());
            let flash_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match flash_progress.recv().await {
                        Ok(p) => {
                            if let Err(error) = flash_handle.emit("flash-progress", p) {
                                eprintln!("failed to emit flash-progress: {error}");
                            }
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            });

            let params = ParamService::new(params_link)
                .with_cache_dir(cache_dir)
                .with_ftp(ftp);
            let mut progress = params.subscribe_progress();
            app.manage(params_cmds::ParamsState {
                service: params.clone(),
            });
            let params_handle = handle.clone();

            // Calibrations write RC/ESC parameters through the parameter service.
            let writer: ParamWriter = {
                let params = params.clone();
                Arc::new(move |name, value| {
                    let params = params.clone();
                    Box::pin(
                        async move { params.set(&name, value).await.map_err(|e| e.to_string()) },
                    )
                })
            };
            // DroneCAN over the MAVLink CAN forward.
            let dronecan = DroneCanService::new(dronecan_link);
            let mut dronecan_events = dronecan.subscribe();
            app.manage(dronecan.clone());
            let dronecan_handle = handle.clone();
            tauri::async_runtime::spawn(async move { dronecan.run().await });
            tauri::async_runtime::spawn(async move {
                loop {
                    match dronecan_events.recv().await {
                        Ok(state) => {
                            if let Err(error) = dronecan_handle.emit("dronecan", state) {
                                eprintln!("failed to emit dronecan: {error}");
                            }
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            });

            // The motor test writes MOT_SPIN_ARM / MOT_SPIN_MIN the same way.
            let motor = MotorTestService::new(motor_link, writer.clone());
            let mut motor_events = motor.subscribe();
            app.manage(motor.clone());
            let motor_handle = handle.clone();
            tauri::async_runtime::spawn(async move { motor.run().await });
            tauri::async_runtime::spawn(async move {
                loop {
                    match motor_events.recv().await {
                        Ok(state) => {
                            if let Err(error) = motor_handle.emit("motor-test", state) {
                                eprintln!("failed to emit motor-test: {error}");
                            }
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            });

            let calibration = CalibrationService::new(calibration_link, writer);
            let mut calibration_events = calibration.subscribe();
            app.manage(calibration.clone());
            let calibration_handle = handle.clone();
            tauri::async_runtime::spawn(async move { calibration.run().await });
            tauri::async_runtime::spawn(async move {
                loop {
                    match calibration_events.recv().await {
                        Ok(state) => {
                            if let Err(error) = calibration_handle.emit("calibration", state) {
                                eprintln!("failed to emit calibration: {error}");
                            }
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            });

            let inspector = InspectorService::new(inspector_link);
            let mut inspector_events = inspector.subscribe();
            app.manage(inspector.clone());
            let inspector_handle = handle.clone();
            tauri::async_runtime::spawn(async move { inspector.run().await });
            tauri::async_runtime::spawn(async move {
                loop {
                    match inspector_events.recv().await {
                        Ok(snapshot) => {
                            if let Err(error) = inspector_handle.emit("inspector", snapshot) {
                                eprintln!("failed to emit inspector: {error}");
                            }
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            });

            let mission = MissionService::new(mission_link.clone());
            let mut mission_progress = mission.subscribe_progress();
            app.manage(mission_cmds::MissionState {
                service: mission,
                link: mission_link,
            });
            let mission_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match mission_progress.recv().await {
                        Ok(p) => {
                            if let Err(error) = mission_handle.emit("mission-progress", p) {
                                eprintln!("failed to emit mission-progress: {error}");
                            }
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            });

            tauri::async_runtime::spawn(async move {
                params.start();
                loop {
                    match progress.recv().await {
                        Ok(p) => {
                            if let Err(error) = params_handle.emit("params-progress", p) {
                                eprintln!("failed to emit params-progress: {error}");
                            }
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            });

            tauri::async_runtime::spawn(async move {
                loop {
                    let sent = match events.recv().await {
                        Ok(LinkEvent::Telemetry(t)) => handle.emit("telemetry", t),
                        Ok(LinkEvent::StatusText(s)) => handle.emit("statustext", s),
                        Ok(LinkEvent::Link(l)) => handle.emit("link", l),
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    };
                    if let Err(error) = sent {
                        eprintln!("failed to emit event: {error}");
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_serial_ports,
            connect,
            disconnect,
            link_status,
            telemetry_snapshot,
            select_vehicle,
            quick_fields,
            list_actions,
            arm,
            disarm,
            set_mode,
            set_waypoint,
            restart_mission,
            do_action,
            reboot,
            takeoff,
            fly_to,
            set_home_here,
            params_cmds::params_refresh,
            params_cmds::params_list,
            params_cmds::param_set,
            params_cmds::params_metadata,
            params_cmds::params_parse_file,
            params_cmds::params_format_file,
            calibration_cmds::calibration_start,
            calibration_cmds::calibration_accel_confirm,
            calibration_cmds::calibration_compass_accept,
            calibration_cmds::calibration_compass_cancel,
            calibration_cmds::calibration_cancel,
            calibration_cmds::radio_calibration_finish,
            calibration_cmds::calibration_status,
            motor_cmds::motor_frame,
            motor_cmds::motor_start,
            motor_cmds::motor_start_all,
            motor_cmds::motor_start_sequence,
            motor_cmds::motor_stop_all,
            motor_cmds::motor_set_spin,
            motor_cmds::motor_watch_esc,
            motor_cmds::motor_status,
            dronecan_cmds::dronecan_start,
            dronecan_cmds::dronecan_stop,
            dronecan_cmds::dronecan_status,
            dronecan_cmds::dronecan_params,
            dronecan_cmds::dronecan_set_param,
            dronecan_cmds::dronecan_save_params,
            dronecan_cmds::dronecan_restart,
            dronecan_cmds::dronecan_inspect_firmware,
            dronecan_cmds::dronecan_update_firmware,
            dronecan_cmds::dronecan_cancel_update,
            dronecan_cmds::dronecan_set_allocation,
            inspector_cmds::inspector_enable,
            inspector_cmds::inspector_watch,
            inspector_cmds::inspector_unwatch,
            inspector_cmds::inspector_clear,
            mission_cmds::mission_download,
            mission_cmds::mission_upload,
            mission_cmds::mission_clear,
            mission_cmds::mission_commands,
            mission_cmds::mission_parse_file,
            mission_cmds::mission_format_file,
            ftp_cmds::ftp_list,
            ftp_cmds::ftp_download,
            ftp_cmds::ftp_upload,
            ftp_cmds::ftp_remove,
            ftp_cmds::ftp_mkdir,
            ftp_cmds::ftp_rmdir,
            ftp_cmds::ftp_rename,
            ftp_cmds::ftp_crc,
            ftp_cmds::ftp_cancel,
            firmware_cmds::firmware_catalog,
            firmware_cmds::firmware_builds,
            firmware_cmds::firmware_load_url,
            firmware_cmds::firmware_load_file,
            firmware_cmds::firmware_reboot_bootloader,
            firmware_cmds::firmware_identify,
            firmware_cmds::firmware_flash,
            firmware_cmds::firmware_cancel
        ])
        .run(tauri::generate_context!())
        .expect("failed to run desktop application");
}
