// Extended Tuning layout for Copter, taken from the legacy ConfigArducopter screen. Each control
// lists alternative parameter names for different firmware generations; the first one present on
// the vehicle is used.
export type TuneControl = {
  id: string;
  label: string;
  names: string[];
  /** `rollpitch`: mirrored to the matching PIT/RLL control when the lock is on. `latlon`: written to the LON twin. */
  mirror?: "rollpitch" | "latlon";
};

export type TuneGroup = { id: string; title: string; controls: TuneControl[] };

const axes = [
  { key: "RLL", title: "Roll" },
  { key: "PIT", title: "Pitch" },
  { key: "YAW", title: "Yaw" },
] as const;

const rateGroup = (key: string, title: string): TuneGroup => {
  const lock = key === "YAW" ? undefined : ("rollpitch" as const);
  return {
    id: `rate-${key}`,
    title: `Rate ${title}`,
    controls: [
      { id: `RATE_${key}_P`, label: "P", names: [`RATE_${key}_P`, `ATC_RAT_${key}_P`, `Q_A_RAT_${key}_P`], mirror: lock },
      { id: `RATE_${key}_I`, label: "I", names: [`RATE_${key}_I`, `ATC_RAT_${key}_I`, `Q_A_RAT_${key}_I`], mirror: lock },
      { id: `RATE_${key}_D`, label: "D", names: [`RATE_${key}_D`, `ATC_RAT_${key}_D`, `Q_A_RAT_${key}_D`], mirror: lock },
      {
        id: `RATE_${key}_IMAX`,
        label: "IMAX",
        names: [`ATC_RAT_${key}_IMAX`, `Q_A_RAT_${key}_IMAX`, `RATE_${key}_IMAX`],
        mirror: lock,
      },
      {
        id: `RATE_${key}_FILT`,
        label: "FILT / FLTE",
        names: [`RATE_${key}_FILT`, `ATC_RAT_${key}_FILT`, `ATC_RAT_${key}_FLTE`, `Q_A_RAT_${key}_FLTE`],
        mirror: lock,
      },
      { id: `ATC_RAT_${key}_FLTD`, label: "FLTD", names: [`ATC_RAT_${key}_FLTD`, `Q_A_RAT_${key}_FLTD`] },
      { id: `ATC_RAT_${key}_FLTT`, label: "FLTT", names: [`ATC_RAT_${key}_FLTT`, `Q_A_RAT_${key}_FLTT`] },
    ],
  };
};

const posz = ["PSC_ACCZ", "Q_P_ACCZ", "PSC_D_ACC", "Q_P_D_ACC"];
const velxy = ["VEL_XY", "PSC_VELXY", "Q_P_VELXY", "PSC_NE_VEL", "Q_P_NE_VEL"];

export const COPTER_GROUPS: TuneGroup[] = [
  ...axes.map((a) => rateGroup(a.key, a.title)),
  {
    id: "stabilize",
    title: "Stabilize",
    controls: axes.map((a) => ({
      id: `STB_${a.key}_P`,
      label: `${a.title} P`,
      names: [`STB_${a.key}_P`, `ATC_ANG_${a.key}_P`, `Q_A_ANG_${a.key}_P`],
      mirror: a.key === "YAW" ? undefined : ("rollpitch" as const),
    })),
  },
  {
    id: "accel-limits",
    title: "Attitude limits",
    controls: [
      { id: "ATC_ACCEL_R_MAX", label: "Roll accel max", names: ["ATC_ACCEL_R_MAX", "Q_A_ACCEL_R_MAX", "ATC_ACC_R_MAX", "Q_A_ACC_R_MAX"] },
      { id: "ATC_ACCEL_P_MAX", label: "Pitch accel max", names: ["ATC_ACCEL_P_MAX", "Q_A_ACCEL_P_MAX", "ATC_ACC_P_MAX", "Q_A_ACC_P_MAX"] },
      { id: "ATC_ACCEL_Y_MAX", label: "Yaw accel max", names: ["ATC_ACCEL_Y_MAX", "Q_A_ACCEL_Y_MAX", "ATC_ACC_Y_MAX", "Q_A_ACC_Y_MAX"] },
      { id: "ATC_INPUT_TC", label: "Input time constant", names: ["ATC_INPUT_TC", "Q_A_INPUT_TC"] },
    ],
  },
  {
    id: "throttle-accel",
    title: "Throttle accel",
    controls: ["P", "I", "D", "IMAX"].map((term) => ({
      id: `THR_ACCEL_${term}`,
      label: term,
      names: [`THR_ACCEL_${term}`, `ACCEL_Z_${term}`, ...posz.map((p) => `${p}_${term}`)],
    })),
  },
  {
    id: "throttle-rate",
    title: "Throttle rate",
    controls: [
      {
        id: "THR_RATE_P",
        label: "P",
        names: ["THR_RATE_P", "VEL_Z_P", "PSC_VELZ_P", "Q_P_VELZ_P", "PSC_D_VEL_P", "Q_P_D_VEL_P"],
      },
    ],
  },
  {
    id: "altitude-hold",
    title: "Altitude hold",
    controls: [
      {
        id: "THR_ALT_P",
        label: "P",
        names: ["THR_ALT_P", "POS_Z_P", "PSC_POSZ_P", "Q_P_POSZ_P", "PSC_D_POS_P", "Q_P_D_POS_P"],
      },
    ],
  },
  {
    id: "position-xy",
    title: "Position XY",
    controls: [{ id: "HLD_LAT_P", label: "P", names: ["HLD_LAT_P", "POS_XY_P", "PSC_POSXY_P", "Q_P_POSXY_P"] }],
  },
  {
    id: "velocity-xy",
    title: "Velocity XY",
    controls: [
      {
        id: "LOITER_LAT_P",
        label: "P",
        names: ["LOITER_LAT_P", ...velxy.map((p) => `${p}_P`)],
        mirror: "latlon",
      },
      {
        id: "LOITER_LAT_I",
        label: "I",
        names: ["LOITER_LAT_I", ...velxy.map((p) => `${p}_I`)],
        mirror: "latlon",
      },
      {
        id: "LOITER_LAT_IMAX",
        label: "IMAX",
        names: ["LOITER_LAT_IMAX", ...velxy.map((p) => `${p}_IMAX`)],
        mirror: "latlon",
      },
      {
        id: "LOITER_LAT_D",
        label: "D",
        names: ["LOITER_LAT_D", "PSC_VELXY_D", "Q_P_VELXY_D", "PSC_NE_VEL_D", "Q_P_NE_VEL_D"],
        mirror: "latlon",
      },
    ],
  },
  {
    id: "wpnav",
    title: "WPNav",
    controls: [
      { id: "WPNAV_SPEED", label: "Speed", names: ["WPNAV_SPEED", "Q_WP_SPEED", "WP_SPD", "Q_WP_SPD"] },
      { id: "WPNAV_SPEED_UP", label: "Speed up", names: ["WPNAV_SPEED_UP", "Q_WP_SPEED_UP", "WP_SPD_UP", "Q_WP_SPD_UP"] },
      { id: "WPNAV_SPEED_DN", label: "Speed down", names: ["WPNAV_SPEED_DN", "Q_WP_SPEED_DN", "WP_SPD_DN", "Q_WP_SPD_DN"] },
      { id: "WPNAV_RADIUS", label: "Radius", names: ["WPNAV_RADIUS", "Q_WP_RADIUS", "WP_RADIUS_M", "Q_WP_RADIUS_M"] },
      {
        id: "WPNAV_LOIT_SPEED",
        label: "Loiter speed",
        names: ["WPNAV_LOIT_SPEED", "LOIT_SPEED", "Q_LOIT_SPEED", "LOIT_SPEED_MS", "Q_LOIT_SPEED_MS"],
      },
    ],
  },
  {
    id: "basic-filters",
    title: "Basic filters",
    controls: [
      { id: "INS_GYRO_FILTER", label: "Gyro filter (Hz)", names: ["INS_GYRO_FILTER"] },
      { id: "INS_ACCEL_FILTER", label: "Accel filter (Hz)", names: ["INS_ACCEL_FILTER"] },
    ],
  },
  {
    id: "static-notch",
    title: "Static notch",
    controls: [
      { id: "INS_NOTCH_ENABLE", label: "Enable", names: ["INS_NOTCH_ENABLE"] },
      { id: "INS_NOTCH_FREQ", label: "Frequency (Hz)", names: ["INS_NOTCH_FREQ"] },
      { id: "INS_NOTCH_BW", label: "Bandwidth (Hz)", names: ["INS_NOTCH_BW"] },
      { id: "INS_NOTCH_ATT", label: "Attenuation (dB)", names: ["INS_NOTCH_ATT"] },
    ],
  },
  {
    id: "harmonic-notch",
    title: "Harmonic notch",
    controls: [
      { id: "INS_HNTCH_ENABLE", label: "Enable", names: ["INS_HNTCH_ENABLE"] },
      { id: "INS_HNTCH_MODE", label: "Mode", names: ["INS_HNTCH_MODE"] },
      { id: "INS_HNTCH_REF", label: "Reference", names: ["INS_HNTCH_REF"] },
      { id: "INS_HNTCH_FREQ", label: "Frequency (Hz)", names: ["INS_HNTCH_FREQ"] },
      { id: "INS_HNTCH_BW", label: "Bandwidth (Hz)", names: ["INS_HNTCH_BW"] },
      { id: "INS_HNTCH_ATT", label: "Attenuation (dB)", names: ["INS_HNTCH_ATT"] },
      { id: "INS_HNTCH_OPTS", label: "Options", names: ["INS_HNTCH_OPTS"] },
      { id: "INS_HNTCH_HMNCS", label: "Harmonics", names: ["INS_HNTCH_HMNCS"] },
    ],
  },
  {
    id: "filter-logs",
    title: "Filter logs",
    controls: [
      { id: "INS_LOG_BAT_MASK", label: "Batch mask", names: ["INS_LOG_BAT_MASK"] },
      { id: "INS_LOG_BAT_OPT", label: "Batch options", names: ["INS_LOG_BAT_OPT"] },
    ],
  },
  {
    id: "rc-tuning",
    title: "RC tuning",
    controls: [
      { id: "TUNE", label: "TUNE channel function", names: ["TUNE"] },
      { id: "TUNE_LOW", label: "TUNE low", names: ["TUNE_LOW", "TUNE_MIN"] },
      { id: "TUNE_HIGH", label: "TUNE high", names: ["TUNE_HIGH", "TUNE_MAX"] },
      ...[6, 7, 8, 9, 10].map((n) => ({
        id: `CH${n}_OPTION`,
        label: `CH${n} option`,
        names: [`CH${n}_OPT`, `CH${n}_OPTION`, `RC${n}_OPTION`],
      })),
    ],
  },
];

/** The control paired with this one when roll/pitch are locked (RLL <-> PIT), or null. */
export function rollPitchPartner(control: TuneControl): string | null {
  if (control.mirror !== "rollpitch") return null;
  return control.id.includes("_RLL_") ? control.id.replace("_RLL_", "_PIT_") : control.id.replace("_PIT_", "_RLL_");
}

/** The LON twin of a LAT parameter name (LOITER_LAT_P -> LOITER_LON_P), or null. */
export function latLonTwin(paramName: string): string | null {
  return paramName.includes("_LAT_") ? paramName.replace("_LAT_", "_LON_") : null;
}
