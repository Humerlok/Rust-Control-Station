//! The mission command catalog: which grid columns each command uses and how they are labelled.
//! Parsed once from the legacy `mavcmd.xml` (embedded), whose elements are named after the
//! MAV_CMD (without the `MAV_CMD_` and `NAV_` prefixes) and grouped by vehicle family.
use std::{collections::HashMap, sync::OnceLock};

use control_station_mavlink::dialect::MavCmd;
use num_traits::FromPrimitive;
use quick_xml::{events::Event, Reader};
use serde::Serialize;

use crate::VehicleType;

const MAVCMD_XML: &str = include_str!("../data/mavcmd.xml");

/// A mission command and the labels of the fields it uses (`None` = unused by the command).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandInfo {
    pub id: u16,
    pub name: String,
    pub description: String,
    pub p1: Option<String>,
    pub p2: Option<String>,
    pub p3: Option<String>,
    pub p4: Option<String>,
    pub x: Option<String>,
    pub y: Option<String>,
    pub z: Option<String>,
}

type Catalog = HashMap<String, Vec<CommandInfo>>;

fn ids_by_name() -> HashMap<String, u16> {
    let mut exact = HashMap::new();
    let mut without_nav = HashMap::new();
    for id in 0..=u32::from(u16::MAX) {
        let Some(command) = MavCmd::from_u32(id) else {
            continue;
        };
        let full = format!("{command:?}");
        let Some(short) = full.strip_prefix("MAV_CMD_") else {
            continue;
        };
        exact.insert(short.to_owned(), id as u16);
        if let Some(stripped) = short.strip_prefix("NAV_") {
            without_nav.insert(stripped.to_owned(), id as u16);
        }
    }
    for (name, id) in without_nav {
        exact.entry(name).or_insert(id);
    }
    exact
}

/// "DO_SET_HOME" becomes "Do Set Home".
fn describe(name: &str) -> String {
    name.split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| {
                    first.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase()
                })
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn field_index(name: &str) -> Option<usize> {
    ["P1", "P2", "P3", "P4", "X", "Y", "Z"]
        .iter()
        .position(|f| *f == name)
}

/// Names of the commands that could not be mapped to a MAV_CMD id (kept out of the catalog).
fn parse(xml: &str, ids: &HashMap<String, u16>) -> (Catalog, Vec<String>) {
    let mut reader = Reader::from_str(xml);
    let mut catalog = Catalog::new();
    let mut unmapped = Vec::new();
    let mut depth = 0usize;
    let mut group = String::new();
    let mut command: Option<(String, [Option<String>; 7])> = None;
    let mut field: Option<usize> = None;
    let mut text = String::new();

    let mut finish_command = |group: &str, command: &mut Option<(String, [Option<String>; 7])>| {
        let Some((name, [p1, p2, p3, p4, x, y, z])) = command.take() else {
            return;
        };
        match ids.get(&name) {
            Some(id) => catalog
                .entry(group.to_owned())
                .or_default()
                .push(CommandInfo {
                    id: *id,
                    description: describe(&name),
                    name,
                    p1,
                    p2,
                    p3,
                    p4,
                    x,
                    y,
                    z,
                }),
            None => unmapped.push(format!("{group}/{name}")),
        }
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                depth += 1;
                let name = e.name().as_ref().to_owned();
                match depth {
                    2 => group = name,
                    3 => command = Some((name, Default::default())),
                    4 => {
                        field = field_index(&name);
                        text.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                // `<P2/>`: a field with no label, or a command with no fields.
                if depth == 2 {
                    let name = e.name().as_ref().to_owned();
                    command = Some((name, Default::default()));
                    finish_command(&group, &mut command);
                }
            }
            Ok(Event::Text(t)) if depth == 4 && field.is_some() => {
                text.push_str(&t.xml10_content())
            }
            Ok(Event::End(_)) => {
                match depth {
                    4 => {
                        if let (Some(i), Some((_, labels))) = (field.take(), command.as_mut()) {
                            let label = text.trim();
                            if !label.is_empty() {
                                labels[i] = Some(label.to_owned());
                            }
                        }
                    }
                    3 => finish_command(&group, &mut command),
                    _ => {}
                }
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
    (catalog, unmapped)
}

fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| parse(MAVCMD_XML, &ids_by_name()).0)
}

fn group_of(vehicle: VehicleType) -> &'static str {
    match vehicle {
        VehicleType::Plane => "APM",
        VehicleType::Rover => "APRover",
        _ => "AC2",
    }
}

/// The commands a mission can contain for this vehicle family, in the legacy tool's order.
/// Copter's list is also used for Sub, Tracker, Blimp and unknown vehicles.
pub fn mission_commands(vehicle: VehicleType) -> Vec<CommandInfo> {
    catalog()
        .get(group_of(vehicle))
        .cloned()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(list: &[CommandInfo], id: u16) -> &CommandInfo {
        list.iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("command {id} missing"))
    }

    #[test]
    fn copter_catalog_has_the_core_commands() {
        let list = mission_commands(VehicleType::Copter);
        assert!(list.len() > 40, "{}", list.len());
        let waypoint = find(&list, 16);
        assert_eq!(waypoint.name, "WAYPOINT");
        assert_eq!(waypoint.p1.as_deref(), Some("Delay"));
        assert_eq!(waypoint.p2, None);
        assert_eq!(waypoint.x.as_deref(), Some("Lat"));
        assert_eq!(waypoint.y.as_deref(), Some("Long"));
        assert_eq!(waypoint.z.as_deref(), Some("Alt"));
        let takeoff = find(&list, 22);
        assert_eq!(takeoff.name, "TAKEOFF");
        assert_eq!(takeoff.x, None);
        assert_eq!(takeoff.z.as_deref(), Some("Alt"));
        let rtl = find(&list, 20);
        assert_eq!(rtl.name, "RETURN_TO_LAUNCH");
        assert!(rtl.p1.is_none() && rtl.x.is_none() && rtl.z.is_none());
        let jump = find(&list, 177);
        assert_eq!(jump.name, "DO_JUMP");
        assert!(jump.p1.is_some() && jump.p2.is_some());
    }

    #[test]
    fn labels_come_from_the_xml_per_vehicle() {
        let copter = mission_commands(VehicleType::Copter);
        let plane = mission_commands(VehicleType::Plane);
        let rover = mission_commands(VehicleType::Rover);
        assert!(
            copter.iter().any(|c| c.id == 82),
            "copter has spline waypoints"
        );
        assert!(!plane.iter().any(|c| c.id == 82));
        assert!(plane.iter().any(|c| c.name == "VTOL_TAKEOFF"));
        assert!(!rover.iter().any(|c| c.name == "TAKEOFF"));
        assert!(rover.iter().any(|c| c.name == "DO_SET_REVERSE"));
        let delay = find(&copter, 93);
        assert_eq!(delay.name, "DELAY");
        assert_eq!(delay.p2.as_deref(), Some("Hour UTC (or -1)"));
    }

    #[test]
    fn unknown_vehicles_use_the_copter_list() {
        assert_eq!(
            mission_commands(VehicleType::Unknown),
            mission_commands(VehicleType::Copter)
        );
        assert_eq!(
            mission_commands(VehicleType::Sub),
            mission_commands(VehicleType::Copter)
        );
    }

    #[test]
    fn every_command_of_the_legacy_file_has_an_id() {
        let (_, unmapped) = parse(MAVCMD_XML, &ids_by_name());
        assert!(unmapped.is_empty(), "no MAV_CMD id for: {unmapped:?}");
    }

    #[test]
    fn ids_are_unique_within_a_vehicle_list() {
        for vehicle in [VehicleType::Copter, VehicleType::Plane, VehicleType::Rover] {
            let list = mission_commands(vehicle);
            let mut ids: Vec<u16> = list.iter().map(|c| c.id).collect();
            ids.sort_unstable();
            let total = ids.len();
            ids.dedup();
            assert_eq!(ids.len(), total, "{vehicle:?} has duplicate ids");
        }
    }

    #[test]
    fn descriptions_are_readable_and_json_is_camel_case() {
        let list = mission_commands(VehicleType::Copter);
        assert_eq!(find(&list, 179).description, "Do Set Home");
        let value = serde_json::to_value(find(&list, 16)).unwrap();
        assert_eq!(value["id"], 16);
        assert_eq!(value["p1"], "Delay");
        assert_eq!(value["p2"], serde_json::Value::Null);
    }

    #[test]
    fn parses_empty_elements_and_unmapped_names() {
        let xml =
            "<CMD><G><TAKEOFF><P1/><Z>Alt</Z></TAKEOFF><NOPE_CMD/><RETURN_TO_LAUNCH/></G></CMD>";
        let (catalog, unmapped) = parse(xml, &ids_by_name());
        let list = &catalog["G"];
        assert_eq!(list.iter().map(|c| c.id).collect::<Vec<_>>(), [22, 20]);
        assert_eq!(list[0].z.as_deref(), Some("Alt"));
        assert_eq!(list[0].p1, None);
        assert_eq!(unmapped, ["G/NOPE_CMD"]);
    }
}
