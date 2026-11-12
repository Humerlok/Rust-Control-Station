import type { Telemetry } from "../types";

/** MAVLink GpsFixType, in enum order. */
const FIX_LABELS = ["No GPS", "No fix", "2D fix", "3D fix", "DGPS", "RTK float", "RTK fixed", "Static", "PPP"];

function fixClass(fix: number): string {
  if (fix <= 1) return "bad";
  if (fix === 5 || fix === 6) return "rtk";
  if (fix >= 3) return "good";
  return "";
}

const fixLabel = (fix: number) => FIX_LABELS[fix] ?? `Fix ${fix}`;

function GpsLine({ name, fix, sats, hdop }: { name: string; fix: number; sats?: number; hdop?: number }) {
  return (
    <span className="mono">
      {name} <span className={`gps-fix ${fixClass(fix)}`}>{fixLabel(fix)}</span>
      {sats !== undefined ? ` · ${sats} sat` : ""}
      {hdop !== undefined && Number.isFinite(hdop) ? ` · HDOP ${hdop.toFixed(1)}` : ""}
    </span>
  );
}

export function GpsBadge({ telemetry }: { telemetry: Telemetry }) {
  const f = telemetry.fields;
  if (!telemetry.connected) return null;
  const hasGps = f.gpsstatus !== undefined;
  const hasGps2 = f.gpsstatus2 !== undefined;
  if (!hasGps && !hasGps2) return null;
  return (
    <div className="gps-badge" aria-label="GPS status">
      {hasGps && <GpsLine name="GPS" fix={f.gpsstatus} sats={f.satcount} hdop={f.gpshdop} />}
      {hasGps2 && <GpsLine name="GPS2" fix={f.gpsstatus2} sats={f.satcount2} hdop={f.gpshdop2} />}
    </div>
  );
}
