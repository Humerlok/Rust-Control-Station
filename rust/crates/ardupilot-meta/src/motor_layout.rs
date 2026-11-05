//! Motor layouts of the ArduPilot frames (`FRAME_CLASS` / `FRAME_TYPE`): for each motor its
//! output number, its position in the motor test order (A, B, C...) and its rotation. The data
//! is the legacy `APMotorLayout.json`, generated from the AP_Motors library test.
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::VehicleType;

const LAYOUT_JSON: &str = include_str!("../data/motor_layout.json");
const SUPPORTED_VERSION: &str = "AP_Motors library test ver 1.2";

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MotorInfo {
    /// Output number (the SERVOn_FUNCTION `Motor n`).
    #[serde(alias = "Number")]
    pub number: u8,
    /// Position in the motor test order; 1 is "motor A".
    #[serde(alias = "TestOrder")]
    pub test_order: u8,
    /// `"CW"`, `"CCW"`, or `"?"` when the frame does not define it.
    #[serde(alias = "Rotation")]
    pub rotation: String,
    #[serde(alias = "Roll")]
    pub roll: f32,
    #[serde(alias = "Pitch")]
    pub pitch: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MotorLayout {
    #[serde(alias = "Class")]
    pub class: u32,
    #[serde(alias = "ClassName")]
    pub class_name: String,
    #[serde(alias = "Type")]
    pub frame_type: u32,
    #[serde(alias = "TypeName")]
    pub type_name: String,
    pub motors: Vec<MotorInfo>,
}

#[derive(Deserialize)]
struct Catalog {
    #[serde(rename = "Version")]
    version: String,
    layouts: Vec<MotorLayout>,
}

fn catalog() -> &'static [MotorLayout] {
    static CATALOG: OnceLock<Vec<MotorLayout>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let parsed: Catalog =
            serde_json::from_str(LAYOUT_JSON).expect("embedded motor_layout.json is valid");
        assert_eq!(parsed.version, SUPPORTED_VERSION, "motor layout version");
        parsed.layouts
    })
}

/// The layout of a `FRAME_CLASS` / `FRAME_TYPE` pair, if ArduPilot defines one.
pub fn motor_layout(class: u32, frame_type: u32) -> Option<&'static MotorLayout> {
    catalog()
        .iter()
        .find(|l| l.class == class && l.frame_type == frame_type)
}

/// How many motors to offer when the frame is unknown (no `FRAME_CLASS` parameter or a frame
/// without a table entry): four for multicopters and rovers, the legacy default of eight otherwise.
pub fn fallback_motor_count(vehicle: VehicleType) -> u8 {
    match vehicle {
        VehicleType::Copter | VehicleType::Rover => 4,
        _ => 8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_has_every_layout() {
        assert_eq!(catalog().len(), 75);
    }

    #[test]
    fn quad_x_follows_the_ardupilot_order() {
        let layout = motor_layout(1, 1).expect("quad X");
        assert_eq!(
            (layout.class_name.as_str(), layout.type_name.as_str()),
            ("QUAD", "X")
        );
        assert_eq!(layout.motors.len(), 4);
        // Motor A (test order 1) is the front right one, output 1, spinning CCW.
        let a = layout.motors.iter().find(|m| m.test_order == 1).unwrap();
        assert_eq!((a.number, a.rotation.as_str()), (1, "CCW"));
        let b = layout.motors.iter().find(|m| m.test_order == 2).unwrap();
        assert_eq!((b.number, b.rotation.as_str()), (4, "CW"));
    }

    #[test]
    fn test_orders_count_from_one_without_gaps() {
        for layout in catalog() {
            let n = layout.motors.len() as u8;
            let mut orders: Vec<u8> = layout.motors.iter().map(|m| m.test_order).collect();
            orders.sort_unstable();
            let expected: Vec<u8> = (1..=n).collect();
            assert_eq!(
                orders, expected,
                "{} {}",
                layout.class_name, layout.type_name
            );
            // Output numbers are distinct; a tricopter's are 1, 2, 4 and 7 (the tail servo).
            let mut numbers: Vec<u8> = layout.motors.iter().map(|m| m.number).collect();
            numbers.sort_unstable();
            numbers.dedup();
            assert_eq!(numbers.len(), layout.motors.len());
        }
    }

    #[test]
    fn tricopter_tail_is_output_seven() {
        let layout = motor_layout(7, 0).expect("tri");
        let numbers: Vec<u8> = {
            let mut n: Vec<u8> = layout.motors.iter().map(|m| m.number).collect();
            n.sort_unstable();
            n
        };
        assert_eq!(numbers, vec![1, 2, 4, 7]);
    }

    #[test]
    fn unknown_frames_have_no_layout() {
        assert!(motor_layout(99, 0).is_none());
        assert!(motor_layout(1, 99).is_none());
    }

    #[test]
    fn fallback_counts() {
        assert_eq!(fallback_motor_count(VehicleType::Copter), 4);
        assert_eq!(fallback_motor_count(VehicleType::Rover), 4);
        assert_eq!(fallback_motor_count(VehicleType::Sub), 8);
    }
}
