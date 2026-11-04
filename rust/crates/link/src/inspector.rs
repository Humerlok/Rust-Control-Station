//! MAVLink inspector: per-message statistics, the last decoded fields of every message and
//! numeric time series for the fields the UI is graphing. A service on top of the packet bus.
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use control_station_mavlink::{MavMessage, Message, Packet};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;

use crate::{
    manager::LinkManager,
    types::{LinkEvent, LinkState},
};

const WINDOW: Duration = Duration::from_secs(3);
/// Shortest span rates are computed over, so the first packets do not read as huge rates.
const MIN_SPAN: Duration = Duration::from_millis(500);
const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(250);
const MAX_PENDING_POINTS: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InspectorField {
    pub name: String,
    /// A number, string, boolean or an array of those.
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectorMessage {
    pub system_id: u8,
    pub component_id: u8,
    pub component_name: String,
    pub message_id: u32,
    pub name: &'static str,
    pub count: u64,
    pub hz: f64,
    pub bytes_per_second: f64,
    pub age_ms: u64,
    pub fields: Vec<InspectorField>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InspectorSnapshot {
    pub messages: Vec<InspectorMessage>,
    /// New `[timeMs, value]` points since the previous snapshot, keyed by watch key.
    pub series: BTreeMap<String, Vec<(f64, f64)>>,
}

/// A parsed watch key: `systemId/componentId/messageId/field` or `.../field[index]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchKey {
    pub system_id: u8,
    pub component_id: u8,
    pub message_id: u32,
    pub field: String,
    pub index: Option<usize>,
}

pub fn parse_key(key: &str) -> Result<WatchKey, String> {
    let bad = || format!("malformed watch key '{key}' (expected system/component/message/field)");
    let mut parts = key.splitn(4, '/');
    let (Some(sys), Some(comp), Some(msg), Some(field)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(bad());
    };
    let system_id = sys.parse().map_err(|_| bad())?;
    let component_id = comp.parse().map_err(|_| bad())?;
    let message_id = msg.parse().map_err(|_| bad())?;
    let (field, index) = match field.strip_suffix(']') {
        Some(open) => {
            let (name, index) = open.split_once('[').ok_or_else(bad)?;
            (name, Some(index.parse::<usize>().map_err(|_| bad())?))
        }
        None => (field, None),
    };
    if field.is_empty() || field.contains(['[', ']', '/']) {
        return Err(bad());
    }
    Ok(WatchKey {
        system_id,
        component_id,
        message_id,
        field: field.to_owned(),
        index,
    })
}

pub fn component_name(id: u8) -> String {
    let numbered = |base: &str, first: u8| {
        if id == first {
            base.to_owned()
        } else {
            format!("{base} {}", id - first + 1)
        }
    };
    match id {
        0 => "All".into(),
        1 => "Autopilot".into(),
        68 => "Telemetry radio".into(),
        100..=105 => numbered("Camera", 100),
        110..=112 => numbered("Radio", 110),
        140..=153 => format!("Servo {}", id - 139),
        154 => "Gimbal".into(),
        155 => "Log".into(),
        156 => "ADS-B".into(),
        157 => "OSD".into(),
        158 => "Peripheral".into(),
        159 => "QX1 gimbal".into(),
        160 => "FLARM".into(),
        161 => "Parachute".into(),
        169 => "Winch".into(),
        171..=175 => format!("Gimbal {}", id - 169),
        180 | 181 => numbered("Battery", 180),
        189 => "MAVCAN".into(),
        190 => "Mission Planner".into(),
        191..=194 => numbered("Onboard computer", 191),
        195 => "Path planner".into(),
        196 => "Obstacle avoidance".into(),
        197 => "Visual-inertial odometry".into(),
        198 => "Pairing manager".into(),
        200..=202 => numbered("IMU", 200),
        220 | 221 => numbered("GPS", 220),
        236..=238 => format!("Open Drone ID {}", id - 235),
        240 => "UDP bridge".into(),
        241 => "UART bridge".into(),
        242 => "Tunnel node".into(),
        243 => "Illuminator".into(),
        250 => "System control".into(),
        other => format!("Component {other}"),
    }
}

/// Turns the serde form of the generated message structs into UI-friendly values: enum objects
/// (`{"type":"MAV_TYPE_X"}`) become their name, strings lose trailing NULs, non-finite floats
/// (serialized as null) become "NaN".
fn normalize(value: Value) -> Value {
    match value {
        Value::Object(mut map) => match map.remove("type") {
            Some(Value::String(name)) if map.is_empty() => Value::String(name),
            Some(other) => {
                map.insert("type".into(), other);
                Value::String(Value::Object(map).to_string())
            }
            None => Value::String(Value::Object(map).to_string()),
        },
        Value::Array(items) => Value::Array(items.into_iter().map(normalize).collect()),
        Value::String(text) => Value::String(
            text.trim_end_matches(|c: char| c == '\0' || c.is_whitespace())
                .to_owned(),
        ),
        Value::Null => Value::String("NaN".into()),
        scalar => scalar,
    }
}

/// The message's fields in wire order, without the `type` tag.
pub fn decode_fields(message: &MavMessage) -> Vec<InspectorField> {
    match serde_json::to_value(message) {
        Ok(Value::Object(map)) => map
            .into_iter()
            .filter(|(name, _)| name != "type")
            .map(|(name, value)| InspectorField {
                name,
                value: normalize(value),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The numeric value of a watched field in the raw serde form (booleans are 0/1).
fn numeric_value(raw: &Value, key: &WatchKey) -> Option<f64> {
    let mut value = raw.as_object()?.get(&key.field)?;
    if let Some(index) = key.index {
        value = value.as_array()?.get(index)?;
    }
    match value {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
}

struct MessageStats {
    name: &'static str,
    count: u64,
    last_seen: Instant,
    /// (arrival, wire length) of the packets inside the sliding window.
    samples: VecDeque<(Instant, usize)>,
    last: Arc<Packet>,
}

struct Inner {
    enabled: bool,
    /// Time zero of the series.
    epoch: Instant,
    counted_since: Instant,
    messages: BTreeMap<(u8, u8, u32), MessageStats>,
    watches: HashMap<String, WatchKey>,
    pending: BTreeMap<String, Vec<(f64, f64)>>,
}

impl Inner {
    fn new(now: Instant) -> Self {
        Self {
            enabled: false,
            epoch: now,
            counted_since: now,
            messages: BTreeMap::new(),
            watches: HashMap::new(),
            pending: BTreeMap::new(),
        }
    }

    fn enable(&mut self, now: Instant) {
        self.enabled = true;
        self.epoch = now;
        self.clear(now);
    }

    fn disable(&mut self, now: Instant) {
        self.enabled = false;
        self.clear(now);
    }

    /// Resets counters and pending series; watches and the series time base stay.
    fn clear(&mut self, now: Instant) {
        self.messages.clear();
        self.pending.clear();
        self.counted_since = now;
    }

    fn on_packet(&mut self, packet: &Arc<Packet>, now: Instant) {
        if !self.enabled {
            return;
        }
        let message_id = packet.message.message_id();
        let key = (packet.system_id(), packet.component_id(), message_id);
        let stats = self.messages.entry(key).or_insert_with(|| MessageStats {
            name: packet.message.message_name(),
            count: 0,
            last_seen: now,
            samples: VecDeque::new(),
            last: packet.clone(),
        });
        stats.count += 1;
        stats.last_seen = now;
        stats.last = packet.clone();
        stats.samples.push_back((now, packet.wire_len));
        prune(&mut stats.samples, now);

        if !self
            .watches
            .values()
            .any(|w| (w.system_id, w.component_id, w.message_id) == key)
        {
            return;
        }
        let Ok(raw) = serde_json::to_value(&packet.message) else {
            return;
        };
        let time_ms = now.saturating_duration_since(self.epoch).as_secs_f64() * 1000.0;
        for (name, watch) in &self.watches {
            if (watch.system_id, watch.component_id, watch.message_id) != key {
                continue;
            }
            if let Some(value) = numeric_value(&raw, watch) {
                let points = self.pending.entry(name.clone()).or_default();
                points.push((time_ms, value));
                if points.len() > MAX_PENDING_POINTS {
                    let excess = points.len() - MAX_PENDING_POINTS;
                    points.drain(..excess);
                }
            }
        }
    }

    fn snapshot(&mut self, now: Instant) -> InspectorSnapshot {
        let span = now
            .saturating_duration_since(self.counted_since)
            .clamp(MIN_SPAN, WINDOW)
            .as_secs_f64();
        let messages = self
            .messages
            .iter_mut()
            .map(|(&(system_id, component_id, message_id), stats)| {
                prune(&mut stats.samples, now);
                let bytes: usize = stats.samples.iter().map(|(_, len)| len).sum();
                InspectorMessage {
                    system_id,
                    component_id,
                    component_name: component_name(component_id),
                    message_id,
                    name: stats.name,
                    count: stats.count,
                    hz: stats.samples.len() as f64 / span,
                    bytes_per_second: bytes as f64 / span,
                    age_ms: now.saturating_duration_since(stats.last_seen).as_millis() as u64,
                    fields: decode_fields(&stats.last.message),
                }
            })
            .collect();
        InspectorSnapshot {
            messages,
            series: std::mem::take(&mut self.pending),
        }
    }
}

fn prune(samples: &mut VecDeque<(Instant, usize)>, now: Instant) {
    let Some(cutoff) = now.checked_sub(WINDOW) else {
        return;
    };
    while samples.front().is_some_and(|(t, _)| *t < cutoff) {
        samples.pop_front();
    }
}

type Receivers = (
    broadcast::Receiver<Arc<Packet>>,
    broadcast::Receiver<LinkEvent>,
);

struct Shared {
    inner: Mutex<Inner>,
    events: broadcast::Sender<InspectorSnapshot>,
    /// Subscribed at construction so nothing is missed before `run` is scheduled.
    receivers: Mutex<Option<Receivers>>,
}

#[derive(Clone)]
pub struct InspectorService {
    shared: Arc<Shared>,
}

impl InspectorService {
    pub fn new(link: LinkManager) -> Self {
        let (events, _) = broadcast::channel(64);
        let receivers = (link.subscribe_packets(), link.subscribe());
        Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner::new(Instant::now())),
                events,
                receivers: Mutex::new(Some(receivers)),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.shared.inner.lock().expect("inspector state lock")
    }

    pub fn subscribe(&self) -> broadcast::Receiver<InspectorSnapshot> {
        self.shared.events.subscribe()
    }

    /// Enabling resets the counters and the series time base; disabling stops the snapshots.
    pub fn set_enabled(&self, enabled: bool) {
        let mut inner = self.lock();
        let now = Instant::now();
        match (enabled, inner.enabled) {
            (true, false) => inner.enable(now),
            (false, true) => inner.disable(now),
            _ => {}
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.lock().enabled
    }

    pub fn watch(&self, key: &str) -> Result<(), String> {
        let parsed = parse_key(key)?;
        self.lock().watches.insert(key.to_owned(), parsed);
        Ok(())
    }

    pub fn unwatch(&self, key: &str) {
        let mut inner = self.lock();
        inner.watches.remove(key);
        inner.pending.remove(key);
    }

    /// Resets the counters and pending series; the watches stay.
    pub fn clear(&self) {
        self.lock().clear(Instant::now());
    }

    /// Consumes the packet bus and publishes a snapshot every 250 ms while enabled. Spawn it
    /// once; a second call returns immediately.
    pub async fn run(&self) {
        let Some((mut packets, mut events)) = self
            .shared
            .receivers
            .lock()
            .expect("inspector receivers lock")
            .take()
        else {
            return;
        };
        let mut ticker = tokio::time::interval(SNAPSHOT_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                packet = packets.recv() => match packet {
                    Ok(packet) => self.lock().on_packet(&packet, Instant::now()),
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                },
                event = events.recv() => match event {
                    Ok(LinkEvent::Link(info)) if info.state != LinkState::Connected => {
                        self.lock().clear(Instant::now());
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                },
                _ = ticker.tick() => {
                    let snapshot = {
                        let mut inner = self.lock();
                        if !inner.enabled {
                            continue;
                        }
                        inner.snapshot(Instant::now())
                    };
                    let _ = self.shared.events.send(snapshot);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use control_station_mavlink::{
        dialect::{
            GpsFixType, MavAutopilot, MavModeFlag, MavParamType, MavSeverity, MavState, MavType,
            ATTITUDE_DATA, GPS_RAW_INT_DATA, HEARTBEAT_DATA, MAG_CAL_PROGRESS_DATA,
            PARAM_VALUE_DATA, RC_CHANNELS_DATA, STATUSTEXT_DATA,
        },
        MavHeader, MavlinkVersion,
    };
    use serde_json::json;

    fn packet_from(system: u8, component: u8, message: MavMessage, wire_len: usize) -> Arc<Packet> {
        Arc::new(Packet {
            header: MavHeader {
                system_id: u32::from(system),
                component_id: component,
                sequence: 0,
            },
            message,
            version: MavlinkVersion::V2,
            wire_len,
        })
    }

    fn attitude(roll: f32) -> MavMessage {
        MavMessage::ATTITUDE(ATTITUDE_DATA {
            roll,
            ..ATTITUDE_DATA::default()
        })
    }

    fn field<'a>(fields: &'a [InspectorField], name: &str) -> &'a Value {
        &fields
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("no field {name} in {fields:?}"))
            .value
    }

    #[test]
    fn heartbeat_fields_are_flattened_and_ordered() {
        let fields = decode_fields(&MavMessage::HEARTBEAT(HEARTBEAT_DATA {
            custom_mode: 5,
            mavtype: MavType::MAV_TYPE_QUADROTOR,
            autopilot: MavAutopilot::MAV_AUTOPILOT_ARDUPILOTMEGA,
            base_mode: MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED
                | MavModeFlag::MAV_MODE_FLAG_CUSTOM_MODE_ENABLED,
            system_status: MavState::MAV_STATE_ACTIVE,
            mavlink_version: 3,
        }));
        assert!(fields.iter().all(|f| f.name != "type"));
        assert_eq!(field(&fields, "custom_mode"), &json!(5));
        assert_eq!(field(&fields, "mavtype"), &json!("MAV_TYPE_QUADROTOR"));
        assert_eq!(field(&fields, "system_status"), &json!("MAV_STATE_ACTIVE"));
        let base_mode = field(&fields, "base_mode")
            .as_str()
            .expect("bitflags are text");
        assert!(base_mode.contains("MAV_MODE_FLAG_SAFETY_ARMED"));
        let names: Vec<_> = fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names[0], "custom_mode",
            "wire order, not alphabetical: {names:?}"
        );
    }

    #[test]
    fn numeric_text_and_enum_fields_decode() {
        let attitude = decode_fields(&attitude(0.5));
        assert_eq!(field(&attitude, "roll"), &json!(0.5));
        assert_eq!(field(&attitude, "time_boot_ms"), &json!(0));

        let text = decode_fields(&MavMessage::STATUSTEXT(STATUSTEXT_DATA {
            severity: MavSeverity::MAV_SEVERITY_WARNING,
            text: "PreArm: Check GPS".into(),
            ..STATUSTEXT_DATA::default()
        }));
        assert_eq!(field(&text, "text"), &json!("PreArm: Check GPS"));
        assert_eq!(field(&text, "severity"), &json!("MAV_SEVERITY_WARNING"));

        let param = decode_fields(&MavMessage::PARAM_VALUE(PARAM_VALUE_DATA {
            param_id: "ATC_RAT_RLL_P".into(),
            param_value: 0.25,
            param_type: MavParamType::MAV_PARAM_TYPE_REAL32,
            param_count: 10,
            param_index: 2,
        }));
        assert_eq!(field(&param, "param_id"), &json!("ATC_RAT_RLL_P"));
        assert_eq!(field(&param, "param_type"), &json!("MAV_PARAM_TYPE_REAL32"));
        assert_eq!(field(&param, "param_count"), &json!(10));

        let gps = decode_fields(&MavMessage::GPS_RAW_INT(GPS_RAW_INT_DATA {
            fix_type: GpsFixType::GPS_FIX_TYPE_3D_FIX,
            lat: -205_475_132,
            ..GPS_RAW_INT_DATA::default()
        }));
        assert_eq!(field(&gps, "fix_type"), &json!("GPS_FIX_TYPE_3D_FIX"));
        assert_eq!(field(&gps, "lat"), &json!(-205_475_132));
    }

    #[test]
    fn rc_channels_and_arrays_decode() {
        let rc = decode_fields(&MavMessage::RC_CHANNELS(RC_CHANNELS_DATA {
            chan1_raw: 1500,
            chan16_raw: 1900,
            chancount: 16,
            ..RC_CHANNELS_DATA::default()
        }));
        assert_eq!(field(&rc, "chan1_raw"), &json!(1500));
        assert_eq!(field(&rc, "chan16_raw"), &json!(1900));
        assert_eq!(field(&rc, "chancount"), &json!(16));

        let cal = decode_fields(&MavMessage::MAG_CAL_PROGRESS(MAG_CAL_PROGRESS_DATA {
            completion_mask: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            ..MAG_CAL_PROGRESS_DATA::default()
        }));
        assert_eq!(
            field(&cal, "completion_mask"),
            &json!([1, 2, 3, 4, 5, 6, 7, 8, 9, 10])
        );
    }

    #[test]
    fn non_finite_floats_and_padded_text_are_made_readable() {
        let nan = decode_fields(&attitude(f32::NAN));
        assert_eq!(field(&nan, "roll"), &json!("NaN"));
        assert_eq!(normalize(json!("abc\u{0}\u{0}  ")), json!("abc"));
        assert_eq!(normalize(json!({"type": "X"})), json!("X"));
        assert_eq!(normalize(json!([{"type": "A"}, 2])), json!(["A", 2]));
    }

    #[test]
    fn watch_keys_parse_and_reject_garbage() {
        assert_eq!(
            parse_key("1/1/30/roll").unwrap(),
            WatchKey {
                system_id: 1,
                component_id: 1,
                message_id: 30,
                field: "roll".into(),
                index: None
            }
        );
        let indexed = parse_key("255/190/191/completion_mask[3]").unwrap();
        assert_eq!(
            (indexed.field.as_str(), indexed.index),
            ("completion_mask", Some(3))
        );
        for bad in [
            "",
            "1/1/30",
            "1/1/30/",
            "a/1/30/roll",
            "300/1/30/roll",
            "1/1/-5/roll",
            "1/1/30/roll[",
            "1/1/30/roll[x]",
            "1/1/30/[2]",
            "1/1/30/ro[ll",
        ] {
            assert!(parse_key(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn watched_values_come_from_fields_arrays_and_booleans() {
        let raw = serde_json::to_value(MavMessage::MAG_CAL_PROGRESS(MAG_CAL_PROGRESS_DATA {
            completion_mask: [0, 0, 0, 42, 0, 0, 0, 0, 0, 0],
            direction_x: 1.5,
            ..MAG_CAL_PROGRESS_DATA::default()
        }))
        .unwrap();
        let get = |key: &str| numeric_value(&raw, &parse_key(&format!("1/1/191/{key}")).unwrap());
        assert_eq!(get("direction_x"), Some(1.5));
        assert_eq!(get("completion_mask[3]"), Some(42.0));
        assert_eq!(get("completion_mask[10]"), None);
        assert_eq!(get("cal_status"), None, "enum objects are not numeric");
        assert_eq!(get("missing"), None);
        let flag = json!({"armed": true, "off": false});
        let key = |f: &str| WatchKey {
            system_id: 1,
            component_id: 1,
            message_id: 0,
            field: f.into(),
            index: None,
        };
        assert_eq!(numeric_value(&flag, &key("armed")), Some(1.0));
        assert_eq!(numeric_value(&flag, &key("off")), Some(0.0));
    }

    fn enabled_inner(t0: Instant) -> Inner {
        let mut inner = Inner::new(t0);
        inner.enable(t0);
        inner
    }

    #[test]
    fn rate_and_bandwidth_use_a_three_second_window() {
        let t0 = Instant::now();
        let mut inner = enabled_inner(t0);
        let p = packet_from(1, 1, attitude(0.0), 40);
        for i in 0..60 {
            inner.on_packet(&p, t0 + Duration::from_millis(50 * i));
        }
        let snap = inner.snapshot(t0 + Duration::from_secs(3));
        let m = &snap.messages[0];
        assert_eq!((m.name, m.count), ("ATTITUDE", 60));
        assert!((m.hz - 20.0).abs() < 0.01, "{}", m.hz);
        assert!(
            (m.bytes_per_second - 800.0).abs() < 0.5,
            "{}",
            m.bytes_per_second
        );
        assert_eq!(m.age_ms, 50);

        // Silence: the window empties, the count stays, the age grows.
        let later = inner.snapshot(t0 + Duration::from_secs(8));
        let m = &later.messages[0];
        assert_eq!(m.hz, 0.0);
        assert_eq!(m.bytes_per_second, 0.0);
        assert_eq!(m.count, 60);
        assert_eq!(m.age_ms, 5050);
    }

    #[test]
    fn early_rates_are_not_inflated_by_a_tiny_span() {
        let t0 = Instant::now();
        let mut inner = enabled_inner(t0);
        let p = packet_from(1, 1, attitude(0.0), 40);
        inner.on_packet(&p, t0 + Duration::from_millis(10));
        // One packet 50 ms after enabling must not read as 20 Hz.
        let snap = inner.snapshot(t0 + Duration::from_millis(50));
        assert!((snap.messages[0].hz - 2.0).abs() < 0.01);
    }

    #[test]
    fn messages_are_sorted_by_system_component_and_id() {
        let t0 = Instant::now();
        let mut inner = enabled_inner(t0);
        let hb = MavMessage::HEARTBEAT(HEARTBEAT_DATA::default());
        inner.on_packet(&packet_from(2, 1, attitude(0.0), 10), t0);
        inner.on_packet(&packet_from(1, 191, hb.clone(), 10), t0);
        inner.on_packet(&packet_from(1, 1, attitude(0.0), 10), t0);
        inner.on_packet(&packet_from(1, 1, hb, 10), t0);
        let keys: Vec<_> = inner
            .snapshot(t0)
            .messages
            .iter()
            .map(|m| (m.system_id, m.component_id, m.message_id))
            .collect();
        assert_eq!(keys, vec![(1, 1, 0), (1, 1, 30), (1, 191, 0), (2, 1, 30)]);
    }

    #[test]
    fn disabled_inspector_records_nothing() {
        let t0 = Instant::now();
        let mut inner = Inner::new(t0);
        inner.on_packet(&packet_from(1, 1, attitude(0.0), 10), t0);
        assert!(inner.snapshot(t0).messages.is_empty());
    }

    fn watch(inner: &mut Inner, key: &str) {
        inner.watches.insert(key.into(), parse_key(key).unwrap());
    }

    #[test]
    fn watched_points_accumulate_every_packet_then_are_taken() {
        let t0 = Instant::now();
        let mut inner = enabled_inner(t0);
        watch(&mut inner, "1/1/30/roll");
        watch(&mut inner, "1/1/30/nope");
        for (i, roll) in [0.1_f32, 0.2, 0.3].into_iter().enumerate() {
            inner.on_packet(
                &packet_from(1, 1, attitude(roll), 10),
                t0 + Duration::from_millis(100 * (i as u64 + 1)),
            );
        }
        // A packet of another message does not feed the series.
        inner.on_packet(
            &packet_from(1, 1, MavMessage::HEARTBEAT(HEARTBEAT_DATA::default()), 10),
            t0 + Duration::from_millis(350),
        );
        let snap = inner.snapshot(t0 + Duration::from_millis(400));
        assert_eq!(
            snap.series.len(),
            1,
            "non-numeric / missing fields are ignored"
        );
        let points = &snap.series["1/1/30/roll"];
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].0, 100.0);
        assert_eq!(points[2].0, 300.0);
        assert!((points[1].1 - 0.2).abs() < 1e-6);
        assert!(inner
            .snapshot(t0 + Duration::from_millis(500))
            .series
            .is_empty());
    }

    #[test]
    fn pending_series_are_capped() {
        let t0 = Instant::now();
        let mut inner = enabled_inner(t0);
        watch(&mut inner, "1/1/30/roll");
        let p = packet_from(1, 1, attitude(1.0), 10);
        for i in 0..(MAX_PENDING_POINTS as u64 + 500) {
            inner.on_packet(&p, t0 + Duration::from_micros(i));
        }
        let points = &inner.snapshot(t0 + Duration::from_secs(1)).series["1/1/30/roll"];
        assert_eq!(points.len(), MAX_PENDING_POINTS);
        assert!(points[0].0 > 0.0, "the oldest points were dropped");
    }

    #[test]
    fn clear_resets_counters_but_keeps_watches_and_time_base() {
        let t0 = Instant::now();
        let mut inner = enabled_inner(t0);
        watch(&mut inner, "1/1/30/roll");
        inner.on_packet(
            &packet_from(1, 1, attitude(1.0), 10),
            t0 + Duration::from_millis(100),
        );
        inner.clear(t0 + Duration::from_millis(200));
        let snap = inner.snapshot(t0 + Duration::from_millis(300));
        assert!(snap.messages.is_empty() && snap.series.is_empty());
        inner.on_packet(
            &packet_from(1, 1, attitude(2.0), 10),
            t0 + Duration::from_millis(1000),
        );
        let snap = inner.snapshot(t0 + Duration::from_millis(1100));
        assert_eq!(snap.messages[0].count, 1);
        assert_eq!(
            snap.series["1/1/30/roll"][0].0, 1000.0,
            "time base is unchanged by clear"
        );
    }

    #[test]
    fn enabling_resets_the_time_base_and_disabling_clears() {
        let t0 = Instant::now();
        let mut inner = Inner::new(t0);
        inner.enable(t0 + Duration::from_secs(5));
        watch(&mut inner, "1/1/30/roll");
        inner.on_packet(
            &packet_from(1, 1, attitude(1.0), 10),
            t0 + Duration::from_secs(6),
        );
        let snap = inner.snapshot(t0 + Duration::from_secs(6));
        assert_eq!(snap.series["1/1/30/roll"][0].0, 1000.0);
        inner.disable(t0 + Duration::from_secs(7));
        assert!(inner
            .snapshot(t0 + Duration::from_secs(7))
            .messages
            .is_empty());
    }

    #[test]
    fn component_names_cover_common_ids_and_fall_back() {
        assert_eq!(component_name(1), "Autopilot");
        assert_eq!(component_name(100), "Camera");
        assert_eq!(component_name(102), "Camera 3");
        assert_eq!(component_name(154), "Gimbal");
        assert_eq!(component_name(172), "Gimbal 3");
        assert_eq!(component_name(190), "Mission Planner");
        assert_eq!(component_name(192), "Onboard computer 2");
        assert_eq!(component_name(221), "GPS 2");
        assert_eq!(component_name(141), "Servo 2");
        assert_eq!(component_name(77), "Component 77");
    }

    #[test]
    fn snapshot_serializes_to_the_ui_contract() {
        let t0 = Instant::now();
        let mut inner = enabled_inner(t0);
        watch(&mut inner, "1/1/30/roll");
        inner.on_packet(
            &packet_from(1, 1, attitude(0.5), 40),
            t0 + Duration::from_millis(250),
        );
        let value = serde_json::to_value(inner.snapshot(t0 + Duration::from_secs(1))).unwrap();
        let message = &value["messages"][0];
        for key in [
            "systemId",
            "componentId",
            "componentName",
            "messageId",
            "name",
            "count",
            "hz",
            "bytesPerSecond",
            "ageMs",
            "fields",
        ] {
            assert!(message.get(key).is_some(), "missing {key}: {message}");
        }
        assert_eq!(message["fields"][0]["name"], "time_boot_ms");
        assert_eq!(value["series"]["1/1/30/roll"], json!([[250.0, 0.5]]));
    }
}
