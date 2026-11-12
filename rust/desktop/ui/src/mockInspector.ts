/**
 * Mock MAVLink Inspector: simulated messages firing at their own rates, aggregated into snapshots.
 * Spread into the mock `Backend` by mockBackend.ts; the shape matches `InspectorSnapshot` in types.ts.
 */
import type { Backend, Unlisten } from "./api";
import type { InspectorField, InspectorMessage, InspectorPoint, InspectorSnapshot } from "./types";

type InspectorMock = Pick<
  Backend,
  "inspectorEnable" | "inspectorWatch" | "inspectorUnwatch" | "inspectorClear" | "onInspector"
>;

type Definition = {
  systemId: number;
  componentId: number;
  componentName: string;
  messageId: number;
  name: string;
  hz: number;
  size: number;
  fields: (t: number) => InspectorField[];
};

// Messages are generated every TICK_MS; snapshots are emitted every SNAPSHOT_MS, hz is averaged over RATE_WINDOW_MS.
const TICK_MS = 50;
const SNAPSHOT_MS = 250;
const RATE_WINDOW_MS = 3000;

const f = (name: string, value: InspectorField["value"]): InspectorField => ({ name, value });
const wave = (t: number, amplitude: number, period: number, phase = 0) =>
  amplitude * Math.sin((2 * Math.PI * t) / period + phase);
const round = (value: number, digits: number) => Number(value.toFixed(digits));

const AUTOPILOT = "Autopilot 1 (1)";
const STATUS_TEXTS = [
  "EKF3 IMU0 is using GPS",
  "PreArm: Need Position Estimate",
  "Frame: QUAD/X",
  "Throttle disarmed",
  "RC12: MotorEStop LOW",
];

function definitions(): Definition[] {
  const autopilot = { systemId: 1, componentId: 1, componentName: AUTOPILOT };
  let statusIndex = 0;
  return [
    {
      ...autopilot,
      messageId: 0,
      name: "HEARTBEAT",
      hz: 1,
      size: 21,
      fields: () => [
        f("type", "MAV_TYPE_QUADROTOR"),
        f("autopilot", "MAV_AUTOPILOT_ARDUPILOTMEGA"),
        f("base_mode", 217),
        f("custom_mode", 5),
        f("system_status", "MAV_STATE_ACTIVE"),
        f("mavlink_version", 3),
      ],
    },
    {
      ...autopilot,
      messageId: 1,
      name: "SYS_STATUS",
      hz: 2,
      size: 43,
      fields: (t) => [
        f("onboard_control_sensors_present", 1073741823),
        f("onboard_control_sensors_enabled", 1073741823),
        f("onboard_control_sensors_health", 1073741823),
        f("load", Math.round(380 + wave(t, 30, 9))),
        f("voltage_battery", Math.round(15800 - t * 4 + wave(t, 40, 3))),
        f("current_battery", Math.round(1450 + wave(t, 120, 5))),
        f("battery_remaining", Math.max(0, Math.round(92 - t / 20))),
        f("drop_rate_comm", 0),
        f("errors_comm", 0),
      ],
    },
    {
      ...autopilot,
      messageId: 30,
      name: "ATTITUDE",
      hz: 10,
      size: 40,
      fields: (t) => [
        f("time_boot_ms", Math.round(t * 1000)),
        f("roll", round(wave(t, 0.35, 7), 5)),
        f("pitch", round(wave(t, 0.2, 5, 1), 5)),
        f("yaw", round(wave(t, 1.5, 23, 0.4), 5)),
        f("rollspeed", round(wave(t, 0.1, 3), 5)),
        f("pitchspeed", round(wave(t, 0.08, 4), 5)),
        f("yawspeed", round(wave(t, 0.05, 6), 5)),
      ],
    },
    {
      ...autopilot,
      messageId: 33,
      name: "GLOBAL_POSITION_INT",
      hz: 5,
      size: 40,
      fields: (t) => [
        f("time_boot_ms", Math.round(t * 1000)),
        f("lat", Math.round((-21.92 + 0.0005 * Math.sin(t / 30)) * 1e7)),
        f("lon", Math.round((-47.93 + 0.0005 * Math.cos(t / 30)) * 1e7)),
        f("alt", Math.round(580000 + wave(t, 2500, 17))),
        f("relative_alt", Math.round(30000 + wave(t, 2500, 17))),
        f("vx", Math.round(wave(t, 300, 11))),
        f("vy", Math.round(wave(t, 250, 13, 1))),
        f("vz", Math.round(wave(t, 80, 17))),
        f("hdg", Math.round((((wave(t, 1.5, 23, 0.4) * 180) / Math.PI + 360) % 360) * 100)),
      ],
    },
    {
      ...autopilot,
      messageId: 74,
      name: "VFR_HUD",
      hz: 4,
      size: 28,
      fields: (t) => [
        f("airspeed", round(4.5 + wave(t, 1.2, 11), 3)),
        f("groundspeed", round(4.2 + wave(t, 1.5, 11), 3)),
        f("alt", round(580 + wave(t, 2.5, 17), 2)),
        f("climb", round(wave(t, 0.8, 17), 3)),
        f("heading", Math.round(((wave(t, 85, 23, 0.4) + 360) % 360))),
        f("throttle", Math.round(52 + wave(t, 6, 8))),
      ],
    },
    {
      ...autopilot,
      messageId: 65,
      name: "RC_CHANNELS",
      hz: 2,
      size: 42,
      fields: (t) => [
        f("time_boot_ms", Math.round(t * 1000)),
        f("chancount", 16),
        ...[0, 1, 2, 3, 4, 5, 6, 7].map((i) =>
          f(`chan${i + 1}_raw`, Math.round(1500 + (i === 2 ? 0 : wave(t, 300, 6 + i, i)) + (i === 2 ? wave(t, 200, 10) - 100 : 0))),
        ),
        f("rssi", 255),
      ],
    },
    {
      ...autopilot,
      messageId: 147,
      name: "BATTERY_STATUS",
      hz: 1,
      size: 41,
      fields: (t) => [
        f("id", 0),
        f("battery_function", "MAV_BATTERY_FUNCTION_ALL"),
        f("type", "MAV_BATTERY_TYPE_LIPO"),
        f("temperature", 3200),
        f(
          "voltages",
          [0, 1, 2, 3, 4, 5, 6, 7, 8, 9].map((i) => (i < 4 ? Math.round(3950 - t * 1 + wave(t, 15, 4, i)) : 65535)),
        ),
        f("current_battery", Math.round(1450 + wave(t, 120, 5))),
        f("battery_remaining", Math.max(0, Math.round(92 - t / 20))),
      ],
    },
    {
      ...autopilot,
      messageId: 253,
      name: "STATUSTEXT",
      hz: 1 / 7,
      size: 54,
      fields: () => {
        const text = STATUS_TEXTS[statusIndex % STATUS_TEXTS.length];
        statusIndex += 1;
        return [f("severity", "MAV_SEVERITY_INFO"), f("text", text)];
      },
    },
    {
      systemId: 1,
      componentId: 191,
      componentName: "Onboard computer (191)",
      messageId: 0,
      name: "HEARTBEAT",
      hz: 1,
      size: 21,
      fields: () => [
        f("type", "MAV_TYPE_ONBOARD_CONTROLLER"),
        f("autopilot", "MAV_AUTOPILOT_INVALID"),
        f("base_mode", 0),
        f("custom_mode", 0),
        f("system_status", "MAV_STATE_ACTIVE"),
        f("mavlink_version", 3),
      ],
    },
  ];
}

type Runtime = {
  def: Definition;
  nextDue: number;
  count: number;
  stamps: number[];
  lastAt: number;
  fields: InspectorField[];
};

/** Resolves `field` or `field[3]` to a number. */
function numberOf(fields: InspectorField[], spec: string): number | undefined {
  const match = /^(.+)\[(\d+)\]$/.exec(spec);
  const field = fields.find((candidate) => candidate.name === (match ? match[1] : spec));
  if (!field) return undefined;
  const value = match && Array.isArray(field.value) ? field.value[Number(match[2])] : field.value;
  return typeof value === "number" ? value : undefined;
}

export function createInspectorMock(): InspectorMock {
  const listeners = new Set<(s: InspectorSnapshot) => void>();
  const watched = new Set<string>();
  let timer: ReturnType<typeof setInterval> | undefined;
  let origin = 0;
  let lastSnapshot = 0;
  let runtimes: Runtime[] = [];
  let pending: Record<string, InspectorPoint[]> = {};

  function reset(now: number) {
    origin = now;
    lastSnapshot = now;
    pending = {};
    runtimes = definitions().map((def, index) => ({
      def,
      nextDue: now + (index * 70) % Math.max(1, 1000 / def.hz),
      count: 0,
      stamps: [],
      lastAt: 0,
      fields: [],
    }));
  }

  function fire(runtime: Runtime, at: number) {
    const t = (at - origin) / 1000;
    runtime.fields = runtime.def.fields(t);
    runtime.count += 1;
    runtime.lastAt = at;
    runtime.stamps.push(at);
    const prefix = `${runtime.def.systemId}/${runtime.def.componentId}/${runtime.def.messageId}/`;
    for (const key of watched) {
      if (!key.startsWith(prefix)) continue;
      const value = numberOf(runtime.fields, key.slice(prefix.length));
      if (value === undefined) continue;
      (pending[key] ??= []).push([at - origin, value]);
    }
  }

  function snapshot(now: number): InspectorSnapshot {
    const elapsed = Math.min(RATE_WINDOW_MS, Math.max(500, now - origin)) / 1000;
    const messages: InspectorMessage[] = [];
    for (const runtime of runtimes) {
      if (runtime.count === 0) continue;
      runtime.stamps = runtime.stamps.filter((stamp) => stamp > now - RATE_WINDOW_MS);
      const hz = runtime.stamps.length / elapsed;
      messages.push({
        systemId: runtime.def.systemId,
        componentId: runtime.def.componentId,
        componentName: runtime.def.componentName,
        messageId: runtime.def.messageId,
        name: runtime.def.name,
        count: runtime.count,
        hz: round(hz, 2),
        bytesPerSecond: Math.round(hz * runtime.def.size),
        ageMs: Math.round(now - runtime.lastAt),
        fields: runtime.fields,
      });
    }
    const series = pending;
    pending = {};
    return { messages, series };
  }

  function tick() {
    const now = performance.now();
    for (const runtime of runtimes) {
      while (runtime.nextDue <= now) {
        fire(runtime, runtime.nextDue);
        runtime.nextDue += 1000 / runtime.def.hz;
      }
    }
    if (now - lastSnapshot >= SNAPSHOT_MS) {
      lastSnapshot = now;
      const next = snapshot(now);
      listeners.forEach((listener) => listener(next));
    }
  }

  return {
    inspectorEnable: async (enabled) => {
      if (enabled && timer === undefined) {
        reset(performance.now());
        timer = setInterval(tick, TICK_MS);
      } else if (!enabled && timer !== undefined) {
        clearInterval(timer);
        timer = undefined;
        watched.clear();
        pending = {};
      }
    },
    inspectorWatch: async (key) => {
      watched.add(key);
    },
    inspectorUnwatch: async (key) => {
      watched.delete(key);
      delete pending[key];
    },
    inspectorClear: async () => {
      if (timer !== undefined) reset(performance.now());
    },
    onInspector: async (cb): Promise<Unlisten> => {
      listeners.add(cb);
      return () => {
        listeners.delete(cb);
      };
    },
  };
}
