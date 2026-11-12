//! DataFlash (.BIN) log reader.
//!
//! A log is a stream of records `0xA3 0x95 <msgid> <payload>`. Record layouts are described by
//! FMT records (message id 128) that appear in the log itself, so the whole file is scanned once
//! up front to build the format table and an index of record offsets per message id; records are
//! then decoded lazily, and reading one message type touches only its own records.
//!
//! FMTU, UNIT and MULT records (when present) describe each column's unit, multiplier and which
//! column is the instance number (`#`); see [`Log::column_info`].
use std::{collections::HashMap, path::Path, sync::Arc};
use thiserror::Error;

const HEAD1: u8 = 0xA3;
const HEAD2: u8 = 0x95;
const FMT_ID: u8 = 128;
const FMT_LEN: usize = 89;
const FMT_PAYLOAD_LEN: usize = 1 + 1 + 4 + 16 + 64;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to read log: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Text(String),
}

impl Value {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(v) => Some(*v as f64),
            Value::Float(v) => Some(*v),
            Value::Text(_) => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(v) => Some(v),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageFormat {
    pub id: u8,
    pub name: String,
    /// Total record length in bytes, including the 3 header bytes.
    pub length: usize,
    pub format: String,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Record {
    pub name: Arc<str>,
    pub values: Vec<Value>,
    columns: Arc<[String]>,
}

impl Record {
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    pub fn get(&self, column: &str) -> Option<&Value> {
        let index = self.columns.iter().position(|c| c == column)?;
        self.values.get(index)
    }

    pub fn f64(&self, column: &str) -> Option<f64> {
        self.get(column)?.as_f64()
    }

    pub fn i64(&self, column: &str) -> Option<i64> {
        self.get(column)?.as_i64()
    }

    pub fn text(&self, column: &str) -> Option<&str> {
        self.get(column)?.as_str()
    }

    pub fn time_us(&self) -> Option<u64> {
        u64::try_from(self.i64("TimeUS")?).ok()
    }
}

struct Entry {
    format: MessageFormat,
    name: Arc<str>,
    columns: Arc<[String]>,
}

impl Entry {
    fn new(format: MessageFormat) -> Self {
        Self {
            name: Arc::from(format.name.as_str()),
            columns: Arc::from(format.columns.clone()),
            format,
        }
    }

    fn decode(&self, payload: &[u8]) -> Record {
        let mut values = Vec::with_capacity(self.format.format.len());
        let mut offset = 0;
        for kind in self.format.format.bytes() {
            let Some(size) = char_size(kind) else { break };
            let Some(bytes) = payload.get(offset..offset + size) else {
                break;
            };
            offset += size;
            values.push(decode_value(kind, bytes));
        }
        Record {
            name: Arc::clone(&self.name),
            values,
            columns: Arc::clone(&self.columns),
        }
    }
}

fn char_size(kind: u8) -> Option<usize> {
    Some(match kind {
        b'b' | b'B' | b'M' => 1,
        b'h' | b'H' | b'c' | b'C' => 2,
        b'i' | b'I' | b'f' | b'e' | b'E' | b'L' | b'n' => 4,
        b'd' | b'q' | b'Q' => 8,
        b'N' => 16,
        b'Z' | b'a' => 64,
        _ => return None,
    })
}

fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn le<const N: usize>(bytes: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes[..N]);
    out
}

fn decode_value(kind: u8, b: &[u8]) -> Value {
    match kind {
        b'b' => Value::Int(i64::from(b[0] as i8)),
        b'B' | b'M' => Value::Int(i64::from(b[0])),
        b'h' => Value::Int(i64::from(i16::from_le_bytes(le(b)))),
        b'H' => Value::Int(i64::from(u16::from_le_bytes(le(b)))),
        b'i' => Value::Int(i64::from(i32::from_le_bytes(le(b)))),
        b'I' => Value::Int(i64::from(u32::from_le_bytes(le(b)))),
        b'q' => Value::Int(i64::from_le_bytes(le(b))),
        b'Q' => Value::Int(u64::from_le_bytes(le(b)) as i64),
        b'f' => Value::Float(f64::from(f32::from_le_bytes(le(b)))),
        b'd' => Value::Float(f64::from_le_bytes(le(b))),
        b'c' => Value::Float(f64::from(i16::from_le_bytes(le(b))) * 0.01),
        b'C' => Value::Float(f64::from(u16::from_le_bytes(le(b))) * 0.01),
        b'e' => Value::Float(f64::from(i32::from_le_bytes(le(b))) * 0.01),
        b'E' => Value::Float(f64::from(u32::from_le_bytes(le(b))) * 0.01),
        b'L' => Value::Float(f64::from(i32::from_le_bytes(le(b))) / 1e7),
        b'n' | b'N' | b'Z' => Value::Text(cstr(b)),
        b'a' => Value::Text(
            (0..b.len() / 2)
                .map(|i| i16::from_le_bytes([b[2 * i], b[2 * i + 1]]).to_string())
                .collect::<Vec<_>>()
                .join(","),
        ),
        _ => unreachable!("char_size rejects unknown format characters"),
    }
}

fn parse_fmt(payload: &[u8]) -> Option<MessageFormat> {
    if payload.len() < FMT_PAYLOAD_LEN {
        return None;
    }
    let columns = cstr(&payload[22..86]);
    Some(MessageFormat {
        id: payload[0],
        length: usize::from(payload[1]),
        name: cstr(&payload[2..6]),
        format: cstr(&payload[6..22]),
        columns: if columns.is_empty() {
            Vec::new()
        } else {
            columns.split(',').map(|c| c.trim().to_string()).collect()
        },
    })
}

/// Finds the next record at or after `pos`. Skips garbage and records of unknown type, and
/// returns `None` at the end of the data or when the final record is truncated.
fn find_frame(
    data: &[u8],
    mut pos: usize,
    length_of: impl Fn(u8) -> Option<usize>,
) -> Option<(usize, u8, usize)> {
    loop {
        if !(data.get(pos) == Some(&HEAD1) && data.get(pos + 1) == Some(&HEAD2)) {
            pos += data
                .get(pos..)?
                .windows(2)
                .position(|w| w == [HEAD1, HEAD2])?;
        }
        let id = *data.get(pos + 2)?;
        match length_of(id) {
            Some(len) if len >= 3 => {
                return (pos + len <= data.len()).then_some((pos, id, len));
            }
            _ => pos += 1,
        }
    }
}

/// Unit metadata of one message type, from its FMTU record and the UNIT/MULT tables.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColumnInfo {
    /// Unit label per column (`None` when unknown or dimensionless).
    pub units: Vec<Option<String>>,
    /// Multiplier per column (`None` when unknown or not applicable, `-`/`?`).
    pub multipliers: Vec<Option<f64>>,
    /// Index of the column holding the instance number (`#` in FMTU), if any.
    pub instance_column: Option<usize>,
}

pub struct Log {
    data: Vec<u8>,
    entries: HashMap<u8, Entry>,
    formats: HashMap<u8, MessageFormat>,
    /// Start offset of every non-FMT record, per message id, in file order.
    offsets: HashMap<u8, Vec<usize>>,
    columns: HashMap<u8, ColumnInfo>,
}

impl Log {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        Ok(Self::from_bytes(std::fs::read(path)?))
    }

    pub fn from_bytes(data: Vec<u8>) -> Self {
        let mut entries = HashMap::new();
        entries.insert(
            FMT_ID,
            Entry::new(MessageFormat {
                id: FMT_ID,
                name: "FMT".into(),
                length: FMT_LEN,
                format: "BBnNZ".into(),
                columns: ["Type", "Length", "Name", "Format", "Columns"]
                    .map(String::from)
                    .to_vec(),
            }),
        );
        let mut offsets: HashMap<u8, Vec<usize>> = HashMap::new();
        let mut pos = 0;
        while let Some((start, id, len)) = find_frame(&data, pos, |id| {
            entries.get(&id).map(|e: &Entry| e.format.length)
        }) {
            if id == FMT_ID {
                if let Some(format) = parse_fmt(&data[start + 3..start + len]) {
                    entries.insert(format.id, Entry::new(format));
                }
            } else {
                offsets.entry(id).or_default().push(start);
            }
            pos = start + len;
        }
        let formats = entries
            .iter()
            .map(|(id, e)| (*id, e.format.clone()))
            .collect();
        let mut log = Self {
            data,
            entries,
            formats,
            offsets,
            columns: HashMap::new(),
        };
        log.columns = log.read_column_info();
        log
    }

    /// The raw size of the log in bytes.
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// The message id of a type name, if the log defines it.
    pub fn id_of(&self, name: &str) -> Option<u8> {
        self.entries
            .iter()
            .find(|(id, e)| **id != FMT_ID && &*e.name == name)
            .map(|(id, _)| *id)
    }

    /// Number of records of a message id.
    pub fn count(&self, id: u8) -> usize {
        self.offsets.get(&id).map_or(0, Vec::len)
    }

    /// Unit metadata of a message id (`None` when the log has no FMTU for it).
    pub fn column_info(&self, id: u8) -> Option<&ColumnInfo> {
        self.columns.get(&id)
    }

    /// Records of one message id, in file order, read through the offset index.
    pub fn records_of(&self, id: u8) -> impl Iterator<Item = Record> + '_ {
        let entry = self.entries.get(&id);
        self.offsets
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(move |start| {
                let entry = entry?;
                let payload = self.data.get(start + 3..start + entry.format.length)?;
                Some(entry.decode(payload))
            })
    }

    /// One column of every record of a message id, in file order, decoding only that column.
    /// Empty when the id or column is unknown.
    pub fn column(&self, id: u8, col: usize) -> Vec<Value> {
        let Some(entry) = self.entries.get(&id) else {
            return Vec::new();
        };
        let kinds = entry.format.format.as_bytes();
        let Some(&kind) = kinds.get(col) else {
            return Vec::new();
        };
        let Some(offset) = kinds[..col]
            .iter()
            .map(|k| char_size(*k))
            .sum::<Option<usize>>()
        else {
            return Vec::new();
        };
        let Some(size) = char_size(kind) else {
            return Vec::new();
        };
        let (from, to) = (3 + offset, 3 + offset + size);
        if to > entry.format.length {
            return Vec::new();
        }
        self.offsets
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|start| self.data.get(start + from..start + to))
            .map(|bytes| decode_value(kind, bytes))
            .collect()
    }

    fn read_column_info(&self) -> HashMap<u8, ColumnInfo> {
        let mut units = HashMap::new();
        let mut mults = HashMap::new();
        if let Some(id) = self.id_of("UNIT") {
            for r in self.records_of(id) {
                if let (Some(k), Some(label)) = (r.i64("Id"), r.text("Label")) {
                    units.insert(k as u8, label.to_string());
                }
            }
        }
        if let Some(id) = self.id_of("MULT") {
            for r in self.records_of(id) {
                if let (Some(k), Some(m)) = (r.i64("Id"), r.f64("Mult")) {
                    mults.insert(k as u8, m);
                }
            }
        }
        let mut out = HashMap::new();
        let Some(fmtu) = self.id_of("FMTU") else {
            return out;
        };
        for r in self.records_of(fmtu) {
            let (Some(kind), Some(unit_ids), Some(mult_ids)) =
                (r.i64("FmtType"), r.text("UnitIds"), r.text("MultIds"))
            else {
                continue;
            };
            let info = ColumnInfo {
                units: unit_ids
                    .bytes()
                    .map(|u| match u {
                        b'-' | b'#' | b'?' => None,
                        _ => units.get(&u).filter(|l| !l.is_empty()).cloned(),
                    })
                    .collect(),
                multipliers: mult_ids
                    .bytes()
                    .map(|m| match m {
                        b'-' | b'?' => None,
                        _ => mults.get(&m).copied(),
                    })
                    .collect(),
                instance_column: unit_ids.bytes().position(|u| u == b'#'),
            };
            out.insert(kind as u8, info);
        }
        out
    }

    pub fn formats(&self) -> &HashMap<u8, MessageFormat> {
        &self.formats
    }

    /// Every record except FMT, in file order.
    pub fn records(&self) -> impl Iterator<Item = Record> + '_ {
        self.frames()
            .filter(|(entry, _)| entry.format.id != FMT_ID)
            .map(|(entry, payload)| entry.decode(payload))
    }

    /// Records of one message type, in file order. Other records are skipped without decoding.
    pub fn records_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = Record> + 'a {
        self.id_of(name)
            .into_iter()
            .flat_map(move |id| self.records_of(id))
    }

    fn frames(&self) -> impl Iterator<Item = (&Entry, &[u8])> + '_ {
        let mut pos = 0;
        std::iter::from_fn(move || {
            let (start, id, len) = find_frame(&self.data, pos, |id| {
                self.entries.get(&id).map(|e| e.format.length)
            })?;
            pos = start + len;
            Some((&self.entries[&id], &self.data[start + 3..start + len]))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn padded(text: &str, len: usize) -> Vec<u8> {
        let mut out = text.as_bytes().to_vec();
        out.resize(len, 0);
        out
    }

    fn fmt_record(id: u8, length: u8, name: &str, format: &str, columns: &str) -> Vec<u8> {
        let mut out = vec![HEAD1, HEAD2, FMT_ID, id, length];
        out.extend(padded(name, 4));
        out.extend(padded(format, 16));
        out.extend(padded(columns, 64));
        out
    }

    fn fmt_of_fmt() -> Vec<u8> {
        fmt_record(
            FMT_ID,
            89,
            "FMT",
            "BBnNZ",
            "Type,Length,Name,Format,Columns",
        )
    }

    const TST: u8 = 200;

    fn tst_fmt() -> Vec<u8> {
        fmt_record(TST, 24, "TST", "QBcCLn", "TimeUS,Cnt,A,B,Pos,Nm")
    }

    fn tst_record(time: u64, cnt: u8, a: i16, b: u16, pos: i32, name: &str) -> Vec<u8> {
        let mut out = vec![HEAD1, HEAD2, TST];
        out.extend(time.to_le_bytes());
        out.push(cnt);
        out.extend(a.to_le_bytes());
        out.extend(b.to_le_bytes());
        out.extend(pos.to_le_bytes());
        out.extend(padded(name, 4));
        out
    }

    fn basic_log() -> Vec<u8> {
        [
            fmt_of_fmt(),
            tst_fmt(),
            tst_record(5, 7, -1234, 36000, 123_456_789, "AB"),
            tst_record(6, 8, 100, 1, -450_000_000, "CD"),
        ]
        .concat()
    }

    #[test]
    fn parses_formats_and_scaled_values() {
        let log = Log::from_bytes(basic_log());
        let format = &log.formats()[&TST];
        assert_eq!(format.name, "TST");
        assert_eq!(format.length, 24);
        assert_eq!(format.columns, ["TimeUS", "Cnt", "A", "B", "Pos", "Nm"]);
        assert_eq!(log.formats().len(), 2);

        let records: Vec<_> = log.records().collect();
        assert_eq!(records.len(), 2);
        let first = &records[0];
        assert_eq!(&*first.name, "TST");
        assert_eq!(first.time_us(), Some(5));
        assert_eq!(first.i64("Cnt"), Some(7));
        assert!((first.f64("A").unwrap() - -12.34).abs() < 1e-9);
        assert!((first.f64("B").unwrap() - 360.0).abs() < 1e-9);
        assert!((first.f64("Pos").unwrap() - 12.345_678_9).abs() < 1e-9);
        assert_eq!(first.text("Nm"), Some("AB"));
        assert_eq!(first.get("Missing"), None);
        assert!((records[1].f64("Pos").unwrap() - -45.0).abs() < 1e-9);
    }

    #[test]
    fn records_named_filters_by_type() {
        let log = Log::from_bytes(basic_log());
        assert_eq!(log.records_named("TST").count(), 2);
        assert_eq!(log.records_named("NOPE").count(), 0);
    }

    #[test]
    fn resyncs_after_garbage() {
        let garbage = [0x00, HEAD1, 0x01, 0xFF, HEAD1, HEAD2, 0x7E, 0x42];
        let data = [
            fmt_of_fmt(),
            tst_fmt(),
            tst_record(1, 1, 0, 0, 0, "A"),
            garbage.to_vec(),
            tst_record(2, 2, 0, 0, 0, "B"),
        ]
        .concat();
        let log = Log::from_bytes(data);
        let times: Vec<_> = log.records().filter_map(|r| r.time_us()).collect();
        assert_eq!(times, [1, 2]);
    }

    #[test]
    fn truncated_tail_is_not_an_error() {
        let mut data = basic_log();
        data.truncate(data.len() - 5);
        let log = Log::from_bytes(data);
        assert_eq!(log.records().count(), 1);
    }

    #[test]
    fn unknown_message_ids_are_skipped() {
        let data = [
            fmt_of_fmt(),
            tst_fmt(),
            vec![HEAD1, HEAD2, 0x55, 1, 2, 3, 4],
            tst_record(9, 1, 0, 0, 0, "Z"),
        ]
        .concat();
        let log = Log::from_bytes(data);
        let times: Vec<_> = log.records().filter_map(|r| r.time_us()).collect();
        assert_eq!(times, [9]);
    }

    fn unit_fmts() -> Vec<u8> {
        [
            fmt_record(201, 3 + 8 + 1 + 64, "UNIT", "QbZ", "TimeUS,Id,Label"),
            fmt_record(202, 3 + 8 + 1 + 8, "MULT", "Qbd", "TimeUS,Id,Mult"),
            fmt_record(
                203,
                3 + 8 + 1 + 16 + 16,
                "FMTU",
                "QBNN",
                "TimeUS,FmtType,UnitIds,MultIds",
            ),
        ]
        .concat()
    }

    fn unit(id: u8, label: &str) -> Vec<u8> {
        let mut out = vec![HEAD1, HEAD2, 201];
        out.extend(0u64.to_le_bytes());
        out.push(id);
        out.extend(padded(label, 64));
        out
    }

    fn mult(id: u8, m: f64) -> Vec<u8> {
        let mut out = vec![HEAD1, HEAD2, 202];
        out.extend(0u64.to_le_bytes());
        out.push(id);
        out.extend(m.to_le_bytes());
        out
    }

    fn fmtu(kind: u8, units: &str, mults: &str) -> Vec<u8> {
        let mut out = vec![HEAD1, HEAD2, 203];
        out.extend(0u64.to_le_bytes());
        out.push(kind);
        out.extend(padded(units, 16));
        out.extend(padded(mults, 16));
        out
    }

    #[test]
    fn reads_units_multipliers_and_instance_column() {
        let data = [
            fmt_of_fmt(),
            tst_fmt(),
            unit_fmts(),
            unit(b's', "s"),
            unit(b'm', "m"),
            mult(b'F', 1e-6),
            mult(b'0', 1.0),
            fmtu(TST, "s#mm-?", "F-00--"),
            tst_record(1, 0, 0, 0, 0, "A"),
            tst_record(2, 1, 0, 0, 0, "B"),
        ]
        .concat();
        let log = Log::from_bytes(data);
        let id = log.id_of("TST").unwrap();
        assert_eq!(id, TST);
        assert_eq!(log.count(id), 2);
        let info = log.column_info(id).unwrap();
        assert_eq!(info.instance_column, Some(1));
        assert_eq!(info.units[0].as_deref(), Some("s"));
        assert_eq!(info.units[1], None);
        assert_eq!(info.units[2].as_deref(), Some("m"));
        assert_eq!(info.units[4], None);
        assert_eq!(info.multipliers[0], Some(1e-6));
        assert_eq!(info.multipliers[1], None);
        assert_eq!(info.multipliers[2], Some(1.0));
        let cnts: Vec<_> = log.records_of(id).filter_map(|r| r.i64("Cnt")).collect();
        assert_eq!(cnts, [0, 1]);
    }

    #[test]
    fn column_decodes_one_column() {
        let log = Log::from_bytes(basic_log());
        let pos: Vec<_> = log
            .column(TST, 4)
            .iter()
            .filter_map(Value::as_f64)
            .collect();
        assert_eq!(pos.len(), 2);
        assert!((pos[0] - 12.345_678_9).abs() < 1e-9);
        assert!((pos[1] - -45.0).abs() < 1e-9);
        assert_eq!(log.column(TST, 5)[1], Value::Text("CD".into()));
        assert!(log.column(TST, 6).is_empty());
        assert!(log.column(99, 0).is_empty());
    }

    #[test]
    fn missing_fmtu_means_no_column_info() {
        let log = Log::from_bytes(basic_log());
        assert!(log.column_info(TST).is_none());
        assert_eq!(log.count(TST), 2);
        assert_eq!(log.count(99), 0);
        assert_eq!(log.records_of(99).count(), 0);
    }

    #[test]
    fn empty_input_has_only_the_builtin_fmt() {
        let log = Log::from_bytes(Vec::new());
        assert_eq!(log.records().count(), 0);
        assert_eq!(log.formats().len(), 1);
    }

    #[test]
    fn sample_log() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../exemplos/Teste 02 - tunning.BIN"
        );
        if !Path::new(path).exists() {
            return;
        }
        let log = Log::open(path).unwrap();
        assert_eq!(log.formats().len(), 175);
        assert_eq!(log.records_named("PARM").count(), 1360);

        // The version banner is not the first MSG: GPS/gripper/RC notices are logged before it.
        assert!(log
            .records_named("MSG")
            .take(10)
            .any(|m| m.text("Message").is_some_and(|t| t.contains("ArduCopter"))));

        let mut att = 0;
        for record in log.records_named("ATT") {
            att += 1;
            let roll = record.f64("Roll").unwrap();
            let yaw = record.f64("Yaw").unwrap();
            assert!((-180.0..=180.0).contains(&roll), "roll {roll}");
            assert!((0.0..=360.0).contains(&yaw), "yaw {yaw}");
        }
        assert!(att > 0);

        let mut gps = 0;
        for record in log.records_named("GPS") {
            gps += 1;
            let lat = record.f64("Lat").unwrap();
            assert!((-90.0..=90.0).contains(&lat), "lat {lat}");
        }
        assert!(gps > 0);
    }
}
