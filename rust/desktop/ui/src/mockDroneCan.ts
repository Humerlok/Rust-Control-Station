import type { Backend, Unlisten } from "./api";
import { idleDroneCan, type DroneCanLog, type DroneCanNode, type DroneCanState, type NodeParam, type NodeParamValue } from "./types";

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
  | "onDroneCan"
> & { reset: () => void };

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
    };
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
    async dronecanStop() {
      stopTimer();
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
    async onDroneCan(cb: (s: DroneCanState) => void): Promise<Unlisten> {
      listeners.add(cb);
      return () => {
        listeners.delete(cb);
      };
    },
  };
}
