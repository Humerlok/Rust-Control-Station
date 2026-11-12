//! The JSON contract between this crate and the UI (camelCase, pinned by the tests below and
//! mirrored in `desktop/ui/src/types.ts`).
use serde::{Deserialize, Serialize};

/// Language of the descriptions and decoded messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    En,
    Pt,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogInfo {
    pub file_name: String,
    pub size: u64,
    /// Firmware string, e.g. `ArduCopter V4.5.7 (2a3dc4b7)`.
    pub firmware: Option<String>,
    pub firmware_hash: Option<String>,
    pub os: Option<String>,
    /// Flight controller line of the boot messages, e.g. `CubeOrange 0034003...`.
    pub board: Option<String>,
    pub board_id: Option<i64>,
    pub vehicle: String,
    pub t_start: f64,
    pub t_end: f64,
    pub duration: f64,
    pub type_count: usize,
    pub record_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldNode {
    pub name: String,
    pub unit: Option<String>,
    pub doc: String,
}

/// One entry of the field tree: a message type, or one instance of it (`IMU[1]`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeNode {
    pub name: String,
    pub base: String,
    pub instance: Option<String>,
    pub count: usize,
    /// Types without `TimeUS`/`TimeMS` cannot be placed on the time axis.
    pub timed: bool,
    pub doc: String,
    pub fields: Vec<FieldNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Stats {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
}

/// A series reduced for a visible window.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesView {
    pub t: Vec<f64>,
    pub v: Vec<f64>,
    pub stats: Option<Stats>,
    /// Full time extent of the series (seconds), independent of the window.
    pub t_first: Option<f64>,
    pub t_last: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeSpan {
    pub t0: f64,
    /// `None` for the last mode, which lasts until the end of the log.
    pub t1: Option<f64>,
    pub name: String,
    pub number: i64,
    /// Colour slot (0..8): the same mode always gets the same slot within a log.
    pub slot: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamRow {
    pub name: String,
    /// Last value in the log.
    pub value: f64,
    /// First value in the log.
    pub first: f64,
    pub changed: bool,
    /// Display name from the documentation.
    pub title: String,
    pub doc: String,
    pub units: Option<String>,
    pub range: Option<String>,
    /// Option name or active bitmask bits of the last value.
    pub meaning: String,
    /// Same for the first value, when it changed.
    pub first_meaning: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogMessage {
    pub t: f64,
    /// `MSG`, `ERR` or `EV`.
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetField {
    #[serde(rename = "type")]
    pub type_name: String,
    pub field: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    /// `attitude`, `altitude`, `battery`, `vibration`, `motors`, `gps`, `rc`.
    pub id: String,
    pub fields: Vec<PresetField>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn contract_is_camel_case() {
        let info = LogInfo {
            file_name: "a.BIN".into(),
            size: 1,
            firmware: None,
            firmware_hash: None,
            os: None,
            board: None,
            board_id: Some(140),
            vehicle: "Copter".into(),
            t_start: 0.0,
            t_end: 2.0,
            duration: 2.0,
            type_count: 3,
            record_count: 4,
        };
        let v = serde_json::to_value(info).unwrap();
        assert_eq!(v["fileName"], "a.BIN");
        assert_eq!(v["firmwareHash"], json!(null));
        assert_eq!(v["boardId"], 140);
        assert_eq!(v["tStart"], 0.0);
        assert_eq!(v["typeCount"], 3);
        assert_eq!(v["recordCount"], 4);

        let p = PresetField {
            type_name: "ATT".into(),
            field: "Roll".into(),
        };
        assert_eq!(
            serde_json::to_value(p).unwrap(),
            json!({"type": "ATT", "field": "Roll"})
        );

        let row = ParamRow {
            name: "X".into(),
            value: 1.0,
            first: 0.0,
            changed: true,
            title: String::new(),
            doc: String::new(),
            units: None,
            range: None,
            meaning: String::new(),
            first_meaning: String::new(),
        };
        let v = serde_json::to_value(row).unwrap();
        assert!(v.get("firstMeaning").is_some());

        let s = SeriesView {
            t: vec![],
            v: vec![],
            stats: None,
            t_first: None,
            t_last: None,
        };
        let v = serde_json::to_value(s).unwrap();
        assert!(v.get("tFirst").is_some());

        assert_eq!(
            serde_json::from_value::<Lang>(json!("pt")).unwrap(),
            Lang::Pt
        );
    }
}
