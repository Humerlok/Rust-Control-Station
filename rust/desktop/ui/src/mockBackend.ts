import type { Backend, Unlisten } from "./api";
import type {
  ActionInfo,
  ConnectionConfig,
  FieldInfo,
  LinkInfo,
  StatusText,
  Telemetry,
} from "./types";
import { emptyLink, emptyTelemetry } from "./types";
import { createInspectorMock } from "./mockInspector";
import { createFirmwareMock } from "./mockFirmware";
import { createFtpMock } from "./mockFtp";
import { createMissionMock } from "./mockMission";
import { createPhase2Mock } from "./mockPhase2";

const CENTER = { lat: -21.92, lon: -47.93 };
const RADIUS_DEG = 0.0012;
const MODES = ["Stabilize", "Acro", "AltHold", "Auto", "Guided", "Loiter", "RTL", "Circle", "Land", "PosHold", "Brake", "Smart_RTL"];

const QUICK_FIELDS: FieldInfo[] = [
  { key: "alt", label: "Altitude (rel)", unit: "m" },
  { key: "altasl", label: "Altitude (MSL)", unit: "m" },
  { key: "groundspeed", label: "Ground speed", unit: "m/s" },
  { key: "airspeed", label: "Air speed", unit: "m/s" },
  { key: "verticalspeed", label: "Vertical speed", unit: "m/s" },
  { key: "roll", label: "Roll", unit: "deg" },
  { key: "pitch", label: "Pitch", unit: "deg" },
  { key: "yaw", label: "Yaw", unit: "deg" },
  { key: "heading", label: "Heading", unit: "deg" },
  { key: "wp_dist", label: "Distance to WP", unit: "m" },
  { key: "wpno", label: "Current WP", unit: "" },
  { key: "nav_bearing", label: "Nav bearing", unit: "deg" },
  { key: "dist_to_home", label: "Distance to home", unit: "m" },
  { key: "battery_voltage", label: "Battery voltage", unit: "V" },
  { key: "battery_current", label: "Battery current", unit: "A" },
  { key: "battery_remaining", label: "Battery remaining", unit: "%" },
  { key: "gpsstatus", label: "GPS fix type", unit: "" },
  { key: "satcount", label: "Satellites", unit: "" },
  { key: "gpshdop", label: "GPS HDOP", unit: "" },
  { key: "throttle", label: "Throttle", unit: "%" },
];

const ACTIONS: ActionInfo[] = [
  { name: "Loiter_Unlim", label: "Loiter Unlimited", confirm: true, danger: false },
  { name: "Return_To_Launch", label: "Return To Launch", confirm: true, danger: false },
  { name: "Preflight_Calibration", label: "Preflight Calibration", confirm: true, danger: false },
  { name: "Mission_Start", label: "Mission Start", confirm: true, danger: false },
  { name: "Trigger_Camera", label: "Trigger Camera", confirm: false, danger: false },
  { name: "Battery_Reset", label: "Battery Reset", confirm: true, danger: false },
  { name: "Do_Parachute", label: "Parachute", confirm: true, danger: true },
  { name: "Terminate_Flight", label: "Terminate Flight", confirm: true, danger: true },
  { name: "Format_SD_Card", label: "Format SD Card", confirm: true, danger: true },
  { name: "Reboot_Mass_Storage", label: "Reboot Mass Storage", confirm: true, danger: false },
];

const MESSAGES: Array<[number, string]> = [
  [6, "ArduCopter V4.5.7 (2a3dc4b7)"],
  [6, "Frame: QUAD/X"],
  [5, "EKF3 IMU0 is using GPS"],
  [4, "PreArm: Compasses inconsistent"],
  [6, "Mission: 5 WP"],
  [3, "Battery 1 is low 21.8V used 8420 mAh"],
  [6, "Reached waypoint #3"],
  [7, "AHRS: EKF3 active"],
];

type Listeners = { telemetry: Set<(t: Telemetry) => void>; statustext: Set<(m: StatusText) => void>; link: Set<(l: LinkInfo) => void> };

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export function createMockBackend(): Backend {
  const listeners: Listeners = { telemetry: new Set(), statustext: new Set(), link: new Set() };
  let connected = true;
  let connecting = false;
  let armed = true;
  let mode = "Auto";
  let description = "Mock link (simulated copter)";
  let t = 0;
  let packets = 0;
  let lastTelemetry: Telemetry = emptyTelemetry;
  let lastLink: LinkInfo = emptyLink;

  function buildTelemetry(): Telemetry {
    if (!connected) return emptyTelemetry;
    const a = t * 0.15;
    const lat = CENTER.lat + RADIUS_DEG * Math.sin(a);
    const lon = CENTER.lon + (RADIUS_DEG * Math.cos(a)) / Math.cos((CENTER.lat * Math.PI) / 180);
    const heading = (((-a * 180) / Math.PI) % 360 + 360) % 360;
    const alt = armed ? 42 + 6 * Math.sin(t * 0.2) : 0;
    const distHome = Math.hypot(RADIUS_DEG * 111_000 * Math.sin(a), RADIUS_DEG * 111_000 * Math.cos(a));
    return {
      vehicle: { systemId: 1, componentId: 1 },
      connected: true,
      armed,
      mode,
      vehicleType: "Copter",
      availableModes: MODES,
      fields: {
        roll: armed ? 18 * Math.sin(t * 0.5) : 0,
        pitch: armed ? 6 * Math.sin(t * 0.3) : 0,
        yaw: heading,
        heading,
        alt,
        altasl: alt + 580,
        groundspeed: armed ? 8.5 + Math.sin(t * 0.4) : 0,
        airspeed: armed ? 8.4 : 0,
        verticalspeed: armed ? 1.2 * Math.cos(t * 0.2) : 0,
        battery_voltage: 22.4 - (t % 600) * 0.001,
        battery_current: armed ? 18.5 : 0.4,
        battery_remaining: 78,
        gpsstatus: 4,
        satcount: 14,
        gpshdop: 0.8,
        wp_dist: 120 + 40 * Math.sin(t * 0.3),
        wpno: 3,
        nav_bearing: heading,
        throttle: armed ? 52 : 0,
        dist_to_home: distHome,
        ...Object.fromEntries(
          Array.from({ length: 8 }, (_, i) => [`ch${i + 1}in`, Math.round(1500 + (i < 4 ? 300 : 0) * Math.sin(t * 0.3 + i))]),
        ),
      },
      position: { lat, lon, altM: alt },
      home: { lat: CENTER.lat, lon: CENTER.lon, altM: 580 },
      vehicles: [{ systemId: 1, componentId: 1, vehicleType: "Copter" }],
    };
  }

  function buildLink(): LinkInfo {
    return {
      state: connecting ? "connecting" : connected ? "connected" : "disconnected",
      description: connected ? description : "",
      error: null,
      stats: {
        bytesRx: packets * 38,
        bytesTx: Math.floor(packets / 20) * 21,
        packetsRx: packets,
        packetsLost: Math.floor(packets / 800),
        badCrc: 0,
        packetsPerSecond: connected ? 62 : 0,
        bytesPerSecond: connected ? 2350 : 0,
        qualityPercent: connected ? 99 : 0,
      },
    };
  }

  function emitStatus(severity: number, text: string) {
    const message: StatusText = { timeMs: Date.now(), severity, text, systemId: 1, componentId: 1 };
    listeners.statustext.forEach((cb) => cb(message));
  }

  function emitLink() {
    lastLink = buildLink();
    listeners.link.forEach((cb) => cb(lastLink));
  }

  let messageIndex = 0;
  setInterval(() => {
    t += 0.1;
    packets += 6;
    lastTelemetry = buildTelemetry();
    listeners.telemetry.forEach((cb) => cb(lastTelemetry));
  }, 100);
  setInterval(() => {
    emitLink();
    if (connected) {
      const [severity, text] = MESSAGES[messageIndex % MESSAGES.length];
      messageIndex += 1;
      emitStatus(severity, text);
    }
  }, 2500);

  lastTelemetry = buildTelemetry();
  lastLink = buildLink();

  function requireConnected() {
    if (!connected) throw "no vehicle connected";
  }

  const phase2 = createPhase2Mock({ isConnected: () => connected, isArmed: () => armed, emitStatus });

  async function command(label: string): Promise<string> {
    await sleep(150);
    requireConnected();
    emitStatus(6, `${label} accepted`);
    return "Accepted";
  }

  function subscribe<T>(set: Set<(payload: T) => void>) {
    return async (cb: (payload: T) => void): Promise<Unlisten> => {
      set.add(cb);
      return () => {
        set.delete(cb);
      };
    };
  }

  return {
    listSerialPorts: async () => [
      { name: "COM3", description: "USB Serial Device (CubeOrange+)" },
      { name: "COM7", description: "Standard Serial over Bluetooth link" },
    ],
    async connect(config: ConnectionConfig) {
      if (config.type === "replay" && config.path.trim() === "") throw "replay file path is empty";
      phase2.reset();
      connecting = true;
      connected = false;
      emitLink();
      await sleep(600);
      connecting = false;
      connected = true;
      description = `Mock ${config.type} link`;
      emitLink();
      emitStatus(6, "Heartbeat received from system 1");
    },
    async disconnect() {
      connected = false;
      connecting = false;
      phase2.reset();
      emitLink();
    },
    linkStatus: async () => lastLink,
    telemetrySnapshot: async () => lastTelemetry,
    selectVehicle: async () => undefined,
    quickFields: async () => QUICK_FIELDS,
    listActions: async () => ACTIONS,
    async arm() {
      await sleep(150);
      requireConnected();
      armed = true;
      emitStatus(6, "Arming motors");
      return "Accepted";
    },
    async disarm(force) {
      await sleep(150);
      requireConnected();
      if (!force && lastTelemetry.fields.alt > 2) throw "Denied: vehicle is flying";
      armed = false;
      emitStatus(6, "Disarming motors");
      return "Accepted";
    },
    async setMode(next) {
      await sleep(100);
      requireConnected();
      mode = next;
      emitStatus(6, `Mode changed to ${next}`);
      return "Accepted";
    },
    setWaypoint: (seq) => command(`Set WP ${seq}`),
    restartMission: () => command("Restart mission"),
    doAction: (name) => command(name),
    async reboot() {
      await sleep(150);
      requireConnected();
      armed = false;
      emitStatus(4, "Rebooting autopilot");
      return "Accepted";
    },
    takeoff: (altitudeM) => command(`Takeoff to ${altitudeM} m`),
    flyTo: () => command("Fly to here"),
    setHomeHere: () => command("Set home here"),
    paramsRefresh: phase2.paramsRefresh,
    paramsList: phase2.paramsList,
    paramSet: phase2.paramSet,
    paramsMetadata: phase2.paramsMetadata,
    paramsParseFile: phase2.paramsParseFile,
    paramsFormatFile: phase2.paramsFormatFile,
    calibrationStart: phase2.calibrationStart,
    calibrationAccelConfirm: phase2.calibrationAccelConfirm,
    calibrationCompassAccept: phase2.calibrationCompassAccept,
    calibrationCompassCancel: phase2.calibrationCompassCancel,
    calibrationCancel: phase2.calibrationCancel,
    radioCalibrationFinish: phase2.radioCalibrationFinish,
    calibrationStatus: phase2.calibrationStatus,
    onTelemetry: subscribe(listeners.telemetry),
    onStatusText: subscribe(listeners.statustext),
    onLink: subscribe(listeners.link),
    onParamsProgress: phase2.onParamsProgress,
    onCalibration: phase2.onCalibration,
    ...createInspectorMock(),
    ...createMissionMock(),
    ...createFtpMock(),
    ...createFirmwareMock(),
  };
}
