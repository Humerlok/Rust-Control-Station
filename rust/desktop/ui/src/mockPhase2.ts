import type { Backend, Unlisten } from "./api";
import { initialMetadata, initialParams } from "./mockParams";
import {
  idleCalibration,
  type CalibrationKind,
  type CalibrationState,
  type ParamEntry,
  type ParamMeta,
  type ParamsProgress,
} from "./types";

type Context = {
  isConnected: () => boolean;
  isArmed: () => boolean;
  emitStatus: (severity: number, text: string) => void;
};

type Phase2 = Pick<
  Backend,
  | "paramsRefresh"
  | "paramsList"
  | "paramSet"
  | "paramsMetadata"
  | "paramsParseFile"
  | "paramsFormatFile"
  | "calibrationStart"
  | "calibrationAccelConfirm"
  | "calibrationCompassAccept"
  | "calibrationCompassCancel"
  | "calibrationCancel"
  | "radioCalibrationFinish"
  | "calibrationStatus"
  | "onParamsProgress"
  | "onCalibration"
> & { reset: () => void };

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const ACCEL_POSITIONS = ["Level", "Left", "Right", "Nose down", "Nose up", "Back"];
const FILE_BLACKLIST = /^(SYSID_SW_MREV|WP_TOTAL|CMD_TOTAL|FENCE_TOTAL|SYS_NUM_RESETS|ARSPD_OFFSET|GND_ABS_PRESS|GND_TEMP|BARO\d?_GND_PRESS|BARO_GND_TEMP|CMD_INDEX|LOG_LASTFILE|FORMAT_VERSION)$/;

function formatNumber(value: number): string {
  return String(Number(value.toPrecision(7)));
}

export function createPhase2Mock({ isConnected, isArmed, emitStatus }: Context): Phase2 {
  const progressListeners = new Set<(p: ParamsProgress) => void>();
  const calibrationListeners = new Set<(c: CalibrationState) => void>();
  const metadata: Record<string, ParamMeta> = initialMetadata();
  let table: ParamEntry[] = [];
  let loaded = false;
  let downloading = false;

  let cal: CalibrationState = idleCalibration;
  let timer: ReturnType<typeof setInterval> | null = null;
  let accelIndex = 0;

  const requireVehicle = () => {
    if (!isConnected()) throw "no vehicle connected; nothing was sent";
  };

  function setCal(next: Partial<CalibrationState>) {
    cal = { ...cal, ...next };
    calibrationListeners.forEach((cb) => cb(cal));
  }

  function stopTimer() {
    if (timer) clearInterval(timer);
    timer = null;
  }

  function finish(state: "succeeded" | "failed" | "idle", message: string) {
    stopTimer();
    setCal({ state, message, accelPosition: null });
  }

  function askAccelPosition(index: number) {
    accelIndex = index;
    setCal({
      state: "waiting",
      message: `Place the vehicle ${ACCEL_POSITIONS[index - 1].toLowerCase()} and press Done`,
      accelPosition: { index, name: ACCEL_POSITIONS[index - 1] },
    });
    emitStatus(6, `Place vehicle ${ACCEL_POSITIONS[index - 1]} and press any key`);
  }

  function startCompass() {
    const compass = [0, 1].map((compassId) => ({ compassId, completionPct: 0, status: "Running", fitness: null as number | null, autosaved: false }));
    setCal({ compass, message: "Rotate the vehicle in all directions" });
    timer = setInterval(() => {
      const next = cal.compass.map((c) => {
        if (c.completionPct >= 100) return c;
        const pct = Math.min(100, c.completionPct + 4 + c.compassId * 2 + Math.random() * 3);
        return pct >= 100 ? { ...c, completionPct: 100, status: "Success", fitness: 4.2 + c.compassId * 1.7 } : { ...c, completionPct: pct };
      });
      setCal({ compass: next });
      if (next.every((c) => c.completionPct >= 100)) {
        stopTimer();
        setCal({ state: "waiting", message: "Calibration complete. Press Accept to save the offsets." });
        emitStatus(6, "Mag cal: calibration complete, press accept");
      }
    }, 300);
  }

  function startRadio() {
    const radio = Array.from({ length: 16 }, (_, i) => ({ channel: i + 1, min: 1500, max: 1500, value: 1500 }));
    setCal({ radio, message: "Move all sticks and switches to their extremes, then press Finish" });
    let t = 0;
    timer = setInterval(() => {
      t += 1;
      setCal({
        radio: cal.radio.map((r) => {
          const swing = r.channel <= 4 ? 400 : r.channel <= 8 ? 380 : 0;
          const value = Math.round(1500 + swing * Math.sin(t * 0.25 + r.channel));
          return { ...r, value, min: Math.min(r.min, value), max: Math.max(r.max, value) };
        }),
      });
    }, 200);
  }

  const params: Phase2 = {
    reset() {
      table = [];
      loaded = false;
      downloading = false;
      stopTimer();
      cal = idleCalibration;
    },

    async paramsRefresh() {
      requireVehicle();
      if (downloading) throw "a download is already in progress";
      downloading = true;
      const all = initialParams();
      const total = all.length;
      try {
        const steps = 30;
        for (let step = 1; step <= steps; step += 1) {
          await sleep(100);
          requireVehicle();
          const received = Math.round((total * step) / steps);
          progressListeners.forEach((cb) => cb({ received, total, done: step === steps, error: null }));
        }
        table = all;
        loaded = true;
        return total;
      } finally {
        downloading = false;
      }
    },

    paramsList: async () => (loaded && isConnected() ? table.map((entry) => ({ ...entry })) : []),

    async paramSet(name, value) {
      await sleep(80);
      requireVehicle();
      const entry = table.find((e) => e.name === name);
      if (!entry) throw `unknown parameter ${name}`;
      if (metadata[name]?.readOnly) throw `${name} is read-only`;
      const meta = metadata[name];
      let echoed = value;
      if (meta?.min !== null && meta?.min !== undefined) echoed = Math.max(meta.min, echoed);
      if (meta?.max !== null && meta?.max !== undefined) echoed = Math.min(meta.max, echoed);
      if (entry.paramType !== "REAL32") echoed = Math.round(echoed);
      entry.value = Math.fround(echoed);
      return entry.value;
    },

    async paramsMetadata() {
      await sleep(400);
      return isConnected() ? metadata : {};
    },

    async paramsParseFile(text) {
      const result: { params: Array<{ name: string; value: number }>; skipped: string[] } = { params: [], skipped: [] };
      for (const raw of text.split(/\r?\n/)) {
        const line = raw.split("#")[0].trim();
        if (!line) continue;
        const [name, value] = line.split(/[\s,]+/);
        const number = Number(value);
        if (!name || value === undefined || !Number.isFinite(number)) continue;
        if (FILE_BLACKLIST.test(name)) result.skipped.push(name);
        else result.params.push({ name, value: number });
      }
      return result;
    },

    async paramsFormatFile() {
      return [...table]
        .sort((a, b) => a.name.localeCompare(b.name, "en", { numeric: true }))
        .map((e) => `${e.name},${formatNumber(e.value)}`)
        .join("\n");
    },

    async calibrationStart(kind: CalibrationKind) {
      await sleep(120);
      requireVehicle();
      if (isArmed()) throw "denied: vehicle is armed";
      if (cal.state === "running" || cal.state === "waiting") throw "another calibration is in progress";
      stopTimer();
      cal = { ...idleCalibration, kind, state: "running", message: "Started" };
      setCal({});
      emitStatus(6, `Calibration ${kind} started`);
      switch (kind) {
        case "accel_full":
          setTimeout(() => cal.kind === kind && askAccelPosition(1), 600);
          break;
        case "accel_simple":
          setTimeout(() => cal.kind === kind && askAccelPosition(1), 600);
          break;
        case "accel_level":
        case "gyro":
        case "baro":
          setTimeout(() => {
            if (cal.kind !== kind || cal.state !== "running") return;
            emitStatus(6, `${kind === "baro" ? "Barometer" : kind === "gyro" ? "Gyro" : "Level"} calibration successful`);
            finish("succeeded", "Calibration successful");
          }, 1500);
          break;
        case "compass":
          startCompass();
          break;
        case "radio":
          startRadio();
          break;
        case "esc":
          setTimeout(() => {
            emitStatus(5, "ESC_CALIBRATION set to 3: power-cycle the vehicle");
            finish("succeeded", "ESC_CALIBRATION set. Power-cycle the vehicle to calibrate.");
          }, 400);
          break;
      }
      return "Started";
    },

    async calibrationAccelConfirm() {
      await sleep(100);
      if (cal.state !== "waiting" || !cal.accelPosition) throw "the vehicle is not waiting for a position";
      const last = cal.kind === "accel_simple" ? 1 : 6;
      setCal({ state: "running", message: "Sampling…" });
      setTimeout(() => {
        if (!cal.kind?.startsWith("accel") || cal.state !== "running") return;
        if (accelIndex >= last) {
          emitStatus(6, "Calibration successful");
          finish("succeeded", "Calibration successful");
        } else {
          askAccelPosition(accelIndex + 1);
        }
      }, 700);
      return "Accepted";
    },

    async calibrationCompassAccept() {
      await sleep(100);
      if (cal.kind !== "compass" || (cal.state !== "waiting" && cal.state !== "running")) throw "no compass calibration to accept";
      setCal({ compass: cal.compass.map((c) => ({ ...c, autosaved: true })) });
      emitStatus(6, "Mag cal: offsets saved");
      finish("succeeded", "Compass offsets saved. Reboot the vehicle.");
      return "Accepted";
    },

    async calibrationCompassCancel() {
      await sleep(100);
      finish("idle", "Compass calibration cancelled");
      return "Cancelled";
    },

    async calibrationCancel() {
      await sleep(100);
      finish("idle", "Calibration cancelled");
      return "Cancelled";
    },

    async radioCalibrationFinish() {
      await sleep(150);
      if (cal.kind !== "radio" || cal.state !== "running") throw "no radio calibration in progress";
      emitStatus(6, "Radio calibration saved: RC1..RC8 MIN/MAX/TRIM updated");
      finish("succeeded", "RCn_MIN/MAX/TRIM written");
      return "Accepted";
    },

    calibrationStatus: async () => cal,

    async onParamsProgress(cb): Promise<Unlisten> {
      progressListeners.add(cb);
      return () => {
        progressListeners.delete(cb);
      };
    },

    async onCalibration(cb): Promise<Unlisten> {
      calibrationListeners.add(cb);
      return () => {
        calibrationListeners.delete(cb);
      };
    },
  };
  return params;
}
