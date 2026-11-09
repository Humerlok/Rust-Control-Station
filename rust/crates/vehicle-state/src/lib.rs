//! Live state of a vehicle, decoded from MAVLink messages. No I/O and no UI shapes here:
//! `link` feeds packets in, the desktop shell reads snapshots out.
use control_station_ardupilot_meta::VehicleType;
use control_station_mavlink::{
    dialect::{MavAutopilot, MavModeFlag, MavType, STATUSTEXT_DATA},
    MavMessage,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VehicleId {
    pub system_id: u8,
    pub component_id: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub lat: f64,
    pub lon: f64,
    pub alt_m: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusText {
    pub time_ms: u64,
    pub severity: u8,
    pub text: String,
    pub system_id: u8,
    pub component_id: u8,
}

mod fields;

pub use fields::{FieldInfo, FIELDS};

const SAFETY_ARMED: u8 = 128;

/// MAV_TYPE values that are never an autopilot (GCS, companions, gimbals, peripherals...).
fn is_peripheral_type(mav_type: u8) -> bool {
    matches!(mav_type, 6 | 18 | 26 | 27 | 30..=34 | 36..=42)
}

/// True when a HEARTBEAT comes from an autopilot rather than a GCS or peripheral.
pub fn is_vehicle_heartbeat(h: &control_station_mavlink::dialect::HEARTBEAT_DATA) -> bool {
    h.autopilot != MavAutopilot::MAV_AUTOPILOT_INVALID && !is_peripheral_type(h.mavtype as u8)
}

/// MAV_COMP_ID_MISSIONPLANNER: ground stations, never a target.
const GCS_COMPONENT: u8 = 190;

/// True when a HEARTBEAT comes from something the user can target: an autopilot or any other
/// component (gimbal, camera, companion computer...), but not a ground station.
pub fn is_component_heartbeat(
    h: &control_station_mavlink::dialect::HEARTBEAT_DATA,
    component_id: u8,
) -> bool {
    // Component 0 is MAV_COMP_ID_ALL, a broadcast address and never a valid source (ArduPilot's
    // ADSB driver sends such heartbeats when ADSB_TYPE is set).
    h.mavtype != MavType::MAV_TYPE_GCS && component_id != GCS_COMPONENT && component_id != 0
}

/// A STATUSTEXT as an event, tagged with the component that sent it.
pub fn status_text(id: VehicleId, t: &STATUSTEXT_DATA, unix_ms: u64) -> StatusText {
    let bytes: &[u8] = &*t.text;
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    StatusText {
        time_ms: unix_ms,
        severity: t.severity as u8,
        text: String::from_utf8_lossy(&bytes[..end]).into_owned(),
        system_id: id.system_id,
        component_id: id.component_id,
    }
}

#[derive(Debug, Clone)]
pub struct VehicleState {
    pub id: VehicleId,
    pub vehicle_type: VehicleType,
    pub armed: bool,
    pub base_mode: u8,
    pub custom_mode: u32,
    pub position: Option<Position>,
    pub home: Option<Position>,
    pub last_heartbeat: Option<Instant>,
    /// Whether the heartbeats come from an autopilot (flight modes, arming, streams) rather
    /// than another component such as a gimbal or a companion computer.
    pub autopilot: bool,
    /// MAV_TYPE of the last heartbeat.
    pub mav_type: MavType,
    /// Last VFR_HUD, for the time-in-air and distance-traveled accumulators.
    last_flight_tick: Option<Instant>,
    fields: BTreeMap<&'static str, f64>,
}

impl VehicleState {
    pub fn new(id: VehicleId) -> Self {
        Self {
            id,
            vehicle_type: VehicleType::Unknown,
            armed: false,
            base_mode: 0,
            custom_mode: 0,
            position: None,
            home: None,
            last_heartbeat: None,
            autopilot: false,
            mav_type: MavType::MAV_TYPE_GENERIC,
            last_flight_tick: None,
            fields: BTreeMap::new(),
        }
    }

    pub fn fields(&self) -> &BTreeMap<&'static str, f64> {
        &self.fields
    }

    /// Name for the vehicle selector, like the legacy `MAV_TYPE` label: `QUADROTOR`, `GIMBAL`.
    pub fn component_name(&self) -> String {
        format!("{:?}", self.mav_type)
            .trim_start_matches("MAV_TYPE_")
            .replace('_', " ")
    }

    pub fn mode(&self) -> Option<String> {
        self.last_heartbeat?;
        if !self.autopilot {
            return None;
        }
        Some(
            self.vehicle_type
                .mode_name(self.custom_mode)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Mode {}", self.custom_mode)),
        )
    }

    pub fn is_alive(&self, now: Instant, timeout: Duration) -> bool {
        self.last_heartbeat
            .is_some_and(|t| now.saturating_duration_since(t) < timeout)
    }

    /// Applies one message from this vehicle. Returns a status text if the message carried one.
    pub fn apply(
        &mut self,
        message: &MavMessage,
        now: Instant,
        unix_ms: u64,
    ) -> Option<StatusText> {
        match message {
            MavMessage::HEARTBEAT(h) => {
                if is_component_heartbeat(h, self.id.component_id) {
                    self.mav_type = h.mavtype;
                    self.last_heartbeat = Some(now);
                }
                if is_vehicle_heartbeat(h) {
                    self.autopilot = true;
                    self.vehicle_type = VehicleType::from_mav_type(h.mavtype as u8);
                    self.base_mode = h.base_mode.bits();
                    self.armed = h
                        .base_mode
                        .contains(MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED)
                        || self.base_mode & SAFETY_ARMED != 0;
                    self.custom_mode = h.custom_mode;
                    self.last_heartbeat = Some(now);
                    self.set("armed", if self.armed { 1.0 } else { 0.0 });
                }
            }
            MavMessage::GLOBAL_POSITION_INT(p) => {
                if p.lat != 0 || p.lon != 0 {
                    let position = Position {
                        lat: f64::from(p.lat) / 1e7,
                        lon: f64::from(p.lon) / 1e7,
                        alt_m: p.alt as f32 / 1000.0,
                    };
                    self.position = Some(position);
                    self.update_home_distance();
                }
                self.set("alt", f64::from(p.relative_alt) / 1000.0);
                self.set("altasl", f64::from(p.alt) / 1000.0);
                self.set("verticalspeed", -f64::from(p.vz) / 100.0);
                if p.hdg != u16::MAX {
                    self.set("heading", f64::from(p.hdg) / 100.0);
                }
                self.decode_fields(message, now);
            }
            MavMessage::HOME_POSITION(h) => {
                self.home = Some(Position {
                    lat: f64::from(h.latitude) / 1e7,
                    lon: f64::from(h.longitude) / 1e7,
                    alt_m: h.altitude as f32 / 1000.0,
                });
                self.update_home_distance();
            }
            MavMessage::STATUSTEXT(t) => return Some(status_text(self.id, t, unix_ms)),
            other => self.decode_fields(other, now),
        }
        None
    }

    fn set(&mut self, key: &'static str, value: f64) {
        self.fields.insert(key, value);
    }

    fn update_home_distance(&mut self) {
        if let (Some(p), Some(h)) = (self.position, self.home) {
            self.set("dist_to_home", haversine_m(p.lat, p.lon, h.lat, h.lon));
        }
    }
}

fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_M: f64 = 6_371_000.0;
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dphi = p2 - p1;
    let dlambda = (lon2 - lon1).to_radians();
    let a = (dphi / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dlambda / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * a.sqrt().asin()
}

#[cfg(test)]
mod tests {
    use super::*;
    use control_station_mavlink::dialect::{
        MavState, MavType, ATTITUDE_DATA, GLOBAL_POSITION_INT_DATA, HEARTBEAT_DATA,
        HOME_POSITION_DATA, STATUSTEXT_DATA, SYS_STATUS_DATA,
    };

    fn vehicle() -> VehicleState {
        VehicleState::new(VehicleId {
            system_id: 1,
            component_id: 1,
        })
    }

    fn heartbeat(mavtype: MavType, autopilot: MavAutopilot, armed: bool, mode: u32) -> MavMessage {
        MavMessage::HEARTBEAT(HEARTBEAT_DATA {
            custom_mode: mode,
            mavtype,
            autopilot,
            base_mode: if armed {
                MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED
            } else {
                MavModeFlag::empty()
            },
            system_status: MavState::MAV_STATE_ACTIVE,
            mavlink_version: 3,
        })
    }

    #[test]
    fn new_vehicle_is_not_alive_and_has_no_mode() {
        let v = vehicle();
        assert!(!v.is_alive(Instant::now(), Duration::from_secs(5)));
        assert_eq!(v.mode(), None);
    }

    #[test]
    fn heartbeat_sets_type_mode_armed_and_liveness() {
        let mut v = vehicle();
        let t0 = Instant::now();
        let hb = heartbeat(
            MavType::MAV_TYPE_QUADROTOR,
            MavAutopilot::MAV_AUTOPILOT_ARDUPILOTMEGA,
            true,
            5,
        );
        v.apply(&hb, t0, 0);
        assert_eq!(v.vehicle_type, VehicleType::Copter);
        assert!(v.armed);
        assert_eq!(v.mode().as_deref(), Some("Loiter"));
        assert!(v.is_alive(t0 + Duration::from_secs(2), Duration::from_secs(5)));
        assert!(!v.is_alive(t0 + Duration::from_secs(6), Duration::from_secs(5)));
    }

    #[test]
    fn peripheral_is_a_component_without_flight_modes() {
        let mut v = VehicleState::new(VehicleId {
            system_id: 1,
            component_id: 154,
        });
        v.apply(
            &heartbeat(
                MavType::MAV_TYPE_GIMBAL,
                MavAutopilot::MAV_AUTOPILOT_INVALID,
                false,
                3,
            ),
            Instant::now(),
            0,
        );
        assert!(v.is_alive(Instant::now(), Duration::from_secs(5)));
        assert!(!v.autopilot);
        assert_eq!(v.mode(), None);
        assert_eq!(v.component_name(), "GIMBAL");
    }

    #[test]
    fn gcs_heartbeats_are_not_components() {
        let gcs = heartbeat(
            MavType::MAV_TYPE_GCS,
            MavAutopilot::MAV_AUTOPILOT_INVALID,
            false,
            0,
        );
        let MavMessage::HEARTBEAT(h) = &gcs else {
            unreachable!()
        };
        assert!(!is_component_heartbeat(h, 1));
        let quad = heartbeat(
            MavType::MAV_TYPE_QUADROTOR,
            MavAutopilot::MAV_AUTOPILOT_ARDUPILOTMEGA,
            false,
            0,
        );
        let MavMessage::HEARTBEAT(h) = &quad else {
            unreachable!()
        };
        assert!(is_component_heartbeat(h, 1));
        assert!(!is_component_heartbeat(h, 190));
        assert!(!is_component_heartbeat(h, 0));
    }

    #[test]
    fn gcs_and_peripheral_heartbeats_are_not_vehicles() {
        let gcs = heartbeat(
            MavType::MAV_TYPE_GCS,
            MavAutopilot::MAV_AUTOPILOT_INVALID,
            false,
            0,
        );
        let gimbal = heartbeat(
            MavType::MAV_TYPE_GIMBAL,
            MavAutopilot::MAV_AUTOPILOT_ARDUPILOTMEGA,
            false,
            0,
        );
        for hb in [gcs, gimbal] {
            let MavMessage::HEARTBEAT(h) = &hb else {
                unreachable!()
            };
            assert!(!is_vehicle_heartbeat(h));
            let mut v = vehicle();
            v.apply(&hb, Instant::now(), 0);
            assert!(!v.autopilot);
            assert_eq!(v.vehicle_type, VehicleType::Unknown);
        }
    }

    #[test]
    fn attitude_is_converted_to_degrees() {
        let mut v = vehicle();
        v.apply(
            &MavMessage::ATTITUDE(ATTITUDE_DATA {
                roll: std::f32::consts::FRAC_PI_2,
                pitch: -std::f32::consts::FRAC_PI_4,
                yaw: -std::f32::consts::FRAC_PI_2,
                ..ATTITUDE_DATA::default()
            }),
            Instant::now(),
            0,
        );
        assert!((v.fields()["roll"] - 90.0).abs() < 1e-3);
        assert!((v.fields()["pitch"] + 45.0).abs() < 1e-3);
        assert!((v.fields()["yaw"] - 270.0).abs() < 1e-3);
    }

    #[test]
    fn position_home_and_distance() {
        let mut v = vehicle();
        let now = Instant::now();
        v.apply(
            &MavMessage::HOME_POSITION(HOME_POSITION_DATA {
                latitude: 0,
                longitude: 10_000_000,
                altitude: 100_000,
                ..HOME_POSITION_DATA::default()
            }),
            now,
            0,
        );
        v.apply(
            &MavMessage::GLOBAL_POSITION_INT(GLOBAL_POSITION_INT_DATA {
                lat: 10_000,
                lon: 10_000_000,
                alt: 120_000,
                relative_alt: 20_000,
                vz: -150,
                hdg: 9000,
                ..GLOBAL_POSITION_INT_DATA::default()
            }),
            now,
            0,
        );
        let p = v.position.unwrap();
        assert!((p.lat - 0.001).abs() < 1e-9);
        assert!((v.fields()["alt"] - 20.0).abs() < 1e-9);
        assert!((v.fields()["verticalspeed"] - 1.5).abs() < 1e-9);
        assert!((v.fields()["heading"] - 90.0).abs() < 1e-9);
        let d = v.fields()["dist_to_home"];
        assert!((d - 111.2).abs() < 1.0, "{d}");
    }

    #[test]
    fn zero_position_is_ignored_and_invalid_battery_skipped() {
        let mut v = vehicle();
        let now = Instant::now();
        v.apply(
            &MavMessage::GLOBAL_POSITION_INT(GLOBAL_POSITION_INT_DATA::default()),
            now,
            0,
        );
        assert!(v.position.is_none());
        v.apply(
            &MavMessage::SYS_STATUS(SYS_STATUS_DATA {
                voltage_battery: u16::MAX,
                current_battery: -1,
                battery_remaining: -1,
                ..SYS_STATUS_DATA::default()
            }),
            now,
            0,
        );
        assert!(!v.fields().contains_key("battery_voltage"));
        v.apply(
            &MavMessage::SYS_STATUS(SYS_STATUS_DATA {
                voltage_battery: 16_800,
                current_battery: 1234,
                battery_remaining: 87,
                ..SYS_STATUS_DATA::default()
            }),
            now,
            0,
        );
        assert!((v.fields()["battery_voltage"] - 16.8).abs() < 1e-9);
        assert!((v.fields()["battery_current"] - 12.34).abs() < 1e-9);
        assert_eq!(v.fields()["battery_remaining"], 87.0);
    }

    #[test]
    fn statustext_is_returned_with_origin() {
        let mut v = vehicle();
        let text = v.apply(
            &MavMessage::STATUSTEXT(STATUSTEXT_DATA {
                severity: control_station_mavlink::dialect::MavSeverity::MAV_SEVERITY_WARNING,
                text: "PreArm: Check GPS".into(),
                ..STATUSTEXT_DATA::default()
            }),
            Instant::now(),
            42,
        );
        let text = text.unwrap();
        assert_eq!(text.text, "PreArm: Check GPS");
        assert_eq!(text.severity, 4);
        assert_eq!(
            (text.system_id, text.component_id, text.time_ms),
            (1, 1, 42)
        );
    }

    #[test]
    fn every_quick_field_key_is_unique() {
        let mut keys: Vec<_> = FIELDS.iter().map(|f| f.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), FIELDS.len());
    }
}
