//! Connection manager: opens a transport, parses MAVLink, tracks vehicles and link statistics,
//! and sends commands with ACK handling. The desktop shell talks only to [`LinkManager`].
mod manager;
mod replay;
mod scan;
mod types;

pub use manager::{LinkManager, Timing};
pub use types::{
    ConnectionConfig, LinkError, LinkEvent, LinkInfo, LinkState, LinkStats, TelemetrySnapshot,
    VehicleSummary,
};
