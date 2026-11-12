//! Connection manager: opens a transport, parses MAVLink, tracks vehicles and link statistics,
//! and sends commands with ACK handling. The desktop shell talks only to [`LinkManager`].
mod calibration;
mod dronecan;
mod dronecan_inspector;
mod firmware;
mod ftp;
mod inspector;
mod logs;
mod manager;
mod mission;
mod motor_test;
mod params;
mod replay;
mod scan;
mod types;

pub use calibration::{
    CalState, CalibrationKind, CalibrationService, CalibrationState, ParamWriter, RadioCal,
};
pub use control_station_firmware::{BoardInfo, Firmware, ManifestEntry};
pub use control_station_mavftp::{DirEntry, FtpError};
pub use control_station_mission::MissionItem;
pub use dronecan::{
    Allocation, AllocationState, DroneCanLog, DroneCanNode, DroneCanService, DroneCanState,
    DroneCanTiming, FirmwareUpdateState, NodeImageInfo, NodeParam, NodeParamValue,
    ParamsProgress as NodeParamsProgress, UpdatePhase, GCS_NODE_ID,
};
pub use firmware::{FirmwareService, FirmwareServiceError, FirmwareTiming, FlashProgress};
pub use ftp::{
    describe_errno, FtpProgress, FtpService, FtpServiceError, FtpTiming, PARAM_PCK_PATH,
};
pub use inspector::{InspectorField, InspectorMessage, InspectorService, InspectorSnapshot};
pub use logs::{
    LogEntry, LogError, LogMethod, LogProgress, LogService, LogSource, LogTiming, LOG_DATA_LEN,
    LOG_DIR,
};
pub use manager::{LinkManager, Timing};
pub use mission::{
    MissionError, MissionKind, MissionPhase, MissionProgress, MissionService, MissionTiming,
};
pub use motor_test::{
    validate as validate_motor_test, EscReading, MotorFrame, MotorSlot, MotorTestRequest,
    MotorTestService, MotorTestState, SpinParam, HIGH_THROTTLE_PERCENT, MAX_SECONDS,
    MAX_SPIN_PERCENT,
};
pub use params::{ParamError, ParamProgress, ParamService, ParamTiming};
pub use types::{
    ConnectionConfig, LinkError, LinkEvent, LinkInfo, LinkState, LinkStats, TelemetrySnapshot,
    VehicleSummary,
};
