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

use control_station_dataflash::Log;
use control_station_mavlink::{
    dialect::{
        GpsFixType, MavAutopilot, MavModeFlag, MavParamType, MavResult, MavSeverity, MavState,
        MavType, ATTITUDE_DATA, COMMAND_ACK_DATA, GLOBAL_POSITION_INT_DATA, GPS_RAW_INT_DATA,
        HEARTBEAT_DATA, HOME_POSITION_DATA, PARAM_VALUE_DATA, STATUSTEXT_DATA, SYS_STATUS_DATA,
        VFR_HUD_DATA,
    },
    encode, MavHeader, MavMessage, MavlinkVersion, Parser,
};
use control_station_transport::Connection;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
    time::Instant,
};

use crate::types::LinkError;

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

pub async fn open(path: &str, speed: f64) -> Result<Connection, LinkError> {
    let path = resolve(path);
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let Loaded { events, params } = tokio::task::spawn_blocking(move || load_log(&path))
        .await
        .map_err(|e| LinkError::Other(e.to_string()))??;
    if events.is_empty() {
        return Err(LinkError::Other("the log has no replayable data".into()));
    }
    let speed = if speed.is_finite() {
        speed.clamp(0.1, 50.0)
    } else {
        1.0
    };
    let (link_side, replay_side) = tokio::io::duplex(1 << 16);
    let (link_reader, link_writer) = tokio::io::split(link_side);
    tokio::spawn(run(events, ReplayParams::new(params), speed, replay_side));
    Ok(Connection::from_io(
        link_reader,
        link_writer,
        format!("Replay {name}"),
    ))
}

/// Everything the replay needs from a log, decoded in a single pass over its records.
struct Loaded {
    events: Vec<Timed>,
    /// Parameters in first-seen order with their last logged value.
    params: Vec<(String, f32)>,
}

fn load_log(path: &Path) -> Result<Loaded, LinkError> {
    let log = Log::open(path).map_err(|e| LinkError::Other(format!("{}: {e}", path.display())))?;
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
            _ => continue,
        };
        if let Some(time_us) = r.time_us() {
            events.push(Timed { time_us, event });
        }
    }
    events.sort_by_key(|e| e.time_us);
    Ok(Loaded { events, params })
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

struct Vehicle {
    mode: u32,
    armed: bool,
    yaw_deg: f64,
    speed: f64,
    home_sent: bool,
}

impl Vehicle {
    fn heartbeat(&self) -> MavMessage {
        let mut base = MavModeFlag::MAV_MODE_FLAG_CUSTOM_MODE_ENABLED;
        if self.armed {
            base |= MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED;
        }
        MavMessage::HEARTBEAT(HEARTBEAT_DATA {
            custom_mode: self.mode,
            mavtype: MavType::MAV_TYPE_QUADROTOR,
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

async fn run(
    events: Vec<Timed>,
    params: ReplayParams,
    speed: f64,
    stream: tokio::io::DuplexStream,
) {
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
    let home = events.iter().find_map(|timed| match &timed.event {
        Event::Position {
            lat,
            lon,
            alt,
            relative,
        } if *lat != 0.0 || *lon != 0.0 => {
            Some(ReplayMissions::home_item(*lat, *lon, alt - relative))
        }
        _ => None,
    });
    let missions = Arc::new(Mutex::new(ReplayMissions::new(home)));
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
    let wall = Instant::now();
    let at = |t_us: u64| wall + Duration::from_secs_f64((t_us - start_us) as f64 / 1e6 / speed);
    let mut vehicle = Vehicle {
        mode: 0,
        armed: false,
        yaw_deg: 0.0,
        speed: 0.0,
        home_sent: false,
    };
    let mut next_heartbeat = start_us;
    let mut alive = true;

    'events: for timed in &events {
        while next_heartbeat <= timed.time_us {
            tokio::time::sleep_until(at(next_heartbeat)).await;
            if !sender.send(vehicle.heartbeat()).await {
                alive = false;
                break 'events;
            }
            next_heartbeat += 1_000_000;
        }
        tokio::time::sleep_until(at(timed.time_us)).await;
        let boot_ms = ((timed.time_us - start_us) / 1000) as u32;
        let messages = match &timed.event {
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
            } => {
                let mut out = Vec::new();
                if !vehicle.home_sent && (*lat != 0.0 || *lon != 0.0) {
                    vehicle.home_sent = true;
                    out.push(MavMessage::HOME_POSITION(HOME_POSITION_DATA {
                        latitude: (lat * 1e7) as i32,
                        longitude: (lon * 1e7) as i32,
                        altitude: ((alt - relative) * 1000.0) as i32,
                        ..HOME_POSITION_DATA::default()
                    }));
                }
                out.push(MavMessage::GLOBAL_POSITION_INT(GLOBAL_POSITION_INT_DATA {
                    time_boot_ms: boot_ms,
                    lat: (lat * 1e7) as i32,
                    lon: (lon * 1e7) as i32,
                    alt: (alt * 1000.0) as i32,
                    relative_alt: (relative * 1000.0) as i32,
                    hdg: (vehicle.yaw_deg * 100.0).clamp(0.0, 35_999.0) as u16,
                    ..GLOBAL_POSITION_INT_DATA::default()
                }));
                out
            }
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
                vec![
                    MavMessage::GPS_RAW_INT(GPS_RAW_INT_DATA {
                        time_usec: timed.time_us,
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
                    MavMessage::VFR_HUD(VFR_HUD_DATA {
                        airspeed: *speed as f32,
                        groundspeed: *speed as f32,
                        alt: *alt as f32,
                        climb: 0.0,
                        heading: vehicle.yaw_deg as i16,
                        throttle: 0,
                    }),
                ]
            }
            Event::Battery {
                volts,
                amps,
                remaining,
            } => {
                vec![MavMessage::SYS_STATUS(SYS_STATUS_DATA {
                    voltage_battery: (volts * 1000.0).clamp(0.0, 65_534.0) as u16,
                    current_battery: (amps * 100.0).clamp(0.0, 32_767.0) as i16,
                    battery_remaining: remaining.map_or(-1, |r| r.clamp(0, 100) as i8),
                    ..SYS_STATUS_DATA::default()
                })]
            }
            Event::Text(text) => vec![MavMessage::STATUSTEXT(STATUSTEXT_DATA {
                severity: MavSeverity::MAV_SEVERITY_INFO,
                text: truncate_text(text).as_str().into(),
                ..STATUSTEXT_DATA::default()
            })],
        };
        for message in messages {
            if !sender.send(message).await {
                alive = false;
                break 'events;
            }
        }
    }

    // End of log: keep the link alive on the last known state until the app disconnects.
    while alive {
        tokio::time::sleep(Duration::from_secs(1)).await;
        alive = sender.send(vehicle.heartbeat()).await;
    }
    responder.abort();
    writer.abort();
}

#[cfg(test)]
mod tests {
    use super::*;

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
