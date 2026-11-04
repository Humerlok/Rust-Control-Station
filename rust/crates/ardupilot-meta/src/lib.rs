//! ArduPilot vehicle metadata: vehicle types and flight-mode tables.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VehicleType {
    Copter,
    Plane,
    Rover,
    Sub,
    Tracker,
    Blimp,
    Unknown,
}

impl VehicleType {
    /// Maps the numeric `MAV_TYPE` of an autopilot heartbeat to an ArduPilot vehicle family.
    pub fn from_mav_type(mav_type: u8) -> Self {
        match mav_type {
            2 | 3 | 4 | 13 | 14 | 15 | 29 | 35 => Self::Copter,
            1 | 19..=24 => Self::Plane,
            10 | 11 => Self::Rover,
            12 => Self::Sub,
            5 => Self::Tracker,
            7 => Self::Blimp,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Copter => "Copter",
            Self::Plane => "Plane",
            Self::Rover => "Rover",
            Self::Sub => "Sub",
            Self::Tracker => "Tracker",
            Self::Blimp => "Blimp",
            Self::Unknown => "Unknown",
        }
    }

    pub fn modes(self) -> &'static [(u32, &'static str)] {
        match self {
            Self::Copter => COPTER_MODES,
            Self::Plane => PLANE_MODES,
            Self::Rover => ROVER_MODES,
            Self::Sub => SUB_MODES,
            Self::Tracker => TRACKER_MODES,
            Self::Blimp => BLIMP_MODES,
            Self::Unknown => &[],
        }
    }

    pub fn mode_name(self, custom_mode: u32) -> Option<&'static str> {
        self.modes()
            .iter()
            .find(|(n, _)| *n == custom_mode)
            .map(|(_, name)| *name)
    }

    pub fn mode_number(self, name: &str) -> Option<u32> {
        self.modes()
            .iter()
            .find(|(_, n)| n.eq_ignore_ascii_case(name))
            .map(|(number, _)| *number)
    }
}

const COPTER_MODES: &[(u32, &str)] = &[
    (0, "Stabilize"),
    (1, "Acro"),
    (2, "AltHold"),
    (3, "Auto"),
    (4, "Guided"),
    (5, "Loiter"),
    (6, "RTL"),
    (7, "Circle"),
    (9, "Land"),
    (11, "Drift"),
    (13, "Sport"),
    (14, "Flip"),
    (15, "AutoTune"),
    (16, "PosHold"),
    (17, "Brake"),
    (18, "Throw"),
    (19, "Avoid_ADSB"),
    (20, "Guided_NoGPS"),
    (21, "Smart_RTL"),
    (22, "FlowHold"),
    (23, "Follow"),
    (24, "ZigZag"),
    (25, "SystemID"),
    (26, "Heli_Autorotate"),
    (27, "Auto RTL"),
    (28, "Turtle"),
];

const PLANE_MODES: &[(u32, &str)] = &[
    (0, "Manual"),
    (1, "Circle"),
    (2, "Stabilize"),
    (3, "Training"),
    (4, "Acro"),
    (5, "FlyByWireA"),
    (6, "FlyByWireB"),
    (7, "Cruise"),
    (8, "Autotune"),
    (10, "Auto"),
    (11, "RTL"),
    (12, "Loiter"),
    (13, "Takeoff"),
    (14, "Avoid_ADSB"),
    (15, "Guided"),
    (17, "QStabilize"),
    (18, "QHover"),
    (19, "QLoiter"),
    (20, "QLand"),
    (21, "QRTL"),
    (22, "QAutotune"),
    (23, "QAcro"),
    (24, "Thermal"),
    (25, "Loiter to QLand"),
];

const ROVER_MODES: &[(u32, &str)] = &[
    (0, "Manual"),
    (1, "Acro"),
    (3, "Steering"),
    (4, "Hold"),
    (5, "Loiter"),
    (6, "Follow"),
    (7, "Simple"),
    (8, "Dock"),
    (9, "Circle"),
    (10, "Auto"),
    (11, "RTL"),
    (12, "SmartRTL"),
    (15, "Guided"),
];

const SUB_MODES: &[(u32, &str)] = &[
    (0, "Stabilize"),
    (1, "Acro"),
    (2, "AltHold"),
    (3, "Auto"),
    (4, "Guided"),
    (7, "Circle"),
    (9, "Surface"),
    (16, "PosHold"),
    (19, "Manual"),
];

const TRACKER_MODES: &[(u32, &str)] = &[
    (0, "Manual"),
    (1, "Stop"),
    (2, "Scan"),
    (3, "Servo Test"),
    (10, "Auto"),
    (16, "Initialising"),
];

const BLIMP_MODES: &[(u32, &str)] = &[(0, "Land"), (1, "Manual"), (2, "Velocity"), (3, "Loiter")];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_mav_types_to_families() {
        assert_eq!(VehicleType::from_mav_type(2), VehicleType::Copter);
        assert_eq!(VehicleType::from_mav_type(13), VehicleType::Copter);
        assert_eq!(VehicleType::from_mav_type(1), VehicleType::Plane);
        assert_eq!(VehicleType::from_mav_type(21), VehicleType::Plane);
        assert_eq!(VehicleType::from_mav_type(10), VehicleType::Rover);
        assert_eq!(VehicleType::from_mav_type(6), VehicleType::Unknown);
    }

    #[test]
    fn copter_mode_lookup_both_ways() {
        assert_eq!(VehicleType::Copter.mode_name(5), Some("Loiter"));
        assert_eq!(VehicleType::Copter.mode_number("rtl"), Some(6));
        assert_eq!(VehicleType::Copter.mode_name(8), None);
        assert!(VehicleType::Unknown.modes().is_empty());
    }

    #[test]
    fn mode_numbers_are_unique_per_vehicle() {
        for v in [
            VehicleType::Copter,
            VehicleType::Plane,
            VehicleType::Rover,
            VehicleType::Sub,
            VehicleType::Tracker,
            VehicleType::Blimp,
        ] {
            let mut numbers: Vec<_> = v.modes().iter().map(|(n, _)| *n).collect();
            numbers.sort_unstable();
            numbers.dedup();
            assert_eq!(numbers.len(), v.modes().len(), "{v:?}");
        }
    }
}
