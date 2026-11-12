/**
 * Flight Data grid of numeric telemetry tiles; double-click a tile to choose which field it shows.
 * Layout (cols 1-8, rows 1-4, field keys) is persisted under the localStorage key "quick.layout"; `fields` maps field key to value.
 */
import { useEffect, useState } from "react";
import { api } from "../api";
import { loadSetting, saveSetting } from "../storage";
import type { FieldInfo } from "../types";
import { QuickPicker } from "./QuickPicker";

// Defaults, plus fallbacks used to fill tiles when the grid grows.
const DEFAULT_KEYS = ["alt", "groundspeed", "wp_dist", "yaw", "verticalspeed", "dist_to_home"];
const FALLBACK_KEYS = ["battery_voltage", "satcount", "throttle", "roll", "pitch", "airspeed", "altasl", "heading", "battery_current", "gpshdop"];
// Decimals per field key; other keys use 0 if they match INTEGER_KEY, otherwise 1. Latitude/longitude need 7 (1e-7 deg).
const DECIMALS: Record<string, number> = {
  yaw: 0,
  heading: 0,
  nav_bearing: 0,
  throttle: 0,
  battery_remaining: 0,
  satcount: 0,
  gpsstatus: 0,
  wpno: 0,
  gpshdop: 2,
  battery_voltage: 2,
  satcount2: 0,
  gpsstatus2: 0,
  gpshdop2: 2,
  target_bearing: 0,
  groundcourse: 0,
  groundcourse2: 0,
  wind_dir: 0,
  lat: 7,
  lng: 7,
  lat2: 7,
  lng2: 7,
  ahrs2_lat: 7,
  ahrs2_lng: 7,
  ekfstatus: 2,
  ekfvelv: 2,
  ekfposhor: 2,
  ekfposvert: 2,
  ekfcompv: 2,
  ekfteralt: 2,
  timeInAir: 0,
  distTraveled: 0,
  armed: 0,
};

// Integer-valued families: RC/servo PWM, ESC RPM, per-battery counters, error counters.
const INTEGER_KEY = /^(ch\d+(in|out)|esc\d+_rpm|rpm\d|battery_usedmah\d*|battery_remaining\d*|errors_count\d|fenceb_\w+|ter_(load|pend|space)|rssi|remrssi|noise|remnoise|rxerrors|fixedp|txbuffer|rcrssi|i2cerrors|vibeclip\d|ekfflags|gen_status|pidaxis|landed_state|vtol_state|opt_qua|opt_x|opt_y|[agm][xyz][23]?)$/;
const decimalsFor = (key: string) => DECIMALS[key] ?? (INTEGER_KEY.test(key) ? 0 : 1);

type Layout = { cols: number; rows: number; keys: string[] };

const KEY = "quick.layout";

// Makes keys.length match cols * rows, filling with unused default/fallback keys.
function normalize(layout: Layout): Layout {
  const count = layout.cols * layout.rows;
  const keys = [...layout.keys];
  const pool = [...DEFAULT_KEYS, ...FALLBACK_KEYS];
  for (const candidate of pool) {
    if (keys.length >= count) break;
    if (!keys.includes(candidate)) keys.push(candidate);
  }
  while (keys.length < count) keys.push(pool[keys.length % pool.length]);
  return { ...layout, keys: keys.slice(0, count) };
}

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

type Props = { fields: Record<string, number> };

export function QuickView({ fields }: Props) {
  const [layout, setLayout] = useState<Layout>(() => {
    const saved = loadSetting<Partial<Layout>>(KEY, {});
    return normalize({
      cols: clamp(saved.cols ?? 3, 1, 8),
      rows: clamp(saved.rows ?? 2, 1, 4),
      keys: Array.isArray(saved.keys) ? saved.keys : DEFAULT_KEYS,
    });
  });
  const [defs, setDefs] = useState<FieldInfo[]>([]);
  const [editing, setEditing] = useState<number | null>(null);

  useEffect(() => {
    api.quickFields().then(setDefs).catch(() => setDefs([]));
  }, []);

  const apply = (next: Layout) => {
    const normalized = normalize(next);
    setLayout(normalized);
    saveSetting(KEY, normalized);
  };

  const info = (key: string): FieldInfo => defs.find((d) => d.key === key) ?? { key, label: key, unit: "", group: "" };

  return (
    <div className="panel quick">
      <div className="quick-controls">
        <h2>Quick view</h2>
        <span className="spacer" />
        <label>
          Cols{" "}
          <select aria-label="Quick view columns" value={layout.cols} onChange={(e) => apply({ ...layout, cols: Number(e.target.value) })}>
            {[1, 2, 3, 4, 5, 6, 7, 8].map((n) => (
              <option key={n}>{n}</option>
            ))}
          </select>
        </label>
        <label>
          Rows{" "}
          <select aria-label="Quick view rows" value={layout.rows} onChange={(e) => apply({ ...layout, rows: Number(e.target.value) })}>
            {[1, 2, 3, 4].map((n) => (
              <option key={n}>{n}</option>
            ))}
          </select>
        </label>
      </div>
      <div className="quick-grid" style={{ gridTemplateColumns: `repeat(${layout.cols}, 1fr)` }}>
        {layout.keys.map((key, index) => {
          const meta = info(key);
          const value = fields[key];
          const text = value === undefined || !Number.isFinite(value) ? "—" : value.toFixed(decimalsFor(key));
          return (
            <div key={index} className="quick-tile" title="Double-click to choose the value" onDoubleClick={() => setEditing(index)}>
              <span className="quick-label">
                {meta.label}
                {meta.unit && <em> ({meta.unit})</em>}
              </span>
              <strong className="mono">{text}</strong>
            </div>
          );
        })}
      </div>
      {editing !== null && (
        <QuickPicker
          fields={defs}
          current={layout.keys[editing]}
          onCancel={() => setEditing(null)}
          onPick={(key) => {
            const keys = [...layout.keys];
            keys[editing] = key;
            apply({ ...layout, keys });
            setEditing(null);
          }}
        />
      )}
    </div>
  );
}
