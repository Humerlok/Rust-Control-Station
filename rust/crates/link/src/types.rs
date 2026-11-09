use std::collections::BTreeMap;

use control_station_vehicle_state::{Position, StatusText, VehicleId};
use serde::{Deserialize, Serialize};

/// How to reach the vehicle. The JSON form is what the desktop UI sends.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ConnectionConfig {
    /// Scans serial ports and bauds for a MAVLink device.
    Auto,
    Serial {
        port: String,
        baud: u32,
    },
    Tcp {
        host: String,
        port: u16,
    },
    Udp {
        port: u16,
    },
    Udpcl {
        host: String,
        port: u16,
    },
    Ws {
        url: String,
    },
    /// Plays a DataFlash `.BIN` log as if it were a live vehicle.
    Replay {
        path: String,
        speed: Option<f64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkState {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkStats {
    pub bytes_rx: u64,
    pub bytes_tx: u64,
    pub packets_rx: u64,
    pub packets_lost: u64,
    pub bad_crc: u64,
    pub packets_per_second: f64,
    pub bytes_per_second: f64,
    pub quality_percent: f64,
}

impl Default for LinkStats {
    fn default() -> Self {
        Self {
            bytes_rx: 0,
            bytes_tx: 0,
            packets_rx: 0,
            packets_lost: 0,
            bad_crc: 0,
            packets_per_second: 0.0,
            bytes_per_second: 0.0,
            quality_percent: 100.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkInfo {
    pub state: LinkState,
    pub description: String,
    pub error: Option<String>,
    pub stats: LinkStats,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VehicleSummary {
    pub system_id: u8,
    pub component_id: u8,
    pub vehicle_type: String,
    /// Selector label: the MAV_TYPE name, e.g. `QUADROTOR`, `GIMBAL`.
    pub component: String,
    /// False for gimbals, cameras, companion computers and other non-autopilot components.
    pub autopilot: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetrySnapshot {
    pub vehicle: Option<VehicleId>,
    /// True while heartbeats from the selected vehicle keep arriving.
    pub connected: bool,
    pub armed: bool,
    pub mode: Option<String>,
    pub vehicle_type: String,
    pub available_modes: Vec<String>,
    pub fields: BTreeMap<String, f64>,
    pub position: Option<Position>,
    pub home: Option<Position>,
    pub vehicles: Vec<VehicleSummary>,
}

#[derive(Debug, Clone)]
pub enum LinkEvent {
    Link(LinkInfo),
    Telemetry(TelemetrySnapshot),
    StatusText(StatusText),
}

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("no vehicle connected; nothing was sent")]
    NoVehicle,
    #[error("{0}")]
    Transport(#[from] control_station_transport::TransportError),
    #[error("denied: {0}")]
    Denied(&'static str),
    #[error("no answer from the vehicle (no ACK)")]
    NoAck,
    #[error("unknown flight mode '{0}' for this vehicle")]
    UnknownMode(String),
    #[error("unknown action '{0}'")]
    UnknownAction(String),
    #[error("send queue is full")]
    Busy,
    #[error("{0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(value: serde_json::Value) -> ConnectionConfig {
        serde_json::from_value(value).expect("valid config")
    }

    #[test]
    fn connection_configs_parse_from_the_ui_json() {
        assert_eq!(parse(json!({"type": "auto"})), ConnectionConfig::Auto);
        assert_eq!(
            parse(json!({"type": "serial", "port": "COM3", "baud": 115200})),
            ConnectionConfig::Serial {
                port: "COM3".into(),
                baud: 115_200
            }
        );
        assert_eq!(
            parse(json!({"type": "tcp", "host": "127.0.0.1", "port": 5760})),
            ConnectionConfig::Tcp {
                host: "127.0.0.1".into(),
                port: 5760
            }
        );
        assert_eq!(
            parse(json!({"type": "udp", "port": 14550})),
            ConnectionConfig::Udp { port: 14550 }
        );
        assert_eq!(
            parse(json!({"type": "udpcl", "host": "10.0.0.2", "port": 14550})),
            ConnectionConfig::Udpcl {
                host: "10.0.0.2".into(),
                port: 14550
            }
        );
        assert_eq!(
            parse(json!({"type": "ws", "url": "ws://host/mav"})),
            ConnectionConfig::Ws {
                url: "ws://host/mav".into()
            }
        );
        assert_eq!(
            parse(json!({"type": "replay", "path": "a.BIN"})),
            ConnectionConfig::Replay {
                path: "a.BIN".into(),
                speed: None
            }
        );
        assert_eq!(
            parse(json!({"type": "replay", "path": "a.BIN", "speed": 4.0})),
            ConnectionConfig::Replay {
                path: "a.BIN".into(),
                speed: Some(4.0)
            }
        );
    }

    #[test]
    fn unknown_connection_type_is_rejected() {
        assert!(
            serde_json::from_value::<ConnectionConfig>(json!({"type": "carrier-pigeon"})).is_err()
        );
    }

    #[test]
    fn link_info_uses_camel_case_and_lowercase_state() {
        let info = LinkInfo {
            state: LinkState::Connected,
            description: "UDP :14550".into(),
            error: None,
            stats: LinkStats::default(),
        };
        let value = serde_json::to_value(info).unwrap();
        assert_eq!(value["state"], "connected");
        assert_eq!(value["error"], serde_json::Value::Null);
        for key in [
            "bytesRx",
            "bytesTx",
            "packetsRx",
            "packetsLost",
            "badCrc",
            "packetsPerSecond",
            "bytesPerSecond",
            "qualityPercent",
        ] {
            assert!(value["stats"].get(key).is_some(), "missing stats.{key}");
        }
    }

    #[test]
    fn telemetry_snapshot_matches_the_ui_contract() {
        let snapshot = TelemetrySnapshot {
            vehicle: Some(VehicleId {
                system_id: 1,
                component_id: 1,
            }),
            connected: true,
            armed: false,
            mode: Some("Loiter".into()),
            vehicle_type: "Copter".into(),
            available_modes: vec!["Loiter".into()],
            fields: BTreeMap::from([("roll".to_owned(), 1.5)]),
            position: Some(Position {
                lat: -20.5,
                lon: -47.4,
                alt_m: 10.0,
            }),
            home: None,
            vehicles: vec![VehicleSummary {
                system_id: 1,
                component_id: 1,
                vehicle_type: "Copter".into(),
                component: "QUADROTOR".into(),
                autopilot: true,
            }],
        };
        let value = serde_json::to_value(snapshot).unwrap();
        assert_eq!(value["vehicle"], json!({"systemId": 1, "componentId": 1}));
        assert_eq!(value["vehicleType"], "Copter");
        assert_eq!(value["availableModes"], json!(["Loiter"]));
        assert_eq!(value["fields"]["roll"], 1.5);
        assert_eq!(value["position"]["altM"], 10.0);
        assert_eq!(value["home"], serde_json::Value::Null);
        assert_eq!(value["vehicles"][0]["systemId"], 1);
        assert_eq!(value["vehicles"][0]["component"], "QUADROTOR");
        assert_eq!(value["vehicles"][0]["autopilot"], true);
    }

    #[test]
    fn status_text_serializes_camel_case() {
        let value = serde_json::to_value(StatusText {
            time_ms: 5,
            severity: 4,
            text: "hi".into(),
            system_id: 1,
            component_id: 1,
        })
        .unwrap();
        assert_eq!(
            value,
            json!({"timeMs": 5, "severity": 4, "text": "hi", "systemId": 1, "componentId": 1})
        );
    }
}
