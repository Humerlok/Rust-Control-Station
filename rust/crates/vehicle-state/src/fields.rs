//! The Quick View field table and the decoders that fill it. Everything here is a plain number
//! keyed by the legacy `CurrentState` property name, so saved Quick View layouts and the
//! Mission Planner habits of users carry over.
use std::time::Instant;

use control_station_mavlink::MavMessage;
use serde::Serialize;

use crate::VehicleState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FieldInfo {
    pub key: &'static str,
    pub label: &'static str,
    pub unit: &'static str,
    /// Category shown in the Quick View picker (the legacy `[GroupText]`).
    pub group: &'static str,
}

const fn f(
    group: &'static str,
    key: &'static str,
    label: &'static str,
    unit: &'static str,
) -> FieldInfo {
    FieldInfo {
        key,
        label,
        unit,
        group,
    }
}

/// Numeric fields a user can put in the Quick View. Keys follow the legacy `CurrentState`
/// names and groups; entries of one group stay together, in the order the picker shows them.
pub const FIELDS: &[FieldInfo] = &[
    f("Position", "alt", "Altitude (relative)", "m"),
    f("Position", "altasl", "Altitude (MSL)", "m"),
    f("Position", "groundspeed", "Ground speed", "m/s"),
    f("Position", "airspeed", "Air speed", "m/s"),
    f("Position", "verticalspeed", "Vertical speed", "m/s"),
    f("Position", "heading", "Heading", "°"),
    f("Position", "dist_to_home", "Distance to home", "m"),
    f("Position", "distTraveled", "Distance traveled", "m"),
    f("Position", "timeInAir", "Time in air", "s"),
    f("Position", "climbrate", "Climb rate", "m/s"),
    f("Position", "vx", "Velocity north", "m/s"),
    f("Position", "vy", "Velocity east", "m/s"),
    f("Position", "vz", "Velocity down", "m/s"),
    f("Position", "lat", "GPS latitude", "°"),
    f("Position", "lng", "GPS longitude", "°"),
    f("Position", "groundcourse", "GPS ground course", "°"),
    f("Position", "gpsstatus", "GPS fix type", ""),
    f("Position", "satcount", "GPS satellites", ""),
    f("Position", "gpshdop", "GPS HDOP", ""),
    f("Position", "gpsh_acc", "GPS horizontal accuracy", "m"),
    f("Position", "gpsv_acc", "GPS vertical accuracy", "m"),
    f("Position", "gpsvel_acc", "GPS speed accuracy", "m/s"),
    f("Position", "gpshdg_acc", "GPS heading accuracy", "°"),
    f("Position", "gpsyaw", "GPS yaw", "°"),
    f("Position", "lat2", "GPS2 latitude", "°"),
    f("Position", "lng2", "GPS2 longitude", "°"),
    f("Position", "altasl2", "GPS2 altitude (MSL)", "m"),
    f("Position", "gpsstatus2", "GPS2 fix type", ""),
    f("Position", "satcount2", "GPS2 satellites", ""),
    f("Position", "gpshdop2", "GPS2 HDOP", ""),
    f("Position", "groundspeed2", "GPS2 ground speed", "m/s"),
    f("Position", "groundcourse2", "GPS2 ground course", "°"),
    f("Position", "gpsh_acc2", "GPS2 horizontal accuracy", "m"),
    f("Position", "gpsv_acc2", "GPS2 vertical accuracy", "m"),
    f("Position", "gpsvel_acc2", "GPS2 speed accuracy", "m/s"),
    f("Position", "gpshdg_acc2", "GPS2 heading accuracy", "°"),
    f("Position", "gpsyaw2", "GPS2 yaw", "°"),
    f("Attitude", "roll", "Roll", "°"),
    f("Attitude", "pitch", "Pitch", "°"),
    f("Attitude", "yaw", "Yaw", "°"),
    f("Attitude", "turnrate", "Turn rate", "°/s"),
    f("Attitude", "throttle", "Throttle", "%"),
    f("Battery", "battery_voltage", "Battery voltage", "V"),
    f("Battery", "battery_current", "Battery current", "A"),
    f("Battery", "battery_remaining", "Battery remaining", "%"),
    f("Battery", "battery_usedmah", "Battery used", "mAh"),
    f("Battery", "battery_temp", "Battery temperature", "°C"),
    f(
        "Battery",
        "battery_remainmin",
        "Battery time remaining",
        "min",
    ),
    f("Battery", "battery_cell1", "Battery cell 1", "V"),
    f("Battery", "battery_cell2", "Battery cell 2", "V"),
    f("Battery", "battery_cell3", "Battery cell 3", "V"),
    f("Battery", "battery_cell4", "Battery cell 4", "V"),
    f("Battery", "battery_cell5", "Battery cell 5", "V"),
    f("Battery", "battery_cell6", "Battery cell 6", "V"),
    f("Battery", "battery_cell7", "Battery cell 7", "V"),
    f("Battery", "battery_cell8", "Battery cell 8", "V"),
    f("Battery", "battery_cell9", "Battery cell 9", "V"),
    f("Battery", "battery_cell10", "Battery cell 10", "V"),
    f("Battery", "battery_cell11", "Battery cell 11", "V"),
    f("Battery", "battery_cell12", "Battery cell 12", "V"),
    f("Battery", "battery_voltage2", "Battery 2 voltage", "V"),
    f("Battery", "battery_current2", "Battery 2 current", "A"),
    f("Battery", "battery_remaining2", "Battery 2 remaining", "%"),
    f("Battery", "battery_usedmah2", "Battery 2 used", "mAh"),
    f("Battery", "battery_temp2", "Battery 2 temperature", "°C"),
    f(
        "Battery",
        "battery_remainmin2",
        "Battery 2 time remaining",
        "min",
    ),
    f("Battery", "battery_voltage3", "Battery 3 voltage", "V"),
    f("Battery", "battery_current3", "Battery 3 current", "A"),
    f("Battery", "battery_remaining3", "Battery 3 remaining", "%"),
    f("Battery", "battery_usedmah3", "Battery 3 used", "mAh"),
    f("Battery", "battery_temp3", "Battery 3 temperature", "°C"),
    f(
        "Battery",
        "battery_remainmin3",
        "Battery 3 time remaining",
        "min",
    ),
    f("Battery", "battery_voltage4", "Battery 4 voltage", "V"),
    f("Battery", "battery_current4", "Battery 4 current", "A"),
    f("Battery", "battery_remaining4", "Battery 4 remaining", "%"),
    f("Battery", "battery_usedmah4", "Battery 4 used", "mAh"),
    f("Battery", "battery_temp4", "Battery 4 temperature", "°C"),
    f(
        "Battery",
        "battery_remainmin4",
        "Battery 4 time remaining",
        "min",
    ),
    f("Battery", "battery_voltage5", "Battery 5 voltage", "V"),
    f("Battery", "battery_current5", "Battery 5 current", "A"),
    f("Battery", "battery_remaining5", "Battery 5 remaining", "%"),
    f("Battery", "battery_usedmah5", "Battery 5 used", "mAh"),
    f("Battery", "battery_temp5", "Battery 5 temperature", "°C"),
    f(
        "Battery",
        "battery_remainmin5",
        "Battery 5 time remaining",
        "min",
    ),
    f("Battery", "battery_voltage6", "Battery 6 voltage", "V"),
    f("Battery", "battery_current6", "Battery 6 current", "A"),
    f("Battery", "battery_remaining6", "Battery 6 remaining", "%"),
    f("Battery", "battery_usedmah6", "Battery 6 used", "mAh"),
    f("Battery", "battery_temp6", "Battery 6 temperature", "°C"),
    f(
        "Battery",
        "battery_remainmin6",
        "Battery 6 time remaining",
        "min",
    ),
    f("Battery", "battery_voltage7", "Battery 7 voltage", "V"),
    f("Battery", "battery_current7", "Battery 7 current", "A"),
    f("Battery", "battery_remaining7", "Battery 7 remaining", "%"),
    f("Battery", "battery_usedmah7", "Battery 7 used", "mAh"),
    f("Battery", "battery_temp7", "Battery 7 temperature", "°C"),
    f(
        "Battery",
        "battery_remainmin7",
        "Battery 7 time remaining",
        "min",
    ),
    f("Battery", "battery_voltage8", "Battery 8 voltage", "V"),
    f("Battery", "battery_current8", "Battery 8 current", "A"),
    f("Battery", "battery_remaining8", "Battery 8 remaining", "%"),
    f("Battery", "battery_usedmah8", "Battery 8 used", "mAh"),
    f("Battery", "battery_temp8", "Battery 8 temperature", "°C"),
    f(
        "Battery",
        "battery_remainmin8",
        "Battery 8 time remaining",
        "min",
    ),
    f("Battery", "battery_voltage9", "Battery 9 voltage", "V"),
    f("Battery", "battery_current9", "Battery 9 current", "A"),
    f("Battery", "battery_remaining9", "Battery 9 remaining", "%"),
    f("Battery", "battery_usedmah9", "Battery 9 used", "mAh"),
    f("Battery", "battery_temp9", "Battery 9 temperature", "°C"),
    f(
        "Battery",
        "battery_remainmin9",
        "Battery 9 time remaining",
        "min",
    ),
    f("NAV", "wp_dist", "Distance to waypoint", "m"),
    f("NAV", "wpno", "Current waypoint", ""),
    f("NAV", "nav_bearing", "Nav bearing", "°"),
    f("NAV", "target_bearing", "Target bearing", "°"),
    f("NAV", "nav_roll", "Nav roll", "°"),
    f("NAV", "nav_pitch", "Nav pitch", "°"),
    f("NAV", "alt_error", "Altitude error", "m"),
    f("NAV", "aspd_error", "Airspeed error", "m/s"),
    f("NAV", "xtrack_error", "Cross-track error", "m"),
    f("Sensor", "press_abs", "Pressure", "hPa"),
    f("Sensor", "press_temp", "Baro temperature", "°C"),
    f("Sensor", "press_abs2", "Pressure 2", "hPa"),
    f("Sensor", "press_temp2", "Baro 2 temperature", "°C"),
    f("Sensor", "sonarrange", "Rangefinder distance", "m"),
    f("Sensor", "sonarvoltage", "Rangefinder voltage", "V"),
    f("Sensor", "rangefinder1", "Distance sensor 1", "m"),
    f("Sensor", "rangefinder2", "Distance sensor 2", "m"),
    f("Sensor", "rangefinder3", "Distance sensor 3", "m"),
    f("Sensor", "rangefinder4", "Distance sensor 4", "m"),
    f("Sensor", "rangefinder5", "Distance sensor 5", "m"),
    f("Sensor", "rangefinder6", "Distance sensor 6", "m"),
    f("Sensor", "rangefinder7", "Distance sensor 7", "m"),
    f("Sensor", "rangefinder8", "Distance sensor 8", "m"),
    f("Sensor", "rangefinder9", "Distance sensor 9", "m"),
    f("Sensor", "rangefinder10", "Distance sensor 10", "m"),
    f("Sensor", "wind_dir", "Wind direction", "°"),
    f("Sensor", "wind_vel", "Wind speed", "m/s"),
    f("Sensor", "rpm1", "RPM 1", "rpm"),
    f("Sensor", "rpm2", "RPM 2", "rpm"),
    f("Sensor", "ax", "IMU 1 accel X", "mG"),
    f("Sensor", "ay", "IMU 1 accel Y", "mG"),
    f("Sensor", "az", "IMU 1 accel Z", "mG"),
    f("Sensor", "gx", "IMU 1 gyro X", "mrad/s"),
    f("Sensor", "gy", "IMU 1 gyro Y", "mrad/s"),
    f("Sensor", "gz", "IMU 1 gyro Z", "mrad/s"),
    f("Sensor", "mx", "IMU 1 mag X", "mGauss"),
    f("Sensor", "my", "IMU 1 mag Y", "mGauss"),
    f("Sensor", "mz", "IMU 1 mag Z", "mGauss"),
    f("Sensor", "ax2", "IMU 2 accel X", "mG"),
    f("Sensor", "ay2", "IMU 2 accel Y", "mG"),
    f("Sensor", "az2", "IMU 2 accel Z", "mG"),
    f("Sensor", "gx2", "IMU 2 gyro X", "mrad/s"),
    f("Sensor", "gy2", "IMU 2 gyro Y", "mrad/s"),
    f("Sensor", "gz2", "IMU 2 gyro Z", "mrad/s"),
    f("Sensor", "mx2", "IMU 2 mag X", "mGauss"),
    f("Sensor", "my2", "IMU 2 mag Y", "mGauss"),
    f("Sensor", "mz2", "IMU 2 mag Z", "mGauss"),
    f("Sensor", "ax3", "IMU 3 accel X", "mG"),
    f("Sensor", "ay3", "IMU 3 accel Y", "mG"),
    f("Sensor", "az3", "IMU 3 accel Z", "mG"),
    f("Sensor", "gx3", "IMU 3 gyro X", "mrad/s"),
    f("Sensor", "gy3", "IMU 3 gyro Y", "mrad/s"),
    f("Sensor", "gz3", "IMU 3 gyro Z", "mrad/s"),
    f("Sensor", "mx3", "IMU 3 mag X", "mGauss"),
    f("Sensor", "my3", "IMU 3 mag Y", "mGauss"),
    f("Sensor", "mz3", "IMU 3 mag Z", "mGauss"),
    f("Sensor", "imu_temp", "IMU temperature", "°C"),
    f("EKF", "ekfstatus", "EKF status (max variance)", ""),
    f("EKF", "ekfvelv", "EKF velocity variance", ""),
    f("EKF", "ekfposhor", "EKF horizontal position variance", ""),
    f("EKF", "ekfposvert", "EKF vertical position variance", ""),
    f("EKF", "ekfcompv", "EKF compass variance", ""),
    f("EKF", "ekfteralt", "EKF terrain variance", ""),
    f("EKF", "ekfflags", "EKF flags", ""),
    f("Vibe", "vibex", "Vibration X", "m/s²"),
    f("Vibe", "vibey", "Vibration Y", "m/s²"),
    f("Vibe", "vibez", "Vibration Z", "m/s²"),
    f("Vibe", "vibeclip0", "Accel 0 clipping", ""),
    f("Vibe", "vibeclip1", "Accel 1 clipping", ""),
    f("Vibe", "vibeclip2", "Accel 2 clipping", ""),
    f("AHRS2", "ahrs2_roll", "AHRS2 roll", "°"),
    f("AHRS2", "ahrs2_pitch", "AHRS2 pitch", "°"),
    f("AHRS2", "ahrs2_yaw", "AHRS2 yaw", "°"),
    f("AHRS2", "ahrs2_alt", "AHRS2 altitude", "m"),
    f("AHRS2", "ahrs2_lat", "AHRS2 latitude", "°"),
    f("AHRS2", "ahrs2_lng", "AHRS2 longitude", "°"),
    f("Telem", "rssi", "Radio RSSI", ""),
    f("Telem", "remrssi", "Radio remote RSSI", ""),
    f("Telem", "noise", "Radio noise", ""),
    f("Telem", "remnoise", "Radio remote noise", ""),
    f("Telem", "txbuffer", "Radio TX buffer", "%"),
    f("Telem", "rxerrors", "Radio RX errors", ""),
    f("Telem", "fixedp", "Radio corrected packets", ""),
    f("Telem", "packetdropremote", "Vehicle dropped packets", "%"),
    f("RadioIn", "ch1in", "RC input 1", "µs"),
    f("RadioIn", "ch2in", "RC input 2", "µs"),
    f("RadioIn", "ch3in", "RC input 3", "µs"),
    f("RadioIn", "ch4in", "RC input 4", "µs"),
    f("RadioIn", "ch5in", "RC input 5", "µs"),
    f("RadioIn", "ch6in", "RC input 6", "µs"),
    f("RadioIn", "ch7in", "RC input 7", "µs"),
    f("RadioIn", "ch8in", "RC input 8", "µs"),
    f("RadioIn", "ch9in", "RC input 9", "µs"),
    f("RadioIn", "ch10in", "RC input 10", "µs"),
    f("RadioIn", "ch11in", "RC input 11", "µs"),
    f("RadioIn", "ch12in", "RC input 12", "µs"),
    f("RadioIn", "ch13in", "RC input 13", "µs"),
    f("RadioIn", "ch14in", "RC input 14", "µs"),
    f("RadioIn", "ch15in", "RC input 15", "µs"),
    f("RadioIn", "ch16in", "RC input 16", "µs"),
    f("RadioIn", "rcrssi", "RC RSSI", ""),
    f("RadioOut", "ch1out", "Servo output 1", "µs"),
    f("RadioOut", "ch2out", "Servo output 2", "µs"),
    f("RadioOut", "ch3out", "Servo output 3", "µs"),
    f("RadioOut", "ch4out", "Servo output 4", "µs"),
    f("RadioOut", "ch5out", "Servo output 5", "µs"),
    f("RadioOut", "ch6out", "Servo output 6", "µs"),
    f("RadioOut", "ch7out", "Servo output 7", "µs"),
    f("RadioOut", "ch8out", "Servo output 8", "µs"),
    f("RadioOut", "ch9out", "Servo output 9", "µs"),
    f("RadioOut", "ch10out", "Servo output 10", "µs"),
    f("RadioOut", "ch11out", "Servo output 11", "µs"),
    f("RadioOut", "ch12out", "Servo output 12", "µs"),
    f("RadioOut", "ch13out", "Servo output 13", "µs"),
    f("RadioOut", "ch14out", "Servo output 14", "µs"),
    f("RadioOut", "ch15out", "Servo output 15", "µs"),
    f("RadioOut", "ch16out", "Servo output 16", "µs"),
    f("Terrain", "ter_curalt", "Terrain: height above", "m"),
    f("Terrain", "ter_alt", "Terrain height", "m"),
    f("Terrain", "ter_load", "Terrain tiles loaded", ""),
    f("Terrain", "ter_pend", "Terrain tiles pending", ""),
    f("Terrain", "ter_space", "Terrain grid spacing", "m"),
    f("Mount", "campointa", "Mount pitch", "°"),
    f("Mount", "campointb", "Mount roll", "°"),
    f("Mount", "campointc", "Mount yaw", "°"),
    f("Fence", "fenceb_status", "Fence breach status", ""),
    f("Fence", "fenceb_type", "Fence breach type", ""),
    f("Fence", "fenceb_count", "Fence breach count", ""),
    f("Generator", "gen_status", "Generator status", ""),
    f("Generator", "gen_speed", "Generator speed", "rpm"),
    f("Generator", "gen_current", "Generator current", "A"),
    f("Generator", "gen_voltage", "Generator voltage", "V"),
    f("Generator", "gen_runtime", "Generator runtime", "s"),
    f(
        "Generator",
        "gen_maint_time",
        "Generator time to maintenance",
        "s",
    ),
    f("EFI", "efi_rpm", "EFI RPM", "rpm"),
    f("EFI", "efi_load", "EFI engine load", "%"),
    f("EFI", "efi_baro", "EFI barometric pressure", "kPa"),
    f("EFI", "efi_headtemp", "EFI cylinder head temperature", "°C"),
    f("EFI", "efi_exhasttemp", "EFI exhaust temperature", "°C"),
    f("EFI", "efi_intaketemp", "EFI intake temperature", "°C"),
    f("EFI", "efi_fuelflow", "EFI fuel flow", "cm³/min"),
    f("EFI", "efi_fuelconsumed", "EFI fuel consumed", "cm³"),
    f("EFI", "efi_fuelpressure", "EFI fuel pressure", "kPa"),
    f("EFI", "efi_health", "EFI health", ""),
    f("ESC", "esc1_rpm", "ESC 1 RPM", "rpm"),
    f("ESC", "esc1_volt", "ESC 1 voltage", "V"),
    f("ESC", "esc1_curr", "ESC 1 current", "A"),
    f("ESC", "esc1_temp", "ESC 1 temperature", "°C"),
    f("ESC", "esc2_rpm", "ESC 2 RPM", "rpm"),
    f("ESC", "esc2_volt", "ESC 2 voltage", "V"),
    f("ESC", "esc2_curr", "ESC 2 current", "A"),
    f("ESC", "esc2_temp", "ESC 2 temperature", "°C"),
    f("ESC", "esc3_rpm", "ESC 3 RPM", "rpm"),
    f("ESC", "esc3_volt", "ESC 3 voltage", "V"),
    f("ESC", "esc3_curr", "ESC 3 current", "A"),
    f("ESC", "esc3_temp", "ESC 3 temperature", "°C"),
    f("ESC", "esc4_rpm", "ESC 4 RPM", "rpm"),
    f("ESC", "esc4_volt", "ESC 4 voltage", "V"),
    f("ESC", "esc4_curr", "ESC 4 current", "A"),
    f("ESC", "esc4_temp", "ESC 4 temperature", "°C"),
    f("ESC", "esc5_rpm", "ESC 5 RPM", "rpm"),
    f("ESC", "esc5_volt", "ESC 5 voltage", "V"),
    f("ESC", "esc5_curr", "ESC 5 current", "A"),
    f("ESC", "esc5_temp", "ESC 5 temperature", "°C"),
    f("ESC", "esc6_rpm", "ESC 6 RPM", "rpm"),
    f("ESC", "esc6_volt", "ESC 6 voltage", "V"),
    f("ESC", "esc6_curr", "ESC 6 current", "A"),
    f("ESC", "esc6_temp", "ESC 6 temperature", "°C"),
    f("ESC", "esc7_rpm", "ESC 7 RPM", "rpm"),
    f("ESC", "esc7_volt", "ESC 7 voltage", "V"),
    f("ESC", "esc7_curr", "ESC 7 current", "A"),
    f("ESC", "esc7_temp", "ESC 7 temperature", "°C"),
    f("ESC", "esc8_rpm", "ESC 8 RPM", "rpm"),
    f("ESC", "esc8_volt", "ESC 8 voltage", "V"),
    f("ESC", "esc8_curr", "ESC 8 current", "A"),
    f("ESC", "esc8_temp", "ESC 8 temperature", "°C"),
    f("ESC", "esc9_rpm", "ESC 9 RPM", "rpm"),
    f("ESC", "esc9_volt", "ESC 9 voltage", "V"),
    f("ESC", "esc9_curr", "ESC 9 current", "A"),
    f("ESC", "esc9_temp", "ESC 9 temperature", "°C"),
    f("ESC", "esc10_rpm", "ESC 10 RPM", "rpm"),
    f("ESC", "esc10_volt", "ESC 10 voltage", "V"),
    f("ESC", "esc10_curr", "ESC 10 current", "A"),
    f("ESC", "esc10_temp", "ESC 10 temperature", "°C"),
    f("ESC", "esc11_rpm", "ESC 11 RPM", "rpm"),
    f("ESC", "esc11_volt", "ESC 11 voltage", "V"),
    f("ESC", "esc11_curr", "ESC 11 current", "A"),
    f("ESC", "esc11_temp", "ESC 11 temperature", "°C"),
    f("ESC", "esc12_rpm", "ESC 12 RPM", "rpm"),
    f("ESC", "esc12_volt", "ESC 12 voltage", "V"),
    f("ESC", "esc12_curr", "ESC 12 current", "A"),
    f("ESC", "esc12_temp", "ESC 12 temperature", "°C"),
    f("ESC", "esc13_rpm", "ESC 13 RPM", "rpm"),
    f("ESC", "esc13_volt", "ESC 13 voltage", "V"),
    f("ESC", "esc13_curr", "ESC 13 current", "A"),
    f("ESC", "esc13_temp", "ESC 13 temperature", "°C"),
    f("ESC", "esc14_rpm", "ESC 14 RPM", "rpm"),
    f("ESC", "esc14_volt", "ESC 14 voltage", "V"),
    f("ESC", "esc14_curr", "ESC 14 current", "A"),
    f("ESC", "esc14_temp", "ESC 14 temperature", "°C"),
    f("ESC", "esc15_rpm", "ESC 15 RPM", "rpm"),
    f("ESC", "esc15_volt", "ESC 15 voltage", "V"),
    f("ESC", "esc15_curr", "ESC 15 current", "A"),
    f("ESC", "esc15_temp", "ESC 15 temperature", "°C"),
    f("ESC", "esc16_rpm", "ESC 16 RPM", "rpm"),
    f("ESC", "esc16_volt", "ESC 16 voltage", "V"),
    f("ESC", "esc16_curr", "ESC 16 current", "A"),
    f("ESC", "esc16_temp", "ESC 16 temperature", "°C"),
    f("Hardware", "boardvoltage", "Board voltage", "V"),
    f("Hardware", "servovoltage", "Servo rail voltage", "V"),
    f("Hardware", "hwvoltage", "HW status voltage", "V"),
    f("Hardware", "i2cerrors", "I2C errors", ""),
    f("Hardware", "mcutemp", "MCU temperature", "°C"),
    f("Hardware", "mcuvoltage", "MCU voltage", "V"),
    f("Hardware", "mcuminvolt", "MCU minimum voltage", "V"),
    f("Hardware", "mcumaxvolt", "MCU maximum voltage", "V"),
    f("Hardware", "load", "CPU load", "%"),
    f("Hardware", "errors_count1", "Internal error count 1", ""),
    f("Hardware", "errors_count2", "Internal error count 2", ""),
    f("Hardware", "errors_count3", "Internal error count 3", ""),
    f("Hardware", "errors_count4", "Internal error count 4", ""),
    f("PID", "pidaxis", "PID axis", ""),
    f("PID", "piddesired", "PID desired", ""),
    f("PID", "pidachieved", "PID achieved", ""),
    f("PID", "pidff", "PID FF", ""),
    f("PID", "pidP", "PID P", ""),
    f("PID", "pidI", "PID I", ""),
    f("PID", "pidD", "PID D", ""),
    f("PID", "pidSRate", "PID slew rate", ""),
    f("PID", "pidPDmod", "PID PD modifier", ""),
    f("PID", "pidSRateRoll", "PID slew rate roll", ""),
    f("PID", "pidSRatePitch", "PID slew rate pitch", ""),
    f("PID", "pidSRateYaw", "PID slew rate yaw", ""),
    f("PID", "pidSRateAccZ", "PID slew rate accel Z", ""),
    f("PID", "pidSRateSteer", "PID slew rate steering", ""),
    f("PID", "pidSRateLanding", "PID slew rate landing", ""),
    f("Flow", "opt_m_x", "Flow compensated X", "m/s"),
    f("Flow", "opt_m_y", "Flow compensated Y", "m/s"),
    f("Flow", "opt_x", "Flow X", "dpix"),
    f("Flow", "opt_y", "Flow Y", "dpix"),
    f("Flow", "opt_qua", "Flow quality", ""),
    f("Other", "armed", "Armed (1/0)", ""),
    f("Other", "landed_state", "Landed state", ""),
    f("Other", "vtol_state", "VTOL state", ""),
];

pub(crate) const CH_IN: [&str; 16] = [
    "ch1in", "ch2in", "ch3in", "ch4in", "ch5in", "ch6in", "ch7in", "ch8in", "ch9in", "ch10in",
    "ch11in", "ch12in", "ch13in", "ch14in", "ch15in", "ch16in",
];
pub(crate) const CH_OUT: [&str; 16] = [
    "ch1out", "ch2out", "ch3out", "ch4out", "ch5out", "ch6out", "ch7out", "ch8out", "ch9out",
    "ch10out", "ch11out", "ch12out", "ch13out", "ch14out", "ch15out", "ch16out",
];
pub(crate) const CELLS: [&str; 12] = [
    "battery_cell1",
    "battery_cell2",
    "battery_cell3",
    "battery_cell4",
    "battery_cell5",
    "battery_cell6",
    "battery_cell7",
    "battery_cell8",
    "battery_cell9",
    "battery_cell10",
    "battery_cell11",
    "battery_cell12",
];
pub(crate) const RANGEFINDERS: [&str; 10] = [
    "rangefinder1",
    "rangefinder2",
    "rangefinder3",
    "rangefinder4",
    "rangefinder5",
    "rangefinder6",
    "rangefinder7",
    "rangefinder8",
    "rangefinder9",
    "rangefinder10",
];
pub(crate) const BAT_VOLTAGE: [&str; 9] = [
    "battery_voltage",
    "battery_voltage2",
    "battery_voltage3",
    "battery_voltage4",
    "battery_voltage5",
    "battery_voltage6",
    "battery_voltage7",
    "battery_voltage8",
    "battery_voltage9",
];
pub(crate) const BAT_CURRENT: [&str; 9] = [
    "battery_current",
    "battery_current2",
    "battery_current3",
    "battery_current4",
    "battery_current5",
    "battery_current6",
    "battery_current7",
    "battery_current8",
    "battery_current9",
];
pub(crate) const BAT_REMAINING: [&str; 9] = [
    "battery_remaining",
    "battery_remaining2",
    "battery_remaining3",
    "battery_remaining4",
    "battery_remaining5",
    "battery_remaining6",
    "battery_remaining7",
    "battery_remaining8",
    "battery_remaining9",
];
pub(crate) const BAT_USEDMAH: [&str; 9] = [
    "battery_usedmah",
    "battery_usedmah2",
    "battery_usedmah3",
    "battery_usedmah4",
    "battery_usedmah5",
    "battery_usedmah6",
    "battery_usedmah7",
    "battery_usedmah8",
    "battery_usedmah9",
];
pub(crate) const BAT_TEMP: [&str; 9] = [
    "battery_temp",
    "battery_temp2",
    "battery_temp3",
    "battery_temp4",
    "battery_temp5",
    "battery_temp6",
    "battery_temp7",
    "battery_temp8",
    "battery_temp9",
];
pub(crate) const BAT_REMAINMIN: [&str; 9] = [
    "battery_remainmin",
    "battery_remainmin2",
    "battery_remainmin3",
    "battery_remainmin4",
    "battery_remainmin5",
    "battery_remainmin6",
    "battery_remainmin7",
    "battery_remainmin8",
    "battery_remainmin9",
];
pub(crate) const ESC_RPM: [&str; 16] = [
    "esc1_rpm",
    "esc2_rpm",
    "esc3_rpm",
    "esc4_rpm",
    "esc5_rpm",
    "esc6_rpm",
    "esc7_rpm",
    "esc8_rpm",
    "esc9_rpm",
    "esc10_rpm",
    "esc11_rpm",
    "esc12_rpm",
    "esc13_rpm",
    "esc14_rpm",
    "esc15_rpm",
    "esc16_rpm",
];
pub(crate) const ESC_VOLT: [&str; 16] = [
    "esc1_volt",
    "esc2_volt",
    "esc3_volt",
    "esc4_volt",
    "esc5_volt",
    "esc6_volt",
    "esc7_volt",
    "esc8_volt",
    "esc9_volt",
    "esc10_volt",
    "esc11_volt",
    "esc12_volt",
    "esc13_volt",
    "esc14_volt",
    "esc15_volt",
    "esc16_volt",
];
pub(crate) const ESC_CURR: [&str; 16] = [
    "esc1_curr",
    "esc2_curr",
    "esc3_curr",
    "esc4_curr",
    "esc5_curr",
    "esc6_curr",
    "esc7_curr",
    "esc8_curr",
    "esc9_curr",
    "esc10_curr",
    "esc11_curr",
    "esc12_curr",
    "esc13_curr",
    "esc14_curr",
    "esc15_curr",
    "esc16_curr",
];
pub(crate) const ESC_TEMP: [&str; 16] = [
    "esc1_temp",
    "esc2_temp",
    "esc3_temp",
    "esc4_temp",
    "esc5_temp",
    "esc6_temp",
    "esc7_temp",
    "esc8_temp",
    "esc9_temp",
    "esc10_temp",
    "esc11_temp",
    "esc12_temp",
    "esc13_temp",
    "esc14_temp",
    "esc15_temp",
    "esc16_temp",
];
pub(crate) const IMU1: [&str; 9] = ["ax", "ay", "az", "gx", "gy", "gz", "mx", "my", "mz"];
pub(crate) const IMU2: [&str; 9] = [
    "ax2", "ay2", "az2", "gx2", "gy2", "gz2", "mx2", "my2", "mz2",
];
pub(crate) const IMU3: [&str; 9] = [
    "ax3", "ay3", "az3", "gx3", "gy3", "gz3", "mx3", "my3", "mz3",
];

/// MAV_SENSOR_ROTATION_PITCH_270: a rangefinder looking down.
const ORIENTATION_DOWN: u32 = 25;

fn cm(value: u16) -> Option<f64> {
    (value != u16::MAX).then(|| f64::from(value) / 100.0)
}

fn milli(value: u32) -> Option<f64> {
    (value != u32::MAX && value != 0).then(|| f64::from(value) / 1000.0)
}

/// GPS fields shared by GPS_RAW_INT and GPS2_RAW, in legacy key order.
struct GpsKeys {
    status: &'static str,
    sats: &'static str,
    hdop: &'static str,
    lat: &'static str,
    lng: &'static str,
    course: &'static str,
    h_acc: &'static str,
    v_acc: &'static str,
    vel_acc: &'static str,
    hdg_acc: &'static str,
    yaw: &'static str,
}

const GPS1: GpsKeys = GpsKeys {
    status: "gpsstatus",
    sats: "satcount",
    hdop: "gpshdop",
    lat: "lat",
    lng: "lng",
    course: "groundcourse",
    h_acc: "gpsh_acc",
    v_acc: "gpsv_acc",
    vel_acc: "gpsvel_acc",
    hdg_acc: "gpshdg_acc",
    yaw: "gpsyaw",
};

const GPS2: GpsKeys = GpsKeys {
    status: "gpsstatus2",
    sats: "satcount2",
    hdop: "gpshdop2",
    lat: "lat2",
    lng: "lng2",
    course: "groundcourse2",
    h_acc: "gpsh_acc2",
    v_acc: "gpsv_acc2",
    vel_acc: "gpsvel_acc2",
    hdg_acc: "gpshdg_acc2",
    yaw: "gpsyaw2",
};

/// The raw values of one GPS message, before unit conversion.
struct GpsRaw {
    fix_type: u8,
    satellites: u8,
    eph: u16,
    lat: i32,
    lon: i32,
    cog: u16,
    h_acc: u32,
    v_acc: u32,
    vel_acc: u32,
    hdg_acc: u32,
    yaw: u16,
}

impl VehicleState {
    /// Fills the Quick View numbers carried by `message`. Position, home, mode and status text
    /// are handled by `apply`; this only touches the field map. BATTERY2 and HWSTATUS are
    /// deprecated in the dialect but still sent by older (BATTERY2) and current (HWSTATUS)
    /// ArduPilot firmwares, so they are decoded anyway.
    #[allow(deprecated)]
    pub(crate) fn decode_fields(&mut self, message: &MavMessage, now: Instant) {
        match message {
            MavMessage::ATTITUDE(a) => {
                self.set("roll", f64::from(a.roll).to_degrees());
                self.set("pitch", f64::from(a.pitch).to_degrees());
                self.set("yaw", f64::from(a.yaw).to_degrees().rem_euclid(360.0));
                self.set("turnrate", f64::from(a.yawspeed).to_degrees());
            }
            MavMessage::GPS_RAW_INT(g) => self.gps(
                &GPS1,
                &GpsRaw {
                    fix_type: g.fix_type as u8,
                    satellites: g.satellites_visible,
                    eph: g.eph,
                    lat: g.lat,
                    lon: g.lon,
                    cog: g.cog,
                    h_acc: g.h_acc,
                    v_acc: g.v_acc,
                    vel_acc: g.vel_acc,
                    hdg_acc: g.hdg_acc,
                    yaw: g.yaw,
                },
            ),
            MavMessage::GPS2_RAW(g) => {
                self.gps(
                    &GPS2,
                    &GpsRaw {
                        fix_type: g.fix_type as u8,
                        satellites: g.satellites_visible,
                        eph: g.eph,
                        lat: g.lat,
                        lon: g.lon,
                        cog: g.cog,
                        h_acc: g.h_acc,
                        v_acc: g.v_acc,
                        vel_acc: g.vel_acc,
                        hdg_acc: g.hdg_acc,
                        yaw: g.yaw,
                    },
                );
                self.set("altasl2", f64::from(g.alt) / 1000.0);
                if let Some(speed) = cm(g.vel) {
                    self.set("groundspeed2", speed);
                }
            }
            MavMessage::VFR_HUD(v) => {
                self.set("airspeed", f64::from(v.airspeed));
                self.set("groundspeed", f64::from(v.groundspeed));
                self.set("throttle", f64::from(v.throttle));
                self.set("heading", f64::from(v.heading));
                self.set("climbrate", f64::from(v.climb));
                self.track_flight(now, f64::from(v.groundspeed), f64::from(v.throttle));
            }
            MavMessage::SYS_STATUS(s) => {
                if s.voltage_battery != u16::MAX {
                    self.set("battery_voltage", f64::from(s.voltage_battery) / 1000.0);
                }
                if s.current_battery >= 0 {
                    self.set("battery_current", f64::from(s.current_battery) / 100.0);
                }
                if s.battery_remaining >= 0 {
                    self.set("battery_remaining", f64::from(s.battery_remaining));
                }
                self.set("load", f64::from(s.load) / 10.0);
                self.set("packetdropremote", f64::from(s.drop_rate_comm) / 100.0);
                for (key, value) in [
                    "errors_count1",
                    "errors_count2",
                    "errors_count3",
                    "errors_count4",
                ]
                .into_iter()
                .zip([
                    s.errors_count1,
                    s.errors_count2,
                    s.errors_count3,
                    s.errors_count4,
                ]) {
                    self.set(key, f64::from(value));
                }
            }
            MavMessage::BATTERY_STATUS(b) => self.battery_status(b),
            MavMessage::BATTERY2(b) => {
                if b.voltage != u16::MAX {
                    self.set(BAT_VOLTAGE[1], f64::from(b.voltage) / 1000.0);
                }
                if b.current_battery >= 0 {
                    self.set(BAT_CURRENT[1], f64::from(b.current_battery) / 100.0);
                }
            }
            MavMessage::POWER_STATUS(p) => {
                self.set("boardvoltage", f64::from(p.Vcc) / 1000.0);
                self.set("servovoltage", f64::from(p.Vservo) / 1000.0);
            }
            MavMessage::HWSTATUS(h) => {
                self.set("hwvoltage", f64::from(h.Vcc) / 1000.0);
                self.set("i2cerrors", f64::from(h.I2Cerr));
            }
            MavMessage::MCU_STATUS(m) => {
                self.set("mcutemp", f64::from(m.MCU_temperature) / 100.0);
                self.set("mcuvoltage", f64::from(m.MCU_voltage) / 1000.0);
                self.set("mcuminvolt", f64::from(m.MCU_voltage_min) / 1000.0);
                self.set("mcumaxvolt", f64::from(m.MCU_voltage_max) / 1000.0);
            }
            MavMessage::EKF_STATUS_REPORT(e) => {
                let variances = [
                    e.velocity_variance,
                    e.pos_horiz_variance,
                    e.pos_vert_variance,
                    e.compass_variance,
                    e.terrain_alt_variance,
                ];
                self.set("ekfvelv", f64::from(e.velocity_variance));
                self.set("ekfposhor", f64::from(e.pos_horiz_variance));
                self.set("ekfposvert", f64::from(e.pos_vert_variance));
                self.set("ekfcompv", f64::from(e.compass_variance));
                self.set("ekfteralt", f64::from(e.terrain_alt_variance));
                self.set("ekfflags", f64::from(e.flags.bits()));
                let worst = variances.into_iter().fold(0.0f32, f32::max);
                self.set("ekfstatus", f64::from(worst));
            }
            MavMessage::NAV_CONTROLLER_OUTPUT(n) => {
                self.set("wp_dist", f64::from(n.wp_dist));
                self.set("nav_bearing", f64::from(n.nav_bearing));
                self.set("target_bearing", f64::from(n.target_bearing));
                self.set("nav_roll", f64::from(n.nav_roll));
                self.set("nav_pitch", f64::from(n.nav_pitch));
                self.set("alt_error", f64::from(n.alt_error));
                self.set("aspd_error", f64::from(n.aspd_error));
                self.set("xtrack_error", f64::from(n.xtrack_error));
            }
            MavMessage::MISSION_CURRENT(m) => self.set("wpno", f64::from(m.seq)),
            MavMessage::GLOBAL_POSITION_INT(p) => {
                self.set("vx", f64::from(p.vx) / 100.0);
                self.set("vy", f64::from(p.vy) / 100.0);
                self.set("vz", f64::from(p.vz) / 100.0);
            }
            MavMessage::EXTENDED_SYS_STATE(e) => {
                self.set("landed_state", f64::from(e.landed_state as u8));
                self.set("vtol_state", f64::from(e.vtol_state as u8));
            }
            MavMessage::OPTICAL_FLOW(o) => {
                self.set("opt_m_x", f64::from(o.flow_comp_m_x));
                self.set("opt_m_y", f64::from(o.flow_comp_m_y));
                self.set("opt_x", f64::from(o.flow_x));
                self.set("opt_y", f64::from(o.flow_y));
                self.set("opt_qua", f64::from(o.quality));
            }
            MavMessage::PID_TUNING(p) => {
                let axis = p.axis as u8;
                self.set("pidaxis", f64::from(axis));
                self.set("piddesired", f64::from(p.desired));
                self.set("pidachieved", f64::from(p.achieved));
                self.set("pidff", f64::from(p.FF));
                self.set("pidP", f64::from(p.P));
                self.set("pidI", f64::from(p.I));
                self.set("pidD", f64::from(p.D));
                self.set("pidSRate", f64::from(p.SRate));
                self.set("pidPDmod", f64::from(p.PDmod));
                // PID_TUNING_AXIS: 1 roll, 2 pitch, 3 yaw, 4 accel Z, 5 steer, 6 landing.
                let per_axis = match axis {
                    1 => Some("pidSRateRoll"),
                    2 => Some("pidSRatePitch"),
                    3 => Some("pidSRateYaw"),
                    4 => Some("pidSRateAccZ"),
                    5 => Some("pidSRateSteer"),
                    6 => Some("pidSRateLanding"),
                    _ => None,
                };
                if let Some(key) = per_axis {
                    self.set(key, f64::from(p.SRate));
                }
            }
            MavMessage::VIBRATION(v) => {
                self.set("vibex", f64::from(v.vibration_x));
                self.set("vibey", f64::from(v.vibration_y));
                self.set("vibez", f64::from(v.vibration_z));
                self.set("vibeclip0", f64::from(v.clipping_0));
                self.set("vibeclip1", f64::from(v.clipping_1));
                self.set("vibeclip2", f64::from(v.clipping_2));
            }
            MavMessage::WIND(w) => {
                self.set("wind_dir", f64::from(w.direction).rem_euclid(360.0));
                self.set("wind_vel", f64::from(w.speed));
            }
            MavMessage::RANGEFINDER(r) => {
                self.set("sonarrange", f64::from(r.distance));
                self.set("sonarvoltage", f64::from(r.voltage));
            }
            MavMessage::DISTANCE_SENSOR(d) => {
                let metres = f64::from(d.current_distance) / 100.0;
                if let Some(key) = RANGEFINDERS.get(usize::from(d.id)) {
                    self.set(key, metres);
                }
                if d.orientation as u32 == ORIENTATION_DOWN {
                    self.set("sonarrange", metres);
                }
            }
            MavMessage::RADIO_STATUS(r) => {
                self.set("rssi", f64::from(r.rssi));
                self.set("remrssi", f64::from(r.remrssi));
                self.set("noise", f64::from(r.noise));
                self.set("remnoise", f64::from(r.remnoise));
                self.set("txbuffer", f64::from(r.txbuf));
                self.set("rxerrors", f64::from(r.rxerrors));
                self.set("fixedp", f64::from(r.fixed));
            }
            MavMessage::SCALED_PRESSURE(p) => {
                self.set("press_abs", f64::from(p.press_abs));
                self.set("press_temp", f64::from(p.temperature) / 100.0);
            }
            MavMessage::SCALED_PRESSURE2(p) => {
                self.set("press_abs2", f64::from(p.press_abs));
                self.set("press_temp2", f64::from(p.temperature) / 100.0);
            }
            MavMessage::TERRAIN_REPORT(t) => {
                self.set("ter_curalt", f64::from(t.current_height));
                self.set("ter_alt", f64::from(t.terrain_height));
                self.set("ter_load", f64::from(t.loaded));
                self.set("ter_pend", f64::from(t.pending));
                self.set("ter_space", f64::from(t.spacing));
            }
            MavMessage::AHRS2(a) => {
                self.set("ahrs2_roll", f64::from(a.roll).to_degrees());
                self.set("ahrs2_pitch", f64::from(a.pitch).to_degrees());
                self.set("ahrs2_yaw", f64::from(a.yaw).to_degrees().rem_euclid(360.0));
                self.set("ahrs2_alt", f64::from(a.altitude));
                self.set("ahrs2_lat", f64::from(a.lat) / 1e7);
                self.set("ahrs2_lng", f64::from(a.lng) / 1e7);
            }
            MavMessage::RPM(r) => {
                self.set("rpm1", f64::from(r.rpm1));
                self.set("rpm2", f64::from(r.rpm2));
            }
            MavMessage::FENCE_STATUS(fs) => {
                self.set("fenceb_status", f64::from(fs.breach_status));
                self.set("fenceb_type", f64::from(fs.breach_type as u8));
                self.set("fenceb_count", f64::from(fs.breach_count));
            }
            MavMessage::GENERATOR_STATUS(g) => {
                self.set("gen_status", g.status.bits() as f64);
                self.set("gen_speed", f64::from(g.generator_speed));
                self.set("gen_current", f64::from(g.load_current));
                self.set("gen_voltage", f64::from(g.bus_voltage));
                self.set("gen_runtime", f64::from(g.runtime));
                self.set("gen_maint_time", f64::from(g.time_until_maintenance));
            }
            MavMessage::EFI_STATUS(e) if e.ecu_index == 0.0 => {
                self.set("efi_rpm", f64::from(e.rpm));
                self.set("efi_load", f64::from(e.engine_load));
                self.set("efi_baro", f64::from(e.barometric_pressure));
                self.set("efi_headtemp", f64::from(e.cylinder_head_temperature));
                self.set("efi_exhasttemp", f64::from(e.exhaust_gas_temperature));
                self.set("efi_intaketemp", f64::from(e.intake_manifold_temperature));
                self.set("efi_fuelflow", f64::from(e.fuel_flow));
                self.set("efi_fuelconsumed", f64::from(e.fuel_consumed));
                self.set("efi_fuelpressure", f64::from(e.fuel_pressure));
                self.set("efi_health", f64::from(e.health));
            }
            MavMessage::MOUNT_STATUS(m) => {
                self.set("campointa", f64::from(m.pointing_a) / 100.0);
                self.set("campointb", f64::from(m.pointing_b) / 100.0);
                self.set("campointc", f64::from(m.pointing_c) / 100.0);
            }
            MavMessage::RAW_IMU(i) => {
                let keys = match i.id {
                    0 => &IMU1,
                    1 => &IMU2,
                    2 => &IMU3,
                    _ => return,
                };
                self.imu(
                    keys,
                    [
                        i.xacc, i.yacc, i.zacc, i.xgyro, i.ygyro, i.zgyro, i.xmag, i.ymag, i.zmag,
                    ],
                );
                if i.id == 0 && i.temperature != 0 {
                    self.set("imu_temp", f64::from(i.temperature) / 100.0);
                }
            }
            MavMessage::SCALED_IMU2(i) => self.imu(
                &IMU2,
                [
                    i.xacc, i.yacc, i.zacc, i.xgyro, i.ygyro, i.zgyro, i.xmag, i.ymag, i.zmag,
                ],
            ),
            MavMessage::SCALED_IMU3(i) => self.imu(
                &IMU3,
                [
                    i.xacc, i.yacc, i.zacc, i.xgyro, i.ygyro, i.zgyro, i.xmag, i.ymag, i.zmag,
                ],
            ),
            MavMessage::RC_CHANNELS(r) => {
                let raw = [
                    r.chan1_raw,
                    r.chan2_raw,
                    r.chan3_raw,
                    r.chan4_raw,
                    r.chan5_raw,
                    r.chan6_raw,
                    r.chan7_raw,
                    r.chan8_raw,
                    r.chan9_raw,
                    r.chan10_raw,
                    r.chan11_raw,
                    r.chan12_raw,
                    r.chan13_raw,
                    r.chan14_raw,
                    r.chan15_raw,
                    r.chan16_raw,
                ];
                for (key, value) in CH_IN.iter().zip(raw) {
                    if value != u16::MAX {
                        self.set(key, f64::from(value));
                    }
                }
                if r.rssi != u8::MAX {
                    self.set("rcrssi", f64::from(r.rssi));
                }
            }
            MavMessage::SERVO_OUTPUT_RAW(s) => {
                let raw = [
                    s.servo1_raw,
                    s.servo2_raw,
                    s.servo3_raw,
                    s.servo4_raw,
                    s.servo5_raw,
                    s.servo6_raw,
                    s.servo7_raw,
                    s.servo8_raw,
                    s.servo9_raw,
                    s.servo10_raw,
                    s.servo11_raw,
                    s.servo12_raw,
                    s.servo13_raw,
                    s.servo14_raw,
                    s.servo15_raw,
                    s.servo16_raw,
                ];
                // Port 1 carries outputs 9..16 on firmwares without the extension fields.
                let offset = if s.port == 1 { 8 } else { 0 };
                for (index, value) in raw.into_iter().enumerate() {
                    // Extension fields are zero when the sender does not fill them.
                    if index >= 8 && value == 0 {
                        continue;
                    }
                    if let Some(key) = CH_OUT.get(index + offset) {
                        self.set(key, f64::from(value));
                    }
                }
            }
            MavMessage::ESC_TELEMETRY_1_TO_4(e) => {
                self.esc(0, e.rpm, e.voltage, e.current, e.temperature)
            }
            MavMessage::ESC_TELEMETRY_5_TO_8(e) => {
                self.esc(4, e.rpm, e.voltage, e.current, e.temperature)
            }
            MavMessage::ESC_TELEMETRY_9_TO_12(e) => {
                self.esc(8, e.rpm, e.voltage, e.current, e.temperature)
            }
            MavMessage::ESC_TELEMETRY_13_TO_16(e) => {
                self.esc(12, e.rpm, e.voltage, e.current, e.temperature)
            }
            _ => {}
        }
    }

    fn gps(&mut self, keys: &GpsKeys, raw: &GpsRaw) {
        self.set(keys.status, f64::from(raw.fix_type));
        if raw.satellites != u8::MAX {
            self.set(keys.sats, f64::from(raw.satellites));
        }
        if let Some(hdop) = cm(raw.eph) {
            self.set(keys.hdop, hdop);
        }
        if raw.lat != 0 || raw.lon != 0 {
            self.set(keys.lat, f64::from(raw.lat) / 1e7);
            self.set(keys.lng, f64::from(raw.lon) / 1e7);
        }
        if let Some(course) = cm(raw.cog) {
            self.set(keys.course, course);
        }
        if let Some(acc) = milli(raw.h_acc) {
            self.set(keys.h_acc, acc);
        }
        if let Some(acc) = milli(raw.v_acc) {
            self.set(keys.v_acc, acc);
        }
        if let Some(acc) = milli(raw.vel_acc) {
            self.set(keys.vel_acc, acc);
        }
        if raw.hdg_acc != 0 && raw.hdg_acc != u32::MAX {
            self.set(keys.hdg_acc, f64::from(raw.hdg_acc) / 1e5);
        }
        // 0 means "no yaw"; north is sent as 36000.
        if raw.yaw != 0 && raw.yaw != u16::MAX {
            self.set(keys.yaw, (f64::from(raw.yaw) / 100.0).rem_euclid(360.0));
        }
    }

    fn battery_status(&mut self, b: &control_station_mavlink::dialect::BATTERY_STATUS_DATA) {
        let index = usize::from(b.id);
        if index >= BAT_VOLTAGE.len() {
            return;
        }
        let cells: Vec<u16> = b
            .voltages
            .iter()
            .chain(b.voltages_ext.iter())
            .copied()
            .filter(|v| *v != u16::MAX && *v != 0)
            .collect();
        if !cells.is_empty() {
            let total: f64 = cells.iter().map(|v| f64::from(*v)).sum::<f64>() / 1000.0;
            self.set(BAT_VOLTAGE[index], total);
        }
        if index == 0 && cells.len() > 1 {
            for (key, value) in CELLS.iter().zip(&cells) {
                self.set(key, f64::from(*value) / 1000.0);
            }
        }
        if b.current_battery >= 0 {
            self.set(BAT_CURRENT[index], f64::from(b.current_battery) / 100.0);
        }
        if b.battery_remaining >= 0 {
            self.set(BAT_REMAINING[index], f64::from(b.battery_remaining));
        }
        if b.current_consumed >= 0 {
            self.set(BAT_USEDMAH[index], f64::from(b.current_consumed));
        }
        if b.temperature != i16::MAX {
            self.set(BAT_TEMP[index], f64::from(b.temperature) / 100.0);
        }
        if b.time_remaining > 0 {
            self.set(BAT_REMAINMIN[index], f64::from(b.time_remaining) / 60.0);
        }
    }

    fn imu(&mut self, keys: &[&'static str; 9], values: [i16; 9]) {
        for (key, value) in keys.iter().zip(values) {
            self.set(key, f64::from(value));
        }
    }

    fn esc(&mut self, first: usize, rpm: [u16; 4], volt: [u16; 4], curr: [u16; 4], temp: [u8; 4]) {
        for i in 0..4 {
            let n = first + i;
            if n >= ESC_RPM.len() {
                return;
            }
            self.set(ESC_RPM[n], f64::from(rpm[i]));
            self.set(ESC_VOLT[n], f64::from(volt[i]) / 100.0);
            self.set(ESC_CURR[n], f64::from(curr[i]) / 100.0);
            self.set(ESC_TEMP[n], f64::from(temp[i]));
        }
    }

    /// Time in air and distance traveled, accumulated like the legacy GCS: while armed and
    /// moving (ground speed over 3 m/s) or with throttle above 12 %.
    fn track_flight(&mut self, now: Instant, groundspeed: f64, throttle: f64) {
        let last = self.last_flight_tick.replace(now);
        self.set("armed", if self.armed { 1.0 } else { 0.0 });
        let Some(last) = last else { return };
        let dt = now.saturating_duration_since(last).as_secs_f64();
        if !self.armed || dt > 5.0 {
            return;
        }
        if groundspeed > 3.0 || throttle > 12.0 {
            let air = self.fields.get("timeInAir").copied().unwrap_or(0.0);
            self.set("timeInAir", air + dt);
        }
        let traveled = self.fields.get("distTraveled").copied().unwrap_or(0.0);
        self.set("distTraveled", traveled + groundspeed * dt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VehicleId;
    use control_station_mavlink::dialect::{
        GpsFixType, BATTERY_STATUS_DATA, DISTANCE_SENSOR_DATA, EKF_STATUS_REPORT_DATA,
        ESC_TELEMETRY_5_TO_8_DATA, GPS2_RAW_DATA, GPS_RAW_INT_DATA, RC_CHANNELS_DATA,
        SERVO_OUTPUT_RAW_DATA, WIND_DATA,
    };

    fn vehicle() -> VehicleState {
        VehicleState::new(VehicleId {
            system_id: 1,
            component_id: 1,
        })
    }

    fn feed(v: &mut VehicleState, message: MavMessage) {
        v.apply(&message, Instant::now(), 0);
    }

    fn close(v: &VehicleState, key: &str, expected: f64) {
        let value = v.fields().get(key).copied();
        assert!(
            value.is_some_and(|x| (x - expected).abs() < 1e-6),
            "{key}: {value:?} != {expected}"
        );
    }

    #[test]
    fn second_gps_has_its_own_fields() {
        let mut v = vehicle();
        feed(
            &mut v,
            MavMessage::GPS_RAW_INT(GPS_RAW_INT_DATA {
                fix_type: GpsFixType::GPS_FIX_TYPE_3D_FIX,
                satellites_visible: 12,
                eph: 80,
                ..Default::default()
            }),
        );
        feed(
            &mut v,
            MavMessage::GPS2_RAW(GPS2_RAW_DATA {
                fix_type: GpsFixType::GPS_FIX_TYPE_RTK_FIXED,
                satellites_visible: 21,
                eph: 55,
                lat: -219_200_000,
                lon: -479_300_000,
                alt: 850_500,
                vel: 250,
                cog: 9_000,
                yaw: 36_000,
                ..Default::default()
            }),
        );
        close(&v, "satcount", 12.0);
        close(&v, "gpsstatus", 3.0);
        close(&v, "satcount2", 21.0);
        close(&v, "gpsstatus2", 6.0);
        close(&v, "gpshdop2", 0.55);
        close(&v, "lat2", -21.92);
        close(&v, "altasl2", 850.5);
        close(&v, "groundspeed2", 2.5);
        close(&v, "groundcourse2", 90.0);
        close(&v, "gpsyaw2", 0.0);
        // GPS_RAW_INT sent no position or yaw: those keys stay absent.
        assert!(!v.fields().contains_key("lat"));
        assert!(!v.fields().contains_key("gpsyaw"));
    }

    #[test]
    fn battery_status_is_routed_by_id() {
        let mut v = vehicle();
        let mut voltages = [u16::MAX; 10];
        voltages[0] = 4_100;
        voltages[1] = 4_000;
        feed(
            &mut v,
            MavMessage::BATTERY_STATUS(BATTERY_STATUS_DATA {
                id: 0,
                voltages,
                current_battery: 1_250,
                current_consumed: 320,
                battery_remaining: 76,
                temperature: i16::MAX,
                ..Default::default()
            }),
        );
        let mut second = [u16::MAX; 10];
        second[0] = 22_200;
        feed(
            &mut v,
            MavMessage::BATTERY_STATUS(BATTERY_STATUS_DATA {
                id: 1,
                voltages: second,
                current_battery: -1,
                battery_remaining: 40,
                temperature: 3_150,
                time_remaining: 600,
                ..Default::default()
            }),
        );
        close(&v, "battery_voltage", 8.1);
        close(&v, "battery_cell1", 4.1);
        close(&v, "battery_cell2", 4.0);
        close(&v, "battery_current", 12.5);
        close(&v, "battery_usedmah", 320.0);
        assert!(!v.fields().contains_key("battery_temp"));
        close(&v, "battery_voltage2", 22.2);
        close(&v, "battery_remaining2", 40.0);
        close(&v, "battery_temp2", 31.5);
        close(&v, "battery_remainmin2", 10.0);
        assert!(!v.fields().contains_key("battery_current2"));
    }

    #[test]
    fn servo_outputs_above_eight_and_rc_inputs_up_to_sixteen() {
        let mut v = vehicle();
        feed(
            &mut v,
            MavMessage::SERVO_OUTPUT_RAW(SERVO_OUTPUT_RAW_DATA {
                servo1_raw: 1_100,
                servo9_raw: 1_900,
                ..Default::default()
            }),
        );
        close(&v, "ch1out", 1100.0);
        close(&v, "ch9out", 1900.0);
        assert!(!v.fields().contains_key("ch10out"));
        feed(
            &mut v,
            MavMessage::SERVO_OUTPUT_RAW(SERVO_OUTPUT_RAW_DATA {
                port: 1,
                servo2_raw: 1_500,
                ..Default::default()
            }),
        );
        close(&v, "ch10out", 1500.0);
        feed(
            &mut v,
            MavMessage::RC_CHANNELS(RC_CHANNELS_DATA {
                chan16_raw: 2_000,
                chan15_raw: u16::MAX,
                rssi: 200,
                ..Default::default()
            }),
        );
        close(&v, "ch16in", 2000.0);
        close(&v, "rcrssi", 200.0);
        assert!(!v.fields().contains_key("ch15in"));
    }

    #[test]
    fn ekf_wind_rangefinder_and_esc() {
        let mut v = vehicle();
        feed(
            &mut v,
            MavMessage::EKF_STATUS_REPORT(EKF_STATUS_REPORT_DATA {
                velocity_variance: 0.2,
                compass_variance: 0.6,
                ..Default::default()
            }),
        );
        close(&v, "ekfstatus", f64::from(0.6f32));
        feed(
            &mut v,
            MavMessage::WIND(WIND_DATA {
                direction: -90.0,
                speed: 4.0,
                ..Default::default()
            }),
        );
        close(&v, "wind_dir", 270.0);
        feed(
            &mut v,
            MavMessage::DISTANCE_SENSOR(DISTANCE_SENSOR_DATA {
                id: 1,
                current_distance: 345,
                orientation:
                    control_station_mavlink::dialect::MavSensorOrientation::MAV_SENSOR_ROTATION_PITCH_270,
                ..Default::default()
            }),
        );
        close(&v, "rangefinder2", 3.45);
        close(&v, "sonarrange", 3.45);
        feed(
            &mut v,
            MavMessage::ESC_TELEMETRY_5_TO_8(ESC_TELEMETRY_5_TO_8_DATA {
                rpm: [0, 0, 0, 5_000],
                voltage: [0, 0, 0, 2_520],
                current: [0, 0, 0, 1_050],
                temperature: [0, 0, 0, 45],
                ..Default::default()
            }),
        );
        close(&v, "esc8_rpm", 5000.0);
        close(&v, "esc8_volt", 25.2);
        close(&v, "esc8_curr", 10.5);
        close(&v, "esc8_temp", 45.0);
    }

    #[test]
    fn every_decoded_key_is_in_the_table() {
        let mut v = vehicle();
        let all: [MavMessage; 9] = [
            MavMessage::GPS_RAW_INT(GPS_RAW_INT_DATA {
                lat: 1,
                h_acc: 1,
                v_acc: 1,
                vel_acc: 1,
                hdg_acc: 1,
                yaw: 1,
                ..Default::default()
            }),
            MavMessage::GPS2_RAW(GPS2_RAW_DATA {
                lat: 1,
                h_acc: 1,
                v_acc: 1,
                vel_acc: 1,
                hdg_acc: 1,
                yaw: 1,
                ..Default::default()
            }),
            MavMessage::BATTERY_STATUS(BATTERY_STATUS_DATA {
                voltages: [3_000; 10],
                voltages_ext: [3_000; 4],
                time_remaining: 1,
                ..Default::default()
            }),
            MavMessage::SERVO_OUTPUT_RAW(SERVO_OUTPUT_RAW_DATA {
                servo16_raw: 1,
                ..Default::default()
            }),
            MavMessage::RC_CHANNELS(RC_CHANNELS_DATA::default()),
            MavMessage::EKF_STATUS_REPORT(EKF_STATUS_REPORT_DATA::default()),
            MavMessage::DISTANCE_SENSOR(DISTANCE_SENSOR_DATA {
                id: 9,
                ..Default::default()
            }),
            MavMessage::ESC_TELEMETRY_13_TO_16(Default::default()),
            MavMessage::RAW_IMU(Default::default()),
        ];
        for message in all {
            feed(&mut v, message);
        }
        for message in [
            MavMessage::SCALED_IMU3(Default::default()),
            MavMessage::AHRS2(Default::default()),
            MavMessage::TERRAIN_REPORT(Default::default()),
            MavMessage::GENERATOR_STATUS(Default::default()),
            MavMessage::EFI_STATUS(Default::default()),
            MavMessage::MOUNT_STATUS(Default::default()),
            MavMessage::FENCE_STATUS(Default::default()),
            MavMessage::RADIO_STATUS(Default::default()),
            MavMessage::SCALED_PRESSURE2(Default::default()),
            MavMessage::MCU_STATUS(Default::default()),
            MavMessage::POWER_STATUS(Default::default()),
            MavMessage::SYS_STATUS(Default::default()),
            MavMessage::VFR_HUD(Default::default()),
            MavMessage::VFR_HUD(Default::default()),
            MavMessage::ATTITUDE(Default::default()),
            MavMessage::VIBRATION(Default::default()),
            MavMessage::RPM(Default::default()),
            MavMessage::GLOBAL_POSITION_INT(Default::default()),
            MavMessage::EXTENDED_SYS_STATE(Default::default()),
            MavMessage::OPTICAL_FLOW(Default::default()),
            MavMessage::PID_TUNING(control_station_mavlink::dialect::PID_TUNING_DATA {
                axis: control_station_mavlink::dialect::PidTuningAxis::PID_TUNING_YAW,
                ..Default::default()
            }),
            MavMessage::RANGEFINDER(Default::default()),
            MavMessage::WIND(Default::default()),
            MavMessage::NAV_CONTROLLER_OUTPUT(Default::default()),
        ] {
            feed(&mut v, message);
        }
        let known: std::collections::HashSet<_> = FIELDS.iter().map(|f| f.key).collect();
        for key in v.fields().keys() {
            assert!(known.contains(key), "{key} is decoded but not offered");
        }
        assert!(v.fields().len() > 150, "{}", v.fields().len());
    }

    #[test]
    fn every_field_has_a_group() {
        assert!(FIELDS.iter().all(|f| !f.group.is_empty()));
    }
}
