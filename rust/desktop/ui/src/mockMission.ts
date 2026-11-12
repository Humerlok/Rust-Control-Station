/**
 * Mock mission/fence/rally: in-memory upload/download round-trip, a small command catalog, .waypoints parse/save.
 * Spread into the mock `Backend` by mockBackend.ts.
 */
import type { Backend, Unlisten } from "./api";
import { browserDownload } from "./mockDownload";
import type { MissionCommandInfo, MissionItem, MissionKind, MissionProgress } from "./types";

type MissionMock = Pick<
  Backend,
  | "missionDownload"
  | "missionUpload"
  | "missionClear"
  | "missionCommands"
  | "missionParseFile"
  | "missionSaveFile"
  | "onMissionProgress"
>;

// Same centre as the simulated vehicle in mockBackend.ts.
const CENTER = { lat: -21.92, lon: -47.93 };
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

const cmd = (
  id: number,
  name: string,
  description: string,
  labels: Partial<Record<"p1" | "p2" | "p3" | "p4" | "x" | "y" | "z", string>> = {},
): MissionCommandInfo => ({
  id,
  name,
  description,
  p1: labels.p1 ?? null,
  p2: labels.p2 ?? null,
  p3: labels.p3 ?? null,
  p4: labels.p4 ?? null,
  x: labels.x ?? null,
  y: labels.y ?? null,
  z: labels.z ?? null,
});

const POS = { x: "Lat", y: "Lon", z: "Alt" };

const CATALOG: MissionCommandInfo[] = [
  cmd(16, "WAYPOINT", "Fly to a position.", { p1: "Delay", p4: "Yaw Angle", ...POS }),
  cmd(17, "LOITER_UNLIM", "Loiter at a position until commanded otherwise.", POS),
  cmd(18, "LOITER_TURNS", "Circle a position for a number of turns.", { p1: "Turns", p3: "Radius", ...POS }),
  cmd(19, "LOITER_TIME", "Loiter at a position for a time.", { p1: "Time (s)", ...POS }),
  cmd(20, "RETURN_TO_LAUNCH", "Return to the launch position."),
  cmd(21, "LAND", "Land at a position.", { p1: "Abort Alt", ...POS }),
  cmd(22, "TAKEOFF", "Take off to an altitude.", { z: "Alt" }),
  cmd(82, "SPLINE_WAYPOINT", "Fly to a position along a spline.", { p1: "Delay", ...POS }),
  cmd(112, "CONDITION_DELAY", "Delay the next command.", { p1: "Time (s)" }),
  cmd(115, "CONDITION_YAW", "Turn to a heading.", { p1: "Angle", p2: "Speed", p3: "Dir", p4: "Rel" }),
  cmd(177, "DO_JUMP", "Jump to another item.", { p1: "WP #", p2: "Repeat" }),
  cmd(178, "DO_CHANGE_SPEED", "Change the speed.", { p1: "Type", p2: "Speed (m/s)", p3: "Throttle %" }),
  cmd(181, "DO_SET_RELAY", "Switch a relay.", { p1: "Relay #", p2: "On/Off" }),
  cmd(183, "DO_SET_SERVO", "Move a servo.", { p1: "Ch #", p2: "PWM" }),
  cmd(5001, "FENCE_POLYGON_VERTEX_INCLUSION", "Vertex of an inclusion fence.", { p1: "Vertex count", x: "Lat", y: "Lon" }),
  cmd(5100, "RALLY_POINT", "Rally point.", POS),
];

const wp = (seq: number, command: number, dLat: number, dLon: number, z: number, p1 = 0): MissionItem => ({
  seq,
  frame: 3,
  command,
  param1: p1,
  param2: 0,
  param3: 0,
  param4: 0,
  x: +(CENTER.lat + dLat).toFixed(7),
  y: +(CENTER.lon + dLon).toFixed(7),
  z,
  autocontinue: true,
});

function sampleMission(): MissionItem[] {
  return [
    { seq: 0, frame: 0, command: 16, param1: 0, param2: 0, param3: 0, param4: 0, x: CENTER.lat, y: CENTER.lon, z: 580, autocontinue: true },
    wp(1, 22, 0, 0, 30),
    wp(2, 16, 0.0015, 0.0008, 40),
    wp(3, 16, 0.0016, -0.0012, 40, 5),
    wp(4, 20, 0, 0, 0),
  ];
}

const num = (value: number, decimals: number) => value.toFixed(decimals);

/** `.waypoints`: QGC WPL 110, tab separated. */
function formatWaypoints(items: MissionItem[]): string {
  const rows = items.map((it, i) =>
    [
      i,
      i === 0 ? 1 : 0,
      it.frame,
      it.command,
      num(it.param1, 6),
      num(it.param2, 6),
      num(it.param3, 6),
      num(it.param4, 6),
      it.x.toFixed(8),
      it.y.toFixed(8),
      num(it.z, 6),
      it.autocontinue ? 1 : 0,
    ].join("\t"),
  );
  return ["QGC WPL 110", ...rows].join("\n") + "\n";
}

function parseWaypoints(text: string): MissionItem[] {
  const lines = text.split(/\r?\n/).filter((line) => line.trim() !== "");
  if (!lines[0]?.startsWith("QGC WPL 110")) throw new Error("not a QGC WPL 110 file");
  return lines.slice(1).map((line, index) => {
    const f = line.split(/\s+/).map(Number);
    if (f.length < 12 || f.some((v) => Number.isNaN(v))) throw new Error(`line ${index + 2}: expected 12 numbers`);
    return {
      seq: index,
      frame: f[2],
      command: f[3],
      param1: f[4],
      param2: f[5],
      param3: f[6],
      param4: f[7],
      x: f[8],
      y: f[9],
      z: f[10],
      autocontinue: f[11] !== 0,
    };
  });
}

export function createMissionMock(): MissionMock {
  const listeners = new Set<(p: MissionProgress) => void>();
  const emit = (p: MissionProgress) => listeners.forEach((listener) => listener(p));
  const store: Record<MissionKind, MissionItem[]> = { mission: sampleMission(), fence: [], rally: [] };

  async function transfer(kind: MissionKind, phase: "download" | "upload", total: number) {
    const steps = Math.max(1, total);
    for (let done = 0; done < steps; done += 1) {
      emit({ kind, phase, done, total, finished: false, error: null });
      await sleep(1000 / steps);
    }
    emit({ kind, phase, done: total, total, finished: true, error: null });
  }

  return {
    missionDownload: async (kind) => {
      const items = store[kind].map((item) => ({ ...item }));
      await transfer(kind, "download", items.length);
      return items;
    },
    missionUpload: async (kind, items) => {
      await transfer(kind, "upload", items.length);
      store[kind] = items.map((item, seq) => ({ ...item, seq }));
    },
    missionClear: async (kind) => {
      await sleep(300);
      store[kind] = kind === "mission" ? store.mission.slice(0, 1) : [];
    },
    missionCommands: async () => CATALOG.map((c) => ({ ...c })),
    missionParseFile: async (text) => {
      try {
        return parseWaypoints(text);
      } catch (error) {
        return Promise.reject(error instanceof Error ? error.message : String(error));
      }
    },
    missionSaveFile: async (defaultName, items) => browserDownload(defaultName, formatWaypoints(items), "text/plain"),
    onMissionProgress: async (cb): Promise<Unlisten> => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
  };
}
