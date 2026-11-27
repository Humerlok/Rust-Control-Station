//! Plays an ArduPilot DataFlash log as a live MAVLink vehicle (sysid 1, compid 1), so the
//! app can be exercised without hardware.
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use control_station_ardupilot_meta::VehicleType;
use control_station_dataflash::Log;
use control_station_mavlink::{
    dialect::{
        GpsFixType, MavAutopilot, MavModeFlag, MavParamType, MavResult, MavSeverity, MavState,
        MavType, ATTITUDE_DATA, COMMAND_ACK_DATA, GLOBAL_POSITION_INT_DATA, GPS_RAW_INT_DATA,
        HEARTBEAT_DATA, HOME_POSITION_DATA, LOG_ENTRY_DATA, NAV_CONTROLLER_OUTPUT_DATA,
        PARAM_VALUE_DATA, STATUSTEXT_DATA, SYS_STATUS_DATA, VFR_HUD_DATA,
    },
    encode, MavHeader, MavMessage, MavlinkVersion, Parser,
};
use control_station_transport::Connection;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{mpsc, watch},
    time::{interval, interval_at, Instant, MissedTickBehavior},
};

use crate::types::{LinkError, ReplayStatus};

mod ftp;
mod missions;
use ftp::ReplayFtp;
use missions::ReplayMissions;

#[derive(Debug, Clone)]
enum Event {
    Attitude {
        roll: f64,
        pitch: f64,
        yaw: f64,
    },
    Position {
        lat: f64,
        lon: f64,
        alt: f64,
        relative: f64,
    },
    Gps {
        status: i64,
        sats: i64,
        hdop: f64,
        lat: f64,
        lon: f64,
        alt: f64,
        speed: f64,
    },
    Battery {
        volts: f64,
        amps: f64,
        remaining: Option<i64>,
    },
    Mode(u32),
    Arm(bool),
    Text(String),
    /// ARSP: measured airspeed in m/s.
    Airspeed(f64),
    /// Plane CTUN: throttle output in percent and the roll/pitch the navigation asks for, in degrees.
    PlaneTuning {
        throttle: f64,
        nav_roll: f64,
        nav_pitch: f64,
    },
    /// Plane NTUN: distance and bearings to the target, altitude, cross-track and airspeed errors.
    PlaneNav {
        wp_dist: f64,
        target_bearing: f64,
        nav_bearing: f64,
        alt_error: f64,
        xtrack_error: f64,
        aspd_error: f64,
    },
}

struct Timed {
    time_us: u64,
    event: Event,
}

/// A relative path that does not exist from the working directory is also tried from each
/// parent directory (the app runs from `src-tauri` in development, the log lives at the repo root).
fn resolve(path: &str) -> std::path::PathBuf {
    let given = Path::new(path);
    if given.is_absolute() || given.exists() {
        return given.to_path_buf();
    }
    std::env::current_dir()
        .ok()
        .and_then(|cwd| {
            cwd.ancestors()
                .take(6)
                .map(|dir| dir.join(given))
                .find(|candidate| candidate.exists())
        })
        .unwrap_or_else(|| given.to_path_buf())
}

/// Playback commands from a [`ReplayHandle`] to the player task.
#[derive(Debug, Clone, Copy)]
enum Cmd {
    Play,
    Pause,
    /// Jump to this many seconds after the first record of the log.
    Seek(f64),
    Speed(f64),
}

/// The controls of a running replay. Every clone shares one player; when the last one is dropped
/// (the session ended) the player stops.
#[derive(Clone)]
pub struct ReplayHandle {
    commands: mpsc::UnboundedSender<Cmd>,
    status: watch::Receiver<ReplayStatus>,
    events: Arc<[Timed]>,
}

impl ReplayHandle {
    fn send(&self, command: Cmd) -> Result<(), LinkError> {
        self.commands
            .send(command)
            .map_err(|_| LinkError::Other("replay is not active".into()))
    }

    pub fn play(&self) -> Result<(), LinkError> {
        self.send(Cmd::Play)
    }

    pub fn pause(&self) -> Result<(), LinkError> {
        self.send(Cmd::Pause)
    }

    pub fn seek(&self, seconds: f64) -> Result<(), LinkError> {
        self.send(Cmd::Seek(seconds))
    }

    pub fn set_speed(&self, speed: f64) -> Result<(), LinkError> {
        self.send(Cmd::Speed(speed))
    }

    pub fn status(&self) -> ReplayStatus {
        self.status.borrow().clone()
    }

    /// A receiver that wakes on every status change.
    pub fn watch(&self) -> watch::Receiver<ReplayStatus> {
        self.status.clone()
    }

    /// The path flown up to `until_s` seconds into the log, as (lat, lon) pairs, thinned to at
    /// most `max_points`. The map rebuilds its track from this after a jump in the timeline.
    pub fn track(&self, until_s: f64, max_points: usize) -> Vec<(f64, f64)> {
        decimated_track(&self.events, until_s, max_points)
    }
}

/// Everything the player task owns.
struct Setup {
    events: Arc<[Timed]>,
    params: ReplayParams,
    vehicle_type: VehicleType,
    speed: f64,
    file_name: String,
    commands: mpsc::UnboundedReceiver<Cmd>,
    status: watch::Sender<ReplayStatus>,
}

/// The playback speed is limited to what the pacing can honor.
fn clamp_speed(speed: f64) -> f64 {
    if speed.is_finite() {
        speed.clamp(0.1, 50.0)
    } else {
        1.0
    }
}

/// Starts the player for an already decoded log (`events` must not be empty and must be sorted by
/// time). Returns the link side of the in-process pipe and the controls.
fn spawn_player(
    events: Vec<Timed>,
    params: Vec<(String, f32)>,
    vehicle_type: VehicleType,
    speed: f64,
    file_name: String,
) -> (tokio::io::DuplexStream, ReplayHandle) {
    let events: Arc<[Timed]> = events.into();
    let speed = clamp_speed(speed);
    let duration_us = events[events.len() - 1].time_us - events[0].time_us;
    let (status_tx, status_rx) = watch::channel(ReplayStatus {
        file_name: file_name.clone(),
        playing: true,
        ended: false,
        speed,
        position_s: 0.0,
        duration_s: duration_us as f64 / 1e6,
        seek_epoch: 0,
    });
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (link_side, replay_side) = tokio::io::duplex(1 << 16);
    let setup = Setup {
        events: Arc::clone(&events),
        params: ReplayParams::new(params),
        vehicle_type,
        speed,
        file_name,
        commands: command_rx,
        status: status_tx,
    };
    tokio::spawn(run(setup, replay_side));
    let handle = ReplayHandle {
        commands: command_tx,
        status: status_rx,
        events,
    };
    (link_side, handle)
}

pub async fn open(path: &str, speed: f64) -> Result<(Connection, ReplayHandle), LinkError> {
    let path = resolve(path);
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let Loaded {
        events,
        params,
        vehicle_type,
    } = tokio::task::spawn_blocking(move || load_log(&path))
        .await
        .map_err(|e| LinkError::Other(e.to_string()))??;
    if events.is_empty() {
        return Err(LinkError::Other("the log has no replayable data".into()));
    }
    let (link_side, handle) = spawn_player(events, params, vehicle_type, speed, name.clone());
    let (link_reader, link_writer) = tokio::io::split(link_side);
    Ok((
        Connection::from_io(link_reader, link_writer, format!("Replay {name}")),
        handle,
    ))
}

/// Everything the replay needs from a log, decoded in a single pass over its records.
struct Loaded {
    events: Vec<Timed>,
    /// Parameters in first-seen order with their last logged value.
    params: Vec<(String, f32)>,
    /// What the log was recorded on; decides the heartbeat type and so the flight-mode names.
    vehicle_type: VehicleType,
}

/// The vehicle family of a log: the `VER` build type, else the firmware banner in the first
/// messages, else a few parameters only one family has.
fn detect_vehicle(log: &Log) -> VehicleType {
    let known = |kind: VehicleType| (kind != VehicleType::Unknown).then_some(kind);
    if let Some(kind) = log
        .records_named("VER")
        .next()
        .and_then(|r| r.i64("BU"))
        .and_then(|bu| known(VehicleType::from_build_type(bu)))
    {
        return kind;
    }
    if let Some(kind) = log
        .records_named("MSG")
        .take(40)
        .filter_map(|r| r.text("Message").map(VehicleType::from_firmware_banner))
        .find_map(known)
    {
        return kind;
    }
    let has_param = |name: &str| {
        log.records_named("PARM")
            .any(|r| r.text("Name") == Some(name))
    };
    if has_param("TRIM_ARSPD_CM") || has_param("Q_ENABLE") {
        VehicleType::Plane
    } else if has_param("WPNAV_SPEED") {
        VehicleType::Copter
    } else {
        VehicleType::Unknown
    }
}

fn load_log(path: &Path) -> Result<Loaded, LinkError> {
    let log = Log::open(path).map_err(|e| LinkError::Other(format!("{}: {e}", path.display())))?;
    let vehicle_type = detect_vehicle(&log);
    let is_plane = vehicle_type == VehicleType::Plane;
    let mut events = Vec::new();
    let mut params: Vec<(String, f32)> = Vec::new();
    let mut param_index: HashMap<String, usize> = HashMap::new();
    let f = |r: &control_station_dataflash::Record, c: &str| r.f64(c).unwrap_or(0.0);
    for r in log.records() {
        let event = match &*r.name {
            "PARM" => {
                if let (Some(name), Some(value)) = (r.text("Name"), r.f64("Value")) {
                    let value = value as f32;
                    match param_index.get(name) {
                        Some(i) => params[*i].1 = value,
                        None => {
                            param_index.insert(name.to_owned(), params.len());
                            params.push((name.to_owned(), value));
                        }
                    }
                }
                continue;
            }
            "ATT" => Event::Attitude {
                roll: f(&r, "Roll"),
                pitch: f(&r, "Pitch"),
                yaw: f(&r, "Yaw"),
            },
            "POS" => Event::Position {
                lat: f(&r, "Lat"),
                lon: f(&r, "Lng"),
                alt: f(&r, "Alt"),
                relative: f(&r, "RelHomeAlt"),
            },
            "GPS" => Event::Gps {
                status: r.i64("Status").unwrap_or(0),
                sats: r.i64("NSats").unwrap_or(0),
                hdop: f(&r, "HDop"),
                lat: f(&r, "Lat"),
                lon: f(&r, "Lng"),
                alt: f(&r, "Alt"),
                speed: f(&r, "Spd"),
            },
            "BAT" => Event::Battery {
                volts: f(&r, "Volt"),
                amps: f(&r, "Curr"),
                remaining: r.i64("RemPct"),
            },
            "MODE" => match r.i64("Mode") {
                Some(mode) => Event::Mode(mode as u32),
                None => continue,
            },
            "ARM" => match r.i64("ArmState") {
                Some(state) => Event::Arm(state != 0),
                None => continue,
            },
            "MSG" => match r.text("Message") {
                Some(text) => Event::Text(text.to_owned()),
                None => continue,
            },
            // Only the first airspeed sensor.
            "ARSP" if r.i64("I").unwrap_or(0) == 0 => match r.f64("Airspeed") {
                Some(speed) => Event::Airspeed(speed),
                None => continue,
            },
            // Copter's CTUN means something else (throttle as a 0..1 fraction), so Plane only.
            "CTUN" if is_plane => match r.f64("ThO") {
                Some(throttle) => Event::PlaneTuning {
                    throttle,
                    nav_roll: f(&r, "NavRoll"),
                    nav_pitch: f(&r, "NavPitch"),
                },
                None => continue,
            },
            "NTUN" if is_plane => match r.f64("Dist") {
                Some(wp_dist) => Event::PlaneNav {
                    wp_dist,
                    target_bearing: f(&r, "TBrg"),
                    nav_bearing: f(&r, "NavBrg"),
                    alt_error: f(&r, "AltE"),
                    xtrack_error: f(&r, "XT"),
                    aspd_error: f(&r, "AsE"),
                },
                None => continue,
            },
            _ => continue,
        };
        if let Some(time_us) = r.time_us() {
            events.push(Timed { time_us, event });
        }
    }
    events.sort_by_key(|e| e.time_us);
    Ok(Loaded {
        events,
        params,
        vehicle_type,
    })
}

/// The parameters the replayed vehicle serves and accepts writes to.
struct ReplayParams {
    items: Vec<(String, f32, MavParamType)>,
}

impl ReplayParams {
    fn new(params: Vec<(String, f32)>) -> Self {
        let items = params
            .into_iter()
            .map(|(name, value)| {
                let kind = if value.fract() == 0.0 && value.abs() < 16_777_216.0 {
                    MavParamType::MAV_PARAM_TYPE_INT32
                } else {
                    MavParamType::MAV_PARAM_TYPE_REAL32
                };
                (name, value, kind)
            })
            .collect();
        Self { items }
    }

    fn value_message(&self, index: usize) -> Option<MavMessage> {
        let (name, value, kind) = self.items.get(index)?;
        Some(MavMessage::PARAM_VALUE(PARAM_VALUE_DATA {
            param_value: *value,
            param_count: self.items.len() as u16,
            param_index: index as u16,
            param_id: name.as_str().into(),
            param_type: *kind,
        }))
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.items.iter().position(|(n, _, _)| n == name)
    }

    /// Stores a written value (integer parameters round, like the real firmware) and returns
    /// the index to echo.
    fn set(&mut self, name: &str, value: f32) -> Option<usize> {
        let index = self.find(name)?;
        let item = &mut self.items[index];
        item.1 = if item.2 == MavParamType::MAV_PARAM_TYPE_INT32 {
            value.round()
        } else {
            value
        };
        Some(index)
    }
}

fn char_array_text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Streams the whole table, a few hundred parameters per second like a real telemetry link.
async fn stream_params(sender: Sender, params: Arc<Mutex<ReplayParams>>) {
    let count = params.lock().expect("replay params").items.len();
    for index in 0..count {
        let message = params.lock().expect("replay params").value_message(index);
        if let Some(message) = message {
            if !sender.send(message).await {
                return;
            }
        }
        if index % 50 == 49 {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[derive(Clone)]
struct Sender {
    tx: mpsc::Sender<Vec<u8>>,
    sequence: Arc<AtomicU8>,
}

impl Sender {
    async fn send(&self, message: MavMessage) -> bool {
        let header = MavHeader {
            system_id: 1,
            component_id: 1,
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed),
        };
        self.tx
            .send(encode(header, &message, MavlinkVersion::V2))
            .await
            .is_ok()
    }
}

#[derive(Default)]
struct Vehicle {
    /// Decides the MAV_TYPE of the heartbeat, which the app turns into mode names and the HUD type.
    kind: VehicleType,
    mode: u32,
    armed: bool,
    yaw_deg: f64,
    /// Ground speed (m/s) and altitude (m) of the last GPS record, shown by VFR_HUD.
    speed: f64,
    gps_alt: f64,
    /// Measured airspeed; without an ARSP record the ground speed stands in for it.
    airspeed: Option<f64>,
    throttle: f64,
    /// Navigation targets from the Plane tuning records, resent whole whenever one part changes.
    nav: NAV_CONTROLLER_OUTPUT_DATA,
}

/// The MAV_TYPE a replayed vehicle of this family announces; an unknown log keeps the old quadrotor.
fn mav_type_of(kind: VehicleType) -> MavType {
    match kind {
        VehicleType::Plane => MavType::MAV_TYPE_FIXED_WING,
        VehicleType::Rover => MavType::MAV_TYPE_GROUND_ROVER,
        VehicleType::Sub => MavType::MAV_TYPE_SUBMARINE,
        VehicleType::Tracker => MavType::MAV_TYPE_ANTENNA_TRACKER,
        VehicleType::Blimp => MavType::MAV_TYPE_AIRSHIP,
        VehicleType::Copter | VehicleType::Unknown => MavType::MAV_TYPE_QUADROTOR,
    }
}

impl Vehicle {
    fn new(kind: VehicleType) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }

    fn vfr_hud(&self) -> MavMessage {
        MavMessage::VFR_HUD(VFR_HUD_DATA {
            airspeed: self.airspeed.unwrap_or(self.speed) as f32,
            groundspeed: self.speed as f32,
            alt: self.gps_alt as f32,
            climb: 0.0,
            heading: self.yaw_deg as i16,
            throttle: self.throttle.clamp(0.0, 100.0) as u16,
        })
    }

    fn heartbeat(&self) -> MavMessage {
        let mut base = MavModeFlag::MAV_MODE_FLAG_CUSTOM_MODE_ENABLED;
        if self.armed {
            base |= MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED;
        }
        MavMessage::HEARTBEAT(HEARTBEAT_DATA {
            custom_mode: self.mode,
            mavtype: mav_type_of(self.kind),
            autopilot: MavAutopilot::MAV_AUTOPILOT_ARDUPILOTMEGA,
            base_mode: base,
            system_status: MavState::MAV_STATE_ACTIVE,
            mavlink_version: 3,
        })
    }
}

fn fix_type(status: i64) -> GpsFixType {
    match status {
        0 => GpsFixType::GPS_FIX_TYPE_NO_GPS,
        1 => GpsFixType::GPS_FIX_TYPE_NO_FIX,
        2 => GpsFixType::GPS_FIX_TYPE_2D_FIX,
        3 => GpsFixType::GPS_FIX_TYPE_3D_FIX,
        4 => GpsFixType::GPS_FIX_TYPE_DGPS,
        5 => GpsFixType::GPS_FIX_TYPE_RTK_FLOAT,
        _ => GpsFixType::GPS_FIX_TYPE_RTK_FIXED,
    }
}

fn command_ack(command: control_station_mavlink::dialect::MavCmd) -> MavMessage {
    MavMessage::COMMAND_ACK(COMMAND_ACK_DATA {
        command,
        result: MavResult::MAV_RESULT_UNSUPPORTED,
        ..COMMAND_ACK_DATA::default()
    })
}

fn truncate_text(text: &str) -> String {
    let mut end = text.len().min(49);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// Where the vehicle took off, from the first position record that has a fix: (lat, lon, altitude
/// above sea level), the same values the mission store starts from.
fn first_home(events: &[Timed]) -> Option<(f64, f64, f64)> {
    events.iter().find_map(|timed| match &timed.event {
        Event::Position {
            lat,
            lon,
            alt,
            relative,
        } if *lat != 0.0 || *lon != 0.0 => Some((*lat, *lon, alt - relative)),
        _ => None,
    })
}

fn home_message((lat, lon, alt_msl): (f64, f64, f64)) -> MavMessage {
    MavMessage::HOME_POSITION(HOME_POSITION_DATA {
        latitude: (lat * 1e7) as i32,
        longitude: (lon * 1e7) as i32,
        altitude: (alt_msl * 1000.0) as i32,
        ..HOME_POSITION_DATA::default()
    })
}

/// The MAVLink a log record becomes. Mode and arm records produce nothing themselves: they change
/// what the next HEARTBEAT says.
fn convert(event: &Event, vehicle: &mut Vehicle, time_us: u64, start_us: u64) -> Vec<MavMessage> {
    let boot_ms = (time_us.saturating_sub(start_us) / 1000) as u32;
    match event {
        Event::Mode(m) => {
            vehicle.mode = *m;
            vec![]
        }
        Event::Arm(a) => {
            vehicle.armed = *a;
            vec![]
        }
        Event::Attitude { roll, pitch, yaw } => {
            vehicle.yaw_deg = *yaw;
            vec![MavMessage::ATTITUDE(ATTITUDE_DATA {
                time_boot_ms: boot_ms,
                roll: roll.to_radians() as f32,
                pitch: pitch.to_radians() as f32,
                yaw: yaw.to_radians() as f32,
                ..ATTITUDE_DATA::default()
            })]
        }
        Event::Position {
            lat,
            lon,
            alt,
            relative,
        } => vec![MavMessage::GLOBAL_POSITION_INT(GLOBAL_POSITION_INT_DATA {
            time_boot_ms: boot_ms,
            lat: (lat * 1e7) as i32,
            lon: (lon * 1e7) as i32,
            alt: (alt * 1000.0) as i32,
            relative_alt: (relative * 1000.0) as i32,
            hdg: (vehicle.yaw_deg * 100.0).clamp(0.0, 35_999.0) as u16,
            ..GLOBAL_POSITION_INT_DATA::default()
        })],
        Event::Gps {
            status,
            sats,
            hdop,
            lat,
            lon,
            alt,
            speed,
        } => {
            vehicle.speed = *speed;
            vehicle.gps_alt = *alt;
            vec![
                MavMessage::GPS_RAW_INT(GPS_RAW_INT_DATA {
                    time_usec: time_us,
                    lat: (lat * 1e7) as i32,
                    lon: (lon * 1e7) as i32,
                    alt: (alt * 1000.0) as i32,
                    eph: (hdop * 100.0).clamp(0.0, 65_534.0) as u16,
                    epv: u16::MAX,
                    vel: (speed * 100.0).clamp(0.0, 65_534.0) as u16,
                    cog: u16::MAX,
                    fix_type: fix_type(*status),
                    satellites_visible: (*sats).clamp(0, 254) as u8,
                    ..GPS_RAW_INT_DATA::default()
                }),
                vehicle.vfr_hud(),
            ]
        }
        Event::Airspeed(speed) => {
            vehicle.airspeed = Some(*speed);
            vec![vehicle.vfr_hud()]
        }
        Event::PlaneTuning {
            throttle,
            nav_roll,
            nav_pitch,
        } => {
            vehicle.throttle = *throttle;
            vehicle.nav.nav_roll = *nav_roll as f32;
            vehicle.nav.nav_pitch = *nav_pitch as f32;
            vec![
                vehicle.vfr_hud(),
                MavMessage::NAV_CONTROLLER_OUTPUT(vehicle.nav.clone()),
            ]
        }
        Event::PlaneNav {
            wp_dist,
            target_bearing,
            nav_bearing,
            alt_error,
            xtrack_error,
            aspd_error,
        } => {
            let nav = &mut vehicle.nav;
            nav.wp_dist = wp_dist.clamp(0.0, 65_534.0) as u16;
            nav.target_bearing = *target_bearing as i16;
            nav.nav_bearing = *nav_bearing as i16;
            nav.alt_error = *alt_error as f32;
            nav.xtrack_error = *xtrack_error as f32;
            nav.aspd_error = *aspd_error as f32;
            vec![MavMessage::NAV_CONTROLLER_OUTPUT(nav.clone())]
        }
        Event::Battery {
            volts,
            amps,
            remaining,
        } => vec![MavMessage::SYS_STATUS(SYS_STATUS_DATA {
            voltage_battery: (volts * 1000.0).clamp(0.0, 65_534.0) as u16,
            current_battery: (amps * 100.0).clamp(0.0, 32_767.0) as i16,
            battery_remaining: remaining.map_or(-1, |r| r.clamp(0, 100) as i8),
            ..SYS_STATUS_DATA::default()
        })],
        Event::Text(text) => vec![MavMessage::STATUSTEXT(STATUSTEXT_DATA {
            severity: MavSeverity::MAV_SEVERITY_INFO,
            text: truncate_text(text).as_str().into(),
            ..STATUSTEXT_DATA::default()
        })],
    }
}

/// The vehicle as it was after the records in `past`, for a jump in the timeline: the last mode and
/// arm state, plus the last attitude, position, GPS and battery record converted as they were
/// while playing. Messages (STATUSTEXT) are not replayed: they were events, not state.
fn keyframe(past: &[Timed], start_us: u64, kind: VehicleType) -> (Vehicle, Vec<MavMessage>) {
    let mut vehicle = Vehicle::new(kind);
    let (mut mode, mut armed) = (None, None);
    let (mut attitude, mut position, mut gps, mut battery) = (None, None, None, None);
    let (mut airspeed, mut tuning, mut nav) = (None, None, None);
    let is_plane = kind == VehicleType::Plane;
    for timed in past.iter().rev() {
        match &timed.event {
            Event::Mode(m) => {
                mode.get_or_insert(*m);
            }
            Event::Arm(a) => {
                armed.get_or_insert(*a);
            }
            Event::Attitude { .. } => {
                attitude.get_or_insert(timed);
            }
            Event::Position { .. } => {
                position.get_or_insert(timed);
            }
            Event::Gps { .. } => {
                gps.get_or_insert(timed);
            }
            Event::Battery { .. } => {
                battery.get_or_insert(timed);
            }
            Event::Airspeed(_) => {
                airspeed.get_or_insert(timed);
            }
            Event::PlaneTuning { .. } => {
                tuning.get_or_insert(timed);
            }
            Event::PlaneNav { .. } => {
                nav.get_or_insert(timed);
            }
            Event::Text(_) => {}
        }
        // A log without airspeed or Plane records is scanned to its start; the plane-only
        // records are not waited for on other vehicles.
        if mode.is_some()
            && armed.is_some()
            && attitude.is_some()
            && position.is_some()
            && gps.is_some()
            && battery.is_some()
            && (!is_plane || (tuning.is_some() && nav.is_some()))
        {
            break;
        }
    }
    vehicle.mode = mode.unwrap_or(0);
    vehicle.armed = armed.unwrap_or(false);
    // Attitude first: the position and GPS messages carry the heading it sets. The airspeed and
    // Plane records come after the GPS one, whose VFR_HUD would otherwise overwrite them.
    let mut messages = Vec::new();
    for timed in [attitude, position, gps, battery, airspeed, tuning, nav]
        .into_iter()
        .flatten()
    {
        messages.extend(convert(&timed.event, &mut vehicle, timed.time_us, start_us));
    }
    (vehicle, messages)
}

/// Positions with a fix up to `until_s` seconds into the log, thinned to at most `max_points`
/// (evenly, always keeping the last one so the track reaches the vehicle).
fn decimated_track(events: &[Timed], until_s: f64, max_points: usize) -> Vec<(f64, f64)> {
    let Some(first) = events.first() else {
        return Vec::new();
    };
    let until_us = first
        .time_us
        .saturating_add((until_s.max(0.0) * 1e6) as u64);
    let points: Vec<(f64, f64)> = events
        .iter()
        .take_while(|timed| timed.time_us <= until_us)
        .filter_map(|timed| match timed.event {
            Event::Position { lat, lon, .. } if lat != 0.0 || lon != 0.0 => Some((lat, lon)),
            _ => None,
        })
        .collect();
    if max_points == 0 || points.len() <= max_points {
        return if max_points == 0 { Vec::new() } else { points };
    }
    let step = points.len().div_ceil(max_points);
    let mut thinned: Vec<(f64, f64)> = points.iter().copied().step_by(step).collect();
    let last = points[points.len() - 1];
    if thinned.last() != Some(&last) {
        thinned.truncate(max_points - 1);
        thinned.push(last);
    }
    thinned
}

/// Maps wall-clock time to a position in the log, across pauses, speed changes and jumps.
#[derive(Debug, Clone, Copy)]
struct Clock {
    anchor_wall: Instant,
    /// Position in the log (microseconds after its first record) at `anchor_wall`.
    anchor_us: u64,
    speed: f64,
    playing: bool,
}

impl Clock {
    fn new(now: Instant, speed: f64) -> Self {
        Self {
            anchor_wall: now,
            anchor_us: 0,
            speed,
            playing: true,
        }
    }

    fn position_us(&self, now: Instant) -> u64 {
        if !self.playing {
            return self.anchor_us;
        }
        let elapsed = now
            .saturating_duration_since(self.anchor_wall)
            .as_secs_f64();
        self.anchor_us + (elapsed * 1e6 * self.speed) as u64
    }

    /// Moves the anchor to `now` without changing the position, so a change of state or speed
    /// only affects the time that follows.
    fn rebase(&mut self, now: Instant) {
        self.anchor_us = self.position_us(now);
        self.anchor_wall = now;
    }

    fn set_playing(&mut self, now: Instant, playing: bool) {
        self.rebase(now);
        self.playing = playing;
    }

    fn set_speed(&mut self, now: Instant, speed: f64) {
        self.rebase(now);
        self.speed = clamp_speed(speed);
    }

    fn seek(&mut self, now: Instant, position_us: u64) {
        self.anchor_us = position_us;
        self.anchor_wall = now;
    }

    /// When the log reaches `position_us` at the current speed (never before the anchor).
    fn wall_at(&self, position_us: u64) -> Instant {
        let ahead = position_us.saturating_sub(self.anchor_us) as f64 / 1e6 / self.speed;
        self.anchor_wall + Duration::from_secs_f64(ahead)
    }
}

/// The playback state machine: sends the log's records at the clock's pace and obeys [`Cmd`]s.
struct Player {
    events: Arc<[Timed]>,
    start_us: u64,
    duration_us: u64,
    home: Option<(f64, f64, f64)>,
    file_name: String,
    sender: Sender,
    status: watch::Sender<ReplayStatus>,
    clock: Clock,
    vehicle: Vehicle,
    /// Index of the next record to send.
    next: usize,
    ended: bool,
    epoch: u64,
}

impl Player {
    async fn send(&self, messages: Vec<MavMessage>) -> bool {
        for message in messages {
            if !self.sender.send(message).await {
                return false;
            }
        }
        true
    }

    fn publish(&self, now: Instant) {
        let position_us = if self.ended {
            self.duration_us
        } else {
            self.clock.position_us(now).min(self.duration_us)
        };
        self.status.send_replace(ReplayStatus {
            file_name: self.file_name.clone(),
            playing: self.clock.playing && !self.ended,
            ended: self.ended,
            speed: self.clock.speed,
            position_s: position_us as f64 / 1e6,
            duration_s: self.duration_us as f64 / 1e6,
            seek_epoch: self.epoch,
        });
    }

    /// First frames of a replay: the vehicle announces itself and where home is.
    async fn start(&self) -> bool {
        let mut messages = vec![self.vehicle.heartbeat()];
        messages.extend(self.home.map(home_message));
        let alive = self.send(messages).await;
        self.publish(Instant::now());
        alive
    }

    /// Sends every record the clock has reached; at the last one the replay stops at the end.
    async fn advance(&mut self) -> bool {
        let now = Instant::now();
        let reached = self.clock.position_us(now);
        let events = Arc::clone(&self.events);
        while let Some(timed) = events.get(self.next) {
            if timed.time_us - self.start_us > reached {
                break;
            }
            let messages = convert(
                &timed.event,
                &mut self.vehicle,
                timed.time_us,
                self.start_us,
            );
            self.next += 1;
            if !self.send(messages).await {
                return false;
            }
        }
        if self.next >= events.len() {
            self.ended = true;
            self.clock.set_playing(now, false);
            self.publish(now);
        }
        true
    }

    /// Jumps to `seconds` into the log: the vehicle state at that point is sent at once, then
    /// playback goes on from there (or stays paused, as before the jump).
    async fn seek(&mut self, seconds: f64) -> bool {
        let seconds = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        let position_us = ((seconds * 1e6) as u64).min(self.duration_us);
        let events = Arc::clone(&self.events);
        self.next = events.partition_point(|timed| timed.time_us <= self.start_us + position_us);
        let (vehicle, motion) = keyframe(&events[..self.next], self.start_us, self.vehicle.kind);
        self.vehicle = vehicle;
        let mut messages = vec![self.vehicle.heartbeat()];
        messages.extend(self.home.map(home_message));
        messages.extend(motion);
        let now = Instant::now();
        self.clock.seek(now, position_us);
        self.ended = self.next >= events.len();
        if self.ended {
            self.clock.playing = false;
        }
        self.epoch += 1;
        let alive = self.send(messages).await;
        self.publish(now);
        alive
    }

    /// Applies commands that arrived together. Several seeks (a drag on the timeline) collapse
    /// into the last one.
    async fn handle(&mut self, commands: Vec<Cmd>) -> bool {
        let now = Instant::now();
        let mut seek = None;
        for command in commands {
            match command {
                Cmd::Play => {
                    // Play at the end of the log starts it over.
                    if self.ended && seek.is_none() {
                        seek = Some(0.0);
                    }
                    self.clock.set_playing(now, true);
                }
                Cmd::Pause => self.clock.set_playing(now, false),
                Cmd::Speed(speed) => self.clock.set_speed(now, speed),
                Cmd::Seek(seconds) => seek = Some(seconds),
            }
        }
        match seek {
            Some(seconds) => self.seek(seconds).await,
            None => {
                self.publish(now);
                true
            }
        }
    }
}

/// Drives the player until the link goes away: every handle dropped or a send fails.
async fn play(mut player: Player, mut commands: mpsc::UnboundedReceiver<Cmd>) {
    if !player.start().await {
        return;
    }
    // The link must look alive whatever the log is doing (paused, at the end, at 0.1x): one
    // heartbeat per second of wall-clock time, never tied to the log's own clock.
    let mut heartbeat = interval_at(
        Instant::now() + Duration::from_secs(1),
        Duration::from_secs(1),
    );
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut progress = interval(Duration::from_millis(100));
    progress.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        let running = player.clock.playing && !player.ended;
        let due = if running {
            player
                .clock
                .wall_at(player.events[player.next].time_us - player.start_us)
        } else {
            Instant::now()
        };
        let alive = tokio::select! {
            command = commands.recv() => match command {
                Some(first) => {
                    let mut batch = vec![first];
                    while let Ok(more) = commands.try_recv() {
                        batch.push(more);
                    }
                    player.handle(batch).await
                }
                None => false,
            },
            _ = heartbeat.tick() => player.send(vec![player.vehicle.heartbeat()]).await,
            _ = progress.tick(), if running => {
                player.publish(Instant::now());
                true
            }
            _ = tokio::time::sleep_until(due), if running => player.advance().await,
        };
        if !alive {
            return;
        }
    }
}

async fn run(setup: Setup, stream: tokio::io::DuplexStream) {
    let Setup {
        events,
        params,
        vehicle_type,
        speed,
        file_name,
        commands,
        status,
    } = setup;
    let (mut incoming, mut outgoing) = tokio::io::split(stream);
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(256);
    let sender = Sender {
        tx,
        sequence: Arc::new(AtomicU8::new(0)),
    };

    let writer = tokio::spawn(async move {
        while let Some(bytes) = rx.recv().await {
            if outgoing.write_all(&bytes).await.is_err() {
                return;
            }
        }
    });

    // The log cannot obey commands (they get "unsupported"), but it serves and accepts
    // parameters so the whole parameter stack can be exercised without a drone.
    let params = Arc::new(Mutex::new(params));
    // Missions, fences and rally points live in memory, starting with home where the log starts.
    let home = first_home(&events);
    let missions = Arc::new(Mutex::new(ReplayMissions::new(
        home.map(|(lat, lon, alt_msl)| ReplayMissions::home_item(lat, lon, alt_msl)),
    )));
    let mut files = ReplayFtp::new();
    let responder = {
        let sender = sender.clone();
        tokio::spawn(async move {
            let mut parser = Parser::new();
            let mut buf = [0u8; 1024];
            while let Ok(n) = incoming.read(&mut buf).await {
                if n == 0 {
                    return;
                }
                for packet in parser.push(&buf[..n]) {
                    let mission_replies = missions
                        .lock()
                        .expect("replay missions")
                        .handle(&packet.message);
                    for reply in mission_replies {
                        if !sender.send(reply).await {
                            return;
                        }
                    }
                    for reply in files.handle(&packet.message) {
                        if !sender.send(reply).await {
                            return;
                        }
                    }
                    let reply = match packet.message {
                        MavMessage::COMMAND_LONG(c) => Some(command_ack(c.command)),
                        MavMessage::COMMAND_INT(c) => Some(command_ack(c.command)),
                        MavMessage::LOG_REQUEST_LIST(_) => Some(no_logs()),
                        MavMessage::PARAM_REQUEST_LIST(_) => {
                            tokio::spawn(stream_params(sender.clone(), params.clone()));
                            None
                        }
                        MavMessage::PARAM_REQUEST_READ(r) => {
                            let table = params.lock().expect("replay params");
                            let index = if r.param_index >= 0 {
                                Some(r.param_index as usize)
                            } else {
                                table.find(&char_array_text(&*r.param_id))
                            };
                            index.and_then(|i| table.value_message(i))
                        }
                        MavMessage::PARAM_SET(set) => {
                            let mut table = params.lock().expect("replay params");
                            table
                                .set(&char_array_text(&*set.param_id), set.param_value)
                                .and_then(|i| table.value_message(i))
                        }
                        _ => None,
                    };
                    if let Some(reply) = reply {
                        if !sender.send(reply).await {
                            return;
                        }
                    }
                }
            }
        })
    };

    let start_us = events[0].time_us;
    let duration_us = events[events.len() - 1].time_us - start_us;
    let player = Player {
        start_us,
        duration_us,
        home,
        file_name,
        sender,
        status,
        clock: Clock::new(Instant::now(), speed),
        vehicle: Vehicle::new(vehicle_type),
        next: 0,
        ended: false,
        epoch: 0,
        events,
    };
    play(player, commands).await;
    responder.abort();
    writer.abort();
}

/// The MAVLink "no logs" answer to LOG_REQUEST_LIST: a replayed log has no onboard logs to offer.
fn no_logs() -> MavMessage {
    MavMessage::LOG_ENTRY(LOG_ENTRY_DATA {
        id: 0,
        num_logs: 0,
        last_log_num: 0,
        time_utc: 0,
        size: 0,
    })
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, future::Future};

    use super::*;

    /// The log's clock does not start at zero, like a real one (boot time).
    const BOOT_US: u64 = 5_000_000;

    fn at_s(seconds: f64) -> u64 {
        BOOT_US + (seconds * 1e6) as u64
    }

    fn attitude(seconds: f64, roll: f64) -> Timed {
        Timed {
            time_us: at_s(seconds),
            event: Event::Attitude {
                roll,
                pitch: 0.0,
                yaw: 90.0,
            },
        }
    }

    fn position(seconds: f64, lat: f64, lon: f64) -> Timed {
        Timed {
            time_us: at_s(seconds),
            event: Event::Position {
                lat,
                lon,
                alt: 100.0,
                relative: 10.0,
            },
        }
    }

    fn timed(seconds: f64, event: Event) -> Timed {
        Timed {
            time_us: at_s(seconds),
            event,
        }
    }

    /// The link side of a running replay, decoded frame by frame.
    struct Frames {
        stream: tokio::io::DuplexStream,
        parser: Parser,
        queue: VecDeque<MavMessage>,
    }

    impl Frames {
        async fn next(&mut self) -> MavMessage {
            loop {
                if let Some(message) = self.queue.pop_front() {
                    return message;
                }
                let mut buf = [0u8; 1024];
                let n = self.stream.read(&mut buf).await.expect("replay stream");
                assert!(n > 0, "the replay closed the link");
                let packets = self.parser.push(&buf[..n]);
                self.queue.extend(packets.into_iter().map(|p| p.message));
            }
        }

        /// The roll, in whole degrees, of the next ATTITUDE; everything else is skipped.
        async fn roll(&mut self) -> f64 {
            loop {
                if let MavMessage::ATTITUDE(a) = self.next().await {
                    return f64::from(a.roll).to_degrees().round();
                }
            }
        }
    }

    fn start(events: Vec<Timed>, speed: f64) -> (Frames, ReplayHandle) {
        let (stream, handle) = spawn_player(
            events,
            Vec::new(),
            VehicleType::Copter,
            speed,
            "test.BIN".into(),
        );
        let frames = Frames {
            stream,
            parser: Parser::new(),
            queue: VecDeque::new(),
        };
        (frames, handle)
    }

    /// Time is virtual in these tests (`start_paused`), so a hang would only show as a timeout.
    async fn within<T>(future: impl Future<Output = T>) -> T {
        tokio::time::timeout(Duration::from_secs(600), future)
            .await
            .expect("the replay stalled")
    }

    #[test]
    fn clock_follows_pauses_speed_changes_and_jumps() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        let mut clock = Clock::new(t0, 1.0);
        assert_eq!(clock.position_us(t0 + s(2)), 2_000_000);

        clock.set_playing(t0 + s(2), false);
        assert_eq!(clock.position_us(t0 + s(10)), 2_000_000);

        clock.set_playing(t0 + s(10), true);
        clock.set_speed(t0 + s(12), 2.0);
        assert_eq!(clock.position_us(t0 + s(12)), 4_000_000);
        assert_eq!(clock.position_us(t0 + s(13)), 6_000_000);
        // Second 8 of the log is 4 log-seconds ahead of second 12 of the wall clock, at 2x.
        assert_eq!(clock.wall_at(8_000_000), t0 + s(14));

        clock.seek(t0 + s(13), 1_000_000);
        assert_eq!(clock.position_us(t0 + s(14)), 3_000_000);
        // A position already passed is due right away, never in the past of the anchor.
        assert_eq!(clock.wall_at(0), t0 + s(13));
    }

    #[test]
    fn speed_is_limited_to_what_the_pacing_can_honor() {
        assert_eq!(clamp_speed(0.0), 0.1);
        assert_eq!(clamp_speed(1000.0), 50.0);
        assert_eq!(clamp_speed(1.5), 1.5);
        assert_eq!(clamp_speed(f64::NAN), 1.0);
    }

    #[test]
    fn keyframe_holds_the_last_record_of_each_kind() {
        let past = vec![
            timed(0.0, Event::Mode(3)),
            timed(0.0, Event::Arm(true)),
            attitude(1.0, 10.0),
            timed(1.5, Event::Text("old news".into())),
            attitude(2.0, 20.0),
            position(2.5, -20.5, -47.4),
            timed(3.0, Event::Mode(5)),
        ];
        let (vehicle, messages) = keyframe(&past, at_s(0.0), VehicleType::Copter);
        assert_eq!(vehicle.mode, 5);
        assert!(vehicle.armed);
        // Only the last attitude and position; no GPS or battery yet, and the text is not replayed.
        assert_eq!(messages.len(), 2);
        let MavMessage::ATTITUDE(a) = &messages[0] else {
            panic!("expected ATTITUDE first, got {:?}", messages[0]);
        };
        assert_eq!(f64::from(a.roll).to_degrees().round(), 20.0);
        let MavMessage::GLOBAL_POSITION_INT(p) = &messages[1] else {
            panic!("expected GLOBAL_POSITION_INT, got {:?}", messages[1]);
        };
        // The heading comes from the attitude that was just sent.
        assert_eq!(p.hdg, 9000);

        let (empty, none) = keyframe(&[], at_s(0.0), VehicleType::Copter);
        assert_eq!((empty.mode, empty.armed), (0, false));
        assert!(none.is_empty());
    }

    #[test]
    fn a_plane_announces_itself_and_reports_airspeed_throttle_and_navigation() {
        let mut plane = Vehicle::new(VehicleType::Plane);
        let MavMessage::HEARTBEAT(h) = plane.heartbeat() else {
            panic!("expected HEARTBEAT");
        };
        assert_eq!(h.mavtype, MavType::MAV_TYPE_FIXED_WING);
        assert_eq!(
            VehicleType::from_mav_type(h.mavtype as u8),
            VehicleType::Plane
        );
        // Unknown logs keep announcing a quadrotor.
        let MavMessage::HEARTBEAT(h) = Vehicle::default().heartbeat() else {
            panic!("expected HEARTBEAT");
        };
        assert_eq!(h.mavtype, MavType::MAV_TYPE_QUADROTOR);

        let gps = Event::Gps {
            status: 3,
            sats: 12,
            hdop: 0.9,
            lat: -20.0,
            lon: -47.0,
            alt: 700.0,
            speed: 21.0,
        };
        let messages = convert(&gps, &mut plane, at_s(1.0), at_s(0.0));
        let MavMessage::VFR_HUD(v) = &messages[1] else {
            panic!("expected VFR_HUD, got {:?}", messages[1]);
        };
        // No ARSP yet: the ground speed stands in for the airspeed.
        assert_eq!((v.airspeed, v.groundspeed), (21.0, 21.0));

        let messages = convert(&Event::Airspeed(18.5), &mut plane, at_s(2.0), at_s(0.0));
        let MavMessage::VFR_HUD(v) = &messages[0] else {
            panic!("expected VFR_HUD");
        };
        assert_eq!((v.airspeed, v.groundspeed, v.alt), (18.5, 21.0, 700.0));

        let tuning = Event::PlaneTuning {
            throttle: 64.0,
            nav_roll: -12.0,
            nav_pitch: 3.0,
        };
        let messages = convert(&tuning, &mut plane, at_s(3.0), at_s(0.0));
        let MavMessage::VFR_HUD(v) = &messages[0] else {
            panic!("expected VFR_HUD");
        };
        assert_eq!(v.throttle, 64);
        let nav = Event::PlaneNav {
            wp_dist: 350.4,
            target_bearing: 90.0,
            nav_bearing: 85.0,
            alt_error: -2.0,
            xtrack_error: 4.0,
            aspd_error: 1.5,
        };
        let messages = convert(&nav, &mut plane, at_s(3.0), at_s(0.0));
        let MavMessage::NAV_CONTROLLER_OUTPUT(n) = &messages[0] else {
            panic!("expected NAV_CONTROLLER_OUTPUT");
        };
        // The roll and pitch from the tuning record are kept when the navigation record arrives.
        assert_eq!((n.wp_dist, n.nav_bearing, n.nav_roll), (350, 85, -12.0));
    }

    #[test]
    fn a_plane_keyframe_restores_airspeed_throttle_and_navigation() {
        let past = vec![
            timed(0.0, Event::Airspeed(15.0)),
            timed(
                1.0,
                Event::PlaneTuning {
                    throttle: 50.0,
                    nav_roll: 5.0,
                    nav_pitch: 1.0,
                },
            ),
            timed(
                1.5,
                Event::PlaneNav {
                    wp_dist: 100.0,
                    target_bearing: 10.0,
                    nav_bearing: 12.0,
                    alt_error: 0.0,
                    xtrack_error: 0.0,
                    aspd_error: 0.0,
                },
            ),
            timed(2.0, Event::Airspeed(17.0)),
        ];
        let (vehicle, messages) = keyframe(&past, at_s(0.0), VehicleType::Plane);
        assert_eq!(vehicle.kind, VehicleType::Plane);
        assert_eq!(vehicle.airspeed, Some(17.0));
        assert_eq!(vehicle.throttle, 50.0);
        assert!(messages
            .iter()
            .any(|m| matches!(m, MavMessage::NAV_CONTROLLER_OUTPUT(n) if n.wp_dist == 100)));
    }

    #[test]
    fn track_stops_at_the_requested_time_and_is_thinned() {
        let events: Vec<Timed> = (0..10)
            .map(|i| position(f64::from(i), -20.0 + f64::from(i), -47.0))
            .collect();
        assert_eq!(decimated_track(&events, 4.0, 100).len(), 5);
        assert_eq!(decimated_track(&events, -3.0, 100).len(), 1);
        assert!(decimated_track(&events, 4.0, 0).is_empty());

        let many: Vec<Timed> = (0..1000)
            .map(|i| position(f64::from(i) / 10.0, 1.0 + f64::from(i), 2.0))
            .collect();
        let thinned = decimated_track(&many, 1e9, 100);
        assert!(thinned.len() <= 100, "{} points", thinned.len());
        assert_eq!(thinned.first(), Some(&(1.0, 2.0)));
        assert_eq!(thinned.last(), Some(&(1000.0, 2.0)));

        // Positions without a fix are not part of the path.
        let no_fix = vec![position(0.0, 0.0, 0.0), position(1.0, 1.0, 1.0)];
        assert_eq!(decimated_track(&no_fix, 10.0, 100), vec![(1.0, 1.0)]);
    }

    #[test]
    fn home_is_the_first_position_with_a_fix() {
        let events = vec![
            position(0.0, 0.0, 0.0),
            position(1.0, -20.5, -47.4),
            position(2.0, -21.0, -47.0),
        ];
        assert_eq!(first_home(&events), Some((-20.5, -47.4, 90.0)));
        assert_eq!(first_home(&[]), None);
    }

    #[tokio::test(start_paused = true)]
    async fn plays_in_log_time_scaled_by_the_speed() {
        within(async {
            let events = vec![
                attitude(0.0, 10.0),
                attitude(2.0, 20.0),
                attitude(4.0, 30.0),
            ];
            let (mut frames, _handle) = start(events, 2.0);
            let began = Instant::now();
            assert_eq!(frames.roll().await, 10.0);
            assert_eq!(frames.roll().await, 20.0);
            let elapsed = began.elapsed().as_secs_f64();
            assert!(
                (0.9..1.2).contains(&elapsed),
                "second record after {elapsed} s"
            );
            assert_eq!(frames.roll().await, 30.0);
            assert!(began.elapsed().as_secs_f64() > 1.9);
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn pausing_holds_the_log_but_keeps_the_link_alive() {
        within(async {
            let (mut frames, handle) = start(vec![attitude(0.0, 10.0), attitude(2.0, 20.0)], 1.0);
            assert_eq!(frames.roll().await, 10.0);
            handle.pause().unwrap();
            let paused_at = Instant::now();
            // Heartbeats keep coming once a second; the record due at 2 s must not.
            let mut beats = 0;
            while beats < 4 {
                match frames.next().await {
                    MavMessage::HEARTBEAT(_) => beats += 1,
                    MavMessage::ATTITUDE(_) => panic!("the log moved while paused"),
                    _ => {}
                }
            }
            assert!(paused_at.elapsed().as_secs_f64() >= 3.0);
            assert!(!handle.status().playing);

            handle.play().unwrap();
            assert_eq!(frames.roll().await, 20.0);
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn seeking_back_sends_the_state_of_that_moment() {
        within(async {
            let events = vec![
                attitude(0.0, 10.0),
                attitude(1.0, 20.0),
                attitude(2.0, 30.0),
            ];
            let (mut frames, handle) = start(events, 50.0);
            let mut watch = handle.watch();
            for expected in [10.0, 20.0, 30.0] {
                assert_eq!(frames.roll().await, expected);
            }
            watch.wait_for(|s| s.ended).await.unwrap();

            handle.seek(1.5).unwrap();
            // The last attitude at or before 1.5 s, sent at once even though nothing is playing.
            assert_eq!(frames.roll().await, 20.0);
            let status = watch.wait_for(|s| s.seek_epoch == 1).await.unwrap().clone();
            assert!(!status.ended && !status.playing);
            assert_eq!(status.position_s, 1.5);

            handle.play().unwrap();
            assert_eq!(frames.roll().await, 30.0);
            let status = watch.wait_for(|s| s.ended).await.unwrap().clone();
            assert_eq!(status.position_s, 2.0);
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn play_at_the_end_starts_the_log_over() {
        within(async {
            let (mut frames, handle) = start(vec![attitude(0.0, 10.0), attitude(1.0, 20.0)], 50.0);
            let mut watch = handle.watch();
            assert_eq!(frames.roll().await, 10.0);
            assert_eq!(frames.roll().await, 20.0);
            watch.wait_for(|s| s.ended).await.unwrap();

            handle.play().unwrap();
            // The state at second 0, then the log plays again.
            assert_eq!(frames.roll().await, 10.0);
            assert_eq!(frames.roll().await, 20.0);
            let status = watch
                .wait_for(|s| s.seek_epoch == 1 && s.ended)
                .await
                .unwrap()
                .clone();
            assert_eq!(status.duration_s, 1.0);
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_speed_change_applies_to_the_rest_of_the_log() {
        within(async {
            let (mut frames, handle) = start(vec![attitude(0.0, 10.0), attitude(4.0, 20.0)], 1.0);
            assert_eq!(frames.roll().await, 10.0);
            handle.set_speed(2.0).unwrap();
            let began = Instant::now();
            assert_eq!(frames.roll().await, 20.0);
            let elapsed = began.elapsed().as_secs_f64();
            assert!(
                (1.9..2.3).contains(&elapsed),
                "4 log-seconds took {elapsed} s"
            );
            assert_eq!(handle.status().speed, 2.0);
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn commands_fail_once_the_session_is_gone() {
        within(async {
            let (frames, handle) = start(vec![attitude(0.0, 10.0)], 1.0);
            // The link side going away ends the player; its controls then say so.
            drop(frames);
            let mut watch = handle.watch();
            for _ in 0..20 {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if handle.pause().is_err() {
                    break;
                }
            }
            assert!(handle.pause().is_err());
            let _ = watch.changed().await;
        })
        .await;
    }

    #[test]
    fn log_list_is_answered_with_no_logs() {
        let MavMessage::LOG_ENTRY(entry) = no_logs() else {
            panic!("not a LOG_ENTRY");
        };
        assert_eq!((entry.id, entry.num_logs), (0, 0));
    }

    #[test]
    fn missing_relative_path_is_returned_unchanged() {
        assert_eq!(
            resolve("definitely/not/here.BIN"),
            Path::new("definitely/not/here.BIN")
        );
    }

    #[test]
    fn relative_path_is_found_in_a_parent_directory() {
        // Cargo.toml lives in this crate's directory, the parent of the test's working directory
        // (`src` is not the cwd, but the crate root is an ancestor-or-self of it).
        let found = resolve("Cargo.toml");
        assert!(found.exists());
    }
}
