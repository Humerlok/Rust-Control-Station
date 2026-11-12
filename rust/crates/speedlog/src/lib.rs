//! SpeedLOG: analysis of ArduPilot DataFlash (`.BIN`) logs for the log plotter.
//!
//! A Rust port of the data side of SpeedLOG (<https://github.com/Humerlok/SpeedLOG>, GPL-3.0), an
//! offline log viewer: the list of message types and fields (instances such as `IMU[0]` apart), the
//! time series of a field and its min/max reduction for a visible window, flight-mode spans, the
//! parameters with the ones changed during the log, the `MSG`/`ERR`/`EV` messages decoded, ready-made
//! panel presets and the ArduPilot documentation texts (English and Brazilian Portuguese), embedded so
//! everything works offline. Drawing stays in the UI.
//!
//! Pure: no I/O besides reading the log file, no Tauri, no React.
mod codes;
pub mod docs;
mod log;
mod series;
mod types;

pub use docs::{param_value_meaning, ParamDoc};
pub use log::{SpeedLog, BUCKETS, MAX_TRACES};
pub use series::{view, Series};
pub use types::*;
