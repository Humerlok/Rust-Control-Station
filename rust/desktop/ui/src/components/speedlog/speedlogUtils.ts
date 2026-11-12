/** Small helpers shared by the SpeedLOG components: colours, trace keys and number formatting. */
import type { SpeedLogTrace } from "../../types";

// Frost and Aurora tokens from theme.css (same order as the Inspector graph); the fallbacks are the Nord values.
const PALETTE: Array<[string, string]> = [
  ["--nord8", "#88c0d0"],
  ["--nord14", "#a3be8c"],
  ["--nord13", "#ebcb8b"],
  ["--nord12", "#d08770"],
  ["--nord15", "#b48ead"],
  ["--nord9", "#81a1c1"],
  ["--nord11", "#bf616a"],
  ["--nord7", "#8fbcbb"],
];

// Traces beyond the fourth also differ by line style, so colour is never the only cue.
const DASHES: Array<number[] | undefined> = [undefined, undefined, undefined, undefined, [6, 4], [2, 3], [8, 3, 2, 3], [12, 4]];

function cssColor(name: string, fallback: string): string {
  if (typeof document === "undefined") return fallback;
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value || fallback;
}

/** Colour of the trace slot `index` (fixed per slot, never cycled within a panel). */
export const traceColor = (index: number): string => {
  const [name, fallback] = PALETTE[index % PALETTE.length];
  return cssColor(name, fallback);
};

export const traceDash = (index: number): number[] | undefined => DASHES[index % DASHES.length];

function hexToRgba(hex: string, alpha: number): string {
  const n = parseInt(hex.replace("#", "").slice(0, 6), 16);
  if (Number.isNaN(n)) return `rgba(136, 192, 208, ${alpha})`;
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

/** Background shade of a flight-mode slot (0..7). */
export const modeColor = (slot: number, alpha: number): string => hexToRgba(traceColor(slot), alpha);

export const traceKey = (t: SpeedLogTrace): string => `${t.type}.${t.field}`;

/** Compact number for axes and readouts: no trailing zeros, no thousands separator. */
export function formatValue(v: number): string {
  if (!Number.isFinite(v)) return String(v);
  const abs = Math.abs(v);
  if (abs !== 0 && (abs >= 1e7 || abs < 1e-4)) return v.toExponential(3).replace(/\.?0+e/, "e");
  const digits = abs >= 1000 ? 1 : abs >= 10 ? 3 : 5;
  return String(Number(v.toFixed(digits)));
}

export function formatBytes(n: number): string {
  if (n >= 1048576) return `${(n / 1048576).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(n / 1024))} KB`;
}

export function formatDuration(seconds: number): string {
  const m = Math.floor(seconds / 60);
  return `${m} min ${Math.round(seconds - m * 60)} s`;
}

export const PRESET_LABELS: Record<string, string> = {
  attitude: "Attitude",
  altitude: "Altitude",
  battery: "Battery",
  vibration: "Vibration",
  motors: "Motors",
  gps: "GPS",
  rc: "RC input",
};
