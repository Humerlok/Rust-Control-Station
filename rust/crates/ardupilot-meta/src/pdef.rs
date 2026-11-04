//! Parameter metadata from the ArduPilot autotest `apm.pdef.xml(.gz)` files: display names,
//! descriptions, units, ranges, value lists and bitmasks, plus a cached loader.
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::Path,
    time::{Duration, SystemTime},
};

use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use serde::Serialize;

use crate::VehicleType;

/// How long a cached definition file is trusted before it is fetched again.
pub const CACHE_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub enum UserLevel {
    #[default]
    Standard,
    Advanced,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValueOption {
    pub code: f64,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BitOption {
    pub bit: u8,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamMeta {
    pub display_name: String,
    pub description: String,
    pub units: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub increment: Option<f64>,
    pub values: Vec<ValueOption>,
    pub bitmask: Vec<BitOption>,
    pub reboot_required: bool,
    pub read_only: bool,
    pub user: UserLevel,
}

#[derive(Debug)]
pub enum MetaError {
    NoData(String),
}

impl std::fmt::Display for MetaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoData(reason) => write!(f, "no parameter metadata available: {reason}"),
        }
    }
}

impl std::error::Error for MetaError {}

fn attribute(element: &BytesStart<'_>, key: &str) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok())
        .map(|v| v.into_owned())
}

fn truthy(text: &str) -> bool {
    matches!(
        text.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes"
    )
}

fn apply_field(meta: &mut ParamMeta, name: &str, text: &str) {
    let text = text.trim();
    match name {
        "Range" => {
            let mut parts = text.split_whitespace().map(str::parse::<f64>);
            if let (Some(Ok(min)), Some(Ok(max))) = (parts.next(), parts.next()) {
                meta.min = Some(min);
                meta.max = Some(max);
            }
        }
        "Increment" => meta.increment = text.parse().ok(),
        "Units" => meta.units = text.to_owned(),
        "UnitText" if meta.units.is_empty() => meta.units = text.to_owned(),
        "RebootRequired" => meta.reboot_required = truthy(text),
        "ReadOnly" => meta.read_only = truthy(text),
        "Bitmask" => {
            meta.bitmask = text
                .split(',')
                .filter_map(|item| {
                    let (bit, label) = item.split_once(':')?;
                    Some(BitOption {
                        bit: bit.trim().parse().ok()?,
                        label: label.trim().to_owned(),
                    })
                })
                .collect();
        }
        _ => {}
    }
}

fn resolve_reference(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => None,
    }
}

/// Parses an `apm.pdef.xml` document into a map keyed by plain parameter name
/// (`ArduCopter:ACRO_BAL_PITCH` becomes `ACRO_BAL_PITCH`). Malformed trailing XML keeps what was
/// parsed so far.
pub fn parse_pdef(xml: &str) -> HashMap<String, ParamMeta> {
    let mut reader = Reader::from_str(xml);
    let mut out = HashMap::new();
    let mut param: Option<(String, ParamMeta)> = None;
    let mut field: Option<String> = None;
    let mut value_code: Option<f64> = None;
    let mut text = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                "param" => {
                    let name = attribute(&e, "name").unwrap_or_default();
                    let name = name.rsplit(':').next().unwrap_or_default().to_owned();
                    let meta = ParamMeta {
                        display_name: attribute(&e, "humanName").unwrap_or_default(),
                        description: attribute(&e, "documentation").unwrap_or_default(),
                        user: match attribute(&e, "user").as_deref() {
                            Some(level) if level.eq_ignore_ascii_case("advanced") => {
                                UserLevel::Advanced
                            }
                            _ => UserLevel::Standard,
                        },
                        ..ParamMeta::default()
                    };
                    param = Some((name, meta));
                }
                "field" if param.is_some() => {
                    field = attribute(&e, "name");
                    text.clear();
                }
                "value" if param.is_some() => {
                    value_code = attribute(&e, "code").and_then(|c| c.trim().parse().ok());
                    text.clear();
                }
                _ => {}
            },
            Ok(Event::Text(t)) if field.is_some() || value_code.is_some() => {
                text.push_str(&t.xml10_content());
            }
            Ok(Event::GeneralRef(r)) if field.is_some() || value_code.is_some() => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    text.push(c);
                } else if let Some(c) = resolve_reference(&r.xml10_content()) {
                    text.push(c);
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                "param" => {
                    if let Some((name, meta)) = param.take() {
                        if !name.is_empty() {
                            out.insert(name, meta);
                        }
                    }
                }
                "field" => {
                    if let (Some((_, meta)), Some(name)) = (param.as_mut(), field.take()) {
                        apply_field(meta, &name, &text);
                    }
                }
                "value" => {
                    if let (Some((_, meta)), Some(code)) = (param.as_mut(), value_code.take()) {
                        meta.values.push(ValueOption {
                            code,
                            label: text.trim().to_owned(),
                        });
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
    out
}

/// The autotest download URL for a vehicle family, or `None` when there is no definition file.
pub fn pdef_url(vehicle: VehicleType) -> Option<String> {
    let folder = match vehicle {
        VehicleType::Copter => "ArduCopter",
        VehicleType::Plane => "ArduPlane",
        VehicleType::Rover => "Rover",
        VehicleType::Sub => "ArduSub",
        VehicleType::Tracker => "AntennaTracker",
        VehicleType::Blimp => "Blimp",
        VehicleType::Unknown => return None,
    };
    Some(format!(
        "https://autotest.ardupilot.org/Parameters/{folder}/apm.pdef.xml.gz"
    ))
}

fn decode(bytes: &[u8]) -> Result<HashMap<String, ParamMeta>, String> {
    let mut xml = String::new();
    if bytes.starts_with(&[0x1f, 0x8b]) {
        flate2::read::GzDecoder::new(bytes)
            .read_to_string(&mut xml)
            .map_err(|e| format!("cannot gunzip definitions: {e}"))?;
    } else {
        xml = String::from_utf8_lossy(bytes).into_owned();
    }
    let parsed = parse_pdef(&xml);
    if parsed.is_empty() {
        return Err("definitions contained no parameters".into());
    }
    Ok(parsed)
}

fn cache_file(vehicle: VehicleType, cache_dir: &Path) -> Option<std::path::PathBuf> {
    pdef_url(vehicle).map(|_| cache_dir.join(format!("{}.pdef.xml.gz", vehicle.as_str())))
}

/// Loads metadata for a vehicle, using `cache_dir` as a cache that stays valid for `max_age`.
/// A stale cache is still used when `fetch` fails; with neither, this is an error.
/// `fetch` returns the raw (gzip) bytes of the URL it is given. Unknown vehicles give an empty map.
pub fn load_metadata<F>(
    vehicle: VehicleType,
    cache_dir: &Path,
    max_age: Duration,
    fetch: F,
) -> Result<HashMap<String, ParamMeta>, MetaError>
where
    F: Fn(&str) -> Result<Vec<u8>, String>,
{
    let (Some(url), Some(path)) = (pdef_url(vehicle), cache_file(vehicle, cache_dir)) else {
        return Ok(HashMap::new());
    };
    let cached = fs::read(&path).ok().map(|bytes| {
        let fresh = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < max_age);
        (bytes, fresh)
    });
    if let Some((bytes, true)) = &cached {
        if let Ok(parsed) = decode(bytes) {
            return Ok(parsed);
        }
    }
    let fetched = fetch(&url).and_then(|bytes| decode(&bytes).map(|parsed| (bytes, parsed)));
    match fetched {
        Ok((bytes, parsed)) => {
            if fs::create_dir_all(cache_dir).is_ok() {
                if let Err(error) = fs::write(&path, bytes) {
                    eprintln!("cannot cache parameter metadata: {error}");
                }
            }
            Ok(parsed)
        }
        Err(fetch_error) => match cached.map(|(bytes, _)| decode(&bytes)) {
            Some(Ok(stale)) => Ok(stale),
            _ => Err(MetaError::NoData(fetch_error)),
        },
    }
}

/// Downloads a file over HTTPS (blocking). Meant for `spawn_blocking`.
pub fn http_fetch(url: &str) -> Result<Vec<u8>, String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent.get(url).call().map_err(|e| format!("{url}: {e}"))?;
    response
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| format!("{url}: {e}"))
}

/// [`load_metadata`] with the 7-day cache and the real HTTPS fetcher.
pub fn load_metadata_online(
    vehicle: VehicleType,
    cache_dir: &Path,
) -> Result<HashMap<String, ParamMeta>, MetaError> {
    load_metadata(vehicle, cache_dir, CACHE_MAX_AGE, http_fetch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        sync::atomic::{AtomicUsize, Ordering},
    };

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<paramfile>
  <vehicles>
    <parameters name="ArduCopter">
      <param humanName="Roll-Pitch mode &amp; balance" name="ArduCopter:ACRO_BAL_PITCH" documentation="Rate at which pitch angle returns to level in acro mode.  A higher value causes the vehicle to return to level faster." user="Advanced">
        <field name="Range">0 3</field>
        <field name="Increment">0.1</field>
        <field name="Units">m/s</field>
        <field name="UnitText">meters per second</field>
      </param>
      <param humanName="Arming check" name="ArduCopter:ARMING_CHECK" documentation="Checks prior to arming motor." user="Standard">
        <field name="RebootRequired">True</field>
        <field name="Bitmask">0:All,1:Barometer,2:Compass</field>
        <values>
          <value code="0">Disabled</value>
          <value code="1">Enabled</value>
          <value code="-1">Skip &lt;all&gt;</value>
        </values>
      </param>
    </parameters>
  </vehicles>
  <libraries>
    <parameters name="AHRS_">
      <param humanName="Board orientation" name="AHRS_ORIENTATION" documentation="Board orientation" user="Advanced">
        <field name="ReadOnly">True</field>
        <field name="UnitText">degrees</field>
      </param>
    </parameters>
  </libraries>
</paramfile>"#;

    fn gz(text: &str) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(text.as_bytes()).unwrap();
        encoder.finish().unwrap()
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cs-meta-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn parses_vehicle_and_library_parameters() {
        let meta = parse_pdef(SAMPLE);
        assert_eq!(meta.len(), 3);
        let acro = &meta["ACRO_BAL_PITCH"];
        assert_eq!(acro.display_name, "Roll-Pitch mode & balance");
        assert!(acro.description.starts_with("Rate at which pitch"));
        assert_eq!(
            (acro.min, acro.max, acro.increment),
            (Some(0.0), Some(3.0), Some(0.1))
        );
        assert_eq!(acro.units, "m/s");
        assert_eq!(acro.user, UserLevel::Advanced);
        assert!(!acro.reboot_required && !acro.read_only);
    }

    #[test]
    fn parses_values_bitmask_flags_and_unit_text_fallback() {
        let meta = parse_pdef(SAMPLE);
        let arming = &meta["ARMING_CHECK"];
        assert!(arming.reboot_required);
        assert_eq!(arming.user, UserLevel::Standard);
        assert_eq!(arming.values.len(), 3);
        assert_eq!(arming.values[2].code, -1.0);
        assert_eq!(arming.values[2].label, "Skip <all>");
        assert_eq!(
            arming.bitmask,
            vec![
                BitOption {
                    bit: 0,
                    label: "All".into()
                },
                BitOption {
                    bit: 1,
                    label: "Barometer".into()
                },
                BitOption {
                    bit: 2,
                    label: "Compass".into()
                },
            ]
        );
        let orientation = &meta["AHRS_ORIENTATION"];
        assert!(orientation.read_only);
        assert_eq!(orientation.units, "degrees");
        assert_eq!((orientation.min, orientation.max), (None, None));
    }

    #[test]
    fn garbage_and_truncated_xml_do_not_panic() {
        assert!(parse_pdef("not xml at all").is_empty());
        let cut = &SAMPLE[..SAMPLE.find("ARMING_CHECK").unwrap()];
        assert_eq!(
            parse_pdef(cut).len(),
            1,
            "keeps the parameter completed before the cut"
        );
    }

    #[test]
    fn serializes_camel_case() {
        let value = serde_json::to_value(&parse_pdef(SAMPLE)["ARMING_CHECK"]).unwrap();
        assert_eq!(value["displayName"], "Arming check");
        assert_eq!(value["rebootRequired"], true);
        assert_eq!(value["user"], "Standard");
        assert_eq!(
            value["values"][1],
            serde_json::json!({"code": 1.0, "label": "Enabled"})
        );
        assert_eq!(
            value["bitmask"][2],
            serde_json::json!({"bit": 2, "label": "Compass"})
        );
    }

    #[test]
    fn urls_per_vehicle() {
        assert_eq!(
            pdef_url(VehicleType::Copter).unwrap(),
            "https://autotest.ardupilot.org/Parameters/ArduCopter/apm.pdef.xml.gz"
        );
        assert!(pdef_url(VehicleType::Rover).unwrap().contains("/Rover/"));
        assert!(pdef_url(VehicleType::Unknown).is_none());
    }

    #[test]
    fn fetches_then_serves_from_a_fresh_cache() {
        let dir = temp_dir("fresh");
        let calls = AtomicUsize::new(0);
        let fetch = |_: &str| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(gz(SAMPLE))
        };
        let first = load_metadata(VehicleType::Copter, &dir, CACHE_MAX_AGE, fetch).unwrap();
        assert_eq!(first.len(), 3);
        let second = load_metadata(VehicleType::Copter, &dir, CACHE_MAX_AGE, fetch).unwrap();
        assert_eq!(second.len(), 3);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn expired_cache_is_refetched() {
        let dir = temp_dir("expired");
        let calls = AtomicUsize::new(0);
        let fetch = |_: &str| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(gz(SAMPLE))
        };
        load_metadata(VehicleType::Plane, &dir, Duration::ZERO, fetch).unwrap();
        load_metadata(VehicleType::Plane, &dir, Duration::ZERO, fetch).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_cache_is_used_when_the_fetch_fails() {
        let dir = temp_dir("stale");
        load_metadata(VehicleType::Copter, &dir, CACHE_MAX_AGE, |_| Ok(gz(SAMPLE))).unwrap();
        let offline = |_: &str| Err::<Vec<u8>, _>("offline".to_owned());
        let meta = load_metadata(VehicleType::Copter, &dir, Duration::ZERO, offline).unwrap();
        assert_eq!(meta.len(), 3);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_cache_and_no_network_is_an_error() {
        let dir = temp_dir("nodata");
        let offline = |_: &str| Err::<Vec<u8>, _>("offline".to_owned());
        let error = load_metadata(VehicleType::Copter, &dir, CACHE_MAX_AGE, offline).unwrap_err();
        assert!(error.to_string().contains("offline"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_download_is_not_cached_and_falls_back() {
        let dir = temp_dir("corrupt");
        load_metadata(VehicleType::Copter, &dir, CACHE_MAX_AGE, |_| Ok(gz(SAMPLE))).unwrap();
        let bad = |_: &str| Ok(b"<html>captive portal</html>".to_vec());
        let meta = load_metadata(VehicleType::Copter, &dir, Duration::ZERO, bad).unwrap();
        assert_eq!(meta.len(), 3, "kept the good stale cache");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_vehicle_gives_an_empty_map_without_fetching() {
        let dir = temp_dir("unknown");
        let never = |_: &str| -> Result<Vec<u8>, String> { panic!("must not fetch") };
        let meta = load_metadata(VehicleType::Unknown, &dir, CACHE_MAX_AGE, never).unwrap();
        assert!(meta.is_empty());
    }

    #[test]
    fn plain_uncompressed_xml_is_accepted() {
        let dir = temp_dir("plain");
        let meta = load_metadata(VehicleType::Copter, &dir, CACHE_MAX_AGE, |_| {
            Ok(SAMPLE.as_bytes().to_vec())
        })
        .unwrap();
        assert_eq!(meta.len(), 3);
        let _ = fs::remove_dir_all(&dir);
    }
}
