//! What log messages, fields and parameters mean, from the ArduPilot documentation.
//!
//! The data is embedded (see `data/SOURCE.md`): one gzip JSON chunk with the message definitions,
//! one with the Portuguese translations and one per vehicle with its parameters. Each chunk is
//! unpacked the first time it is needed and kept for the life of the process.
use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
    sync::OnceLock,
};

use control_station_ardupilot_meta::VehicleType;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::types::Lang;

static COMMON_GZ: &[u8] = include_bytes!("../data/descriptions/common.json.gz");
static PT_GZ: &[u8] = include_bytes!("../data/descriptions/pt.json.gz");
static COPTER_GZ: &[u8] = include_bytes!("../data/descriptions/copter.json.gz");
static PLANE_GZ: &[u8] = include_bytes!("../data/descriptions/plane.json.gz");
static ROVER_GZ: &[u8] = include_bytes!("../data/descriptions/rover.json.gz");
static SUB_GZ: &[u8] = include_bytes!("../data/descriptions/sub.json.gz");

#[derive(Debug, Default, Deserialize)]
struct MsgDoc {
    #[serde(default)]
    d: String,
    #[serde(default)]
    f: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct Common {
    #[serde(default)]
    msgs: HashMap<String, MsgDoc>,
    #[serde(default)]
    enums: HashMap<String, HashMap<String, String>>,
}

/// Documentation of one parameter (field names as in the packed data).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParamDoc {
    /// Display name.
    #[serde(default)]
    pub n: String,
    /// Description.
    #[serde(default)]
    pub d: String,
    /// Units.
    #[serde(default)]
    pub u: Option<String>,
    /// Range, `low to high`.
    #[serde(default)]
    pub r: Option<String>,
    /// Value options: value -> name.
    #[serde(default)]
    pub v: Option<BTreeMap<String, String>>,
    /// Bitmask: bit number -> name.
    #[serde(default)]
    pub b: Option<BTreeMap<String, String>>,
}

/// Unpacks one chunk; a broken chunk gives empty docs instead of failing the viewer.
fn unpack<T: DeserializeOwned + Default>(gz: &[u8]) -> T {
    let mut text = String::new();
    if flate2::read::GzDecoder::new(gz)
        .read_to_string(&mut text)
        .is_err()
    {
        return T::default();
    }
    serde_json::from_str(&text).unwrap_or_default()
}

fn common() -> &'static Common {
    static C: OnceLock<Common> = OnceLock::new();
    C.get_or_init(|| unpack(COMMON_GZ))
}

fn pt() -> &'static HashMap<String, String> {
    static P: OnceLock<HashMap<String, String>> = OnceLock::new();
    P.get_or_init(|| unpack(PT_GZ))
}

fn params(vehicle: VehicleType) -> &'static HashMap<String, ParamDoc> {
    static COPTER: OnceLock<HashMap<String, ParamDoc>> = OnceLock::new();
    static PLANE: OnceLock<HashMap<String, ParamDoc>> = OnceLock::new();
    static ROVER: OnceLock<HashMap<String, ParamDoc>> = OnceLock::new();
    static SUB: OnceLock<HashMap<String, ParamDoc>> = OnceLock::new();
    match vehicle {
        VehicleType::Plane => PLANE.get_or_init(|| unpack(PLANE_GZ)),
        VehicleType::Rover => ROVER.get_or_init(|| unpack(ROVER_GZ)),
        VehicleType::Sub => SUB.get_or_init(|| unpack(SUB_GZ)),
        // Copter is the default, as in SpeedLOG
        _ => COPTER.get_or_init(|| unpack(COPTER_GZ)),
    }
}

/// The documentation sometimes has the literal text "null" where a description is missing.
fn localize(lang: Lang, text: &str) -> String {
    if text.is_empty() || text == "null" {
        return String::new();
    }
    if lang == Lang::Pt {
        if let Some(t) = pt().get(text) {
            return t.clone();
        }
    }
    text.to_string()
}

/// The viewer lists multi-instance messages as `BARO[0]`; the documentation has one entry, `BARO`.
fn doc_entry(type_name: &str) -> Option<&'static MsgDoc> {
    let msgs = &common().msgs;
    msgs.get(type_name).or_else(|| {
        let base = type_name.split_once('[').map_or(type_name, |(b, _)| b);
        msgs.get(base)
    })
}

/// Description of a message type (empty when undocumented).
pub fn message_doc(lang: Lang, type_name: &str) -> String {
    doc_entry(type_name).map_or_else(String::new, |m| localize(lang, &m.d))
}

/// Description of one field of a message type (empty when undocumented).
pub fn field_doc(lang: Lang, type_name: &str, field: &str) -> String {
    doc_entry(type_name)
        .and_then(|m| m.f.get(field))
        .map_or_else(String::new, |d| localize(lang, d))
}

/// Name of an enum value (`LogErrorSubsystem`, `LogEvent`).
pub fn enum_name(enum_type: &str, value: i64) -> Option<&'static str> {
    common()
        .enums
        .get(enum_type)?
        .get(&value.to_string())
        .map(String::as_str)
}

/// Documentation of a parameter for a vehicle. Instance parameters such as `BATT2_FS_LOW_ACT` are
/// documented once, as `BATT_FS_LOW_ACT`.
pub fn param_doc(vehicle: VehicleType, name: &str) -> Option<&'static ParamDoc> {
    let table = params(vehicle);
    if let Some(doc) = table.get(name) {
        return Some(doc);
    }
    let (prefix, number, rest) = split_instance(name)?;
    table
        .get(&format!("{prefix}{rest}"))
        .or_else(|| table.get(&format!("{prefix}{number}{rest}")))
}

/// Splits `BATT2_FS_LOW_ACT` into (`BATT`, `2`, `_FS_LOW_ACT`): the regex
/// `^([A-Z_]+?)([0-9]+)(_.*)$` of SpeedLOG.
fn split_instance(name: &str) -> Option<(&str, &str, &str)> {
    let digit = name.find(|c: char| c.is_ascii_digit())?;
    let prefix = &name[..digit];
    if prefix.is_empty() || !prefix.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
        return None;
    }
    let after = &name[digit..];
    let digits_end = after
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(after.len());
    let rest = &after[digits_end..];
    rest.starts_with('_')
        .then_some((prefix, &after[..digits_end], rest))
}

/// Key under which a value is listed in the documentation's value table (`1`, `0.5`).
fn value_key(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// What a parameter value means: the option name, or the names of the active bits of a bitmask.
pub fn param_value_meaning(doc: Option<&ParamDoc>, value: f64) -> String {
    let Some(doc) = doc else {
        return String::new();
    };
    if let Some(name) = doc.v.as_ref().and_then(|v| v.get(&value_key(value))) {
        return name.clone();
    }
    if let Some(bits) = &doc.b {
        if value.fract() == 0.0 && value > 0.0 && value < 2f64.powi(63) {
            let v = value as u64;
            let mut active: Vec<(u32, &String)> = bits
                .iter()
                .filter_map(|(bit, name)| bit.parse::<u32>().ok().map(|b| (b, name)))
                .filter(|(b, _)| *b < 64 && v & (1u64 << b) != 0)
                .collect();
            active.sort_by_key(|(b, _)| *b);
            return active
                .into_iter()
                .map(|(_, n)| n.as_str())
                .collect::<Vec<_>>()
                .join(", ");
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_and_field_docs() {
        assert_eq!(message_doc(Lang::En, "ATT"), "Canonical vehicle attitude");
        assert_eq!(field_doc(Lang::En, "ATT", "Roll"), "achieved vehicle roll");
        // Instances share the base entry
        assert_eq!(
            message_doc(Lang::En, "BARO[1]"),
            message_doc(Lang::En, "BARO")
        );
        assert!(!message_doc(Lang::En, "BARO").is_empty());
        assert_eq!(message_doc(Lang::En, "NOPE_XYZ"), "");
        assert_eq!(field_doc(Lang::En, "ATT", "NoSuchField"), "");
    }

    #[test]
    fn portuguese_translation() {
        let en = message_doc(Lang::En, "ATT");
        let pt = message_doc(Lang::Pt, "ATT");
        assert!(!pt.is_empty());
        assert_ne!(en, pt);
    }

    #[test]
    fn enums() {
        assert_eq!(enum_name("LogErrorSubsystem", 10), Some("FLIGHT_MODE"));
        assert_eq!(enum_name("LogEvent", 10), Some("ARMED"));
        assert_eq!(enum_name("LogEvent", 99_999), None);
    }

    #[test]
    fn param_docs_and_meanings() {
        let doc = param_doc(VehicleType::Copter, "FS_THR_ENABLE").unwrap();
        assert_eq!(doc.n, "Throttle Failsafe Enable");
        assert_eq!(param_value_meaning(Some(doc), 1.0), "Enabled always RTL");
        assert_eq!(param_value_meaning(Some(doc), 42.0), "");
        assert_eq!(
            param_doc(VehicleType::Copter, "ATC_RAT_RLL_P")
                .unwrap()
                .u
                .as_deref(),
            Some("s/rad")
        );
        for vehicle in [
            VehicleType::Plane,
            VehicleType::Rover,
            VehicleType::Sub,
            VehicleType::Unknown,
        ] {
            assert!(param_doc(vehicle, "LOG_BITMASK").is_some(), "{vehicle:?}");
        }
    }

    #[test]
    fn instance_param_fallback() {
        // BATT2_ params are documented as BATT_ (or listed directly)
        assert!(param_doc(VehicleType::Copter, "BATT2_FS_LOW_ACT").is_some());
        assert_eq!(
            split_instance("BATT2_FS_LOW_ACT"),
            Some(("BATT", "2", "_FS_LOW_ACT"))
        );
        assert_eq!(split_instance("ATC_RAT_RLL_P"), None);
        assert_eq!(split_instance("2ABC_X"), None);
    }

    #[test]
    fn bitmask_meaning() {
        let mut b = BTreeMap::new();
        b.insert("0".to_string(), "A".to_string());
        b.insert("2".to_string(), "C".to_string());
        b.insert("10".to_string(), "K".to_string());
        let doc = ParamDoc {
            b: Some(b),
            ..Default::default()
        };
        assert_eq!(param_value_meaning(Some(&doc), 5.0), "A, C");
        assert_eq!(param_value_meaning(Some(&doc), 1029.0), "A, C, K");
        assert_eq!(param_value_meaning(Some(&doc), 0.0), "");
        assert_eq!(param_value_meaning(Some(&doc), 1.5), "");
        assert_eq!(param_value_meaning(None, 1.0), "");
    }

    #[test]
    fn value_keys() {
        assert_eq!(value_key(1.0), "1");
        assert_eq!(value_key(-3.0), "-3");
        assert_eq!(value_key(0.5), "0.5");
    }
}
