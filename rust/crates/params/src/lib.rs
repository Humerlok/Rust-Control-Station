//! Parameter table, natural ordering and `.param` file handling. Pure logic: no I/O, no async.
use std::{cmp::Ordering, collections::BTreeMap, collections::HashMap};

use serde::Serialize;

mod pck;
pub use pck::{unpack_param_pck, PckError};

/// `MAV_PARAM_TYPE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ParamType {
    Int8,
    Uint8,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Int64,
    Uint64,
    Real32,
    Real64,
}

impl ParamType {
    pub fn from_mav(value: u8) -> Self {
        match value {
            1 => Self::Uint8,
            2 => Self::Int8,
            3 => Self::Uint16,
            4 => Self::Int16,
            5 => Self::Uint32,
            6 => Self::Int32,
            7 => Self::Uint64,
            8 => Self::Int64,
            10 => Self::Real64,
            _ => Self::Real32,
        }
    }

    pub fn to_mav(self) -> u8 {
        match self {
            Self::Uint8 => 1,
            Self::Int8 => 2,
            Self::Uint16 => 3,
            Self::Int16 => 4,
            Self::Uint32 => 5,
            Self::Int32 => 6,
            Self::Uint64 => 7,
            Self::Int64 => 8,
            Self::Real32 => 9,
            Self::Real64 => 10,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Int8 => "INT8",
            Self::Uint8 => "UINT8",
            Self::Int16 => "INT16",
            Self::Uint16 => "UINT16",
            Self::Int32 => "INT32",
            Self::Uint32 => "UINT32",
            Self::Int64 => "INT64",
            Self::Uint64 => "UINT64",
            Self::Real32 => "REAL32",
            Self::Real64 => "REAL64",
        }
    }

    pub fn is_integer(self) -> bool {
        !matches!(self, Self::Real32 | Self::Real64)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Param {
    pub name: String,
    /// The numeric value as ArduPilot sends it (a C-cast to `f32`).
    pub value: f64,
    pub param_type: ParamType,
    pub index: u16,
}

/// Compares names so that `RC2_MIN` < `RC10_MIN` and `Q_PLT_Y_RATE` < `Q_PLT_Y_RATE_TC`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (digits_a, rest_a) = split_digits(a);
                let (digits_b, rest_b) = split_digits(b);
                let (trim_a, trim_b) = (trim_zeros(digits_a), trim_zeros(digits_b));
                let order = trim_a
                    .len()
                    .cmp(&trim_b.len())
                    .then_with(|| trim_a.cmp(trim_b))
                    .then_with(|| digits_a.len().cmp(&digits_b.len()));
                if order != Ordering::Equal {
                    return order;
                }
                a = rest_a;
                b = rest_b;
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(y);
                }
                a = &a[1..];
                b = &b[1..];
            }
        }
    }
}

fn split_digits(bytes: &[u8]) -> (&[u8], &[u8]) {
    let end = bytes
        .iter()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(bytes.len());
    bytes.split_at(end)
}

fn trim_zeros(digits: &[u8]) -> &[u8] {
    let start = digits
        .iter()
        .position(|b| *b != b'0')
        .unwrap_or(digits.len());
    &digits[start..]
}

/// The parameters a vehicle has reported so far.
#[derive(Debug, Default, Clone)]
pub struct ParamTable {
    expected: u16,
    by_index: BTreeMap<u16, Param>,
    by_name: HashMap<String, u16>,
}

impl ParamTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn set_expected(&mut self, count: u16) {
        self.expected = count;
    }

    pub fn expected_count(&self) -> u16 {
        self.expected
    }

    pub fn received_count(&self) -> usize {
        self.by_index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_index.is_empty()
    }

    pub fn is_complete(&self) -> bool {
        self.expected > 0 && self.by_index.len() >= usize::from(self.expected)
    }

    /// Returns false (and keeps the stored value) when this index was already received.
    pub fn insert(&mut self, param: Param) -> bool {
        if self.by_index.contains_key(&param.index) {
            return false;
        }
        self.by_name.insert(param.name.clone(), param.index);
        self.by_index.insert(param.index, param);
        true
    }

    /// Indexes below the expected count that have not arrived yet.
    pub fn missing_indices(&self) -> Vec<u16> {
        (0..self.expected)
            .filter(|i| !self.by_index.contains_key(i))
            .collect()
    }

    pub fn get(&self, name: &str) -> Option<&Param> {
        self.by_name.get(name).and_then(|i| self.by_index.get(i))
    }

    /// Updates the value of a known parameter (e.g. from a PARAM_SET echo).
    pub fn set_value(&mut self, name: &str, value: f64) -> bool {
        match self
            .by_name
            .get(name)
            .and_then(|i| self.by_index.get_mut(i))
        {
            Some(param) => {
                param.value = value;
                true
            }
            None => false,
        }
    }

    /// All parameters, naturally sorted by name.
    pub fn sorted(&self) -> Vec<Param> {
        let mut params: Vec<Param> = self.by_index.values().cloned().collect();
        params.sort_by(|a, b| natural_cmp(&a.name, &b.name));
        params
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FileParam {
    pub name: String,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedFile {
    pub params: Vec<FileParam>,
    /// Names present in the file that are never loaded (counters, ground calibration...).
    pub skipped: Vec<String>,
}

const SKIPPED_ON_LOAD: &[&str] = &[
    "SYSID_SW_MREV",
    "WP_TOTAL",
    "CMD_TOTAL",
    "FENCE_TOTAL",
    "SYS_NUM_RESETS",
    "ARSPD_OFFSET",
    "GND_ABS_PRESS",
    "GND_TEMP",
    "BARO1_GND_PRESS",
    "BARO2_GND_PRESS",
    "BARO3_GND_PRESS",
    "BARO_GND_TEMP",
    "CMD_INDEX",
    "LOG_LASTFILE",
    "FORMAT_VERSION",
];

/// Parses `.param` text: `NAME,value`, `NAME value` or `NAME<TAB>value`, `#` comments, and the
/// QGroundControl layout (`sysid compid NAME value type`). Later duplicates replace earlier ones.
pub fn parse_param_file(text: &str) -> ParsedFile {
    let mut params: Vec<FileParam> = Vec::new();
    let mut skipped = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line
            .split([' ', ',', '\t'])
            .filter(|f| !f.is_empty())
            .collect();
        let (name, value) = match fields.as_slice() {
            [sys, comp, name, value, ..]
                if sys.parse::<u32>().is_ok()
                    && comp.parse::<u32>().is_ok()
                    && name.parse::<f64>().is_err() =>
            {
                (*name, *value)
            }
            [name, value, ..] => (*name, *value),
            _ => continue,
        };
        let Ok(value) = value.parse::<f64>() else {
            continue;
        };
        if !value.is_finite() {
            continue;
        }
        let name = name.to_ascii_uppercase();
        if SKIPPED_ON_LOAD.contains(&name.as_str()) {
            if !skipped.contains(&name) {
                skipped.push(name);
            }
            continue;
        }
        match params.iter_mut().find(|p| p.name == name) {
            Some(existing) => existing.value = value,
            None => params.push(FileParam { name, value }),
        }
    }
    ParsedFile { params, skipped }
}

/// Formats one value the way it should appear in a `.param` file.
pub fn format_value(value: f64, param_type: ParamType) -> String {
    match param_type {
        t if t.is_integer() => format!("{}", value.round() as i64),
        ParamType::Real64 => format!("{value}"),
        _ => format!("{}", value as f32),
    }
}

/// Sorted `NAME,value` lines, `.` as the decimal separator.
pub fn format_param_file(params: &[Param]) -> String {
    let mut sorted: Vec<&Param> = params.iter().collect();
    sorted.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    let mut out = String::new();
    for p in sorted {
        out.push_str(&p.name);
        out.push(',');
        out.push_str(&format_value(p.value, p.param_type));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn param(name: &str, value: f64, param_type: ParamType, index: u16) -> Param {
        Param {
            name: name.into(),
            value,
            param_type,
            index,
        }
    }

    #[test]
    fn param_types_roundtrip_with_mavlink_numbers() {
        for n in 1..=10u8 {
            assert_eq!(ParamType::from_mav(n).to_mav(), n);
        }
        assert_eq!(ParamType::from_mav(2), ParamType::Int8);
        assert_eq!(ParamType::from_mav(6), ParamType::Int32);
        assert_eq!(ParamType::from_mav(9), ParamType::Real32);
        assert_eq!(ParamType::from_mav(0), ParamType::Real32);
        assert!(ParamType::Uint16.is_integer());
        assert!(!ParamType::Real32.is_integer());
        assert_eq!(ParamType::Uint64.as_str(), "UINT64");
    }

    #[test]
    fn param_serializes_camel_case_with_uppercase_type() {
        let value = serde_json::to_value(param("A", 1.0, ParamType::Int32, 7)).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"name": "A", "value": 1.0, "paramType": "INT32", "index": 7})
        );
    }

    #[test]
    fn natural_order_handles_numbers_and_prefixes() {
        let mut names = vec![
            "RC10_MIN",
            "RC2_MIN",
            "Q_PLT_Y_RATE_TC",
            "Q_PLT_Y_RATE",
            "ATC_RAT_RLL_P",
            "ACRO_BAL_PITCH",
            "RC1_MIN",
        ];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            [
                "ACRO_BAL_PITCH",
                "ATC_RAT_RLL_P",
                "Q_PLT_Y_RATE",
                "Q_PLT_Y_RATE_TC",
                "RC1_MIN",
                "RC2_MIN",
                "RC10_MIN"
            ]
        );
        assert_eq!(natural_cmp("A", "A"), Ordering::Equal);
        assert_eq!(natural_cmp("SERVO01", "SERVO1"), Ordering::Greater);
        assert_eq!(natural_cmp("X9", "X10"), Ordering::Less);
    }

    #[test]
    fn table_ignores_duplicate_indexes_and_tracks_missing() {
        let mut t = ParamTable::new();
        t.set_expected(5);
        assert!(t.insert(param("A", 1.0, ParamType::Int8, 0)));
        assert!(t.insert(param("C", 3.0, ParamType::Real32, 2)));
        assert!(!t.insert(param("A", 99.0, ParamType::Int8, 0)));
        assert_eq!(t.get("A").unwrap().value, 1.0);
        assert_eq!(t.received_count(), 2);
        assert_eq!(t.missing_indices(), vec![1, 3, 4]);
        assert!(!t.is_complete());
        for (i, n) in [(1u16, "B"), (3, "D"), (4, "E")] {
            t.insert(param(n, 0.0, ParamType::Real32, i));
        }
        assert!(t.is_complete());
        assert!(t.missing_indices().is_empty());
    }

    #[test]
    fn table_set_value_sorted_and_clear() {
        let mut t = ParamTable::new();
        t.set_expected(3);
        t.insert(param("RC10_MIN", 1000.0, ParamType::Int16, 0));
        t.insert(param("RC2_MIN", 1100.0, ParamType::Int16, 1));
        t.insert(param("ARMING_CHECK", 1.0, ParamType::Int32, 2));
        assert!(t.set_value("RC2_MIN", 1234.0));
        assert!(!t.set_value("NOPE", 1.0));
        assert_eq!(t.get("RC2_MIN").unwrap().value, 1234.0);
        let names: Vec<_> = t.sorted().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["ARMING_CHECK", "RC2_MIN", "RC10_MIN"]);
        t.clear();
        assert!(t.is_empty());
        assert_eq!(t.expected_count(), 0);
        assert!(t.get("RC2_MIN").is_none());
    }

    #[test]
    fn parse_accepts_comma_space_and_tab_and_comments() {
        let text = "# frame params\n\nATC_RAT_RLL_P,0.135\nATC_RAT_RLL_I 0.135\nRC1_MIN\t1100\n  \nbad line here\nONLYNAME\nNAN_PARAM,nan\n";
        let parsed = parse_param_file(text);
        let got: Vec<_> = parsed
            .params
            .iter()
            .map(|p| (p.name.as_str(), p.value))
            .collect();
        assert_eq!(
            got,
            [
                ("ATC_RAT_RLL_P", 0.135),
                ("ATC_RAT_RLL_I", 0.135),
                ("RC1_MIN", 1100.0)
            ]
        );
        assert!(parsed.skipped.is_empty());
    }

    #[test]
    fn parse_skips_blacklisted_names_and_replaces_duplicates() {
        let text = "FORMAT_VERSION,13\nWP_TOTAL,4\nAHRS_TRIM_X,0.1\nahrs_trim_x,0.2\nBARO1_GND_PRESS,101325\nFORMAT_VERSION,14\n";
        let parsed = parse_param_file(text);
        assert_eq!(parsed.params.len(), 1);
        assert_eq!(parsed.params[0].name, "AHRS_TRIM_X");
        assert_eq!(parsed.params[0].value, 0.2);
        assert_eq!(
            parsed.skipped,
            ["FORMAT_VERSION", "WP_TOTAL", "BARO1_GND_PRESS"]
        );
    }

    #[test]
    fn parse_understands_qgroundcontrol_layout() {
        let text = "# Onboard parameters\n1\t1\tACRO_BAL_PITCH\t1\t9\n1 1 FORMAT_VERSION 120 9\n";
        let parsed = parse_param_file(text);
        assert_eq!(parsed.params.len(), 1);
        assert_eq!(parsed.params[0].name, "ACRO_BAL_PITCH");
        assert_eq!(parsed.params[0].value, 1.0);
        assert_eq!(parsed.skipped, ["FORMAT_VERSION"]);
    }

    #[test]
    fn parse_handles_scientific_notation_and_crlf() {
        let parsed = parse_param_file("A,1e-3\r\nB,-2.5\r\n");
        assert_eq!(parsed.params[0].value, 0.001);
        assert_eq!(parsed.params[1].value, -2.5);
    }

    #[test]
    fn format_prints_integers_plain_and_floats_shortest() {
        assert_eq!(format_value(4.0, ParamType::Int32), "4");
        assert_eq!(format_value(-3.0, ParamType::Int8), "-3");
        assert_eq!(
            format_value(f64::from(0.135f32), ParamType::Real32),
            "0.135"
        );
        assert_eq!(format_value(1.0, ParamType::Real32), "1");
        assert_eq!(format_value(0.1, ParamType::Real64), "0.1");
        assert_eq!(format_value(1_000_000.0, ParamType::Real32), "1000000");
    }

    #[test]
    fn format_is_sorted_and_roundtrips_through_parse() {
        let params = vec![
            param("RC10_MIN", 1000.0, ParamType::Int16, 0),
            param("ATC_RAT_RLL_P", f64::from(0.135f32), ParamType::Real32, 1),
            param("RC2_MIN", 1100.0, ParamType::Int16, 2),
            param("ARMING_CHECK", 1.0, ParamType::Int32, 3),
        ];
        let text = format_param_file(&params);
        assert_eq!(
            text,
            "ARMING_CHECK,1\nATC_RAT_RLL_P,0.135\nRC2_MIN,1100\nRC10_MIN,1000\n"
        );
        let parsed = parse_param_file(&text);
        assert_eq!(parsed.params.len(), 4);
        let rll = parsed
            .params
            .iter()
            .find(|p| p.name == "ATC_RAT_RLL_P")
            .unwrap();
        assert_eq!(rll.value as f32, 0.135f32);
    }
}
