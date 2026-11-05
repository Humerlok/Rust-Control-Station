//! The ArduPilot firmware manifest (`manifest.json.gz`): every released build per board.
use std::{cmp::Ordering, collections::BTreeMap, io::Read};

use flate2::read::GzDecoder;
use serde::{Deserialize, Deserializer, Serialize};

pub const MANIFEST_URL: &str = "https://firmware.ardupilot.org/manifest.json.gz";
const MAX_MANIFEST_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("the manifest is not valid: {0}")]
    Invalid(String),
}

/// One downloadable build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    #[serde(default, rename(deserialize = "board_id"))]
    pub board_id: Option<u32>,
    #[serde(default)]
    pub platform: String,
    #[serde(default, rename(deserialize = "vehicletype"))]
    pub vehicle_type: String,
    #[serde(default, rename(deserialize = "mav-type"))]
    pub mav_type: String,
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub url: String,
    #[serde(default, rename(deserialize = "mav-firmware-version"))]
    pub version: String,
    /// `OFFICIAL` (stable), `BETA` or `DEV`.
    #[serde(default, rename(deserialize = "mav-firmware-version-type"))]
    pub version_type: String,
    #[serde(default, deserialize_with = "flag")]
    pub latest: bool,
    #[serde(default, rename(deserialize = "git-sha"))]
    pub git_sha: String,
    #[serde(default, rename(deserialize = "brand_name"))]
    pub brand_name: String,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(
        default,
        rename(deserialize = "USBID"),
        deserialize_with = "string_or_list"
    )]
    pub usb_ids: Vec<String>,
    #[serde(default, rename(deserialize = "image_size"))]
    pub image_size: Option<u64>,
}

fn flag<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Flag {
        Bool(bool),
        Int(i64),
        Text(String),
    }
    Ok(match Flag::deserialize(d)? {
        Flag::Bool(b) => b,
        Flag::Int(n) => n != 0,
        Flag::Text(t) => matches!(t.as_str(), "1" | "true" | "True"),
    })
}

fn string_or_list<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum One {
        One(String),
        Many(Vec<String>),
    }
    Ok(match One::deserialize(d)? {
        One::One(s) => vec![s],
        One::Many(v) => v,
    })
}

/// Only HTTPS downloads from `ardupilot.org` hosts are trusted: a tampered manifest must not be
/// able to point the flasher at an arbitrary server.
pub fn is_trusted_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.is_empty()
        && !host.contains(['@', ':', '\\'])
        && (host == "ardupilot.org" || host.ends_with(".ardupilot.org"))
}

/// Parses a manifest. Only `.apj` builds with a board id and a trusted URL are kept; entries
/// that do not parse are skipped so one odd record cannot hide the rest.
pub fn parse_manifest(json: &[u8]) -> Result<Vec<ManifestEntry>, ManifestError> {
    #[derive(Deserialize)]
    struct Root {
        firmware: Vec<serde_json::Value>,
    }
    let root: Root =
        serde_json::from_slice(json).map_err(|e| ManifestError::Invalid(e.to_string()))?;
    Ok(root
        .firmware
        .into_iter()
        .filter_map(|value| serde_json::from_value::<ManifestEntry>(value).ok())
        .filter(|e| e.format == "apj" && e.board_id.is_some() && is_trusted_url(&e.url))
        .collect())
}

/// [`parse_manifest`] for the gzip-compressed file served by ardupilot.org.
pub fn parse_manifest_gz(bytes: &[u8]) -> Result<Vec<ManifestEntry>, ManifestError> {
    let mut json = Vec::new();
    GzDecoder::new(bytes)
        .take(MAX_MANIFEST_BYTES)
        .read_to_end(&mut json)
        .map_err(|e| ManifestError::Invalid(format!("not gzip data ({e})")))?;
    parse_manifest(&json)
}

#[derive(Debug, Clone, Default)]
pub struct Query {
    pub board_id: Option<u32>,
    /// `Copter`, `Plane`, `Rover`, `Sub`, `Heli`, ... (case-insensitive).
    pub vehicle_type: Option<String>,
    /// `OFFICIAL`, `BETA` or `DEV` (case-insensitive).
    pub version_type: Option<String>,
    /// Keep only builds the manifest marks as the latest of their kind.
    pub latest_only: bool,
}

/// Builds matching `query`, grouped by vehicle and newest version first.
pub fn select<'a>(entries: &'a [ManifestEntry], query: &Query) -> Vec<&'a ManifestEntry> {
    let mut found: Vec<&ManifestEntry> = entries
        .iter()
        .filter(|e| query.board_id.is_none() || e.board_id == query.board_id)
        .filter(|e| {
            query
                .vehicle_type
                .as_deref()
                .is_none_or(|v| e.vehicle_type.eq_ignore_ascii_case(v))
        })
        .filter(|e| {
            query
                .version_type
                .as_deref()
                .is_none_or(|v| e.version_type.eq_ignore_ascii_case(v))
        })
        .filter(|e| !query.latest_only || e.latest)
        .collect();
    found.sort_by(|a, b| {
        a.vehicle_type
            .cmp(&b.vehicle_type)
            .then_with(|| compare_versions(&b.version, &a.version))
            .then_with(|| a.version_type.cmp(&b.version_type))
            .then_with(|| a.platform.cmp(&b.platform))
    });
    found
}

/// Compares dotted versions numerically (`4.10.0` > `4.9.1`); non-numeric parts compare as text.
fn compare_versions(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| {
        v.split(['.', '-'])
            .map(|p| p.parse::<u64>().map_err(|_| p.to_owned()))
            .collect::<Vec<_>>()
    };
    let (pa, pb) = (parts(a), parts(b));
    for (x, y) in pa.iter().zip(&pb) {
        let ord = match (x, y) {
            (Ok(x), Ok(y)) => x.cmp(y),
            (Err(x), Err(y)) => x.cmp(y),
            (Ok(_), Err(_)) => Ordering::Greater,
            (Err(_), Ok(_)) => Ordering::Less,
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    pa.len().cmp(&pb.len())
}

/// A board that has at least one build, for choosing a board by hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardSummary {
    pub board_id: u32,
    pub platform: String,
    pub brand_name: String,
    pub manufacturer: String,
}

/// Distinct boards in the manifest, sorted by platform name.
pub fn boards(entries: &[ManifestEntry]) -> Vec<BoardSummary> {
    let mut by_key: BTreeMap<(String, u32), BoardSummary> = BTreeMap::new();
    for e in entries {
        let Some(board_id) = e.board_id else { continue };
        by_key
            .entry((e.platform.to_lowercase(), board_id))
            .or_insert_with(|| BoardSummary {
                board_id,
                platform: e.platform.clone(),
                brand_name: e.brand_name.clone(),
                manufacturer: e.manufacturer.clone(),
            });
    }
    by_key.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use serde_json::json;
    use std::io::Write;

    fn entry(
        board: u32,
        vehicle: &str,
        version: &str,
        kind: &str,
        latest: i32,
    ) -> serde_json::Value {
        json!({
            "board_id": board,
            "platform": format!("Board{board}"),
            "vehicletype": vehicle,
            "mav-type": "QUADROTOR",
            "format": "apj",
            "url": format!("https://firmware.ardupilot.org/{vehicle}/{version}/Board{board}/x.apj"),
            "mav-firmware-version": version,
            "mav-firmware-version-type": kind,
            "latest": latest,
            "git-sha": "deadbeef",
            "USBID": ["0x2DAE/0x1016"],
            "image_size": 1234,
        })
    }

    fn sample() -> Vec<ManifestEntry> {
        let manifest = json!({"format-version": "1.0.0", "firmware": [
            entry(140, "Copter", "4.5.7", "OFFICIAL", 1),
            entry(140, "Copter", "4.10.0", "BETA", 1),
            entry(140, "Copter", "4.9.1", "OFFICIAL", 0),
            entry(140, "Plane", "4.5.1", "OFFICIAL", 1),
            entry(9, "Copter", "4.5.7", "OFFICIAL", 1),
            {"board_id": 140, "format": "px4", "url": "https://firmware.ardupilot.org/a.px4"},
            {"board_id": 140, "format": "apj", "url": "https://evil.example.com/a.apj"},
            {"format": "apj", "url": "https://firmware.ardupilot.org/noboard.apj"},
            {"board_id": "not a number", "format": "apj"},
        ]});
        parse_manifest(manifest.to_string().as_bytes()).unwrap()
    }

    #[test]
    fn keeps_only_trusted_apj_builds_with_a_board() {
        let entries = sample();
        assert_eq!(entries.len(), 5);
        let first = &entries[0];
        assert_eq!(first.board_id, Some(140));
        assert_eq!(first.vehicle_type, "Copter");
        assert_eq!(first.version, "4.5.7");
        assert_eq!(first.version_type, "OFFICIAL");
        assert!(first.latest);
        assert_eq!(first.usb_ids, vec!["0x2DAE/0x1016"]);
        assert_eq!(first.image_size, Some(1234));
    }

    #[test]
    fn select_filters_and_sorts_newest_first() {
        let entries = sample();
        let q = Query {
            board_id: Some(140),
            vehicle_type: Some("copter".into()),
            ..Default::default()
        };
        let versions: Vec<_> = select(&entries, &q)
            .iter()
            .map(|e| e.version.as_str())
            .collect();
        assert_eq!(versions, ["4.10.0", "4.9.1", "4.5.7"]);

        let official = Query {
            board_id: Some(140),
            version_type: Some("official".into()),
            latest_only: true,
            ..Default::default()
        };
        let found = select(&entries, &official);
        assert_eq!(found.len(), 2); // Copter 4.5.7 and Plane 4.5.1
        assert!(found.iter().all(|e| e.board_id == Some(140) && e.latest));
        assert!(select(
            &entries,
            &Query {
                board_id: Some(1),
                ..Default::default()
            }
        )
        .is_empty());
    }

    #[test]
    fn trusted_urls_cannot_be_spoofed() {
        assert!(is_trusted_url(
            "https://firmware.ardupilot.org/Copter/x.apj"
        ));
        assert!(is_trusted_url("https://ardupilot.org/x.apj"));
        assert!(!is_trusted_url("http://firmware.ardupilot.org/x.apj"));
        assert!(!is_trusted_url(
            "https://firmware.ardupilot.org@evil.com/x.apj"
        ));
        assert!(!is_trusted_url(
            "https://firmware.ardupilot.org:8443@evil.com/x"
        ));
        assert!(!is_trusted_url("https://notardupilot.org/x.apj"));
        assert!(!is_trusted_url("https://ardupilot.org.evil.com/x.apj"));
        assert!(!is_trusted_url("https://evil.com/?firmware.ardupilot.org"));
        assert!(!is_trusted_url("file:///etc/passwd"));
        assert!(!is_trusted_url(""));
    }

    #[test]
    fn boards_are_listed_once_and_gz_is_decoded() {
        let entries = sample();
        let list = boards(&entries);
        assert_eq!(
            list.iter().map(|b| b.board_id).collect::<Vec<_>>(),
            [140, 9] // sorted by platform name
        );
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(
            json!({"firmware": [entry(5, "Rover", "4.4.0", "OFFICIAL", 1)]})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
        let parsed = parse_manifest_gz(&gz.finish().unwrap()).unwrap();
        assert_eq!(parsed.len(), 1);
        assert!(parse_manifest_gz(b"plain").is_err());
        assert!(parse_manifest(b"{}").is_err());
    }
}
