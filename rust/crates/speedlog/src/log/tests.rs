//! Tests with a synthetic log built byte by byte.
use super::*;

const HEAD: [u8; 2] = [0xA3, 0x95];

enum F<'a> {
    Q(u64),
    B(u8),
    I(u32),
    Li(i32),
    Fl(f32),
    Z(&'a str),
    N(&'a str),
}

fn padded(text: &str, len: usize) -> Vec<u8> {
    let mut out = text.as_bytes().to_vec();
    out.resize(len, 0);
    out
}

fn size(kind: u8) -> usize {
    match kind {
        b'B' | b'M' | b'b' => 1,
        b'I' | b'i' | b'f' | b'n' => 4,
        b'Q' | b'd' => 8,
        b'N' => 16,
        b'Z' => 64,
        _ => panic!("kind {kind}"),
    }
}

fn fmt(id: u8, name: &str, format: &str, columns: &str) -> Vec<u8> {
    let len = 3 + format.bytes().map(size).sum::<usize>();
    let mut out = HEAD.to_vec();
    out.extend([128, id, len as u8]);
    out.extend(padded(name, 4));
    out.extend(padded(format, 16));
    out.extend(padded(columns, 64));
    out
}

fn rec(id: u8, fields: &[F]) -> Vec<u8> {
    let mut out = HEAD.to_vec();
    out.push(id);
    for f in fields {
        match f {
            F::Q(v) => out.extend(v.to_le_bytes()),
            F::B(v) => out.push(*v),
            F::I(v) => out.extend(v.to_le_bytes()),
            F::Li(v) => out.extend(v.to_le_bytes()),
            F::Fl(v) => out.extend(v.to_le_bytes()),
            F::Z(v) => out.extend(padded(v, 64)),
            F::N(v) => out.extend(padded(v, 16)),
        }
    }
    out
}

const VER: u8 = 50;
const MSG: u8 = 51;
const MODE: u8 = 52;
const PARM: u8 = 53;
const ERR: u8 = 54;
const EV: u8 = 55;
const ATT: u8 = 56;
const IMU: u8 = 57;
const UNTIMED: u8 = 58;
const ALT: u8 = 59;
const UNIT: u8 = 60;
const MULT: u8 = 61;
const FMTU: u8 = 62;

fn s(t: f64) -> F<'static> {
    F::Q((t * 1e6) as u64)
}

fn sample() -> Vec<u8> {
    let mut d = Vec::new();
    d.extend(fmt(128, "FMT", "BBnNZ", "Type,Length,Name,Format,Columns"));
    d.extend(fmt(VER, "VER", "QBIZI", "TimeUS,BU,GH,FWS,APJ"));
    d.extend(fmt(MSG, "MSG", "QZ", "TimeUS,Message"));
    d.extend(fmt(MODE, "MODE", "QMBB", "TimeUS,Mode,ModeNum,Rsn"));
    d.extend(fmt(PARM, "PARM", "QNf", "TimeUS,Name,Value"));
    d.extend(fmt(ERR, "ERR", "QBB", "TimeUS,Subsys,ECode"));
    d.extend(fmt(EV, "EV", "QB", "TimeUS,Id"));
    d.extend(fmt(ATT, "ATT", "Qff", "TimeUS,Roll,Pitch"));
    d.extend(fmt(IMU, "IMU", "QBf", "TimeUS,I,GyrX"));
    d.extend(fmt(UNTIMED, "UNTM", "Bf", "A,B"));
    d.extend(fmt(ALT, "ALTI", "Qi", "TimeUS,Alt"));
    d.extend(fmt(UNIT, "UNIT", "QbZ", "TimeUS,Id,Label"));
    d.extend(fmt(MULT, "MULT", "Qbd", "TimeUS,Id,Mult"));
    d.extend(fmt(FMTU, "FMTU", "QBNN", "TimeUS,FmtType,UnitIds,MultIds"));
    for (id, label) in [(b's', "s"), (b'd', "deg"), (b'm', "m"), (b'E', "rad/s")] {
        d.extend(rec(UNIT, &[F::Q(0), F::B(id), F::Z(label)]));
    }
    for (id, m) in [(b'F', 1e-6f64), (b'B', 1e-2), (b'0', 1.0)] {
        let mut r = rec(MULT, &[F::Q(0), F::B(id)]);
        r.extend(m.to_le_bytes());
        d.extend(r);
    }
    d.extend(rec(FMTU, &[F::Q(0), F::B(ATT), F::N("sdd"), F::N("F00")]));
    d.extend(rec(FMTU, &[F::Q(0), F::B(IMU), F::N("s#E"), F::N("F-0")]));
    d.extend(rec(FMTU, &[F::Q(0), F::B(ALT), F::N("sm"), F::N("FB")]));
    d.extend(rec(
        VER,
        &[
            s(0.1),
            F::B(2),
            F::I(0x2a3d_c4b7),
            F::Z("ArduCopter V4.5.7 (2a3dc4b7)"),
            F::I(140),
        ],
    ));
    for m in [
        "ArduCopter V4.5.7 (2a3dc4b7)",
        "ChibiOS: 6a85082c",
        "CubeOrange 00340036 3231510D",
        "Param space used: 1234/3200",
    ] {
        d.extend(rec(MSG, &[s(0.2), F::Z(m)]));
    }
    d.extend(rec(PARM, &[s(0.3), F::N("FS_THR_ENABLE"), F::Fl(1.0)]));
    d.extend(rec(PARM, &[s(0.3), F::N("ATC_RAT_RLL_P"), F::Fl(0.135)]));
    d.extend(rec(MODE, &[s(1.0), F::B(0), F::B(0), F::B(0)]));
    for i in 0..10u32 {
        let t = 1.0 + f64::from(i);
        d.extend(rec(ATT, &[s(t), F::Fl(i as f32), F::Fl(-(i as f32))]));
        d.extend(rec(IMU, &[s(t), F::B(0), F::Fl(0.5)]));
        d.extend(rec(IMU, &[s(t), F::B(1), F::Fl(-0.5)]));
        d.extend(rec(ALT, &[s(t), F::Li(1234 * i as i32)]));
    }
    d.extend(rec(UNTIMED, &[F::B(1), F::Fl(2.0)]));
    d.extend(rec(EV, &[s(2.0), F::B(10)]));
    d.extend(rec(MODE, &[s(5.0), F::B(6), F::B(6), F::B(0)]));
    d.extend(rec(ERR, &[s(6.0), F::B(6), F::B(1)]));
    d.extend(rec(MODE, &[s(7.0), F::B(0), F::B(0), F::B(0)]));
    d.extend(rec(PARM, &[s(8.0), F::N("FS_THR_ENABLE"), F::Fl(3.0)]));
    d
}

fn log() -> SpeedLog {
    SpeedLog::from_bytes("test.BIN", sample())
}

#[test]
fn info_reads_version_and_board() {
    let info = log().info();
    assert_eq!(info.file_name, "test.BIN");
    assert_eq!(
        info.firmware.as_deref(),
        Some("ArduCopter V4.5.7 (2a3dc4b7)")
    );
    assert_eq!(info.firmware_hash.as_deref(), Some("2a3dc4b7"));
    assert_eq!(info.os.as_deref(), Some("ChibiOS: 6a85082c"));
    assert_eq!(info.board.as_deref(), Some("CubeOrange 00340036 3231510D"));
    assert_eq!(info.board_id, Some(140));
    assert_eq!(info.vehicle, "Copter");
    // ATT/IMU/ALTI all span 1..10 s
    assert!((info.t_start - 0.1).abs() < 1e-9 || (info.t_start - 1.0).abs() < 1e-9);
    assert!((info.t_end - 10.0).abs() < 1e-9);
}

#[test]
fn tree_splits_instances_and_skips_text_only_types() {
    let tree = log().tree(Lang::En);
    let names: Vec<_> = tree.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"ATT"));
    assert!(names.contains(&"IMU[0]"));
    assert!(names.contains(&"IMU[1]"));
    assert!(!names.contains(&"IMU"));
    // MSG has only a text field: nothing to plot
    assert!(!names.contains(&"MSG"));
    let imu = tree.iter().find(|t| t.name == "IMU[1]").unwrap();
    assert_eq!(imu.base, "IMU");
    assert_eq!(imu.instance.as_deref(), Some("1"));
    assert_eq!(imu.count, 10);
    // The instance column is not a field
    assert_eq!(
        imu.fields
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["GyrX"]
    );
    assert_eq!(imu.fields[0].unit.as_deref(), Some("rad/s"));
    let att = tree.iter().find(|t| t.name == "ATT").unwrap();
    assert_eq!(att.doc, "Canonical vehicle attitude");
    assert_eq!(att.fields[0].unit.as_deref(), Some("deg"));
    assert_eq!(att.fields[0].doc, "achieved vehicle roll");
    let untimed = tree.iter().find(|t| t.name == "UNTM").unwrap();
    assert!(!untimed.timed);
    // Sorted
    let mut sorted = names.clone();
    sorted.sort_by(|a, b| type_order(a, b));
    assert_eq!(names, sorted);
}

#[test]
fn series_of_instances_and_multipliers() {
    let log = log();
    let s = log.series("IMU[1]", "GyrX").unwrap();
    assert_eq!(s.t.len(), 10);
    assert!(s.v.iter().all(|v| (*v + 0.5).abs() < 1e-6));
    assert!((s.t[3] - 4.0).abs() < 1e-9);
    // Raw integer centimetres with multiplier B (0.01) become metres
    let alt = log.series("ALTI", "Alt").unwrap();
    assert!((alt.v[2] - 24.68).abs() < 1e-9);
    // Cached
    assert!(Arc::ptr_eq(&alt, &log.series("ALTI", "Alt").unwrap()));
    assert!(log.series("UNTM", "A").is_none());
    assert!(log.series("ATT", "Nope").is_none());
    assert!(log.series("NOPE", "Roll").is_none());
    let v = log
        .series_view("ATT", "Roll", Some((2.5, 4.5)), BUCKETS)
        .unwrap();
    assert_eq!(v.t, [2.0, 3.0, 4.0, 5.0]);
    assert_eq!(v.t_first, Some(1.0));
}

#[test]
fn modes_spans_and_slots() {
    let modes = log().modes();
    assert_eq!(modes.len(), 3);
    assert_eq!(modes[0].name, "Stabilize");
    assert_eq!(modes[0].t1, Some(5.0));
    assert_eq!(modes[1].name, "RTL");
    assert_eq!(modes[1].slot, 1);
    // Same mode, same colour slot
    assert_eq!(modes[2].slot, 0);
    assert_eq!(modes[2].t1, None);
}

#[test]
fn params_mark_changes_and_meanings() {
    let params = log().params(Lang::En);
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "ATC_RAT_RLL_P");
    assert_eq!(params[0].units.as_deref(), Some("s/rad"));
    assert!(!params[0].changed);
    let fs = &params[1];
    assert_eq!(fs.name, "FS_THR_ENABLE");
    assert!(fs.changed);
    assert_eq!(fs.first, 1.0);
    assert_eq!(fs.value, 3.0);
    assert_eq!(fs.meaning, "Enabled always Land");
    assert_eq!(fs.first_meaning, "Enabled always RTL");
    assert_eq!(fs.title, "Throttle Failsafe Enable");
}

#[test]
fn messages_are_decoded_and_sorted() {
    let msgs = log().messages(Lang::En);
    let kinds: Vec<_> = msgs.iter().map(|m| m.kind.as_str()).collect();
    assert_eq!(kinds, ["MSG", "MSG", "MSG", "MSG", "EV", "ERR"]);
    assert_eq!(msgs[4].text, "Armed (10)");
    assert_eq!(
        msgs[5].text,
        "Battery failsafe (6): Failsafe triggered [code 1]"
    );
    let pt = log().messages(Lang::Pt);
    assert_eq!(pt[4].text, "Armado (10)");
}

#[test]
fn presets_keep_existing_fields() {
    let presets = log().presets();
    assert_eq!(presets.len(), 1);
    assert_eq!(presets[0].id, "attitude");
    assert_eq!(
        presets[0]
            .fields
            .iter()
            .map(|f| f.field.as_str())
            .collect::<Vec<_>>(),
        ["Roll", "Pitch"]
    );
}

#[test]
fn empty_log_is_harmless() {
    let log = SpeedLog::from_bytes("x", Vec::new());
    assert!(log.tree(Lang::En).is_empty());
    assert!(log.modes().is_empty());
    assert!(log.params(Lang::En).is_empty());
    assert!(log.messages(Lang::En).is_empty());
    assert!(log.presets().is_empty());
    assert_eq!(log.info().duration, 0.0);
    assert_eq!(log.info().vehicle, "Unknown");
}
