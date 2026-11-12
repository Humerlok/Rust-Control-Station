import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { loadSetting, saveSetting } from "../storage";
import type { Telemetry } from "../types";

const PX_PER_DEG = 3;
const CARDINALS = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];

function normalizeHeading(value: number): number {
  return ((value % 360) + 360) % 360;
}

function HeadingTape({ heading }: { heading: number }) {
  const ticks: JSX.Element[] = [];
  const start = Math.floor((heading - 40) / 5) * 5;
  for (let d = start; d <= heading + 40; d += 5) {
    const x = 100 + (d - heading) * 2.5;
    const wrapped = normalizeHeading(d);
    const major = wrapped % 30 === 0;
    ticks.push(
      <g key={d}>
        <line x1={x} x2={x} y1={major ? 4 : 8} y2={14} stroke="var(--nord4)" strokeWidth={1} />
        {major && (
          <text x={x} y={26} textAnchor="middle" fontSize={9} fill="var(--nord4)">
            {wrapped % 90 === 0 ? CARDINALS[wrapped / 45] : wrapped}
          </text>
        )}
      </g>,
    );
  }
  return (
    <svg viewBox="0 0 200 30" width="100%" aria-label="Heading tape">
      {ticks}
      <path d="M100 0 L95 6 L105 6 Z" fill="var(--nord13)" />
    </svg>
  );
}

function Horizon({ roll, pitch }: { roll: number; pitch: number }) {
  const ladder: JSX.Element[] = [];
  for (let p = -30; p <= 30; p += 10) {
    if (p === 0) continue;
    const y = 100 - p * PX_PER_DEG;
    ladder.push(
      <g key={p}>
        <line x1={80} x2={120} y1={y} y2={y} stroke="var(--nord6)" strokeWidth={1} />
        <text x={124} y={y + 3} fontSize={8} fill="var(--nord6)">
          {Math.abs(p)}
        </text>
      </g>,
    );
  }
  return (
    <svg viewBox="0 0 200 200" width="100%" aria-label="Artificial horizon">
      <defs>
        <clipPath id="hud-clip">
          <circle cx={100} cy={100} r={92} />
        </clipPath>
      </defs>
      <g clipPath="url(#hud-clip)">
        <g transform={`rotate(${-roll} 100 100) translate(0 ${pitch * PX_PER_DEG})`}>
          <rect x={-200} y={-300} width={600} height={400} fill="var(--nord10)" />
          <rect x={-200} y={100} width={600} height={400} fill="#6b5a4e" />
          <line x1={-200} x2={400} y1={100} y2={100} stroke="var(--nord6)" strokeWidth={1.5} />
          {ladder}
        </g>
      </g>
      <circle cx={100} cy={100} r={92} fill="none" stroke="var(--nord4)" strokeWidth={2} />
      <g transform={`rotate(${-roll} 100 100)`}>
        <path d="M100 8 L94 -2 L106 -2 Z" fill="var(--nord13)" transform="translate(0 6)" />
      </g>
      <path d="M62 100 H88 M112 100 H138 M100 100 m-3 0 a3 3 0 1 0 6 0 a3 3 0 1 0 -6 0" stroke="var(--nord13)" strokeWidth={3} fill="none" />
    </svg>
  );
}

const fmt = (value: number | undefined, digits = 0) => (value === undefined || !Number.isFinite(value) ? "—" : value.toFixed(digits));

/** How long the flight mode stays highlighted after it changes (legacy HUD: 2 s in red). */
const MODE_CHANGE_MS = 2000;

/** True for a short while after `mode` changes, not on the first value seen. */
function useRecentChange(mode: string | null): boolean {
  const previous = useRef(mode);
  const [changed, setChanged] = useState(false);
  useEffect(() => {
    const before = previous.current;
    previous.current = mode;
    if (before === null || mode === null || before === mode) return;
    setChanged(true);
    const timer = window.setTimeout(() => setChanged(false), MODE_CHANGE_MS);
    return () => window.clearTimeout(timer);
  }, [mode]);
  return changed;
}

/** HUD box inside the map panel: `left`/`bottom` offsets and width, in px. */
type HudLayout = { x: number; b: number; w: number };

const HUD_KEY = "hud.layout";
const HUD_DEFAULT: HudLayout = { x: 10, b: 28, w: 210 };
const HUD_MIN_W = 160;

function validLayout(value: Partial<HudLayout>): HudLayout {
  const ok = (n: unknown, fallback: number) => (typeof n === "number" && Number.isFinite(n) ? n : fallback);
  return { x: ok(value.x, HUD_DEFAULT.x), b: ok(value.b, HUD_DEFAULT.b), w: ok(value.w, HUD_DEFAULT.w) };
}

/** Keeps the HUD fully inside `bounds` (the map panel). */
function clampLayout(layout: HudLayout, bounds: { width: number; height: number }, hudHeight: number): HudLayout {
  const w = Math.max(HUD_MIN_W, Math.min(layout.w, bounds.width));
  const x = Math.max(0, Math.min(layout.x, bounds.width - w));
  const b = Math.max(0, Math.min(layout.b, bounds.height - hudHeight));
  return { x, b, w };
}

type Drag = { mode: "move" | "resize"; startX: number; startY: number; origin: HudLayout };

export function Hud({ telemetry }: { telemetry: Telemetry }) {
  const f = telemetry.fields;
  const live = telemetry.connected;
  const mode = live ? telemetry.mode : null;
  const modeChanged = useRecentChange(mode);
  const heading = normalizeHeading(f.heading ?? f.yaw ?? 0);

  const rootRef = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);
  const [layout, setLayout] = useState<HudLayout>(() => validLayout(loadSetting<Partial<HudLayout>>(HUD_KEY, {})));

  const bounds = useCallback(() => {
    const parent = rootRef.current?.parentElement;
    return parent ? { width: parent.clientWidth, height: parent.clientHeight } : null;
  }, []);

  /** Re-clamps after the panel or the HUD itself changes size. */
  const reclamp = useCallback(() => {
    const box = bounds();
    const hud = rootRef.current;
    if (!box || !hud || box.width === 0) return;
    setLayout((current) => {
      const next = clampLayout(current, box, hud.offsetHeight);
      return next.x === current.x && next.b === current.b && next.w === current.w ? current : next;
    });
  }, [bounds]);

  useLayoutEffect(() => {
    reclamp();
    const hud = rootRef.current;
    const parent = hud?.parentElement;
    if (!hud || !parent) return;
    const observer = new ResizeObserver(reclamp);
    observer.observe(parent);
    observer.observe(hud);
    return () => observer.disconnect();
  }, [reclamp]);

  const start = (event: React.PointerEvent<HTMLElement>, kind: Drag["mode"]) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { mode: kind, startX: event.clientX, startY: event.clientY, origin: layout };
  };

  const move = (event: React.PointerEvent<HTMLElement>) => {
    const d = drag.current;
    const box = bounds();
    const hud = rootRef.current;
    if (!d || !box || !hud) return;
    const dx = event.clientX - d.startX;
    const dy = event.clientY - d.startY;
    const next = d.mode === "move" ? { ...d.origin, x: d.origin.x + dx, b: d.origin.b - dy } : { ...d.origin, w: d.origin.w + dx };
    setLayout(clampLayout(next, box, hud.offsetHeight));
  };

  const layoutRef = useRef(layout);
  layoutRef.current = layout;
  const end = () => {
    if (!drag.current) return;
    drag.current = null;
    saveSetting(HUD_KEY, layoutRef.current);
  };

  const reset = () => {
    saveSetting(HUD_KEY, HUD_DEFAULT);
    setLayout(HUD_DEFAULT);
  };

  return (
    <div className="hud" ref={rootRef} aria-label="Heads-up display" style={{ left: layout.x, bottom: layout.b, width: layout.w }}>
      <div
        className="hud-badges hud-handle"
        title="Drag to move the HUD, double-click to reset"
        onPointerDown={(e) => start(e, "move")}
        onPointerMove={move}
        onPointerUp={end}
        onPointerCancel={end}
        onDoubleClick={reset}
      >
        <span className={`badge ${live ? (telemetry.armed ? "danger" : "ok") : ""}`}>{live ? (telemetry.armed ? "ARMED" : "DISARMED") : "NO LINK"}</span>
        <span className="hud-type dim">{live ? telemetry.vehicleType : ""}</span>
      </div>
      <HeadingTape heading={heading} />
      <Horizon roll={f.roll ?? 0} pitch={f.pitch ?? 0} />
      <div className={`hud-mode${modeChanged ? " changed" : ""}`} aria-label="Flight mode" title="Flight mode">
        {mode ?? "—"}
      </div>
      <div className="hud-readouts mono">
        <span>
          HDG {fmt(heading)}° {CARDINALS[Math.round(heading / 45) % 8]}
        </span>
        <span>GS {fmt(f.groundspeed, 1)} m/s</span>
        <span>ALT {fmt(f.alt, 1)} m</span>
        <span>
          BAT {fmt(f.battery_voltage, 1)} V {fmt(f.battery_current, 1)} A{" "}
          {f.battery_remaining !== undefined ? `${fmt(f.battery_remaining)}%` : ""}
        </span>
      </div>
      <div
        className="hud-resize"
        title="Drag to resize"
        onPointerDown={(e) => start(e, "resize")}
        onPointerMove={move}
        onPointerUp={end}
        onPointerCancel={end}
      />
    </div>
  );
}
