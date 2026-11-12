//! An opened log: the field tree, cached series and the log-wide tables (modes, parameters,
//! messages, presets, info).
use std::{
    cmp::Ordering,
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

use control_station_ardupilot_meta::VehicleType;
use control_station_dataflash::{Log, Value};
use control_station_params::natural_cmp;

use crate::{
    codes, docs,
    series::{view, Series},
    types::*,
};

/// Traces per panel: one colour per series slot, never cycled.
pub const MAX_TRACES: usize = 8;
/// Min/max pairs kept per trace for the visible window.
pub const BUCKETS: usize = 1500;

/// Format characters that are not numbers and cannot be plotted.
const NON_NUMERIC: &[u8] = b"nNZaA";
/// Integer format characters: a FMTU multiplier applies to these (the scaled ones, `cCeEL`, are
/// already converted by the reader).
const RAW_INTEGER: &[u8] = b"bBhHiIqQ";

#[derive(Debug)]
struct FieldEntry {
    name: String,
    col: usize,
    unit: Option<String>,
    scale: Option<f64>,
}

#[derive(Debug)]
struct TypeEntry {
    name: String,
    base: String,
    id: u8,
    instance: Option<String>,
    /// Record indices (within the message id) of this instance; `None` for all records.
    rows: Option<Vec<usize>>,
    count: usize,
    /// Time column and its scale to seconds.
    time: Option<(usize, f64)>,
    fields: Vec<FieldEntry>,
}

#[derive(Debug, Default, Clone)]
struct Version {
    firmware: Option<String>,
    hash: Option<String>,
    os: Option<String>,
    board: Option<String>,
    board_id: Option<i64>,
    build_type: Option<i64>,
}

/// An opened DataFlash log, ready for the plotter. Cheap to share behind an `Arc`; series are
/// read from the log the first time they are asked for and cached.
pub struct SpeedLog {
    log: Log,
    file_name: String,
    types: Vec<TypeEntry>,
    by_name: HashMap<String, usize>,
    version: Version,
    vehicle: VehicleType,
    span: (f64, f64),
    series: Mutex<HashMap<(usize, usize), Arc<Series>>>,
    times: Mutex<HashMap<usize, Arc<Vec<f64>>>>,
}

fn vehicle_of(build_type: Option<i64>) -> VehicleType {
    match build_type {
        Some(1) => VehicleType::Rover,
        Some(2) => VehicleType::Copter,
        Some(3) => VehicleType::Plane,
        Some(4) => VehicleType::Tracker,
        Some(7) => VehicleType::Sub,
        Some(12) => VehicleType::Blimp,
        _ => VehicleType::Unknown,
    }
}

const BUILD_TYPES: &[(&str, i64)] = &[
    ("ArduRover", 1),
    ("ArduCopter", 2),
    ("ArduPlane", 3),
    ("AntennaTracker", 4),
    ("ArduSub", 7),
    ("Blimp", 12),
];

/// `IMU[10]` after `IMU[9]`: natural order, as in SpeedLOG.
fn type_order(a: &str, b: &str) -> Ordering {
    natural_cmp(a, b).then_with(|| a.cmp(b))
}

impl SpeedLog {
    pub fn open(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Self::from_bytes(name, std::fs::read(path)?))
    }

    pub fn from_bytes(file_name: impl Into<String>, data: Vec<u8>) -> Self {
        let log = Log::from_bytes(data);
        let types = build_types(&log);
        let by_name = types
            .iter()
            .enumerate()
            .map(|(i, t)| (t.name.clone(), i))
            .collect();
        let version = read_version(&log);
        let vehicle = vehicle_of(version.build_type);
        let mut this = Self {
            log,
            file_name: file_name.into(),
            types,
            by_name,
            version,
            vehicle,
            span: (0.0, 0.0),
            series: Mutex::new(HashMap::new()),
            times: Mutex::new(HashMap::new()),
        };
        this.span = this.estimate_span();
        this
    }

    pub fn vehicle(&self) -> VehicleType {
        self.vehicle
    }

    /// Time span of the log in seconds (biggest timed type and the first mode change).
    pub fn span(&self) -> (f64, f64) {
        self.span
    }

    fn estimate_span(&self) -> (f64, f64) {
        let best = self
            .types
            .iter()
            .enumerate()
            .filter(|(_, t)| t.time.is_some())
            .max_by_key(|(_, t)| t.count);
        let mut span = best
            .and_then(|(i, _)| {
                let t = self.time_of(i)?;
                Some((*t.first()?, *t.last()?))
            })
            .unwrap_or((0.0, 0.0));
        if let Some(first) = self.modes().first() {
            span.0 = span.0.min(first.t0);
        }
        span
    }

    pub fn info(&self) -> LogInfo {
        LogInfo {
            file_name: self.file_name.clone(),
            size: self.log.size() as u64,
            firmware: self.version.firmware.clone(),
            firmware_hash: self.version.hash.clone(),
            os: self.version.os.clone(),
            board: self.version.board.clone(),
            board_id: self.version.board_id,
            vehicle: self.vehicle.as_str().to_string(),
            t_start: self.span.0,
            t_end: self.span.1,
            duration: (self.span.1 - self.span.0).max(0.0),
            type_count: self.types.len(),
            record_count: self.types.iter().map(|t| t.count).sum(),
        }
    }

    /// The field tree: every message type (instances apart) with plottable fields.
    pub fn tree(&self, lang: Lang) -> Vec<TypeNode> {
        self.types
            .iter()
            .map(|t| TypeNode {
                name: t.name.clone(),
                base: t.base.clone(),
                instance: t.instance.clone(),
                count: t.count,
                timed: t.time.is_some(),
                doc: docs::message_doc(lang, &t.name),
                fields: t
                    .fields
                    .iter()
                    .map(|f| FieldNode {
                        name: f.name.clone(),
                        unit: f.unit.clone(),
                        doc: docs::field_doc(lang, &t.name, &f.name),
                    })
                    .collect(),
            })
            .collect()
    }

    fn rows<T: Clone>(entry: &TypeEntry, all: Vec<T>) -> Vec<T> {
        match &entry.rows {
            None => all,
            Some(rows) => rows.iter().filter_map(|r| all.get(*r).cloned()).collect(),
        }
    }

    fn time_of(&self, index: usize) -> Option<Arc<Vec<f64>>> {
        if let Some(t) = self.times.lock().ok()?.get(&index) {
            return Some(Arc::clone(t));
        }
        let entry = &self.types[index];
        let (col, scale) = entry.time?;
        let raw = self.log.column(entry.id, col);
        let t: Vec<f64> = Self::rows(entry, raw)
            .iter()
            .map(|v| v.as_f64().map_or(f64::NAN, |x| x * scale))
            .collect();
        let t = Arc::new(t);
        self.times.lock().ok()?.insert(index, Arc::clone(&t));
        Some(t)
    }

    /// Time (seconds) and values of one field; `None` when the type has no time or no such field.
    pub fn series(&self, type_name: &str, field: &str) -> Option<Arc<Series>> {
        let index = *self.by_name.get(type_name)?;
        let entry = &self.types[index];
        let f = entry.fields.iter().position(|f| f.name == field)?;
        if let Some(s) = self.series.lock().ok()?.get(&(index, f)) {
            return Some(Arc::clone(s));
        }
        let t = self.time_of(index)?;
        let fe = &entry.fields[f];
        let raw = Self::rows(entry, self.log.column(entry.id, fe.col));
        let v: Vec<f64> = raw
            .iter()
            .map(|x| {
                let x = x.as_f64().unwrap_or(f64::NAN);
                fe.scale.map_or(x, |s| x * s)
            })
            .collect();
        if v.len() != t.len() {
            return None;
        }
        let s = Arc::new(Series { t, v });
        self.series.lock().ok()?.insert((index, f), Arc::clone(&s));
        Some(s)
    }

    /// One field reduced for the visible window `range` (seconds; `None` for the whole log).
    pub fn series_view(
        &self,
        type_name: &str,
        field: &str,
        range: Option<(f64, f64)>,
        buckets: usize,
    ) -> Option<SeriesView> {
        Some(view(&*self.series(type_name, field)?, range, buckets))
    }

    fn mode_name(&self, number: i64) -> Option<String> {
        let n = u32::try_from(number).ok()?;
        self.vehicle.mode_name(n).map(str::to_string)
    }

    /// Flight-mode spans from the `MODE` messages.
    pub fn modes(&self) -> Vec<ModeSpan> {
        let Some(id) = self.log.id_of("MODE") else {
            return Vec::new();
        };
        let records: Vec<_> = self.log.records_of(id).collect();
        let mut order: Vec<String> = Vec::new();
        let mut out = Vec::new();
        for (i, r) in records.iter().enumerate() {
            let (Some(t), Some(number)) = (r.time_us(), r.i64("ModeNum").or_else(|| r.i64("Mode")))
            else {
                continue;
            };
            let name = self
                .mode_name(number)
                .unwrap_or_else(|| format!("Mode {number}"));
            let slot = match order.iter().position(|n| *n == name) {
                Some(p) => p,
                None => {
                    order.push(name.clone());
                    order.len() - 1
                }
            };
            out.push(ModeSpan {
                t0: t as f64 * 1e-6,
                t1: records
                    .get(i + 1)
                    .and_then(|n| n.time_us())
                    .map(|t| t as f64 * 1e-6),
                name,
                number,
                slot: (slot % 8) as u8,
            });
        }
        out
    }

    /// Parameters from `PARM`, sorted by name, with the first and last value of each and the
    /// documentation of the log's vehicle.
    pub fn params(&self, lang: Lang) -> Vec<ParamRow> {
        let _ = lang; // parameter texts exist in English only
        let mut seen: HashMap<String, (f64, f64)> = HashMap::new();
        let mut order = Vec::new();
        for r in self.log.records_named("PARM") {
            let (Some(name), Some(value)) = (r.text("Name"), r.f64("Value")) else {
                continue;
            };
            match seen.get_mut(name) {
                Some(p) => p.1 = value,
                None => {
                    order.push(name.to_string());
                    seen.insert(name.to_string(), (value, value));
                }
            }
        }
        order.sort();
        order
            .into_iter()
            .map(|name| {
                let (first, last) = seen[&name];
                let doc = docs::param_doc(self.vehicle, &name);
                let changed = first != last;
                ParamRow {
                    title: doc.map(|d| d.n.clone()).unwrap_or_default(),
                    doc: doc.map(|d| d.d.clone()).unwrap_or_default(),
                    units: doc.and_then(|d| d.u.clone()),
                    range: doc.and_then(|d| d.r.clone()),
                    meaning: docs::param_value_meaning(doc, last),
                    first_meaning: if changed {
                        docs::param_value_meaning(doc, first)
                    } else {
                        String::new()
                    },
                    name,
                    value: last,
                    first,
                    changed,
                }
            })
            .collect()
    }

    /// `MSG`, `ERR` and `EV` messages in time order, `ERR` and `EV` decoded to text.
    pub fn messages(&self, lang: Lang) -> Vec<LogMessage> {
        let mut out = Vec::new();
        let mut add = |kind: &str, text: &dyn Fn(&control_station_dataflash::Record) -> String| {
            for r in self.log.records_named(kind) {
                if let Some(t) = r.time_us() {
                    out.push(LogMessage {
                        t: t as f64 * 1e-6,
                        kind: kind.to_string(),
                        text: text(&r),
                    });
                }
            }
        };
        add("MSG", &|r| {
            r.text("Message").unwrap_or_default().to_string()
        });
        add("ERR", &|r| {
            codes::err_text(
                lang,
                r.i64("Subsys").unwrap_or(0),
                r.i64("ECode").unwrap_or(0),
                |n| self.mode_name(n),
            )
        });
        add("EV", &|r| codes::ev_text(lang, r.i64("Id").unwrap_or(0)));
        out.sort_by(|a, b| a.t.total_cmp(&b.t));
        out
    }

    /// Ready-made panels, keeping only the fields this log has; a preset with none is left out.
    pub fn presets(&self) -> Vec<Preset> {
        const PRESETS: &[(&str, &[&str])] = &[
            ("attitude", &["ATT.Roll", "ATT.Pitch", "ATT.Yaw"]),
            ("altitude", &["CTUN.Alt", "CTUN.DAlt", "BARO.Alt"]),
            ("battery", &["BAT.Volt", "BAT.Curr"]),
            ("vibration", &["VIBE.VibeX", "VIBE.VibeY", "VIBE.VibeZ"]),
            ("motors", &["RCOU.C1", "RCOU.C2", "RCOU.C3", "RCOU.C4"]),
            ("gps", &["GPS.NSats", "GPS.HDop"]),
            ("rc", &["RCIN.C1", "RCIN.C2", "RCIN.C3", "RCIN.C4"]),
        ];
        PRESETS
            .iter()
            .map(|(id, keys)| Preset {
                id: id.to_string(),
                fields: keys.iter().filter_map(|k| self.resolve(k)).collect(),
            })
            .filter(|p| !p.fields.is_empty())
            .collect()
    }

    /// `MESSAGE.Field`; an instanced message (`BAT[0]`) is found by its base name.
    fn resolve(&self, key: &str) -> Option<PresetField> {
        let (name, field) = key.split_once('.')?;
        let entry = self
            .by_name
            .get(name)
            .map(|i| &self.types[*i])
            .or_else(|| self.types.iter().find(|t| t.base == name))?;
        (entry.time.is_some() && entry.fields.iter().any(|f| f.name == field)).then(|| {
            PresetField {
                type_name: entry.name.clone(),
                field: field.to_string(),
            }
        })
    }
}

fn clean_unit(unit: Option<&String>) -> Option<String> {
    unit.filter(|u| !matches!(u.as_str(), "" | "?" | "UNKNOWN" | "instance" | "-"))
        .cloned()
}

fn build_types(log: &Log) -> Vec<TypeEntry> {
    let mut out = Vec::new();
    for (&id, format) in log.formats() {
        let count = log.count(id);
        if format.name == "FMT" || count == 0 {
            continue;
        }
        let kinds = format.format.as_bytes();
        let time = format
            .columns
            .iter()
            .position(|c| c == "TimeUS")
            .map(|c| (c, 1e-6))
            .or_else(|| {
                format
                    .columns
                    .iter()
                    .position(|c| c == "TimeMS")
                    .map(|c| (c, 1e-3))
            });
        let info = log.column_info(id);
        let instance_col = info.and_then(|i| i.instance_column);
        let fields: Vec<FieldEntry> = format
            .columns
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                Some(*i) != time.map(|t| t.0)
                    && Some(*i) != instance_col
                    && kinds.get(*i).is_some_and(|k| !NON_NUMERIC.contains(k))
            })
            .map(|(i, name)| {
                let mult = info.and_then(|c| c.multipliers.get(i).copied().flatten());
                let scale = mult.filter(|m| {
                    *m != 1.0 && *m != 0.0 && kinds.get(i).is_some_and(|k| RAW_INTEGER.contains(k))
                });
                FieldEntry {
                    name: name.clone(),
                    col: i,
                    unit: clean_unit(info.and_then(|c| c.units.get(i)).and_then(Option::as_ref)),
                    scale,
                }
            })
            .collect();
        if fields.is_empty() {
            continue;
        }
        let instances = instance_col.map(|col| {
            let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
            for (row, value) in log.column(id, col).iter().enumerate() {
                let key = match value {
                    Value::Int(v) => v.to_string(),
                    Value::Float(v) => v.to_string(),
                    Value::Text(v) => v.clone(),
                };
                match groups.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, rows)) => rows.push(row),
                    None => groups.push((key, vec![row])),
                }
            }
            groups
        });
        match instances {
            Some(groups) => {
                for (instance, rows) in groups {
                    out.push(TypeEntry {
                        name: format!("{}[{instance}]", format.name),
                        base: format.name.clone(),
                        id,
                        count: rows.len(),
                        instance: Some(instance),
                        rows: Some(rows),
                        time,
                        fields: fields
                            .iter()
                            .map(|f| FieldEntry {
                                name: f.name.clone(),
                                col: f.col,
                                unit: f.unit.clone(),
                                scale: f.scale,
                            })
                            .collect(),
                    });
                }
            }
            None => out.push(TypeEntry {
                name: format.name.clone(),
                base: format.name.clone(),
                id,
                instance: None,
                rows: None,
                count,
                time,
                fields,
            }),
        }
    }
    out.sort_by(|a, b| type_order(&a.name, &b.name));
    out
}

/// Firmware and board, from `VER` and the boot messages (port of `get_version_and_board`).
fn read_version(log: &Log) -> Version {
    let mut v = Version::default();
    if let Some(ver) = log.records_named("VER").next() {
        v.firmware = ver.text("FWS").map(str::to_string);
        v.hash = ver.i64("GH").map(|h| format!("{:08x}", h as u32));
        v.board_id = ver.i64("APJ").filter(|id| *id != 0);
        v.build_type = ver.i64("BU");
        if let (Some(bt), Some(fw)) = (v.build_type, v.firmware.as_mut()) {
            if let Some((name, _)) = BUILD_TYPES.iter().find(|(_, n)| *n == bt) {
                if !fw.starts_with(name) {
                    // OEM firmware with AP_CUSTOM_FIRMWARE_STRING: append the base firmware
                    let part = |c: &str| ver.i64(c).map_or("?".to_string(), |x| x.to_string());
                    fw.push_str(&format!(
                        " [{name} V{}.{}.{}]",
                        part("Maj"),
                        part("Min"),
                        part("Pat")
                    ));
                }
            }
        }
    }
    let messages: Vec<String> = log
        .records_named("MSG")
        .filter_map(|r| r.text("Message").map(str::to_string))
        .collect();
    // The firmware banner is followed by the OS, the board and "Param space used:"
    for i in 0..messages.len().saturating_sub(3) {
        let msg = &messages[i];
        if v.firmware.as_ref().is_some_and(|fw| fw != msg) {
            continue;
        }
        if !messages[i + 3].starts_with("Param space used:") {
            continue;
        }
        let Some((name, bt)) = BUILD_TYPES.iter().find(|(n, _)| msg.contains(n)) else {
            continue;
        };
        let (Some(open), Some(close)) = (msg.rfind('('), msg.rfind(')')) else {
            continue;
        };
        if close < open || !msg[msg.find(name).unwrap_or(0)..].contains('(') {
            continue;
        }
        v.firmware.get_or_insert_with(|| msg.clone());
        v.build_type.get_or_insert(*bt);
        v.hash
            .get_or_insert_with(|| msg[open + 1..close].to_string());
        v.os = Some(messages[i + 1].clone());
        v.board = Some(messages[i + 2].clone());
        break;
    }
    v
}

#[cfg(test)]
mod tests;
