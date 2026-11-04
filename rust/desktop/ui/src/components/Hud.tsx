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

export function Hud({ telemetry }: { telemetry: Telemetry }) {
  const f = telemetry.fields;
  const live = telemetry.connected;
  const heading = normalizeHeading(f.heading ?? f.yaw ?? 0);
  return (
    <div className="hud" aria-label="Heads-up display">
      <div className="hud-badges">
        <span className={`badge ${live ? (telemetry.armed ? "danger" : "ok") : ""}`}>{live ? (telemetry.armed ? "ARMED" : "DISARMED") : "NO LINK"}</span>
        <span className="badge">{telemetry.mode ?? "—"}</span>
      </div>
      <HeadingTape heading={heading} />
      <Horizon roll={f.roll ?? 0} pitch={f.pitch ?? 0} />
      <div className="hud-readouts mono">
        <span>
          HDG {fmt(heading)}° {CARDINALS[Math.round(heading / 45) % 8]}
        </span>
        <span>GS {fmt(f.groundspeed, 1)} m/s</span>
        <span>ALT {fmt(f.alt, 1)} m</span>
        <span>
          BAT {fmt(f.battery_voltage, 1)} V {f.battery_remaining !== undefined ? `${fmt(f.battery_remaining)}%` : ""}
        </span>
      </div>
    </div>
  );
}
