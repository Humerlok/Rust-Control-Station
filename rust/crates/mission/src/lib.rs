//! Mission items and the QGC `.waypoints` text format (pure, no I/O).
use serde::{Deserialize, Serialize};

const HEADER: &str = "QGC WPL 110";

/// One mission/fence/rally item in friendly units: `x`/`y` are degrees for global frames and
/// `z` is metres. This is the JSON the desktop UI exchanges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionItem {
    pub seq: u16,
    /// MAV_FRAME number: 0 global (absolute), 3 relative to home, 10 terrain.
    pub frame: u8,
    /// MAV_CMD number.
    pub command: u16,
    pub param1: f64,
    pub param2: f64,
    pub param3: f64,
    pub param4: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub autocontinue: bool,
}

impl MissionItem {
    /// The blank home row the legacy tool inserts when a file does not start at index 0.
    pub fn blank_home() -> Self {
        Self {
            seq: 0,
            frame: 0,
            command: 0,
            param1: 0.0,
            param2: 0.0,
            param3: 0.0,
            param4: 0.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            autocontinue: true,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("not a QGC WPL 110 waypoint file (the first line must be \"QGC WPL 110\")")]
    BadHeader,
    #[error("line {line}: {reason}")]
    BadRow { line: usize, reason: String },
}

/// Parses a `.waypoints` file. Fields may be separated by tabs, spaces or commas; blank lines
/// and `#` comments are skipped. Items are renumbered by position; when the first row is not
/// index 0 a blank home row is inserted, as the legacy tool does.
pub fn parse_waypoints(text: &str) -> Result<Vec<MissionItem>, ParseError> {
    let mut lines = text.lines().enumerate();
    let header = lines
        .by_ref()
        .map(|(_, line)| line.trim_start_matches('\u{feff}').trim())
        .find(|line| !line.is_empty());
    if header != Some(HEADER) {
        return Err(ParseError::BadHeader);
    }

    let mut items = Vec::new();
    for (index, line) in lines {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = trimmed
            .split(['\t', ' ', ','])
            .filter(|f| !f.is_empty())
            .collect();
        let bad = |reason: String| ParseError::BadRow {
            line: line_number,
            reason,
        };
        if fields.len() < 11 {
            return Err(bad(format!(
                "expected at least 11 fields (seq current frame command p1-p4 lat lon alt), found {}",
                fields.len()
            )));
        }
        let int = |i: usize, what: &str| -> Result<i64, ParseError> {
            fields[i]
                .parse::<i64>()
                .map_err(|_| bad(format!("{what} '{}' is not an integer", fields[i])))
        };
        let float = |i: usize, what: &str| -> Result<f64, ParseError> {
            fields[i]
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| bad(format!("{what} '{}' is not a number", fields[i])))
        };
        let seq = int(0, "sequence")?;
        let frame =
            u8::try_from(int(2, "frame")?).map_err(|_| bad("frame is out of range".into()))?;
        let command =
            u16::try_from(int(3, "command")?).map_err(|_| bad("command is out of range".into()))?;
        let autocontinue = match fields.get(11) {
            Some(_) => int(11, "autocontinue")? != 0,
            None => true,
        };
        if items.is_empty() && seq != 0 {
            items.push(MissionItem::blank_home());
        }
        items.push(MissionItem {
            seq: u16::try_from(items.len()).map_err(|_| bad("too many items".into()))?,
            frame,
            command,
            param1: float(4, "param1")?,
            param2: float(5, "param2")?,
            param3: float(6, "param3")?,
            param4: float(7, "param4")?,
            x: float(8, "latitude")?,
            y: float(9, "longitude")?,
            z: float(10, "altitude")?,
            autocontinue,
        });
    }
    Ok(items)
}

/// Avoids printing `-0.00000000`.
fn clean(value: f64) -> f64 {
    if value == 0.0 {
        0.0
    } else {
        value
    }
}

/// Formats items like the legacy tool: row `n` is `n`, `current` (1 for row 0), `frame`,
/// `command`, `p1..p4` (8 decimals), lat/lon (8 decimals), alt (6 decimals), `autocontinue`.
pub fn format_waypoints(items: &[MissionItem]) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');
    for (row, item) in items.iter().enumerate() {
        out.push_str(&format!(
            "{row}\t{current}\t{frame}\t{command}\t{p1:.8}\t{p2:.8}\t{p3:.8}\t{p4:.8}\t{x:.8}\t{y:.8}\t{z:.6}\t{auto}\n",
            current = u8::from(row == 0),
            frame = item.frame,
            command = item.command,
            p1 = clean(item.param1),
            p2 = clean(item.param2),
            p3 = clean(item.param3),
            p4 = clean(item.param4),
            x = clean(item.x),
            y = clean(item.y),
            z = clean(item.z),
            auto = u8::from(item.autocontinue),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(seq: u16, command: u16, frame: u8, x: f64, y: f64, z: f64) -> MissionItem {
        MissionItem {
            seq,
            frame,
            command,
            param1: 0.0,
            param2: 0.0,
            param3: 0.0,
            param4: 0.0,
            x,
            y,
            z,
            autocontinue: true,
        }
    }

    /// A file as the legacy Mission Planner writes it (tabs, 8/6 decimals).
    const LEGACY: &str = "QGC WPL 110\n\
0\t1\t0\t16\t0.00000000\t0.00000000\t0.00000000\t0.00000000\t-35.36326210\t149.16523500\t584.000000\t1\n\
1\t0\t3\t22\t0.00000000\t0.00000000\t0.00000000\t0.00000000\t-35.36326210\t149.16523500\t10.000000\t1\n\
2\t0\t3\t16\t5.00000000\t0.00000000\t0.00000000\t0.00000000\t-35.36150000\t149.16750000\t30.000000\t1\n\
3\t0\t3\t20\t0.00000000\t0.00000000\t0.00000000\t0.00000000\t0.00000000\t0.00000000\t0.000000\t1\n";

    #[test]
    fn parses_a_legacy_file() {
        let items = parse_waypoints(LEGACY).unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].command, 16);
        assert_eq!(items[0].frame, 0);
        assert_eq!(items[0].z, 584.0);
        assert_eq!(items[1].command, 22);
        assert_eq!(items[1].frame, 3);
        assert_eq!(items[2].param1, 5.0);
        assert_eq!((items[2].x, items[2].y), (-35.3615, 149.1675));
        assert_eq!(items[3].command, 20);
        assert!(items.iter().all(|i| i.autocontinue));
        assert_eq!(
            items.iter().map(|i| i.seq).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn legacy_file_round_trips_byte_for_byte() {
        assert_eq!(format_waypoints(&parse_waypoints(LEGACY).unwrap()), LEGACY);
    }

    #[test]
    fn items_round_trip_through_text() {
        let mut a = item(0, 16, 0, -23.5, 46.25, 700.0);
        a.param2 = -1.5;
        let mut b = item(1, 16, 3, -23.501234567, 46.251234567, 30.5);
        b.autocontinue = false;
        let items = vec![a, b, item(2, 20, 3, 0.0, 0.0, 0.0)];
        let parsed = parse_waypoints(&format_waypoints(&items)).unwrap();
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].param2, -1.5);
        assert!(!parsed[1].autocontinue);
        assert!((parsed[1].x + 23.50123457).abs() < 1e-8, "{}", parsed[1].x);
        assert_eq!(parsed[2].command, 20);
    }

    #[test]
    fn row_zero_is_current_and_negative_zero_is_clean() {
        let mut home = item(0, 16, 0, 0.0, 0.0, 0.0);
        home.param1 = -0.0;
        let text = format_waypoints(&[home, item(1, 22, 3, 1.0, 2.0, 3.0)]);
        let rows: Vec<&str> = text.lines().collect();
        assert!(
            rows[1].starts_with("0\t1\t0\t16\t0.00000000\t"),
            "{}",
            rows[1]
        );
        assert!(rows[2].starts_with("1\t0\t3\t22\t"), "{}", rows[2]);
        assert!(!text.contains('-'));
    }

    #[test]
    fn accepts_spaces_commas_comments_blanks_crlf_and_bom() {
        let text = "\u{feff}QGC WPL 110\r\n# a comment\r\n\r\n\
0 1 0 16 0 0 0 0 -35.0 149.0 584 1\r\n\
1,0,3,22,0,0,0,0,-35.0,149.0,10,1\r\n";
        let items = parse_waypoints(text).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].command, 22);
        assert_eq!(items[1].z, 10.0);
    }

    #[test]
    fn missing_autocontinue_defaults_to_true() {
        let items = parse_waypoints("QGC WPL 110\n0 1 0 16 0 0 0 0 1 2 3\n").unwrap();
        assert!(items[0].autocontinue);
    }

    #[test]
    fn a_file_not_starting_at_zero_gets_a_blank_home() {
        let items =
            parse_waypoints("QGC WPL 110\n1 0 3 22 0 0 0 0 1 2 3 1\n2 0 3 16 0 0 0 0 1 2 3 1\n")
                .unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], MissionItem::blank_home());
        assert_eq!(items[1].seq, 1);
        assert_eq!(items[1].command, 22);
        assert_eq!(items[2].seq, 2);
    }

    #[test]
    fn rejects_wrong_headers() {
        for text in [
            "",
            "hello\n",
            "QGC WPL 120\n0 1 0 16 0 0 0 0 1 2 3 1\n",
            "0 1 0 16 0 0 0 0 1 2 3 1\n",
        ] {
            assert_eq!(
                parse_waypoints(text),
                Err(ParseError::BadHeader),
                "{text:?}"
            );
        }
    }

    #[test]
    fn malformed_rows_report_their_line_number() {
        let text = "QGC WPL 110\n0 1 0 16 0 0 0 0 1 2 3 1\n1 0 3 22 0 0 0 0 oops 2 3 1\n";
        match parse_waypoints(text) {
            Err(ParseError::BadRow { line, reason }) => {
                assert_eq!(line, 3);
                assert!(reason.contains("latitude"), "{reason}");
            }
            other => panic!("expected a row error, got {other:?}"),
        }
        let short = "QGC WPL 110\n\n0 1 0 16 0 0\n";
        assert!(matches!(
            parse_waypoints(short),
            Err(ParseError::BadRow { line: 3, .. })
        ));
        let range = "QGC WPL 110\n0 1 300 16 0 0 0 0 1 2 3 1\n";
        assert!(matches!(
            parse_waypoints(range),
            Err(ParseError::BadRow { line: 2, .. })
        ));
    }

    #[test]
    fn an_empty_list_formats_to_just_the_header() {
        assert_eq!(format_waypoints(&[]), "QGC WPL 110\n");
        assert_eq!(parse_waypoints("QGC WPL 110\n").unwrap(), vec![]);
    }

    #[test]
    fn serializes_camel_case_for_the_ui() {
        let value = serde_json::to_value(item(2, 16, 3, 1.5, 2.5, 30.0)).unwrap();
        assert_eq!(value["param1"], 0.0);
        assert_eq!(value["autocontinue"], true);
        assert_eq!(value["seq"], 2);
        assert_eq!(value["command"], 16);
    }
}
