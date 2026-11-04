//! Live state of a vehicle, decoded from MAVLink messages. No I/O and no UI shapes here:
//! `link` feeds packets in, the desktop shell reads snapshots out.
use control_station_ardupilot_meta::VehicleType;
use control_station_mavlink::{
    dialect::{MavAutopilot, MavModeFlag},
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FieldInfo {
    pub key: &'static str,
    pub label: &'static str,
    pub unit: &'static str,
}

const fn f(key: &'static str, label: &'static str, unit: &'static str) -> FieldInfo {
    FieldInfo { key, label, unit }
}

/// Numeric fields a user can put in the Quick View. Keys follow the legacy `CurrentState` names.
pub const FIELDS: &[FieldInfo] = &[
    f("alt", "Altitude (relative)", "m"),
    f("altasl", "Altitude (MSL)", "m"),
    f("groundspeed", "Ground speed", "m/s"),
    f("airspeed", "Air speed", "m/s"),
    f("verticalspeed", "Vertical speed", "m/s"),
    f("roll", "Roll", "°"),
    f("pitch", "Pitch", "°"),
    f("yaw", "Yaw", "°"),
    f("heading", "Heading", "°"),
    f("throttle", "Throttle", "%"),
    f("battery_voltage", "Battery voltage", "V"),
    f("battery_current", "Battery current", "A"),
    f("battery_remaining", "Battery remaining", "%"),
    f("gpsstatus", "GPS fix type", ""),
    f("satcount", "Satellites", ""),
    f("gpshdop", "GPS HDOP", ""),
    f("wp_dist", "Distance to waypoint", "m"),
    f("wpno", "Current waypoint", ""),
    f("nav_bearing", "Nav bearing", "°"),
    f("nav_roll", "Nav roll", "°"),
    f("nav_pitch", "Nav pitch", "°"),
    f("alt_error", "Altitude error", "m"),
    f("xtrack_error", "Cross-track error", "m"),
    f("dist_to_home", "Distance to home", "m"),
    f("vibex", "Vibration X", "m/s²"),
    f("vibey", "Vibration Y", "m/s²"),
    f("vibez", "Vibration Z", "m/s²"),
    f("ch1in", "RC input 1", "µs"),
    f("ch2in", "RC input 2", "µs"),
    f("ch3in", "RC input 3", "µs"),
    f("ch4in", "RC input 4", "µs"),
    f("ch5in", "RC input 5", "µs"),
    f("ch6in", "RC input 6", "µs"),
    f("ch7in", "RC input 7", "µs"),
    f("ch8in", "RC input 8", "µs"),
    f("ch1out", "Servo output 1", "µs"),
    f("ch2out", "Servo output 2", "µs"),
    f("ch3out", "Servo output 3", "µs"),
    f("ch4out", "Servo output 4", "µs"),
    f("ch5out", "Servo output 5", "µs"),
    f("ch6out", "Servo output 6", "µs"),
    f("ch7out", "Servo output 7", "µs"),
    f("ch8out", "Servo output 8", "µs"),
];

const CH_IN: [&str; 8] = [
    "ch1in", "ch2in", "ch3in", "ch4in", "ch5in", "ch6in", "ch7in", "ch8in",
];
const CH_OUT: [&str; 8] = [
    "ch1out", "ch2out", "ch3out", "ch4out", "ch5out", "ch6out", "ch7out", "ch8out",
];

const SAFETY_ARMED: u8 = 128;

/// MAV_TYPE values that are never an autopilot (GCS, companions, gimbals, peripherals...).
fn is_peripheral_type(mav_type: u8) -> bool {
    matches!(mav_type, 6 | 18 | 26 | 27 | 30..=34 | 36..=42)
}

/// True when a HEARTBEAT comes from an autopilot rather than a GCS or peripheral.
pub fn is_vehicle_heartbeat(h: &control_station_mavlink::dialect::HEARTBEAT_DATA) -> bool {
    h.autopilot != MavAutopilot::MAV_AUTOPILOT_INVALID && !is_peripheral_type(h.mavtype as u8)
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
            fields: BTreeMap::new(),
        }
    }

    pub fn fields(&self) -> &BTreeMap<&'static str, f64> {
        &self.fields
    }

    pub fn mode(&self) -> Option<String> {
        self.last_heartbeat?;
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
                if is_vehicle_heartbeat(h) {
                    self.vehicle_type = VehicleType::from_mav_type(h.mavtype as u8);
                    self.base_mode = h.base_mode.bits();
                    self.armed = h
                        .base_mode
                        .contains(MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED)
                        || self.base_mode & SAFETY_ARMED != 0;
                    self.custom_mode = h.custom_mode;
                    self.last_heartbeat = Some(now);
                }
            }
            MavMessage::ATTITUDE(a) => {
                self.set("roll", f64::from(a.roll).to_degrees());
                self.set("pitch", f64::from(a.pitch).to_degrees());
                self.set("yaw", f64::from(a.yaw).to_degrees().rem_euclid(360.0));
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
            }
            MavMessage::GPS_RAW_INT(g) => {
                self.set("gpsstatus", f64::from(g.fix_type as u8));
                if g.satellites_visible != u8::MAX {
                    self.set("satcount", f64::from(g.satellites_visible));
                }
                if g.eph != u16::MAX {
                    self.set("gpshdop", f64::from(g.eph) / 100.0);
                }
            }
            MavMessage::VFR_HUD(v) => {
                self.set("airspeed", f64::from(v.airspeed));
                self.set("groundspeed", f64::from(v.groundspeed));
                self.set("throttle", f64::from(v.throttle));
                self.set("heading", f64::from(v.heading));
            }
            MavMessage::SYS_STATUS(s) => {
                if s.voltage_battery != u16::MAX {
                    self.set("battery_voltage", f64::from(s.voltage_battery) / 1000.0);
                }
                if s.current_battery >= 0 {
                    self.set("battery_current", f64::from(s.current_battery) / 100.0);
                }
                if s.battery_remaining >= 0 {
                    self.set("battery_remaining", f64::from(s.battery_remaining));
                }
            }
            MavMessage::NAV_CONTROLLER_OUTPUT(n) => {
                self.set("wp_dist", f64::from(n.wp_dist));
                self.set("nav_bearing", f64::from(n.nav_bearing));
                self.set("nav_roll", f64::from(n.nav_roll));
                self.set("nav_pitch", f64::from(n.nav_pitch));
                self.set("alt_error", f64::from(n.alt_error));
                self.set("xtrack_error", f64::from(n.xtrack_error));
            }
            MavMessage::MISSION_CURRENT(m) => self.set("wpno", f64::from(m.seq)),
            MavMessage::HOME_POSITION(h) => {
                self.home = Some(Position {
                    lat: f64::from(h.latitude) / 1e7,
                    lon: f64::from(h.longitude) / 1e7,
                    alt_m: h.altitude as f32 / 1000.0,
                });
                self.update_home_distance();
            }
            MavMessage::VIBRATION(v) => {
                self.set("vibex", f64::from(v.vibration_x));
                self.set("vibey", f64::from(v.vibration_y));
                self.set("vibez", f64::from(v.vibration_z));
            }
            MavMessage::RC_CHANNELS(r) => {
                let raw = [
                    r.chan1_raw,
                    r.chan2_raw,
                    r.chan3_raw,
                    r.chan4_raw,
                    r.chan5_raw,
                    r.chan6_raw,
                    r.chan7_raw,
                    r.chan8_raw,
                ];
                for (key, value) in CH_IN.iter().zip(raw) {
                    if value != u16::MAX {
                        self.set(key, f64::from(value));
                    }
                }
            }
            MavMessage::SERVO_OUTPUT_RAW(s) => {
                let raw = [
                    s.servo1_raw,
                    s.servo2_raw,
                    s.servo3_raw,
                    s.servo4_raw,
                    s.servo5_raw,
                    s.servo6_raw,
                    s.servo7_raw,
                    s.servo8_raw,
                ];
                for (key, value) in CH_OUT.iter().zip(raw) {
                    self.set(key, f64::from(value));
                }
            }
            MavMessage::STATUSTEXT(t) => {
                let bytes: &[u8] = &*t.text;
                let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
                return Some(StatusText {
                    time_ms: unix_ms,
                    severity: t.severity as u8,
                    text: String::from_utf8_lossy(&bytes[..end]).into_owned(),
                    system_id: self.id.system_id,
                    component_id: self.id.component_id,
                });
            }
            _ => {}
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
            assert!(v.last_heartbeat.is_none());
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
