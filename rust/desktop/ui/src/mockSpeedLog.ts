/**
 * Mock SpeedLOG backend: a synthetic 10-minute copter log (attitude, altitude, battery, vibration,
 * motors, GPS, two IMU instances), with modes, parameters and messages. Spread into the mock `Backend`
 * by mockBackend.ts. The real reduction and decoding are in crates/speedlog.
 */
import type { Backend, Unlisten } from "./api";
import type {
  SpeedLogInfo,
  SpeedLogMessage,
  SpeedLogMode,
  SpeedLogParam,
  SpeedLogSeries,
  SpeedLogType,
  VehicleLogEntry,
  VehicleLogProgress,
} from "./types";

type SpeedLogMock = Pick<
  Backend,
  | "speedlogOpen"
  | "speedlogClose"
  | "speedlogInfo"
  | "speedlogTree"
  | "speedlogSeries"
  | "speedlogModes"
  | "speedlogParams"
  | "speedlogMessages"
  | "speedlogPresets"
  | "logsList"
  | "logsOpen"
  | "logsSave"
  | "logsCancel"
  | "onLogsProgress"
>;

const VEHICLE_LOGS: VehicleLogEntry[] = [40, 41, 42].map((n, i) => ({
  name: `000000${n}.BIN`,
  size: [1_245_184, 18_874_368, 48_234_496][i],
  timeUtc: 1_794_400_000 + i * 86_400,
  source: { type: "ftp", path: `/APM/LOGS/000000${n}.BIN` },
}));

const DURATION = 600;
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

type Gen = { unit: string | null; doc: string; rate: number; f: (t: number) => number };

const noise = (t: number, k: number) => Math.sin(t * 12.9898 * k) * 43758.5453 % 1;

const TYPES: Record<string, { doc: string; base?: string; instance?: string; fields: Record<string, Gen> }> = {
  ATT: {
    doc: "Canonical vehicle attitude",
    fields: {
      DesRoll: { unit: "deg", doc: "vehicle desired roll", rate: 10, f: (t) => 12 * Math.sin(t / 7) },
      Roll: { unit: "deg", doc: "achieved vehicle roll", rate: 10, f: (t) => 12 * Math.sin(t / 7 - 0.05) + noise(t, 1) },
      Pitch: { unit: "deg", doc: "achieved vehicle pitch", rate: 10, f: (t) => 8 * Math.cos(t / 9) + noise(t, 2) },
      Yaw: { unit: "deg", doc: "achieved vehicle yaw", rate: 10, f: (t) => (t * 6) % 360 },
    },
  },
  CTUN: {
    doc: "Control Tuning information",
    fields: {
      Alt: { unit: "m", doc: "achieved altitude", rate: 10, f: (t) => altitude(t) },
      DAlt: { unit: "m", doc: "desired altitude", rate: 10, f: (t) => altitude(t + 0.6) },
      ThO: { unit: null, doc: "throttle output", rate: 10, f: (t) => (t > 20 && t < 560 ? 0.45 + 0.05 * Math.sin(t) : 0) },
    },
  },
  "BAT[0]": {
    doc: "Gathered battery data",
    base: "BAT",
    instance: "0",
    fields: {
      Volt: { unit: "V", doc: "measured voltage", rate: 2, f: (t) => 25.2 - t * 0.004 - (t > 20 && t < 560 ? 0.8 : 0) },
      Curr: { unit: "A", doc: "measured current", rate: 2, f: (t) => (t > 20 && t < 560 ? 28 + 6 * Math.sin(t / 5) : 0.6) },
    },
  },
  VIBE: {
    doc: "Processed (acceleration) vibration information",
    fields: {
      VibeX: { unit: "m/s/s", doc: "Primary accelerometer filtered vibration, x-axis", rate: 5, f: (t) => 8 + 3 * Math.abs(noise(t, 3)) },
      VibeY: { unit: "m/s/s", doc: "Primary accelerometer filtered vibration, y-axis", rate: 5, f: (t) => 9 + 3 * Math.abs(noise(t, 4)) },
      VibeZ: { unit: "m/s/s", doc: "Primary accelerometer filtered vibration, z-axis", rate: 5, f: (t) => 14 + (t > 300 && t < 302 ? 40 : 4 * Math.abs(noise(t, 5))) },
    },
  },
  RCOU: {
    doc: "Servo channel output values 1 to 14",
    fields: Object.fromEntries(
      [1, 2, 3, 4].map((c) => [
        `C${c}`,
        { unit: "PWM", doc: `channel ${c} output`, rate: 10, f: (t: number) => (t > 20 && t < 560 ? 1500 + 60 * Math.sin(t / 3 + c) : 1000) },
      ]),
    ),
  },
  GPS: {
    doc: "Information received from GNSS systems attached to the autopilot",
    fields: {
      NSats: { unit: "satellites", doc: "number of satellites visible", rate: 5, f: (t) => 14 + Math.round(2 * Math.sin(t / 40)) },
      HDop: { unit: null, doc: "horizontal dilution of precision", rate: 5, f: (t) => 0.7 + 0.1 * Math.sin(t / 30) },
    },
  },
  "IMU[0]": {
    doc: "Inertial Measurement Unit data",
    base: "IMU",
    instance: "0",
    fields: {
      GyrX: { unit: "rad/s", doc: "measured rotation rate about X axis", rate: 25, f: (t) => 0.2 * Math.sin(t * 2) + 0.05 * noise(t, 6) },
      AccZ: { unit: "m/s/s", doc: "acceleration along Z axis", rate: 25, f: (t) => -9.8 + 0.6 * noise(t, 7) },
    },
  },
  "IMU[1]": {
    doc: "Inertial Measurement Unit data",
    base: "IMU",
    instance: "1",
    fields: {
      GyrX: { unit: "rad/s", doc: "measured rotation rate about X axis", rate: 25, f: (t) => 0.2 * Math.sin(t * 2) + 0.07 * noise(t, 8) },
      AccZ: { unit: "m/s/s", doc: "acceleration along Z axis", rate: 25, f: (t) => -9.8 + 0.7 * noise(t, 9) },
    },
  },
};

function altitude(t: number): number {
  if (t < 20) return 0;
  if (t < 40) return (t - 20) * 1.5;
  if (t < 540) return 30 + 5 * Math.sin(t / 25);
  if (t < 560) return Math.max(0, 30 - (t - 540) * 1.5);
  return 0;
}

const MODES: SpeedLogMode[] = [
  { t0: 2, t1: 120, name: "Stabilize", number: 0, slot: 0 },
  { t0: 120, t1: 300, name: "Loiter", number: 5, slot: 1 },
  { t0: 300, t1: 420, name: "Auto", number: 3, slot: 2 },
  { t0: 420, t1: 520, name: "Loiter", number: 5, slot: 1 },
  { t0: 520, t1: null, name: "RTL", number: 6, slot: 3 },
];

const PARAMS: SpeedLogParam[] = [
  { name: "ATC_RAT_PIT_P", value: 0.135, first: 0.135, changed: false, title: "Pitch axis rate controller P gain", doc: "Pitch axis rate controller P gain.", units: "s/rad", range: "0.0 to 0.35", meaning: "", firstMeaning: "" },
  { name: "ATC_RAT_RLL_P", value: 0.15, first: 0.135, changed: true, title: "Roll axis rate controller P gain", doc: "Roll axis rate controller P gain.", units: "s/rad", range: "0.0 to 0.35", meaning: "", firstMeaning: "" },
  { name: "FS_THR_ENABLE", value: 1, first: 1, changed: false, title: "Throttle Failsafe Enable", doc: "The throttle failsafe allows you to configure a software failsafe.", units: null, range: null, meaning: "Enabled always RTL", firstMeaning: "" },
  { name: "LOG_BITMASK", value: 176126, first: 176126, changed: false, title: "Log bitmask", doc: "Bitmap of what on-board log types to enable.", units: null, range: null, meaning: "Fast Attitude, Medium Attitude, GPS, PM, CTUN, NTUN, IMU, CMD, CURRENT, RCOUT, OPTFLOW, COMPASS, CAMERA, MOTBATT, IMU_FAST", firstMeaning: "" },
  { name: "MOT_SPIN_ARM", value: 0.1, first: 0.08, changed: true, title: "Motor Spin armed", doc: "Point at which the motors start to spin expressed as a number from 0 to 1 in the entire output range.", units: null, range: "0.0 to 0.2", meaning: "", firstMeaning: "" },
];

const MESSAGES_EN: SpeedLogMessage[] = [
  { t: 0.4, kind: "MSG", text: "ArduCopter V4.5.7 (2a3dc4b7)" },
  { t: 0.4, kind: "MSG", text: "ChibiOS: 6a85082c" },
  { t: 0.4, kind: "MSG", text: "CubeOrange 00340036 3231510D 30373935" },
  { t: 18, kind: "EV", text: "Armed (10)" },
  { t: 21, kind: "EV", text: "Left the ground (not landed) (28)" },
  { t: 301, kind: "ERR", text: "EKF check (16): Resolved (back to normal) [code 0]" },
  { t: 520, kind: "ERR", text: "Battery failsafe (6): Failsafe triggered [code 1]" },
  { t: 561, kind: "EV", text: "Landing complete (18)" },
  { t: 565, kind: "EV", text: "Disarmed (11)" },
];

const MESSAGES_PT: SpeedLogMessage[] = MESSAGES_EN.map((m) => ({
  ...m,
  text: m.text
    .replace("Armed", "Armado")
    .replace("Disarmed", "Desarmado")
    .replace("Battery failsafe", "Failsafe de bateria")
    .replace("Failsafe triggered", "Failsafe acionado")
    .replace("code", "código"),
}));

const INFO: SpeedLogInfo = {
  fileName: "00000042.BIN",
  size: 48_234_496,
  firmware: "ArduCopter V4.5.7 (2a3dc4b7)",
  firmwareHash: "2a3dc4b7",
  os: "ChibiOS: 6a85082c",
  board: "CubeOrange 00340036 3231510D 30373935",
  boardId: 140,
  vehicle: "Copter",
  tStart: 0,
  tEnd: DURATION,
  duration: DURATION,
  typeCount: Object.keys(TYPES).length,
  recordCount: 412_345,
};

function makeSeries(type: string, field: string, range: [number, number] | null, buckets: number): SpeedLogSeries | null {
  const gen = TYPES[type]?.fields[field];
  if (!gen) return null;
  const [a, b] = range ?? [0, DURATION];
  const lo = Math.max(0, a - 1 / gen.rate);
  const hi = Math.min(DURATION, b + 1 / gen.rate);
  const total = Math.max(1, Math.round((hi - lo) * gen.rate));
  const step = Math.max(1, Math.ceil(total / (buckets * 2)));
  const t: number[] = [];
  const v: number[] = [];
  let min = Infinity;
  let max = -Infinity;
  let sum = 0;
  for (let i = 0; i <= total; i += 1) {
    const time = lo + i / gen.rate;
    const value = gen.f(time);
    min = Math.min(min, value);
    max = Math.max(max, value);
    sum += value;
    if (i % step === 0) {
      t.push(time);
      v.push(value);
    }
  }
  return { t, v, stats: { min, max, mean: sum / (total + 1) }, tFirst: 0, tLast: DURATION };
}

export function createSpeedLogMock(): SpeedLogMock {
  let open = false;
  let cancelled = false;
  const listeners = new Set<(p: VehicleLogProgress) => void>();
  const emit = (p: VehicleLogProgress) => listeners.forEach((listener) => listener(p));

  // Simulated MAVFTP download at ~400 kB/s, sped up so a big log takes a few seconds.
  const download = async (entry: VehicleLogEntry) => {
    cancelled = false;
    const steps = 25;
    for (let i = 0; i <= steps; i += 1) {
      if (cancelled) {
        emit({ op: "download", name: entry.name, done: 0, total: 0, method: "ftp", rate: 0, finished: true, error: "cancelled" });
        throw new Error("cancelled");
      }
      const done = Math.round((entry.size * i) / steps);
      emit({ op: "download", name: entry.name, done, total: entry.size, method: "ftp", rate: 412_000, finished: i === steps, error: null });
      await sleep(120);
    }
  };
  const need = () => {
    if (!open) throw new Error("no log open");
  };
  return {
    speedlogOpen: async () => {
      await sleep(400);
      open = true;
      return INFO;
    },
    speedlogClose: async () => {
      open = false;
    },
    speedlogInfo: async () => (open ? INFO : null),
    speedlogTree: async (lang) => {
      need();
      const tree: SpeedLogType[] = Object.entries(TYPES).map(([name, def]) => ({
        name,
        base: def.base ?? name,
        instance: def.instance ?? null,
        count: 1000,
        timed: true,
        doc: lang === "pt" && name === "ATT" ? "Atitude canônica do veículo" : def.doc,
        fields: Object.entries(def.fields).map(([field, gen]) => ({ name: field, unit: gen.unit, doc: gen.doc })),
      }));
      tree.push({ name: "UNTM", base: "UNTM", instance: null, count: 3, timed: false, doc: "A message without time", fields: [{ name: "A", unit: null, doc: "" }] });
      return tree;
    },
    speedlogSeries: async (requests, range, buckets = 1500) => {
      need();
      return requests.map((r) => makeSeries(r.type, r.field, range, buckets));
    },
    speedlogModes: async () => (need(), MODES),
    speedlogParams: async () => (need(), PARAMS),
    speedlogMessages: async (lang) => (need(), lang === "pt" ? MESSAGES_PT : MESSAGES_EN),
    logsList: async () => {
      emit({ op: "list", name: "", done: 0, total: 0, method: "ftp", rate: 0, finished: false, error: null });
      await sleep(500);
      emit({ op: "list", name: "", done: VEHICLE_LOGS.length, total: VEHICLE_LOGS.length, method: "ftp", rate: 0, finished: true, error: null });
      return VEHICLE_LOGS;
    },
    logsOpen: async (entry) => {
      await download(entry);
      open = true;
      return { ...INFO, fileName: entry.name, size: entry.size };
    },
    logsSave: async (entry) => {
      await download(entry);
      return `C:/Users/pilot/Documents/${entry.name}`;
    },
    logsCancel: async () => {
      cancelled = true;
    },
    onLogsProgress: async (cb): Promise<Unlisten> => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    speedlogPresets: async () => {
      need();
      return [
        { id: "attitude", fields: [{ type: "ATT", field: "Roll" }, { type: "ATT", field: "Pitch" }, { type: "ATT", field: "Yaw" }] },
        { id: "altitude", fields: [{ type: "CTUN", field: "Alt" }, { type: "CTUN", field: "DAlt" }] },
        { id: "battery", fields: [{ type: "BAT[0]", field: "Volt" }, { type: "BAT[0]", field: "Curr" }] },
        { id: "vibration", fields: [{ type: "VIBE", field: "VibeX" }, { type: "VIBE", field: "VibeY" }, { type: "VIBE", field: "VibeZ" }] },
        { id: "motors", fields: [1, 2, 3, 4].map((c) => ({ type: "RCOU", field: `C${c}` })) },
        { id: "gps", fields: [{ type: "GPS", field: "NSats" }, { type: "GPS", field: "HDop" }] },
      ];
    },
  };
}
