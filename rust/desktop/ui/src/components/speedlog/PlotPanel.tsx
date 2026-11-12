/**
 * One SpeedLOG plot panel: up to MAX_TRACES traces on one Y axis, sharing the time window with the
 * other panels. The backend reduces each trace to the visible window (min/max buckets), so the panel
 * refetches when the window changes. Drag to zoom (all panels), double click to reset; the cursor is
 * synced across panels and the chips read out every trace at that instant.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import { api } from "../../api";
import type { SpeedLogMode, SpeedLogSeries, SpeedLogStats, SpeedLogTrace } from "../../types";
import { formatValue, modeColor, traceColor, traceKey, traceDash } from "./speedlogUtils";

export const MAX_TRACES = 8;
const FETCH_DELAY_MS = 120;
const SYNC_KEY = "speedlog";

export type Panel = { id: number; traces: SpeedLogTrace[] };

type Props = {
  panel: Panel;
  index: number;
  count: number;
  active: boolean;
  /** Visible window, seconds; null = whole log. */
  range: [number, number] | null;
  span: [number, number];
  units: Record<string, string | null>;
  modes: SpeedLogMode[];
  showModes: boolean;
  marker: number | null;
  onActivate: () => void;
  onRemoveTrace: (trace: SpeedLogTrace) => void;
  onZoom: (range: [number, number]) => void;
  onReset: () => void;
  onClose: () => void;
  onClear: () => void;
  onMove: (step: number) => void;
  notify: (text: string, ok: boolean) => void;
};

function cssColor(name: string, fallback: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value || fallback;
}

/** Nearest non-null value around `idx`, so traces logged at different rates all read out. */
function valueNear(values: ArrayLike<number | null | undefined>, idx: number): number | null {
  for (let d = 0; d < 400; d += 1) {
    const a = values[idx - d];
    if (a !== null && a !== undefined) return a;
    const b = values[idx + d];
    if (b !== null && b !== undefined) return b;
  }
  return null;
}

export function PlotPanel(props: Props) {
  const { panel, index, count, active, range, span, units, modes, showModes, marker } = props;
  const hostRef = useRef<HTMLDivElement>(null);
  const plotRef = useRef<uPlot | null>(null);
  const readoutRefs = useRef<Array<HTMLSpanElement | null>>([]);
  const timeRef = useRef<HTMLSpanElement>(null);
  const [stats, setStats] = useState<Array<SpeedLogStats | null>>([]);
  const [missing, setMissing] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);

  // Latest values for the uPlot callbacks, which are created once per set of traces.
  const xRange = useRef<[number, number]>(range ?? span);
  xRange.current = range ?? span;
  const live = useRef(props);
  live.current = props;

  const signature = panel.traces.map(traceKey).join("|");
  const yUnits = useMemo(() => {
    const list = [...new Set(panel.traces.map((t) => units[traceKey(t)]).filter((u): u is string => !!u))];
    return list.join(", ");
  }, [panel.traces, units]);

  // Build the plot when the set of traces changes.
  useEffect(() => {
    const host = hostRef.current;
    if (!host || panel.traces.length === 0) return;
    const text = cssColor("--text-dim", "#9aa5b8");
    const grid = "rgba(76, 86, 106, 0.55)";
    const options: uPlot.Options = {
      width: Math.max(host.clientWidth, 100),
      height: Math.max(host.clientHeight, 80),
      legend: { show: false },
      cursor: {
        sync: { key: SYNC_KEY },
        drag: { x: true, y: false, setScale: false },
        points: { size: 5 },
      },
      select: { show: true, left: 0, top: 0, width: 0, height: 0 },
      scales: {
        x: { time: false, range: () => xRange.current },
      },
      axes: [
        {
          stroke: text,
          grid: { stroke: grid, width: 1 },
          ticks: { stroke: grid, width: 1 },
          values: (_u, splits) => splits.map((v) => formatTime(v)),
        },
        {
          stroke: text,
          grid: { stroke: grid, width: 1 },
          ticks: { stroke: grid, width: 1 },
          size: 60,
          values: (_u, splits) => splits.map((v) => formatValue(v)),
        },
      ],
      series: [
        {},
        ...panel.traces.map((trace, i) => ({
          label: traceKey(trace),
          stroke: traceColor(i),
          width: 1.4,
          dash: traceDash(i),
          spanGaps: true,
          points: { show: false },
        })),
      ],
      hooks: {
        // Mode shading under the traces
        drawAxes: [
          (u) => {
            const p = live.current;
            if (!p.showModes || p.modes.length === 0) return;
            const ctx = u.ctx;
            const top = u.bbox.top;
            const height = u.bbox.height;
            ctx.save();
            for (const mode of p.modes) {
              const x0 = Math.max(u.bbox.left, u.valToPos(mode.t0, "x", true));
              const x1 = Math.min(u.bbox.left + u.bbox.width, u.valToPos(mode.t1 ?? p.span[1], "x", true));
              if (x1 <= x0) continue;
              ctx.fillStyle = modeColor(mode.slot, 0.13);
              ctx.fillRect(x0, top, x1 - x0, height);
            }
            ctx.restore();
          },
        ],
        // Mode names and the message marker over the traces
        draw: [
          (u) => {
            const p = live.current;
            const ctx = u.ctx;
            ctx.save();
            const ratio = window.devicePixelRatio || 1;
            if (p.showModes) {
              ctx.font = `${10 * ratio}px ${cssColor("--font", "sans-serif")}`;
              ctx.textBaseline = "top";
              for (const mode of p.modes) {
                const x0 = Math.max(u.bbox.left, u.valToPos(mode.t0, "x", true));
                const x1 = Math.min(u.bbox.left + u.bbox.width, u.valToPos(mode.t1 ?? p.span[1], "x", true));
                const width = ctx.measureText(mode.name).width;
                if (x1 - x0 < width + 8 * ratio) continue;
                ctx.fillStyle = modeColor(mode.slot, 0.95);
                ctx.fillText(mode.name, x0 + 4 * ratio, u.bbox.top + 3 * ratio);
              }
            }
            if (p.marker !== null) {
              const x = u.valToPos(p.marker, "x", true);
              if (x >= u.bbox.left && x <= u.bbox.left + u.bbox.width) {
                ctx.strokeStyle = cssColor("--nord13", "#ebcb8b");
                ctx.lineWidth = 1.5 * ratio;
                ctx.setLineDash([4 * ratio, 3 * ratio]);
                ctx.beginPath();
                ctx.moveTo(x, u.bbox.top);
                ctx.lineTo(x, u.bbox.top + u.bbox.height);
                ctx.stroke();
              }
            }
            ctx.restore();
          },
        ],
        setSelect: [
          (u) => {
            if (u.select.width > 4) {
              const a = u.posToVal(u.select.left, "x");
              const b = u.posToVal(u.select.left + u.select.width, "x");
              if (b > a) live.current.onZoom([a, b]);
            }
            u.setSelect({ left: 0, top: 0, width: 0, height: 0 }, false);
          },
        ],
        setCursor: [
          (u) => {
            const idx = u.cursor.idx;
            const time = idx === null || idx === undefined ? null : u.data[0][idx];
            if (timeRef.current) timeRef.current.textContent = time === null || time === undefined ? "" : formatTime(time);
            for (let i = 1; i < u.data.length; i += 1) {
              const el = readoutRefs.current[i - 1];
              if (!el) continue;
              const value = idx === null || idx === undefined ? null : valueNear(u.data[i], idx);
              el.textContent = value === null ? "" : formatValue(value);
            }
          },
        ],
      },
    };
    const plot = new uPlot(options, [[], ...panel.traces.map(() => [])], host);
    plotRef.current = plot;
    const onDouble = () => live.current.onReset();
    plot.over.addEventListener("dblclick", onDouble);
    const observer = new ResizeObserver(() => {
      if (host.clientWidth > 0 && host.clientHeight > 0) {
        plot.setSize({ width: host.clientWidth, height: host.clientHeight });
      }
    });
    observer.observe(host);
    return () => {
      observer.disconnect();
      plot.over.removeEventListener("dblclick", onDouble);
      plot.destroy();
      plotRef.current = null;
    };
    // Rebuilt only when the set of traces changes; everything else goes through refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature]);

  // Fetch the traces reduced to the visible window.
  const rangeKey = range ? `${range[0]}:${range[1]}` : "all";
  useEffect(() => {
    if (panel.traces.length === 0) return;
    let cancelled = false;
    const timer = window.setTimeout(async () => {
      setLoading(true);
      try {
        const width = hostRef.current?.clientWidth ?? 1200;
        const buckets = Math.min(4000, Math.max(200, Math.round(width * 1.5)));
        const result = await api.speedlogSeries(panel.traces, range, buckets);
        if (cancelled) return;
        const plot = plotRef.current;
        const tables: uPlot.AlignedData[] = result.map((s: SpeedLogSeries | null) => (s ? [s.t, s.v] : [[], []]));
        const data: uPlot.AlignedData =
          tables.length === 1 ? tables[0] : uPlot.join(tables as uPlot.AlignedData[]);
        plot?.setData(data);
        setStats(result.map((s) => s?.stats ?? null));
        setMissing(panel.traces.filter((_, i) => !result[i]).map(traceKey));
      } catch (error) {
        if (!cancelled) live.current.notify(`Could not read the traces: ${String(error)}`, false);
      } finally {
        if (!cancelled) setLoading(false);
      }
    }, FETCH_DELAY_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature, rangeKey, span[0], span[1]]);

  // Overlays (modes, marker) only need a redraw.
  useEffect(() => {
    plotRef.current?.redraw(false, false);
  }, [showModes, modes, marker]);

  return (
    <section
      className={`panel sl-panel ${active ? "active" : ""}`}
      onMouseDown={props.onActivate}
      aria-label={`Panel ${index + 1}`}
    >
      <header>
        <div className="sl-panel-title">
          <span className="sl-panel-name">Panel {index + 1}</span>
          {yUnits && <span className="dim sl-panel-units">{yUnits}</span>}
          {loading && <span className="dim sl-panel-loading">loading…</span>}
          <span className="mono dim sl-panel-time" ref={timeRef} />
        </div>
        <div className="sl-panel-tools">
          <button title="Move up" aria-label="Move panel up" disabled={index === 0} onClick={() => props.onMove(-1)}>
            ↑
          </button>
          <button title="Move down" aria-label="Move panel down" disabled={index === count - 1} onClick={() => props.onMove(1)}>
            ↓
          </button>
          <button title="Remove every trace" disabled={panel.traces.length === 0} onClick={props.onClear}>
            Clear
          </button>
          <button title="Close the panel" aria-label="Close panel" onClick={props.onClose}>
            ×
          </button>
        </div>
      </header>
      {panel.traces.length > 0 && (
        <ul className="sl-chips">
          {panel.traces.map((trace, i) => {
            const key = traceKey(trace);
            const st = stats[i];
            return (
              <li key={key} className={missing.includes(key) ? "missing" : ""}>
                <span className="sl-swatch" style={{ background: traceColor(i) }} />
                <span className="sl-chip-name mono">{key}</span>
                <span
                  className="sl-chip-value mono"
                  ref={(el) => {
                    readoutRefs.current[i] = el;
                  }}
                />
                {st && (
                  <span className="sl-chip-stats mono dim" title="Min / mean / max in the visible window">
                    {formatValue(st.min)} / {formatValue(st.mean)} / {formatValue(st.max)}
                  </span>
                )}
                <button className="sl-chip-remove" aria-label={`Remove ${key}`} title="Remove the trace" onClick={() => props.onRemoveTrace(trace)}>
                  ×
                </button>
              </li>
            );
          })}
        </ul>
      )}
      {panel.traces.length === 0 ? (
        <div className="sl-panel-empty dim">
          {active ? "Tick fields in the sidebar to plot them here." : "Click this panel, then tick fields in the sidebar."}
        </div>
      ) : (
        <div className="sl-plot" ref={hostRef} />
      )}
    </section>
  );
}

/** Seconds as m:ss.s on the time axis. */
export function formatTime(seconds: number): string {
  const sign = seconds < 0 ? "-" : "";
  const s = Math.abs(seconds);
  const m = Math.floor(s / 60);
  return `${sign}${m}:${(s - m * 60).toFixed(1).padStart(4, "0")}`;
}
