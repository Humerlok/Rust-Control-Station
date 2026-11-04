import type { ParamEntry, ParamMeta } from "./types";

// Deterministic fake Copter parameter table + metadata for the browser mock backend.

type Seed = [name: string, value: number, type?: string];

const REAL32 = "REAL32";
const params: Seed[] = [];
const add = (name: string, value: number, type = REAL32) => params.push([name, value, type]);

// Names used by Extended Tuning (current-generation names).
for (const [axis, p, i, d, flte] of [
  ["RLL", 0.135, 0.135, 0.0036, 0],
  ["PIT", 0.135, 0.135, 0.0036, 0],
  ["YAW", 0.18, 0.018, 0, 2.5],
] as const) {
  add(`ATC_RAT_${axis}_P`, p);
  add(`ATC_RAT_${axis}_I`, i);
  add(`ATC_RAT_${axis}_D`, d);
  add(`ATC_RAT_${axis}_IMAX`, 0.5);
  add(`ATC_RAT_${axis}_FLTE`, flte);
  add(`ATC_RAT_${axis}_FLTD`, axis === "YAW" ? 0 : 20);
  add(`ATC_RAT_${axis}_FLTT`, 20);
  add(`ATC_ANG_${axis}_P`, 4.5);
}
add("ATC_ACCEL_R_MAX", 110000);
add("ATC_ACCEL_P_MAX", 110000);
add("ATC_ACCEL_Y_MAX", 27000);
add("ATC_INPUT_TC", 0.15);
for (const [name, value] of [
  ["PSC_ACCZ_P", 0.5], ["PSC_ACCZ_I", 1], ["PSC_ACCZ_D", 0], ["PSC_ACCZ_IMAX", 800],
  ["PSC_VELZ_P", 5], ["PSC_POSZ_P", 1], ["PSC_POSXY_P", 1],
  ["PSC_VELXY_P", 2], ["PSC_VELXY_I", 1], ["PSC_VELXY_D", 0.5], ["PSC_VELXY_IMAX", 1000],
  ["WPNAV_SPEED", 500], ["WPNAV_SPEED_UP", 250], ["WPNAV_SPEED_DN", 150], ["WPNAV_RADIUS", 200], ["WPNAV_ACCEL", 100],
  ["LOIT_SPEED", 1250], ["LOIT_ACC_MAX", 500], ["LOIT_BRK_ACCEL", 250], ["LOIT_ANG_MAX", 0],
  ["INS_GYRO_FILTER", 20], ["INS_ACCEL_FILTER", 20],
  ["INS_NOTCH_ENABLE", 0], ["INS_NOTCH_FREQ", 80], ["INS_NOTCH_BW", 40], ["INS_NOTCH_ATT", 40],
  ["INS_HNTCH_ENABLE", 1], ["INS_HNTCH_MODE", 4], ["INS_HNTCH_REF", 0.2], ["INS_HNTCH_FREQ", 80],
  ["INS_HNTCH_BW", 40], ["INS_HNTCH_ATT", 40], ["INS_HNTCH_OPTS", 2], ["INS_HNTCH_HMNCS", 3],
  ["INS_LOG_BAT_MASK", 1], ["INS_LOG_BAT_OPT", 4],
  ["TUNE", 0], ["TUNE_LOW", 0], ["TUNE_HIGH", 1000],
  ["ARMING_CHECK", 1], ["ARMING_REQUIRE", 1], ["ARMING_ACCTHRESH", 0.75], ["ARMING_RUDDER", 2],
  ["SYSID_THISMAV", 1], ["SYSID_MYGCS", 255], ["FORMAT_VERSION", 120],
  ["FRAME_CLASS", 1], ["FRAME_TYPE", 1], ["MOT_THST_HOVER", 0.35], ["MOT_SPIN_MIN", 0.15], ["MOT_SPIN_MAX", 0.95],
  ["MOT_BAT_VOLT_MAX", 50.4], ["MOT_BAT_VOLT_MIN", 39.6], ["MOT_PWM_TYPE", 6],
] as Array<[string, number]>) {
  add(name, value);
}
for (let n = 6; n <= 10; n += 1) add(`RC${n}_OPTION`, n === 7 ? 4 : 0, "INT16");

const family = (prefix: string, suffixes: Array<[string, number]>) =>
  suffixes.forEach(([suffix, value]) => add(`${prefix}${suffix}`, value));

for (let n = 1; n <= 16; n += 1) {
  family(`RC${n}_`, [["MIN", 1100], ["MAX", 1900], ["TRIM", 1500], ["DZ", 20], ["REVERSED", 0]]);
  family(`SERVO${n}_`, [["MIN", 1000], ["MAX", 2000], ["TRIM", 1500], ["REVERSED", 0], ["FUNCTION", n <= 4 ? 32 + n : 0]]);
}
for (let n = 0; n <= 6; n += 1) {
  family(`SERIAL${n}_`, [["PROTOCOL", n === 0 ? 2 : n === 1 ? 2 : 5], ["BAUD", n === 2 ? 115 : 57], ["OPTIONS", 0]]);
  family(`SR${n}_`, [
    ["RAW_SENS", 0], ["EXT_STAT", 2], ["RC_CHAN", 2], ["RAW_CTRL", 0], ["POSITION", 2], ["EXTRA1", 4],
    ["EXTRA2", 4], ["EXTRA3", 2], ["PARAMS", 10], ["ADSB", 5],
  ]);
}
for (const bat of ["BATT_", "BATT2_"]) {
  family(bat, [
    ["MONITOR", bat === "BATT_" ? 4 : 0], ["VOLT_PIN", 14], ["CURR_PIN", 15], ["VOLT_MULT", 18.18], ["AMP_PERVLT", 36.36],
    ["AMP_OFFSET", 0], ["CAPACITY", 16000], ["LOW_VOLT", 21.6], ["LOW_TIMER", 10], ["CRT_VOLT", 20.4], ["FS_VOLTSRC", 0],
    ["LOW_MAH", 3000], ["CRT_MAH", 1500], ["FS_LOW_ACT", 2], ["FS_CRT_ACT", 1], ["ARM_VOLT", 0], ["ARM_MAH", 0],
    ["OPTIONS", 0], ["SERIAL_NUM", -1],
  ]);
}
for (const [ek, count] of [["EK3_", 70], ["EK2_", 38]] as const) {
  family(ek, [["ENABLE", ek === "EK3_" ? 1 : 0], ["GPS_TYPE", 0], ["VELNE_M_NSE", 0.3], ["VELD_M_NSE", 0.5], ["POSNE_M_NSE", 1], ["ALT_M_NSE", 3]]);
  for (let i = 0; i < count - 6; i += 1) add(`${ek}PARAM_${String(i + 1).padStart(2, "0")}`, ((i * 7) % 13) / 4);
}
for (let c = 0; c <= 2; c += 1) {
  const s = c === 0 ? "" : String(c + 1);
  family(`COMPASS_`, c === 0 ? [["USE", 1], ["AUTODEC", 1], ["DEC", 0], ["LEARN", 0], ["ORIENT", 0], ["EXTERNAL", 1]] : []);
  family(`COMPASS_OFS${s}_`, [["X", -22 + c * 5], ["Y", 148 - c * 9], ["Z", 36 + c]]);
  family(`COMPASS_DIA${s}_`, [["X", 1], ["Y", 1], ["Z", 1]]);
  family(`COMPASS_ODI${s}_`, [["X", 0], ["Y", 0], ["Z", 0]]);
  family(`COMPASS_MOT${s}_`, [["X", 0], ["Y", 0], ["Z", 0]]);
  add(`COMPASS_DEV_ID${s}`, 97539 + c * 10000, "INT32");
  add(`COMPASS_USE${s}`, 1, "INT8");
}
for (let i = 1; i <= 3; i += 1) {
  family(`INS_ACC${i}OFFS_`, [["X", 0.01 * i], ["Y", -0.02 * i], ["Z", 0.03 * i]]);
  family(`INS_ACC${i}SCAL_`, [["X", 1], ["Y", 1], ["Z", 1]]);
  family(`INS_GYR${i}OFFS_`, [["X", 0.001], ["Y", -0.002], ["Z", 0.0005]]);
  family(`INS_POS${i}_`, [["X", 0], ["Y", 0], ["Z", 0]]);
  add(`INS_ACC${i}_ID`, 2490378 + i, "INT32");
  add(`INS_GYR${i}_ID`, 2490378 + i, "INT32");
  add(`INS_USE${i === 1 ? "" : i}`, 1, "INT8");
}
for (const [prefix, suffixes] of [
  ["GPS_", ["TYPE", "TYPE2", "NAVFILTER", "AUTO_SWITCH", "MIN_DGPS", "SBAS_MODE", "MIN_ELEV", "INJECT_TO", "SBP_LOGMASK", "RAW_DATA", "GNSS_MODE", "SAVE_CFG", "AUTO_CONFIG", "RATE_MS", "DRV_OPTIONS", "COM_PORT", "CAN_NODEID1", "CAN_NODEID2", "BLEND_MASK", "PRIMARY"]],
  ["FS_", ["OPTIONS", "THR_ENABLE", "THR_VALUE", "GCS_ENABLE", "GCS_TIMEOUT", "EKF_ACTION", "EKF_THRESH", "CRASH_CHECK", "VIBE_ENABLE", "DR_ENABLE", "DR_TIMEOUT"]],
  ["RTL_", ["ALT", "ALT_FINAL", "ALT_TYPE", "CLIMB_MIN", "CONE_SLOPE", "LOIT_TIME", "SPEED", "OPTIONS"]],
  ["LAND_", ["SPEED", "SPEED_HIGH", "ALT_LOW", "REPOSITION", "ALT_LOW_M"]],
  ["PILOT_", ["SPEED_UP", "SPEED_DN", "ACCEL_Z", "THR_FILT", "THR_BHV", "TKOFF_ALT", "Y_RATE", "Y_EXPO", "Y_RATE_TC"]],
  ["AHRS_", ["GPS_GAIN", "GPS_USE", "YAW_P", "RP_P", "WIND_MAX", "TRIM_X", "TRIM_Y", "TRIM_Z", "ORIENTATION", "COMP_BETA", "GPS_MINSATS", "EKF_TYPE", "CUSTOM_ROLL", "CUSTOM_PIT", "CUSTOM_YAW"]],
  ["LOG_", ["BACKEND_TYPE", "FILE_BUFSIZE", "DISARMED", "REPLAY", "FILE_DSRMROT", "MAV_BUFSIZE", "FILE_TIMEOUT", "FILE_MB_FREE", "BITMASK", "FILE_RATEMAX", "DARM_RATEMAX"]],
  ["BRD_", ["PWM_COUNT", "SER1_RTSCTS", "SER2_RTSCTS", "SAFETY_DEFLT", "SAFETY_MASK", "SAFETYOPTION", "SD_SLOWDOWN", "BOOT_DELAY", "HEAT_TARG", "TYPE", "IO_ENABLE", "RTC_TYPES", "ALT_CONFIG", "SD_MISSION", "SD_FENCE", "VBUS_MIN", "VSERVO_MIN"]],
  ["SCR_", ["ENABLE", "VM_I_COUNT", "HEAP_SIZE", "DEBUG_OPTS", "USER1", "USER2", "USER3", "USER4", "DIR_DISABLE", "THD_PRIORITY", "SDEV_EN"]],
  ["AVOID_", ["ENABLE", "ANGLE_MAX", "DIST_MAX", "MARGIN", "BEHAVE", "ALT_MIN", "ACCEL_MAX", "BACKUP_SPD", "BACKUP_DZ"]],
  ["FENCE_", ["ENABLE", "TYPE", "ACTION", "ALT_MAX", "RADIUS", "MARGIN", "TOTAL", "ALT_MIN", "RET_ALT", "RET_RALLY", "AUTOENABLE", "OPTIONS"]],
  ["MNT1_", ["TYPE", "DEFLT_MODE", "RC_RATE", "ROLL_MIN", "ROLL_MAX", "PITCH_MIN", "PITCH_MAX", "YAW_MIN", "YAW_MAX", "RETRACT_X", "NEUTRAL_X", "LEAD_RLL", "LEAD_PTCH", "SYSID_DFLT"]],
  ["NTF_", ["LED_BRIGHT", "BUZZ_TYPES", "LED_OVERRIDE", "DISPLAY_TYPE", "OREO_THEME", "BUZZ_PIN", "LED_TYPES", "BUZZ_ON_LVL", "BUZZ_VOLUME", "LED_LEN"]],
  ["OSD1_", ["ENABLE", "CHAN_MIN", "CHAN_MAX", "SW_METHOD", "OPTIONS", "FONT", "V_OFFSET", "H_OFFSET", "UNITS", "MSG_TIME", "ARM_SCR", "FS_SCR", "TXT_RES", "WARN_RSSI", "WARN_NSAT"]],
  ["LGR_", ["ENABLE", "DEPLOY_PIN", "DEPLOY_POL", "SERVO_RTRACT", "SERVO_DEPLOY", "STARTUP", "WOW_PIN", "WOW_POL", "ANGLE_DEP", "ANGLE_LAND", "OPTIONS"]],
  ["RNGFND1_", ["TYPE", "PIN", "SCALING", "OFFSET", "FUNCTION", "MIN_CM", "MAX_CM", "STOP_PIN", "SETTLE", "RMETRIC", "PWRRNG", "GNDCLEAR", "ADDR", "POS_X", "POS_Y", "POS_Z", "ORIENT"]],
  ["CAN_D1_", ["PROTOCOL", "PROTOCOL2", "UC_NODE", "UC_SRV_BM", "UC_ESC_BM", "UC_SRV_RT", "UC_OPTION", "UC_NTF_RT", "UC_POOL", "UC_ESC_OF", "UC_RLY_RT", "PR_MIN"]],
  ["DISARM_", ["DELAY"]],
  ["MIS_", ["TOTAL", "RESTART", "OPTIONS"]],
  ["TERRAIN_", ["ENABLE", "SPACING", "OPTIONS", "MARGIN", "CACHE_SZ"]],
  ["WINCH_", ["TYPE", "RATE_MAX", "POS_P", "RATE_P", "RATE_I", "RATE_D", "RATE_IMAX", "OPTIONS", "RC_FUNC"]],
  ["ACRO_", ["BAL_ROLL", "BAL_PITCH", "TRAINER", "RP_RATE", "Y_RATE", "THR_MID", "RP_EXPO", "Y_EXPO", "OPTIONS", "RP_RATE_TC"]],
  ["SIMPLE_", ["MODE"]],
  ["THR_", ["DZ", "FAILSAFE"]],
  ["VTX_", ["ENABLE", "FREQ", "POWER", "MAX_POWER", "OPTIONS", "CHANNEL", "BAND"]],
] as Array<[string, string[]]>) {
  suffixes.forEach((suffix, i) => add(`${prefix}${suffix}`, ((i * 5 + prefix.length) % 17) * 0.5, i % 3 === 0 ? "INT16" : REAL32));
}
for (let i = 1; i <= 6; i += 1) add(`FLTMODE${i}`, [0, 5, 6, 3, 2, 16][i - 1], "INT8");
add("FLTMODE_CH", 5, "INT8");
for (let i = 1; i <= 48; i += 1) add(`SCR_USER_VAL_${String(i).padStart(2, "0")}`, i / 8);
for (let i = 1; i <= 80; i += 1) add(`Q_FILLER_${String(i).padStart(3, "0")}`, (i % 9) / 3);
for (const group of ["SIM", "TKOFF", "PRX", "RALLY", "RCMAP", "RELAY", "RSSI", "SPRAY", "TEMP", "WP", "ADSB", "BARO", "CIRCLE", "CAM", "CHUTE", "EFI", "ESC", "FOLL", "GEN", "H", "J", "K", "MSP", "OA", "OF", "PLND", "RPM", "SHOW", "TEL", "TUNE_X", "VISO", "WENC", "AUTOTUNE", "STAT", "SURFTRAK", "SYSID", "TCAL", "TERR", "ZIGZ"]) {
  for (let i = 1; i <= 6; i += 1) add(`${group}_OPT${i}`, ((i * 3 + group.length) % 11) * 0.25);
}

const sorted = [...new Map(params.map(([name, value, type]) => [name, { value, type: type ?? REAL32 }])).entries()];

const INT_NAME = /(ENABLE|MODE|TYPE|PROTOCOL|PIN|OPTION|OPTIONS|BAUD|MASK|FUNCTION|REVERSED|ID|COUNT|TOTAL|ACTION|LEARN|CHECK|REQUIRE|ORIENT)/;

export function initialParams(): ParamEntry[] {
  return sorted.map(([name, { value, type }], index) => ({
    name,
    value: Math.fround(value),
    paramType: type !== REAL32 ? type : INT_NAME.test(name) && Number.isInteger(value) ? "INT16" : REAL32,
    index,
  }));
}

const std = (displayName: string, description: string, rest: Partial<ParamMeta> = {}): ParamMeta => ({
  displayName,
  description,
  units: "",
  min: null,
  max: null,
  increment: null,
  values: [],
  bitmask: [],
  rebootRequired: false,
  readOnly: false,
  user: "Standard",
  ...rest,
});

const enable = (name: string, description: string, reboot = false) =>
  std(name, description, {
    values: [{ code: 0, label: "Disabled" }, { code: 1, label: "Enabled" }],
    rebootRequired: reboot,
  });

const options = [
  [0, "Do Nothing"], [2, "Flip"], [3, "Simple Mode"], [4, "RTL"], [5, "Save Trim"], [7, "Save WP"], [9, "Camera Trigger"],
  [10, "RangeFinder"], [11, "Fence"], [13, "Super Simple Mode"], [14, "Acro Trainer"], [15, "Sprayer"], [16, "Auto"],
  [17, "AutoTune"], [18, "Land"], [19, "Gripper"], [21, "Parachute Enable"], [24, "Auto Mission Reset"], [28, "Relay On/Off"],
  [31, "Motor Emergency Stop"], [41, "Arm/Disarm"], [56, "Loiter"], [57, "PosHold"], [58, "AltHold"], [60, "Stabilize"],
].map(([code, label]) => ({ code: code as number, label: label as string }));

const tuneValues = [
  [0, "None"], [1, "Stab Roll/Pitch kP"], [4, "Rate Roll/Pitch kP"], [5, "Rate Roll/Pitch kI"], [21, "Rate Yaw kP"],
  [3, "Stab Yaw kP"], [6, "Yaw kP"], [7, "Loiter Position kP"], [12, "Loiter Speed"], [14, "AltHold kP"], [17, "Throttle Accel kP"],
  [26, "Rate Roll/Pitch kD"], [38, "Rate Yaw kD"], [52, "Rate Roll/Pitch FLTE"], [53, "Rate Yaw FLTE"],
].map(([code, label]) => ({ code: code as number, label: label as string }));

export function initialMetadata(): Record<string, ParamMeta> {
  const meta: Record<string, ParamMeta> = {
    ATC_RAT_RLL_P: std("Roll axis rate controller P gain", "Roll axis rate controller P gain.", { min: 0.01, max: 0.5, increment: 0.005 }),
    ATC_RAT_RLL_I: std("Roll axis rate controller I gain", "Roll axis rate controller I gain.", { min: 0.01, max: 2, increment: 0.01 }),
    ATC_RAT_RLL_D: std("Roll axis rate controller D gain", "Roll axis rate controller D gain.", { min: 0, max: 0.05, increment: 0.001 }),
    ATC_RAT_RLL_IMAX: std("Roll axis rate controller I gain maximum", "Maximum output from the I term.", { min: 0, max: 1, increment: 0.01, user: "Advanced" }),
    ATC_RAT_RLL_FLTE: std("Roll axis rate controller error filter", "Error filter cutoff frequency.", { units: "Hz", min: 0, max: 100, increment: 1, user: "Advanced" }),
    ATC_RAT_RLL_FLTD: std("Roll axis rate controller derivative filter", "D term filter cutoff frequency.", { units: "Hz", min: 0, max: 100, increment: 1, user: "Advanced" }),
    ATC_RAT_RLL_FLTT: std("Roll axis rate controller target filter", "Target filter cutoff frequency.", { units: "Hz", min: 0, max: 100, increment: 1, user: "Advanced" }),
    ATC_ANG_RLL_P: std("Roll axis angle controller P gain", "Converts the angle error into a desired rate.", { min: 3, max: 12, increment: 0.1 }),
    ATC_ACCEL_R_MAX: std("Acceleration Max for Roll", "Maximum acceleration in roll axis.", { units: "cdeg/s/s", min: 0, max: 180000, increment: 1000, values: [{ code: 0, label: "Disabled" }, { code: 72000, label: "Slow" }, { code: 110000, label: "Medium" }, { code: 180000, label: "Fast" }] }),
    ATC_INPUT_TC: std("Attitude control input time constant", "Smaller values give a crisper response.", { units: "s", min: 0, max: 1, increment: 0.01 }),
    PSC_ACCZ_P: std("Acceleration (vertical) controller P gain", "Converts acceleration error into throttle output.", { min: 0.2, max: 1.5, increment: 0.05 }),
    PSC_POSXY_P: std("Position (horizontal) controller P gain", "Position error to desired velocity.", { min: 0.5, max: 2, increment: 0.1 }),
    PSC_VELXY_P: std("Velocity (horizontal) P gain", "Velocity error to desired acceleration.", { min: 0.1, max: 6, increment: 0.1 }),
    WPNAV_SPEED: std("Waypoint Horizontal Speed Target", "Target speed in cm/s during auto missions.", { units: "cm/s", min: 20, max: 2000, increment: 50 }),
    WPNAV_RADIUS: std("Waypoint Radius", "Distance from a waypoint when it is considered reached.", { units: "cm", min: 5, max: 1000, increment: 1 }),
    INS_GYRO_FILTER: std("Gyro filter cutoff frequency", "Software low-pass on the gyros.", { units: "Hz", min: 0, max: 256, increment: 1, rebootRequired: true, user: "Advanced" }),
    INS_NOTCH_ENABLE: enable("INS_NOTCH_ENABLE", "Enable the static notch filter.", true),
    INS_HNTCH_ENABLE: enable("INS_HNTCH_ENABLE", "Enable the harmonic notch filter.", true),
    INS_HNTCH_MODE: std("Harmonic Notch Filter dynamic frequency mode", "Source of the notch center frequency.", {
      values: [[0, "Fixed"], [1, "Throttle"], [2, "RPM Sensor"], [3, "ESC Telemetry"], [4, "Dynamic FFT"], [5, "Second RPM Sensor"]].map(([code, label]) => ({ code: code as number, label: label as string })),
      rebootRequired: true,
    }),
    INS_HNTCH_OPTS: std("Harmonic Notch Filter options", "Options for the harmonic notch.", {
      bitmask: [[0, "Double notch"], [1, "Multi-source"], [2, "Update at loop rate"], [3, "EnableOnAllIMUs"], [4, "Triple notch"], [5, "Use min freq"]].map(([bit, label]) => ({ bit: bit as number, label: label as string })),
      rebootRequired: true,
      user: "Advanced",
    }),
    INS_HNTCH_HMNCS: std("Harmonic Notch Filter harmonics", "Bitmask of harmonics to filter.", {
      bitmask: [[0, "1st harmonic"], [1, "2nd harmonic"], [2, "3rd harmonic"], [3, "4th harmonic"], [4, "5th harmonic"]].map(([bit, label]) => ({ bit: bit as number, label: label as string })),
    }),
    INS_HNTCH_FREQ: std("Harmonic Notch Filter base center frequency", "Center frequency of the notch.", { units: "Hz", min: 10, max: 495, increment: 1 }),
    INS_LOG_BAT_MASK: std("Sensor Bitmask", "Which IMUs to log batches for.", {
      bitmask: [[0, "IMU1"], [1, "IMU2"], [2, "IMU3"]].map(([bit, label]) => ({ bit: bit as number, label: label as string })),
    }),
    INS_LOG_BAT_OPT: std("Batch Logging Options Mask", "Options for the batch sampler.", {
      bitmask: [[0, "Sensor-Rate Logging"], [1, "Sample Post-Filter"], [2, "Sample Pre- and Post-Filter"]].map(([bit, label]) => ({ bit: bit as number, label: label as string })),
      user: "Advanced",
    }),
    TUNE: std("Channel 6 Tuning", "Controls which parameters are tuned by the channel 6 knob.", { values: tuneValues }),
    TUNE_LOW: std("Tuning minimum", "Lower value of the tuned parameter.", { min: 0, max: 32760, increment: 1 }),
    TUNE_HIGH: std("Tuning maximum", "Upper value of the tuned parameter.", { min: 0, max: 32760, increment: 1 }),
    ARMING_CHECK: std("Arming check", "Checks prior to arming motor.", {
      bitmask: [[0, "All"], [1, "Barometer"], [2, "Compass"], [3, "GPS lock"], [4, "INS"], [5, "Parameters"], [6, "RC channels"], [7, "Board voltage"], [8, "Battery level"], [10, "Logging available"], [11, "Hardware safety switch"], [12, "GPS configuration"], [13, "System"]].map(([bit, label]) => ({ bit: bit as number, label: label as string })),
    }),
    ARMING_REQUIRE: std("Require Arming Motors", "Arming before flight.", { values: [{ code: 0, label: "Disabled" }, { code: 1, label: "Throttle-Yaw-Arm" }, { code: 2, label: "Arming-Switch" }] }),
    FORMAT_VERSION: std("Eeprom format version number", "Internal use.", { readOnly: true, user: "Advanced" }),
    SYSID_THISMAV: std("MAVLink system ID of this vehicle", "Allows setting a unique system id for each vehicle.", { min: 1, max: 255, increment: 1, rebootRequired: true, user: "Advanced" }),
    FRAME_CLASS: std("Frame Class", "Controls major frame class for multicopter.", {
      values: [[0, "Undefined"], [1, "Quad"], [2, "Hexa"], [3, "Octa"], [4, "OctaQuad"], [5, "Y6"], [6, "Heli"], [7, "Tri"]].map(([code, label]) => ({ code: code as number, label: label as string })),
      rebootRequired: true,
    }),
    MOT_SPIN_MAX: std("Motor Spin maximum", "Point at which the thrust saturates.", { min: 0.9, max: 1, increment: 0.01, user: "Advanced" }),
    MOT_THST_HOVER: std("Thrust Hover Value", "Motor thrust needed to hover.", { min: 0.2, max: 0.8, increment: 0.01 }),
    BATT_MONITOR: std("Battery monitoring", "Controls battery monitoring.", {
      values: [[0, "Disabled"], [3, "Analog Voltage Only"], [4, "Analog Voltage and Current"], [5, "Solo"], [8, "DroneCAN-BatteryInfo"], [9, "ESC"]].map(([code, label]) => ({ code: code as number, label: label as string })),
      rebootRequired: true,
    }),
    BATT_CAPACITY: std("Battery capacity", "Capacity of the battery in mAh when full.", { units: "mAh", min: 0, increment: 50 }),
    BATT_LOW_VOLT: std("Low battery voltage", "Battery voltage that triggers a low battery failsafe.", { units: "V", min: 0, increment: 0.1 }),
    FS_THR_ENABLE: std("Throttle Failsafe Enable", "Throttle failsafe action.", { values: [{ code: 0, label: "Disabled" }, { code: 1, label: "Enabled always RTL" }, { code: 3, label: "Enabled always Land" }] }),
    LOIT_SPEED: std("Loiter Horizontal Maximum Speed", "Maximum horizontal speed in Loiter.", { units: "cm/s", min: 20, max: 3500, increment: 50 }),
    RTL_ALT: std("RTL Altitude", "Altitude to return at.", { units: "cm", min: 200, max: 300000, increment: 1 }),
    SERIAL1_PROTOCOL: std("Telemetry 1 protocol selection", "Serial protocol.", {
      values: [[-1, "None"], [1, "MAVLink1"], [2, "MAVLink2"], [5, "GPS"], [23, "RCIN"], [28, "Scripting"]].map(([code, label]) => ({ code: code as number, label: label as string })),
      rebootRequired: true,
    }),
    SCR_ENABLE: enable("SCR_ENABLE", "Enable Lua scripting.", true),
    GPS_TYPE: std("1st GPS type", "GPS type of the first GPS.", {
      values: [[0, "None"], [1, "AUTO"], [2, "uBlox"], [9, "DroneCAN"], [14, "MAV"]].map(([code, label]) => ({ code: code as number, label: label as string })),
      rebootRequired: true,
    }),
  };
  for (const axis of ["PIT", "YAW"]) {
    for (const term of ["P", "I", "D", "IMAX", "FLTE", "FLTD", "FLTT"]) {
      const source = meta[`ATC_RAT_RLL_${term}`];
      if (source) meta[`ATC_RAT_${axis}_${term}`] = { ...source, displayName: source.displayName.replace("Roll", axis === "PIT" ? "Pitch" : "Yaw") };
    }
    meta[`ATC_ANG_${axis}_P`] = { ...meta.ATC_ANG_RLL_P, displayName: meta.ATC_ANG_RLL_P.displayName.replace("Roll", axis === "PIT" ? "Pitch" : "Yaw") };
  }
  for (let n = 6; n <= 10; n += 1) {
    meta[`RC${n}_OPTION`] = std(`RC${n} option`, "Function assigned to this RC channel.", { values: options });
  }
  for (let n = 1; n <= 16; n += 1) {
    meta[`RC${n}_MIN`] = std(`RC${n} minimum PWM`, "Minimum PWM pulse width.", { units: "PWM", min: 800, max: 2200, increment: 1, user: "Advanced" });
    meta[`RC${n}_MAX`] = std(`RC${n} maximum PWM`, "Maximum PWM pulse width.", { units: "PWM", min: 800, max: 2200, increment: 1, user: "Advanced" });
    meta[`RC${n}_TRIM`] = std(`RC${n} trim PWM`, "Trim PWM pulse width.", { units: "PWM", min: 800, max: 2200, increment: 1, user: "Advanced" });
  }
  return meta;
}
