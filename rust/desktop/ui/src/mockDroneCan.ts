/**
 * Mock DroneCAN: two fake nodes (GPS, ESC) with parameters, logs, firmware update, allocation and a bus inspector.
 * Spread into the mock `Backend` by mockBackend.ts, which also calls `reset()` on (dis)connect.
 */
import type { Backend, Unlisten } from "./api";
import {
  idleDroneCan,
  type InspectorField,
  type InspectorMessage,
  type InspectorPoint,
  type InspectorSnapshot,
  type DroneCanAllocation,
  type DroneCanLog,
  type DroneCanNode,
  type DroneCanState,
  type FirmwareUpdateState,
  type NodeParam,
  type NodeParamValue,
} from "./types";

type Context = {
  isConnected: () => boolean;
};

type DroneCanMock = Pick<
  Backend,
  | "dronecanStart"
  | "dronecanStop"
  | "dronecanStatus"
  | "dronecanParams"
  | "dronecanSetParam"
  | "dronecanSaveParams"
  | "dronecanRestart"
  | "dronecanInspectFirmware"
  | "dronecanUpdateFirmware"
  | "dronecanCancelUpdate"
  | "dronecanSetAllocation"
  | "dronecanInspectorEnable"
  | "dronecanInspectorWatch"
  | "dronecanInspectorUnwatch"
  | "dronecanInspectorClear"
  | "onDroneCan"
  | "onDroneCanInspector"
> & { reset: () => void };

type TrafficSpec = {
  node: number;
  kind: 0 | 1 | 2;
  id: number;
  name: string;
  hz: number;
  bytes: number;
  fields: (t: number) => InspectorField[];
};

/** The simulated traffic of the two mock nodes. */
const TRAFFIC: TrafficSpec[] = [
  {
    node: 10,
    kind: 0,
    id: 341,
    name: "NodeStatus",
    hz: 1,
    bytes: 7,
    fields: (t) => [
      { name: "uptime_sec", value: 3600 + Math.floor(t) },
      { name: "health", value: 0 },
      { name: "mode", value: 0 },
      { name: "sub_mode", value: 0 },
      { name: "vendor_specific_status_code", value: 0 },
    ],
  },
  {
    node: 10,
    kind: 0,
    id: 1063,
    name: "Fix2",
    hz: 10,
    bytes: 40,
    fields: (t) => [
      { name: "latitude_deg_1e8", value: -2054756450 + Math.round(Math.sin(t / 5) * 3000) },
      { name: "longitude_deg_1e8", value: -4742026140 + Math.round(Math.cos(t / 5) * 3000) },
      { name: "height_msl_mm", value: 580_000 + Math.round(Math.sin(t) * 1500) },
      { name: "sats_used", value: 14 },
      { name: "pdop", value: 1.1 + Math.abs(Math.sin(t / 7)) * 0.4 },
      { name: "ned_velocity", value: [Math.sin(t) * 2, Math.cos(t) * 2, 0.1] },
    ],
  },
  {
    node: 20,
    kind: 0,
    id: 341,
    name: "NodeStatus",
    hz: 1,
    bytes: 7,
    fields: (t) => [
      { name: "uptime_sec", value: 3600 + Math.floor(t) },
      { name: "health", value: 0 },
      { name: "mode", value: 0 },
      { name: "sub_mode", value: 0 },
      { name: "vendor_specific_status_code", value: 0 },
    ],
  },
  {
    node: 20,
    kind: 0,
    id: 1034,
    name: "Status",
    hz: 50,
    bytes: 16,
    fields: (t) => [
      { name: "error_count", value: 0 },
      { name: "voltage", value: 22.1 - Math.abs(Math.sin(t / 9)) * 0.3 },
      { name: "current", value: 12 + Math.sin(t * 2) * 3 },
      { name: "temperature", value: 311.5 },
      { name: "rpm", value: 5200 + Math.round(Math.sin(t * 3) * 300) },
      { name: "power_rating_pct", value: 38 },
      { name: "esc_index", value: 0 },
    ],
  },
  {
    node: 10,
    kind: 2,
    id: 1,
    name: "GetNodeInfo",
    hz: 0.1,
    bytes: 90,
    fields: () => [
      { name: "status.uptime_sec", value: 3600 },
      { name: "software_version.major", value: 1 },
      { name: "software_version.minor", value: 4 },
      { name: "name", value: "com.hex.here3" },
    ],
  },
];

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const int = (value: number): NodeParamValue => ({ type: "integer", value });
const real = (value: number): NodeParamValue => ({ type: "real", value });

function param(index: number, name: string, value: NodeParamValue, def: NodeParamValue | null, min: number | null, max: number | null): NodeParam {
  return { index, name, value, default: def, min, max };
}

function gpsParams(): NodeParam[] {
  return [
    param(0, "CAN_BAUDRATE", int(1_000_000), int(1_000_000), 10_000, 1_000_000),
    param(1, "GPS_TYPE", int(2), int(1), 0, 20),
    param(2, "GPS_RATE_MS", int(200), int(200), 50, 2000),
    param(3, "GPS_AUTO_CONFIG", { type: "boolean", value: true }, { type: "boolean", value: true }, null, null),
    param(4, "MAG_ENABLE", int(1), int(1), 0, 1),
    param(5, "OFFSET_X", real(0.5), real(0), null, null),
    param(6, "NODE_NAME", { type: "text", value: "gps" }, null, null, null),
  ];
}

function escParams(): NodeParam[] {
  return [
    param(0, "MOTOR_POLES", int(14), int(14), 2, 40),
    param(1, "BRAKE_ENABLE", int(0), int(0), 0, 1),
    param(2, "TIMING_ADV", real(16), real(16), 0, 30),
  ];
}

const NODES: Array<{ id: number; name: string; sw: string; hw: string; uid: string; params: () => NodeParam[] }> = [
  { id: 10, name: "com.hex.here3", sw: "1.4 (1a2b3c4d)", hw: "2.1", uid: "3A0021000C5133323532303700000000", params: gpsParams },
  { id: 20, name: "org.ardupilot.esc.periph", sw: "4.5 (9f8e7d6c)", hw: "1.0", uid: "1F0030004E5035315050303700000000", params: escParams },
];

export function createDroneCanMock({ isConnected }: Context): DroneCanMock {
  const listeners = new Set<(s: DroneCanState) => void>();
  let state: DroneCanState = idleDroneCan;
  let started = 0;
  let timer: ReturnType<typeof setInterval> | null = null;
  const params = new Map<number, NodeParam[]>();
  const logs: DroneCanLog[] = [];
  let update: FirmwareUpdateState | null = null;
  let updateTimer: ReturnType<typeof setInterval> | null = null;
  let allocationEnabled = false;
  const allocations: DroneCanAllocation[] = [];

  // The inspector: counters that grow at each simulated message rate.
  const inspectorListeners = new Set<(s: InspectorSnapshot) => void>();
  const inspectorWatches = new Set<string>();
  let inspectorTimer: ReturnType<typeof setInterval> | null = null;
  let inspectorStart = 0;
  let inspectorCounts: number[] = TRAFFIC.map(() => 0);
  const INSPECTOR_PERIOD_MS = 250;

  function stopInspector() {
    if (inspectorTimer) clearInterval(inspectorTimer);
    inspectorTimer = null;
  }

  function inspectorSnapshot(): InspectorSnapshot {
    const t = (Date.now() - inspectorStart) / 1000;
    const series: Record<string, InspectorPoint[]> = {};
    const messages: InspectorMessage[] = TRAFFIC.map((spec, i) => {
      const step = (spec.hz * INSPECTOR_PERIOD_MS) / 1000;
      inspectorCounts[i] += Math.max(step, Math.random() < step ? 1 : 0);
      const fields = spec.fields(t);
      const samples = Math.min(5, Math.max(1, Math.round(step)));
      for (const key of inspectorWatches) {
        const [node, kind, id, ...rest] = key.split("/");
        if (Number(node) !== spec.node || Number(kind) !== spec.kind || Number(id) !== spec.id) continue;
        const field = rest.join("/");
        const match = /^(.*)\[(\d+)\]$/.exec(field);
        const row = fields.find((f) => f.name === (match ? match[1] : field));
        const raw = row ? (match && Array.isArray(row.value) ? row.value[Number(match[2])] : row.value) : undefined;
        if (typeof raw !== "number") continue;
        series[key] = Array.from({ length: samples }, (_, s): InspectorPoint => [
          (t - ((samples - 1 - s) * INSPECTOR_PERIOD_MS) / samples / 1000) * 1000,
          raw,
        ]);
      }
      return {
        systemId: spec.node,
        componentId: spec.kind,
        componentName: ["Broadcast", "Service requests", "Service responses"][spec.kind],
        messageId: spec.id,
        name: spec.name,
        count: Math.floor(inspectorCounts[i]),
        hz: spec.hz,
        bytesPerSecond: spec.hz * spec.bytes,
        ageMs: Math.round(Math.random() * (1000 / Math.max(spec.hz, 1))),
        fields,
      };
    });
    return { messages, series };
  }

  const requireRunning = () => {
    if (!state.active) throw "DroneCAN is not running; connect to a CAN bus first";
  };

  function build(bus: number): DroneCanState {
    const uptime = Math.floor((Date.now() - started) / 1000);
    const nodes: DroneCanNode[] = NODES.map((n) => ({
      id: n.id,
      online: true,
      name: n.name,
      health: "OK",
      mode: "OPERATIONAL",
      uptimeSec: 3600 + uptime,
      vendorStatus: 0,
      swVersion: n.sw,
      swCrc: "1122334455667788",
      hwVersion: n.hw,
      uniqueId: n.uid,
    }));
    return {
      active: true,
      bus,
      nodes,
      logs: [...logs],
      framesRx: uptime * 40,
      framesTx: uptime * 3,
      progress: null,
      update,
      allocation: { enabled: allocationEnabled, recent: [...allocations] },
    };
  }

  const base64Bytes = (data: string) => Math.floor((data.trim().length * 3) / 4);

  function stopUpdate() {
    if (updateTimer) clearInterval(updateTimer);
    updateTimer = null;
  }

  function refresh() {
    if (state.active) push(build(state.bus ?? 1));
  }

  function push(next: DroneCanState) {
    state = next;
    listeners.forEach((cb) => cb(state));
  }

  function stopTimer() {
    if (timer) clearInterval(timer);
    timer = null;
  }

  return {
    reset() {
      stopTimer();
      stopInspector();
      inspectorWatches.clear();
      stopUpdate();
      update = null;
      allocationEnabled = false;
      allocations.length = 0;
      params.clear();
      logs.length = 0;
      push(idleDroneCan);
    },
    async dronecanStart(bus) {
      await sleep(150);
      if (!isConnected()) throw "no vehicle connected; nothing was sent";
      if (bus < 1 || bus > 2) throw "the CAN bus must be 1 or 2";
      if (state.active) throw "DroneCAN is already running; stop it first";
      started = Date.now();
      logs.length = 0;
      logs.unshift({ timeMs: 0, node: 10, level: "INFO", source: "gps", text: "u-blox M8 detected" });
      push(build(bus));
      timer = setInterval(() => push(build(bus)), 1000);
      return `Relaying CAN${bus}`;
    },
    async dronecanInspectorEnable(enabled) {
      stopInspector();
      if (!enabled) return;
      inspectorStart = Date.now();
      inspectorCounts = TRAFFIC.map(() => 0);
      inspectorTimer = setInterval(() => {
        if (state.active) {
          const snapshot = inspectorSnapshot();
          inspectorListeners.forEach((cb) => cb(snapshot));
        }
      }, INSPECTOR_PERIOD_MS);
    },
    async dronecanInspectorWatch(key) {
      if (key.split("/").length < 4) throw `malformed watch key '${key}' (expected node/kind/message/field)`;
      inspectorWatches.add(key);
    },
    async dronecanInspectorUnwatch(key) {
      inspectorWatches.delete(key);
    },
    async dronecanInspectorClear() {
      inspectorCounts = TRAFFIC.map(() => 0);
    },
    async onDroneCanInspector(cb: (s: InspectorSnapshot) => void): Promise<Unlisten> {
      inspectorListeners.add(cb);
      return () => {
        inspectorListeners.delete(cb);
      };
    },
    async dronecanStop() {
      stopTimer();
      stopInspector();
      stopUpdate();
      update = null;
      allocationEnabled = false;
      push(idleDroneCan);
      return "Stopped";
    },
    dronecanStatus: async () => state,
    async dronecanParams(node) {
      await sleep(600);
      requireRunning();
      const entry = NODES.find((n) => n.id === node);
      if (!entry) throw `node ${node} did not answer (after 0 parameters)`;
      if (!params.has(node)) params.set(node, entry.params());
      return params.get(node)!.map((p) => ({ ...p }));
    },
    async dronecanSetParam(node, name, value) {
      await sleep(200);
      requireRunning();
      const list = params.get(node);
      const found = list?.find((p) => p.name === name);
      if (!list || !found) throw `node ${node} has no parameter ${name}`;
      const numeric = value.type === "integer" || value.type === "real" ? value.value : null;
      if (numeric !== null && ((found.min !== null && numeric < found.min) || (found.max !== null && numeric > found.max))) {
        return { ...found };
      }
      found.value = value;
      return { ...found };
    },
    async dronecanSaveParams(node) {
      await sleep(300);
      requireRunning();
      return `Node ${node} saved its parameters`;
    },
    async dronecanRestart(node) {
      await sleep(300);
      requireRunning();
      logs.unshift({ timeMs: Date.now() - started, node, level: "WARNING", source: "boot", text: "restarting" });
      return `Node ${node} is restarting`;
    },
    async dronecanInspectFirmware(data) {
      await sleep(100);
      const size = base64Bytes(data);
      if (size === 0) throw "the firmware file is empty";
      // A file that starts like JSON is an .apj; the mock gives every file a descriptor CRC.
      return { format: data.startsWith("e") ? "apj" : "bin", size, crc: "AAAABBBBCCCCDDDD", description: null };
    },
    async dronecanUpdateFirmware(node, data, force) {
      await sleep(200);
      requireRunning();
      const entry = NODES.find((n) => n.id === node);
      if (!entry) throw `node ${node} is not on the bus`;
      if (update && (update.phase === "starting" || update.phase === "sending")) throw "another node is being updated; wait for it or cancel it";
      const total = base64Bytes(data);
      if (total === 0) throw "the firmware file is empty";
      void force;
      update = { node, phase: "starting", sent: 0, total, message: "Asking the node to fetch the firmware" };
      refresh();
      stopUpdate();
      updateTimer = setInterval(() => {
        if (!update || update.phase === "failed" || update.phase === "done") return stopUpdate();
        const sent = Math.min(update.total, update.sent + Math.max(256, Math.ceil(update.total / 25)));
        update =
          sent >= update.total
            ? { ...update, sent, phase: "done", message: `Firmware sent to node ${node}; it is restarting` }
            : { ...update, sent, phase: "sending", message: "Sending the firmware" };
        refresh();
      }, 300);
      return `Node ${node} was told to fetch ${total} bytes of firmware`;
    },
    async dronecanCancelUpdate() {
      if (!update || (update.phase !== "starting" && update.phase !== "sending")) throw "no update is running";
      stopUpdate();
      update = { ...update, phase: "failed", message: "cancelled" };
      refresh();
      return "Update cancelled";
    },
    async dronecanSetAllocation(enabled) {
      requireRunning();
      allocationEnabled = enabled;
      if (enabled && allocations.length === 0) {
        allocations.unshift({ uniqueId: "3A0021000C513332353230370000AB01", nodeId: 125 });
      }
      refresh();
      return enabled ? "Handing out node ids to new devices" : "No longer handing out node ids";
    },
    async onDroneCan(cb: (s: DroneCanState) => void): Promise<Unlisten> {
      listeners.add(cb);
      return () => {
        listeners.delete(cb);
      };
    },
  };
}
