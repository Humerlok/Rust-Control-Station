//! Builders for the MAVLink messages behind the UI actions (arm, mode, reboot, takeoff, ...).
//!
//! Everything here is pure: it only builds `MavMessage`s and the `ACTIONS` list shown by the UI.
//! Sending, ACK waiting and retries live in `control_station_link::LinkManager`.

// ArduPilot firmware still depends on REQUEST_DATA_STREAM and MISSION_SET_CURRENT, which the
// MAVLink dialect marks as superseded.
#![allow(deprecated)]

use control_station_ardupilot_meta::VehicleType;
use control_station_mavlink::{
    dialect::{
        MavCmd, MavDataStream, MavFrame, COMMAND_INT_DATA, COMMAND_LONG_DATA,
        MISSION_SET_CURRENT_DATA, REQUEST_DATA_STREAM_DATA,
    },
    MavMessage,
};
use control_station_vehicle_state::VehicleId;
use serde::Serialize;

/// Magic values that make ArduPilot arm or disarm despite failing pre-arm / safety checks.
const FORCE_ARM: f32 = 2989.0;
const FORCE_DISARM: f32 = 21196.0;

pub fn command_long(target: VehicleId, command: MavCmd, p: [f32; 7]) -> MavMessage {
    MavMessage::COMMAND_LONG(COMMAND_LONG_DATA {
        param1: p[0],
        param2: p[1],
        param3: p[2],
        param4: p[3],
        param5: p[4],
        param6: p[5],
        param7: p[6],
        command,
        target_system: target.system_id,
        target_component: target.component_id,
        confirmation: 0,
    })
}

/// COMMAND_INT with a global frame: `lat`/`lon` are degrees, `alt_m` is in the frame's reference.
pub fn command_int_global(
    target: VehicleId,
    command: MavCmd,
    frame: MavFrame,
    p: [f32; 4],
    lat: f64,
    lon: f64,
    alt_m: f32,
) -> MavMessage {
    MavMessage::COMMAND_INT(COMMAND_INT_DATA {
        param1: p[0],
        param2: p[1],
        param3: p[2],
        param4: p[3],
        x: (lat * 1e7).round() as i32,
        y: (lon * 1e7).round() as i32,
        z: alt_m,
        command,
        target_system: target.system_id,
        target_component: target.component_id,
        frame,
        current: 0,
        autocontinue: 0,
    })
}

pub fn arm_disarm(target: VehicleId, arm: bool, force: bool) -> MavMessage {
    let force_value = match (arm, force) {
        (_, false) => 0.0,
        (true, true) => FORCE_ARM,
        (false, true) => FORCE_DISARM,
    };
    command_long(
        target,
        MavCmd::MAV_CMD_COMPONENT_ARM_DISARM,
        [
            if arm { 1.0 } else { 0.0 },
            force_value,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ],
    )
}

/// `MAV_CMD_DO_SET_MODE` with the custom-mode flag set (ArduPilot flight mode number).
pub fn set_mode(target: VehicleId, custom_mode: u32) -> MavMessage {
    command_long(
        target,
        MavCmd::MAV_CMD_DO_SET_MODE,
        [1.0, custom_mode as f32, 0.0, 0.0, 0.0, 0.0, 0.0],
    )
}

pub fn reboot(target: VehicleId) -> MavMessage {
    command_long(
        target,
        MavCmd::MAV_CMD_PREFLIGHT_REBOOT_SHUTDOWN,
        [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    )
}

/// Reboots into the bootloader and stays there until firmware is flashed or the bootloader
/// is told to boot (`MAV_CMD_PREFLIGHT_REBOOT_SHUTDOWN` with param1 = 3).
pub fn reboot_to_bootloader(target: VehicleId) -> MavMessage {
    command_long(
        target,
        MavCmd::MAV_CMD_PREFLIGHT_REBOOT_SHUTDOWN,
        [3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    )
}

/// `MAV_CMD_SET_MESSAGE_INTERVAL`: stream one message every `interval_us` microseconds;
/// 0 restores the vehicle's own rate and -1 stops the message.
pub fn set_message_interval(target: VehicleId, message_id: u32, interval_us: i32) -> MavMessage {
    command_long(
        target,
        MavCmd::MAV_CMD_SET_MESSAGE_INTERVAL,
        [
            message_id as f32,
            interval_us as f32,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ],
    )
}

/// What the throttle value of `MAV_CMD_DO_MOTOR_TEST` means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotorThrottleType {
    /// 0..100 % of the motor range.
    Percent,
    /// A PWM pulse in microseconds.
    Pwm,
    /// The pilot's throttle stick passes straight through.
    Pilot,
}

impl MotorThrottleType {
    fn code(self) -> f32 {
        match self {
            Self::Percent => 0.0,
            Self::Pwm => 1.0,
            Self::Pilot => 2.0,
        }
    }
}

/// `MAV_CMD_DO_MOTOR_TEST`: spin `motor` (1-based position in the test order, so motor A is 1)
/// for `seconds`. With `count > 1` the autopilot runs the whole sequence itself and `seconds`
/// becomes the delay between motors; `count == 0` tests that single motor. Throttle 0 with
/// duration 0 stops it.
pub fn motor_test(
    target: VehicleId,
    motor: u8,
    throttle_type: MotorThrottleType,
    throttle: f32,
    seconds: f32,
    count: u8,
) -> MavMessage {
    command_long(
        target,
        MavCmd::MAV_CMD_DO_MOTOR_TEST,
        [
            f32::from(motor),
            throttle_type.code(),
            throttle,
            seconds,
            f32::from(count),
            0.0, // MOTOR_TEST_ORDER_DEFAULT: motor numbers follow the test order (A, B, C...)
            0.0,
        ],
    )
}

pub fn mission_set_current(target: VehicleId, seq: u16) -> MavMessage {
    MavMessage::MISSION_SET_CURRENT(MISSION_SET_CURRENT_DATA {
        seq,
        target_system: target.system_id,
        target_component: target.component_id,
    })
}

pub fn takeoff(target: VehicleId, altitude_m: f32) -> MavMessage {
    command_long(
        target,
        MavCmd::MAV_CMD_NAV_TAKEOFF,
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, altitude_m],
    )
}

/// Guided "fly to here" (`MAV_CMD_DO_REPOSITION`, switching to guided mode).
pub fn fly_to(target: VehicleId, lat: f64, lon: f64, altitude_m: f32) -> MavMessage {
    command_int_global(
        target,
        MavCmd::MAV_CMD_DO_REPOSITION,
        MavFrame::MAV_FRAME_GLOBAL_RELATIVE_ALT,
        [-1.0, 1.0, 0.0, f32::NAN],
        lat,
        lon,
        altitude_m,
    )
}

/// `altitude_m` is above mean sea level.
pub fn set_home(target: VehicleId, lat: f64, lon: f64, altitude_m: f32) -> MavMessage {
    command_int_global(
        target,
        MavCmd::MAV_CMD_DO_SET_HOME,
        MavFrame::MAV_FRAME_GLOBAL,
        [0.0; 4],
        lat,
        lon,
        altitude_m,
    )
}

/// Legacy-style telemetry stream requests; every ArduPilot firmware honors them.
pub fn request_data_streams(target: VehicleId) -> Vec<MavMessage> {
    [
        (MavDataStream::MAV_DATA_STREAM_RAW_SENSORS, 2),
        (MavDataStream::MAV_DATA_STREAM_EXTENDED_STATUS, 2),
        (MavDataStream::MAV_DATA_STREAM_RC_CHANNELS, 2),
        (MavDataStream::MAV_DATA_STREAM_POSITION, 3),
        (MavDataStream::MAV_DATA_STREAM_EXTRA1, 10),
        (MavDataStream::MAV_DATA_STREAM_EXTRA2, 4),
        (MavDataStream::MAV_DATA_STREAM_EXTRA3, 2),
    ]
    .into_iter()
    .map(|(stream, rate)| {
        MavMessage::REQUEST_DATA_STREAM(REQUEST_DATA_STREAM_DATA {
            req_message_rate: rate,
            target_system: target.system_id,
            target_component: target.component_id,
            req_stream_id: stream,
            start_stop: 1,
        })
    })
    .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ActionInfo {
    pub name: &'static str,
    pub label: &'static str,
    pub confirm: bool,
    pub danger: bool,
}

const fn action(
    name: &'static str,
    label: &'static str,
    confirm: bool,
    danger: bool,
) -> ActionInfo {
    ActionInfo {
        name,
        label,
        confirm,
        danger,
    }
}

/// The "Do Action" list of the legacy Actions tab (the subset ArduPilot accepts as commands).
pub const ACTIONS: &[ActionInfo] = &[
    action("Return_To_Launch", "Return to launch", true, false),
    action(
        "Preflight_Calibration",
        "Preflight calibration (gyro/baro)",
        true,
        false,
    ),
    action("Mission_Start", "Mission start", true, false),
    action("Preflight_Reboot_Shutdown", "Preflight reboot", true, true),
    action("Reboot_Mass_Storage", "Reboot to mass storage", true, true),
    action("Trigger_Camera", "Trigger camera", false, false),
    action("Battery_Reset", "Battery reset (100%)", true, false),
    action("Scripting_cmd_stop", "Scripting: stop", true, false),
    action(
        "Scripting_cmd_stop_and_restart",
        "Scripting: stop and restart",
        true,
        false,
    ),
    action("Format_SD_Card", "Format SD card", true, true),
    action("Terminate_Flight", "Terminate flight", true, true),
];

/// Builds the command for a named action, or `None` for an unknown name.
pub fn action_message(name: &str, target: VehicleId, vehicle: VehicleType) -> Option<MavMessage> {
    let long = |cmd, p: [f32; 7]| Some(command_long(target, cmd, p));
    let int = |cmd, p1: f32, p2: f32| {
        Some(command_int_global(
            target,
            cmd,
            MavFrame::MAV_FRAME_GLOBAL,
            [p1, p2, 0.0, 0.0],
            0.0,
            0.0,
            0.0,
        ))
    };
    match name {
        "Return_To_Launch" => long(MavCmd::MAV_CMD_NAV_RETURN_TO_LAUNCH, [0.0; 7]),
        "Preflight_Calibration" => {
            let gyro = if vehicle == VehicleType::Copter {
                1.0
            } else {
                0.0
            };
            long(
                MavCmd::MAV_CMD_PREFLIGHT_CALIBRATION,
                [gyro, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            )
        }
        "Mission_Start" => long(MavCmd::MAV_CMD_MISSION_START, [0.0; 7]),
        "Preflight_Reboot_Shutdown" => Some(reboot(target)),
        "Reboot_Mass_Storage" => long(
            MavCmd::MAV_CMD_PREFLIGHT_REBOOT_SHUTDOWN,
            [5.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
        "Trigger_Camera" => long(
            MavCmd::MAV_CMD_DO_DIGICAM_CONTROL,
            [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        ),
        "Battery_Reset" => long(
            MavCmd::MAV_CMD_BATTERY_RESET,
            [255.0, 100.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
        "Scripting_cmd_stop" => int(MavCmd::MAV_CMD_SCRIPTING, 2.0, 0.0),
        "Scripting_cmd_stop_and_restart" => int(MavCmd::MAV_CMD_SCRIPTING, 3.0, 0.0),
        "Format_SD_Card" => int(MavCmd::MAV_CMD_STORAGE_FORMAT, 1.0, 1.0),
        "Terminate_Flight" => long(
            MavCmd::MAV_CMD_DO_FLIGHTTERMINATION,
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: VehicleId = VehicleId {
        system_id: 1,
        component_id: 1,
    };

    fn long(msg: &MavMessage) -> &COMMAND_LONG_DATA {
        match msg {
            MavMessage::COMMAND_LONG(c) => c,
            other => panic!("expected COMMAND_LONG, got {other:?}"),
        }
    }

    #[test]
    fn arm_and_disarm_use_force_magic_only_when_forced() {
        let arm = arm_disarm(TARGET, true, false);
        assert_eq!(long(&arm).param1, 1.0);
        assert_eq!(long(&arm).param2, 0.0);
        assert_eq!(long(&arm_disarm(TARGET, true, true)).param2, 2989.0);
        let disarm = arm_disarm(TARGET, false, true);
        assert_eq!(long(&disarm).param1, 0.0);
        assert_eq!(long(&disarm).param2, 21196.0);
        assert_eq!(long(&arm).command, MavCmd::MAV_CMD_COMPONENT_ARM_DISARM);
    }

    #[test]
    fn set_mode_sends_custom_mode_flag_and_number() {
        let msg = set_mode(TARGET, 6);
        let c = long(&msg);
        assert_eq!(c.command, MavCmd::MAV_CMD_DO_SET_MODE);
        assert_eq!((c.param1, c.param2), (1.0, 6.0));
        assert_eq!((c.target_system, c.target_component), (1, 1));
    }

    #[test]
    fn reboot_asks_to_reboot_the_autopilot() {
        let msg = reboot(TARGET);
        let c = long(&msg);
        assert_eq!(c.command, MavCmd::MAV_CMD_PREFLIGHT_REBOOT_SHUTDOWN);
        assert_eq!(c.param1, 1.0);
    }

    #[test]
    fn reboot_to_bootloader_uses_param1_3() {
        let msg = reboot_to_bootloader(TARGET);
        let c = long(&msg);
        assert_eq!(c.command, MavCmd::MAV_CMD_PREFLIGHT_REBOOT_SHUTDOWN);
        assert_eq!(c.param1, 3.0);
    }

    #[test]
    fn motor_test_packs_the_do_motor_test_parameters() {
        let msg = motor_test(TARGET, 3, MotorThrottleType::Percent, 7.5, 2.0, 0);
        let c = long(&msg);
        assert_eq!(c.command, MavCmd::MAV_CMD_DO_MOTOR_TEST);
        assert_eq!((c.param1, c.param2, c.param3), (3.0, 0.0, 7.5));
        assert_eq!((c.param4, c.param5, c.param6), (2.0, 0.0, 0.0));
        let pwm = motor_test(TARGET, 1, MotorThrottleType::Pwm, 1100.0, 1.0, 4);
        assert_eq!((long(&pwm).param2, long(&pwm).param5), (1.0, 4.0));
        let pilot = motor_test(TARGET, 1, MotorThrottleType::Pilot, 0.0, 1.0, 0);
        assert_eq!(long(&pilot).param2, 2.0);
    }

    #[test]
    fn message_interval_carries_id_and_period() {
        let msg = set_message_interval(TARGET, 11030, 250_000);
        let c = long(&msg);
        assert_eq!(c.command, MavCmd::MAV_CMD_SET_MESSAGE_INTERVAL);
        assert_eq!((c.param1, c.param2), (11030.0, 250_000.0));
    }

    #[test]
    fn fly_to_encodes_scaled_coordinates() {
        match fly_to(TARGET, -23.5, 46.25, 30.0) {
            MavMessage::COMMAND_INT(c) => {
                assert_eq!(c.command, MavCmd::MAV_CMD_DO_REPOSITION);
                assert_eq!((c.x, c.y), (-235_000_000, 462_500_000));
                assert_eq!(c.z, 30.0);
                assert_eq!(c.param2, 1.0);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn data_stream_requests_cover_attitude_and_position() {
        let requests = request_data_streams(TARGET);
        assert_eq!(requests.len(), 7);
        let rate_of = |wanted: MavDataStream| {
            requests.iter().find_map(|m| match m {
                MavMessage::REQUEST_DATA_STREAM(r) if r.req_stream_id == wanted => {
                    Some(r.req_message_rate)
                }
                _ => None,
            })
        };
        assert_eq!(rate_of(MavDataStream::MAV_DATA_STREAM_EXTRA1), Some(10));
        assert_eq!(rate_of(MavDataStream::MAV_DATA_STREAM_POSITION), Some(3));
    }

    #[test]
    fn every_listed_action_builds_a_message() {
        for info in ACTIONS {
            assert!(
                action_message(info.name, TARGET, VehicleType::Copter).is_some(),
                "{}",
                info.name
            );
        }
        assert!(action_message("Nope", TARGET, VehicleType::Copter).is_none());
    }

    #[test]
    fn copter_preflight_calibration_includes_gyro() {
        let copter = action_message("Preflight_Calibration", TARGET, VehicleType::Copter).unwrap();
        let plane = action_message("Preflight_Calibration", TARGET, VehicleType::Plane).unwrap();
        assert_eq!(long(&copter).param1, 1.0);
        assert_eq!(long(&plane).param1, 0.0);
        assert_eq!(long(&copter).param3, 1.0);
    }

    #[test]
    fn dangerous_actions_always_confirm() {
        assert!(ACTIONS.iter().filter(|a| a.danger).all(|a| a.confirm));
    }
}
