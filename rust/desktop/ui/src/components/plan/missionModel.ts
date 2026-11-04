import type { MissionCommandInfo, MissionItem, MissionKind } from "../../types";

export const CMD_WAYPOINT = 16;
export const CMD_FENCE_VERTEX = 5001;
export const CMD_RALLY_POINT = 5100;

export const FRAME_OPTIONS = [
  { value: 3, label: "Relative" },
  { value: 0, label: "Absolute" },
  { value: 10, label: "Terrain" },
] as const;

export const KIND_LABELS: Record<MissionKind, string> = { mission: "Mission", fence: "Fence", rally: "Rally" };

export const frameLabel = (frame: number) => FRAME_OPTIONS.find((f) => f.value === frame)?.label ?? `Frame ${frame}`;

export type CommandMap = Map<number, MissionCommandInfo>;

export const hasHome = (kind: MissionKind) => kind === "mission";
/** First index that can be edited, moved or deleted freely (the home row is fixed). */
export const firstFreeIndex = (kind: MissionKind) => (hasHome(kind) ? 1 : 0);

export function commandName(id: number, commands: CommandMap): string {
  return commands.get(id)?.name ?? `CMD ${id}`;
}

export function defaultCommand(kind: MissionKind, catalog: MissionCommandInfo[]): number {
  const wanted = kind === "fence" ? CMD_FENCE_VERTEX : kind === "rally" ? CMD_RALLY_POINT : CMD_WAYPOINT;
  if (catalog.some((c) => c.id === wanted)) return wanted;
  return kind === "mission" ? CMD_WAYPOINT : (catalog[0]?.id ?? wanted);
}

export function makeItem(partial: Partial<MissionItem> & Pick<MissionItem, "command" | "x" | "y" | "z" | "frame">): MissionItem {
  return { seq: 0, param1: 0, param2: 0, param3: 0, param4: 0, autocontinue: true, ...partial };
}

export function homeItem(lat: number, lon: number, altM: number): MissionItem {
  return makeItem({ frame: 0, command: CMD_WAYPOINT, x: lat, y: lon, z: altM });
}

export const renumber = (items: MissionItem[]): MissionItem[] => items.map((item, seq) => ({ ...item, seq }));

export function addPoint(items: MissionItem[], item: MissionItem): MissionItem[] {
  return renumber([...items, item]);
}

export function insertAt(items: MissionItem[], index: number, item: MissionItem): MissionItem[] {
  const next = [...items];
  next.splice(index, 0, item);
  return renumber(next);
}

export function removeAt(items: MissionItem[], index: number): MissionItem[] {
  return renumber(items.filter((_, i) => i !== index));
}

export function moveBy(items: MissionItem[], index: number, delta: -1 | 1): MissionItem[] {
  const target = index + delta;
  if (target < 0 || target >= items.length) return items;
  const next = [...items];
  [next[index], next[target]] = [next[target], next[index]];
  return renumber(next);
}

export function patchAt(items: MissionItem[], index: number, patch: Partial<MissionItem>): MissionItem[] {
  return items.map((item, i) => (i === index ? { ...item, ...patch } : item));
}

/** Items that sit on the map: the command takes a position and it is not left at 0,0. */
export function isPositional(item: MissionItem, commands: CommandMap): boolean {
  const info = commands.get(item.command);
  const takesPosition = info ? info.x !== null && info.y !== null : true;
  return takesPosition && (item.x !== 0 || item.y !== 0);
}

const EARTH_RADIUS_M = 6_371_000;
const rad = (deg: number) => (deg * Math.PI) / 180;

export function distanceM(a: Pick<MissionItem, "x" | "y">, b: Pick<MissionItem, "x" | "y">): number {
  const dLat = rad(b.x - a.x);
  const dLon = rad(b.y - a.y);
  const h = Math.sin(dLat / 2) ** 2 + Math.cos(rad(a.x)) * Math.cos(rad(b.x)) * Math.sin(dLon / 2) ** 2;
  return 2 * EARTH_RADIUS_M * Math.asin(Math.min(1, Math.sqrt(h)));
}

export function bearingDeg(a: Pick<MissionItem, "x" | "y">, b: Pick<MissionItem, "x" | "y">): number {
  const dLon = rad(b.y - a.y);
  const y = Math.sin(dLon) * Math.cos(rad(b.x));
  const x = Math.cos(rad(a.x)) * Math.sin(rad(b.x)) - Math.sin(rad(a.x)) * Math.cos(rad(b.x)) * Math.cos(dLon);
  return (((Math.atan2(y, x) * 180) / Math.PI) % 360 + 360) % 360;
}

export type RowStats = { dist: number | null; az: number | null; grade: number | null };

/** Distance, azimuth and gradient of each positional item from the previous positional one. */
export function computeStats(items: MissionItem[], commands: CommandMap): RowStats[] {
  let previous: MissionItem | null = null;
  return items.map((item) => {
    if (!isPositional(item, commands)) return { dist: null, az: null, grade: null };
    let stats: RowStats = { dist: null, az: null, grade: null };
    if (previous) {
      const dist = distanceM(previous, item);
      stats = {
        dist,
        az: dist > 0 ? bearingDeg(previous, item) : null,
        grade: dist > 0 && previous.frame === item.frame ? ((item.z - previous.z) / dist) * 100 : null,
      };
    }
    previous = item;
    return stats;
  });
}

export const totalDistance = (stats: RowStats[]): number => stats.reduce((sum, s) => sum + (s.dist ?? 0), 0);

export const validLat = (v: number) => Number.isFinite(v) && v >= -90 && v <= 90;
export const validLon = (v: number) => Number.isFinite(v) && v >= -180 && v <= 180;

/** Human-readable problems that must block an upload. */
export function itemProblems(items: MissionItem[], commands: CommandMap): string[] {
  const problems: string[] = [];
  items.forEach((item, i) => {
    if (!validLat(item.x) || !validLon(item.y)) problems.push(`#${i} ${commandName(item.command, commands)}: position out of range`);
    for (const [name, value] of [["P1", item.param1], ["P2", item.param2], ["P3", item.param3], ["P4", item.param4], ["Alt", item.z]] as const) {
      if (!Number.isFinite(value)) problems.push(`#${i} ${commandName(item.command, commands)}: ${name} is not a number`);
    }
  });
  return problems;
}

export const formatDistance = (m: number) => (m >= 1000 ? `${(m / 1000).toFixed(2)} km` : `${Math.round(m)} m`);

export function midpoint(a: Pick<MissionItem, "x" | "y" | "z">, b: Pick<MissionItem, "x" | "y" | "z">) {
  return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2, z: (a.z + b.z) / 2 };
}
