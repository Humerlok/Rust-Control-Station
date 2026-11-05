//! The DroneCAN inspector: per node and message type counters, rate and bandwidth over a
//! 3 s window, the last decoded value and the watched numeric series. It produces the same
//! [`InspectorSnapshot`] the MAVLink inspector does, so the screen reuses the tree, the fields
//! table and the graph:
//!
//! - the "system" is the source node id (0 for anonymous transfers),
//! - the "component" is the kind of transfer: 0 broadcast, 1 service request, 2 service response,
//! - the "message" is the data type id (the service id for requests and responses).
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    time::{Duration, Instant},
};

use control_station_dronecan::Value;
use serde_json::Value as Json;

use crate::inspector::{parse_key, InspectorField, InspectorMessage, InspectorSnapshot, WatchKey};

const WINDOW: Duration = Duration::from_secs(3);
/// Shortest span rates are computed over, so the first transfers do not read as huge rates.
const MIN_SPAN: Duration = Duration::from_millis(500);
pub(crate) const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(250);
const MAX_PENDING_POINTS: usize = 20_000;

pub(crate) const KIND_BROADCAST: u8 = 0;
pub(crate) const KIND_REQUEST: u8 = 1;
pub(crate) const KIND_RESPONSE: u8 = 2;

fn kind_name(kind: u8) -> &'static str {
    match kind {
        KIND_BROADCAST => "Broadcast",
        KIND_REQUEST => "Service requests",
        _ => "Service responses",
    }
}

fn scalar(value: &Value) -> Option<Json> {
    Some(match value {
        Value::Bool(v) => Json::Bool(*v),
        Value::Uint(v) => Json::from(*v),
        Value::Int(v) => Json::from(*v),
        Value::Float(v) if v.is_finite() => Json::from(*v),
        Value::Float(v) => Json::String(v.to_string()),
        _ => return None,
    })
}

/// A byte array that reads as text (`name`, `text`, `source`...).
fn printable_text(items: &[Value]) -> Option<String> {
    if items.len() < 2 {
        return None;
    }
    let bytes: Option<Vec<u8>> = items
        .iter()
        .map(|v| match v {
            Value::Uint(b) if (0x20..0x7F).contains(b) => Some(*b as u8),
            _ => None,
        })
        .collect();
    bytes.and_then(|b| String::from_utf8(b).ok())
}

fn flatten_into(prefix: &str, value: &Value, out: &mut Vec<InspectorField>) {
    let join = |name: &str| {
        if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}.{name}")
        }
    };
    match value {
        Value::Struct(fields) => {
            for (name, inner) in fields {
                flatten_into(&join(name), inner, out);
            }
        }
        Value::Union(variant, inner) => match &**inner {
            // A variant that carries nothing (`empty`) is shown by its name.
            Value::Struct(fields) if fields.is_empty() => out.push(InspectorField {
                name: prefix.to_owned(),
                value: Json::String(variant.clone()),
            }),
            other => flatten_into(&join(variant), other, out),
        },
        Value::Array(items) => {
            if let Some(text) = printable_text(items) {
                out.push(InspectorField {
                    name: prefix.to_owned(),
                    value: Json::String(text),
                });
            } else if items.iter().all(|v| scalar(v).is_some()) {
                out.push(InspectorField {
                    name: prefix.to_owned(),
                    value: Json::Array(items.iter().filter_map(scalar).collect()),
                });
            } else {
                for (index, item) in items.iter().enumerate() {
                    flatten_into(&format!("{prefix}[{index}]"), item, out);
                }
            }
        }
        other => {
            if let Some(json) = scalar(other) {
                out.push(InspectorField {
                    name: prefix.to_owned(),
                    value: json,
                });
            }
        }
    }
}

/// A decoded value as the rows of the fields table: nested structs are joined with `.`, a union
/// is shown under its variant, arrays of numbers stay arrays and arrays of structs get an index.
pub(crate) fn flatten(value: &Value) -> Vec<InspectorField> {
    let mut out = Vec::new();
    flatten_into("", value, &mut out);
    out
}

/// The number a watch key points at, if the field exists and is numeric.
fn numeric_value(value: &Value, watch: &WatchKey) -> Option<f64> {
    let fields = flatten(value);
    let field = fields.iter().find(|f| f.name == watch.field)?;
    let leaf = match (&field.value, watch.index) {
        (Json::Array(items), Some(index)) => items.get(index)?,
        (single, None) => single,
        _ => return None,
    };
    match leaf {
        Json::Number(n) => n.as_f64(),
        Json::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
}

struct Stats {
    name: &'static str,
    count: u64,
    last_seen: Instant,
    /// (arrival, payload length) of the transfers inside the sliding window.
    samples: VecDeque<(Instant, usize)>,
    last: Option<Value>,
}

pub(crate) struct DroneCanInspector {
    pub(crate) enabled: bool,
    /// Time zero of the series.
    epoch: Instant,
    counted_since: Instant,
    messages: BTreeMap<(u8, u8, u32), Stats>,
    watches: HashMap<String, WatchKey>,
    pending: BTreeMap<String, Vec<(f64, f64)>>,
    pub(crate) last_emit: Instant,
}

impl DroneCanInspector {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            enabled: false,
            epoch: now,
            counted_since: now,
            messages: BTreeMap::new(),
            watches: HashMap::new(),
            pending: BTreeMap::new(),
            last_emit: now,
        }
    }

    pub(crate) fn enable(&mut self, now: Instant) {
        self.enabled = true;
        self.epoch = now;
        self.clear(now);
    }

    pub(crate) fn disable(&mut self, now: Instant) {
        self.enabled = false;
        self.clear(now);
    }

    /// Resets counters and pending series; watches and the series time base stay.
    pub(crate) fn clear(&mut self, now: Instant) {
        self.messages.clear();
        self.pending.clear();
        self.counted_since = now;
    }

    pub(crate) fn watch(&mut self, key: &str) -> Result<(), String> {
        let parsed = parse_key(key)?;
        self.watches.insert(key.to_owned(), parsed);
        Ok(())
    }

    pub(crate) fn unwatch(&mut self, key: &str) {
        self.watches.remove(key);
        self.pending.remove(key);
    }

    /// Counts one transfer: `node`, `kind` (broadcast, request or response), `id` and its name.
    pub(crate) fn on_transfer(
        &mut self,
        (node, kind, id): (u8, u8, u32),
        name: &'static str,
        payload_len: usize,
        value: Option<Value>,
        now: Instant,
    ) {
        if !self.enabled {
            return;
        }
        let stats = self
            .messages
            .entry((node, kind, id))
            .or_insert_with(|| Stats {
                name,
                count: 0,
                last_seen: now,
                samples: VecDeque::new(),
                last: None,
            });
        stats.count += 1;
        stats.last_seen = now;
        stats.samples.push_back((now, payload_len));
        prune(&mut stats.samples, now);
        let Some(value) = value else {
            stats.last = None;
            return;
        };
        let time_ms = now.saturating_duration_since(self.epoch).as_secs_f64() * 1000.0;
        for (watch_key, watch) in &self.watches {
            if (watch.system_id, watch.component_id, watch.message_id) != (node, kind, id) {
                continue;
            }
            if let Some(number) = numeric_value(&value, watch) {
                let points = self.pending.entry(watch_key.clone()).or_default();
                points.push((time_ms, number));
                if points.len() > MAX_PENDING_POINTS {
                    let excess = points.len() - MAX_PENDING_POINTS;
                    points.drain(..excess);
                }
            }
        }
        stats.last = Some(value);
    }

    pub(crate) fn snapshot(&mut self, now: Instant) -> InspectorSnapshot {
        let span = now
            .saturating_duration_since(self.counted_since)
            .clamp(MIN_SPAN, WINDOW)
            .as_secs_f64();
        let messages = self
            .messages
            .iter_mut()
            .map(|(&(node, kind, id), stats)| {
                prune(&mut stats.samples, now);
                let bytes: usize = stats.samples.iter().map(|(_, len)| len).sum();
                InspectorMessage {
                    system_id: node,
                    component_id: kind,
                    component_name: kind_name(kind).to_owned(),
                    message_id: id,
                    name: stats.name,
                    count: stats.count,
                    hz: stats.samples.len() as f64 / span,
                    bytes_per_second: bytes as f64 / span,
                    age_ms: now.saturating_duration_since(stats.last_seen).as_millis() as u64,
                    fields: stats.last.as_ref().map(flatten).unwrap_or_default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn status(uptime: u64) -> Value {
        Value::strukt([
            ("uptime_sec", Value::Uint(uptime)),
            ("health", Value::Uint(0)),
            ("mode", Value::Uint(2)),
        ])
    }

    fn names(fields: &[InspectorField]) -> Vec<&str> {
        fields.iter().map(|f| f.name.as_str()).collect()
    }

    #[test]
    fn nested_structs_unions_and_arrays_become_rows() {
        let value = Value::strukt([
            ("status", status(9)),
            ("name", Value::text("com.example.gps")),
            (
                "ids",
                Value::Array(vec![Value::Uint(1), Value::Uint(2), Value::Uint(3)]),
            ),
            ("one_byte", Value::Array(vec![Value::Uint(65)])),
            ("value", Value::union("integer_value", Value::Int(-5))),
            ("nothing", Value::union("empty", Value::Struct(vec![]))),
            (
                "cells",
                Value::Array(vec![
                    Value::strukt([("volts", Value::Float(3.7))]),
                    Value::strukt([("volts", Value::Float(3.8))]),
                ]),
            ),
        ]);
        let fields = flatten(&value);
        assert_eq!(
            names(&fields),
            [
                "status.uptime_sec",
                "status.health",
                "status.mode",
                "name",
                "ids",
                "one_byte",
                "value.integer_value",
                "nothing",
                "cells[0].volts",
                "cells[1].volts"
            ]
        );
        let get = |n: &str| fields.iter().find(|f| f.name == n).unwrap().value.clone();
        assert_eq!(get("name"), Json::String("com.example.gps".into()));
        assert_eq!(get("ids"), serde_json::json!([1, 2, 3]));
        assert_eq!(
            get("one_byte"),
            serde_json::json!([65]),
            "one byte is a number, not text"
        );
        assert_eq!(get("value.integer_value"), Json::from(-5));
        assert_eq!(get("nothing"), Json::String("empty".into()));
        assert_eq!(get("cells[1].volts"), Json::from(3.8));
    }

    #[test]
    fn non_finite_floats_do_not_break_the_json() {
        let fields = flatten(&Value::strukt([("x", Value::Float(f64::NAN))]));
        assert_eq!(fields[0].value, Json::String("NaN".into()));
    }

    #[test]
    fn rate_and_bandwidth_use_a_three_second_window() {
        let t0 = Instant::now();
        let mut inspector = DroneCanInspector::new(t0);
        inspector.enable(t0);
        for i in 0..30u64 {
            inspector.on_transfer(
                (10, KIND_BROADCAST, 341),
                "NodeStatus",
                7,
                Some(status(i)),
                t0 + Duration::from_millis(100 * i),
            );
        }
        let snapshot = inspector.snapshot(t0 + Duration::from_secs(3));
        let m = &snapshot.messages[0];
        assert_eq!((m.system_id, m.component_id, m.message_id), (10, 0, 341));
        assert_eq!(m.component_name, "Broadcast");
        assert_eq!(m.count, 30);
        assert!((m.hz - 10.0).abs() < 1.0, "{}", m.hz);
        assert!(
            (m.bytes_per_second - 70.0).abs() < 8.0,
            "{}",
            m.bytes_per_second
        );
        assert!(m
            .fields
            .iter()
            .any(|f| f.name == "status.uptime_sec" || f.name == "uptime_sec"));
        // Silence: the window empties, the count stays, the age grows.
        let later = inspector.snapshot(t0 + Duration::from_secs(9));
        assert_eq!(later.messages[0].hz, 0.0);
        assert_eq!(later.messages[0].count, 30);
        assert!(later.messages[0].age_ms >= 5_000);
    }

    #[test]
    fn watched_fields_record_every_sample_once() {
        let t0 = Instant::now();
        let mut inspector = DroneCanInspector::new(t0);
        assert!(inspector.watch("nonsense").is_err());
        inspector.watch("10/0/341/uptime_sec").unwrap();
        inspector.watch("10/0/341/missing").unwrap();
        inspector.enable(t0);
        for i in 0..5u64 {
            inspector.on_transfer(
                (10, KIND_BROADCAST, 341),
                "NodeStatus",
                7,
                Some(status(100 + i)),
                t0 + Duration::from_millis(10 * i),
            );
        }
        // Another node or kind is not recorded.
        inspector.on_transfer(
            (11, KIND_BROADCAST, 341),
            "NodeStatus",
            7,
            Some(status(1)),
            t0,
        );
        let snapshot = inspector.snapshot(t0 + Duration::from_millis(100));
        let points = &snapshot.series["10/0/341/uptime_sec"];
        assert_eq!(points.len(), 5);
        assert_eq!(points[4].1, 104.0);
        assert!(points[1].0 > points[0].0);
        assert!(!snapshot.series.contains_key("10/0/341/missing"));
        // Points are handed out once.
        assert!(inspector
            .snapshot(t0 + Duration::from_millis(200))
            .series
            .is_empty());
        // Unwatching stops the recording.
        inspector.unwatch("10/0/341/uptime_sec");
        inspector.on_transfer(
            (10, KIND_BROADCAST, 341),
            "NodeStatus",
            7,
            Some(status(1)),
            t0,
        );
        assert!(inspector
            .snapshot(t0 + Duration::from_millis(300))
            .series
            .is_empty());
    }

    #[test]
    fn array_elements_can_be_watched_and_disabled_means_idle() {
        let t0 = Instant::now();
        let mut inspector = DroneCanInspector::new(t0);
        inspector.watch("5/0/1/ids[1]").unwrap();
        let value = Value::strukt([("ids", Value::Array(vec![Value::Uint(7), Value::Uint(8)]))]);
        inspector.on_transfer((5, 0, 1), "X", 2, Some(value.clone()), t0);
        assert!(
            inspector.snapshot(t0).messages.is_empty(),
            "off: nothing is counted"
        );
        inspector.enable(t0);
        inspector.on_transfer((5, 0, 1), "X", 2, Some(value), t0);
        assert_eq!(inspector.snapshot(t0).series["5/0/1/ids[1]"][0].1, 8.0);
        inspector.clear(t0);
        assert!(inspector.snapshot(t0).messages.is_empty());
        inspector.disable(t0);
        assert!(!inspector.enabled);
    }

    #[test]
    fn a_transfer_that_does_not_decode_still_counts() {
        let t0 = Instant::now();
        let mut inspector = DroneCanInspector::new(t0);
        inspector.enable(t0);
        inspector.on_transfer((3, KIND_RESPONSE, 9), "unknown", 4, None, t0);
        let m = &inspector.snapshot(t0).messages[0];
        assert_eq!(
            (m.count, m.component_name.as_str()),
            (1, "Service responses")
        );
        assert!(m.fields.is_empty());
    }
}
