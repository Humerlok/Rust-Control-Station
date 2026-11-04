//! Connection manager: opens a transport, parses MAVLink, tracks vehicles and link statistics,
//! and sends commands with ACK handling. The desktop shell talks only to [`LinkManager`].
mod calibration;
mod manager;
mod params;
mod replay;
mod scan;
mod types;

pub use calibration::{
    CalState, CalibrationKind, CalibrationService, CalibrationState, ParamWriter, RadioCal,
};
pub use manager::{LinkManager, Timing};
pub use params::{ParamError, ParamProgress, ParamService, ParamTiming};
pub use types::{
    ConnectionConfig, LinkError, LinkEvent, LinkInfo, LinkState, LinkStats, TelemetrySnapshot,
    VehicleSummary,
};
