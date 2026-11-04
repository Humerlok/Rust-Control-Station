import { useEffect, useState } from "react";
import { api } from "../api";
import { loadSetting, saveSetting } from "../storage";
import type { FieldInfo } from "../types";
import { QuickPicker } from "./QuickPicker";

const DEFAULT_KEYS = ["alt", "groundspeed", "wp_dist", "yaw", "verticalspeed", "dist_to_home"];
const FALLBACK_KEYS = ["battery_voltage", "satcount", "throttle", "roll", "pitch", "airspeed", "altasl", "heading", "battery_current", "gpshdop"];
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
};

type Layout = { cols: number; rows: number; keys: string[] };

const KEY = "quick.layout";

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

  const info = (key: string): FieldInfo => defs.find((d) => d.key === key) ?? { key, label: key, unit: "" };

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
          const text = value === undefined || !Number.isFinite(value) ? "—" : value.toFixed(DECIMALS[key] ?? 1);
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
