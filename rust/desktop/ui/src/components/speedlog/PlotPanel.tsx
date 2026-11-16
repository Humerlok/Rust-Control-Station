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
  measure: { a: number | null; b: number | null };
  onMeasure: (t: number, which: "a" | "b") => void;
  /** Wheel zoom (not added to the zoom history). */
  onWheelZoom: (range: [number, number] | null) => void;
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
  const { panel, index, count, active, range, span, units, modes, showModes, marker, measure } = props;
  const hostRef = useRef<HTMLDivElement>(null);
  const plotRef = useRef<uPlot | null>(null);
  const readoutRefs = useRef<Array<HTMLSpanElement | null>>([]);
  const timeRef = useRef<HTMLSpanElement>(null);
  const [stats, setStats] = useState<Array<SpeedLogStats | null>>([]);
  const [missing, setMissing] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  // Per trace: the points are min/max buckets, so other samples hide between them.
  const reducedRef = useRef<boolean[]>([]);
  const [reduced, setReduced] = useState<boolean[]>([]);
  // A/B measurement: value change of every trace between the two markers.
  const [deltas, setDeltas] = useState<Array<number | null>>([]);

  // Latest values for the uPlot callbacks, which are created once per set of traces.
  const xRange = useRef<[number, number]>(range ?? span);
  xRange.current = range ?? span;
  const live = useRef(props);
  live.current = props;

  // Units are part of the signature: the plot is rebuilt (one Y axis per unit) when they load.
  const signature = panel.traces.map((t) => `${traceKey(t)}@${units[traceKey(t)] ?? ""}`).join("|");
  const unitList = useMemo(() => [...new Set(panel.traces.map((t) => units[traceKey(t)] ?? ""))], [panel.traces, units]);
  const yUnits = useMemo(() => {
    const list = [...new Set(panel.traces.map((t) => units[traceKey(t)]).filter((u): u is string => !!u))];
    return list.join(", ");
  }, [panel.traces, units]);

  /** Value change between A and B for every trace; null when a marker is outside the loaded window. */
  const computeDeltas = (): Array<number | null> => {
    const u = plotRef.current;
    const { a, b } = live.current.measure;
    if (!u || a === null || b === null) return [];
    const x = u.data[0];
    const inside = (t: number) => x.length > 0 && t >= x[0] && t <= x[x.length - 1];
    const out: Array<number | null> = [];
    for (let i = 1; i < u.data.length; i += 1) {
      if (!inside(a) || !inside(b)) {
        out.push(null);
        continue;
      }
      const va = valueNear(u.data[i], u.valToIdx(a));
      const vb = valueNear(u.data[i], u.valToIdx(b));
      out.push(va === null || vb === null ? null : vb - va);
    }
    return out;
  };

  // Build the plot when the set of traces changes.
  useEffect(() => {
    const host = hostRef.current;
    if (!host || panel.traces.length === 0) return;
    const text = cssColor("--text-dim", "#9aa5b8");
    const grid = "rgba(76, 86, 106, 0.55)";
    const dual = unitList.length === 2;
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
          values: (_u, splits) => {
            const visible = xRange.current[1] - xRange.current[0];
            const digits = visible > 60 ? 1 : visible > 5 ? 2 : 3;
            return splits.map((v) => formatTime(v, digits));
          },
        },
        {
          stroke: text,
          grid: { stroke: grid, width: 1 },
          ticks: { stroke: grid, width: 1 },
          size: 60,
          label: dual && unitList[0] ? unitList[0] : undefined,
          values: (_u, splits) => splits.map((v) => formatValue(v)),
        },
        ...(dual
          ? [
              {
                scale: "y2",
                side: 1 as const,
                stroke: text,
                grid: { show: false },
                ticks: { stroke: grid, width: 1 },
                size: 60,
                label: unitList[1] || undefined,
                values: (_u: uPlot, splits: number[]) => splits.map((v) => formatValue(v)),
              },
            ]
          : []),
      ],
      series: [
        {},
        ...panel.traces.map((trace, i) => ({
          label: traceKey(trace),
          stroke: traceColor(i),
          width: 1.4,
          dash: traceDash(i),
          spanGaps: true,
          scale: dual && (units[traceKey(trace)] ?? "") === unitList[1] ? "y2" : "y",
          points: { show: (u: uPlot, _sidx: number, i0: number, i1: number) => i1 - i0 < u.bbox.width / (10 * (window.devicePixelRatio || 1)), size: 4 },
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
            const marks: Array<[number | null, string, string]> = [
              [p.measure.a, "A", cssColor("--nord8", "#88c0d0")],
              [p.measure.b, "B", cssColor("--nord15", "#b48ead")],
            ];
            for (const [t, letter, color] of marks) {
              if (t === null) continue;
              const x = u.valToPos(t, "x", true);
              if (x < u.bbox.left || x > u.bbox.left + u.bbox.width) continue;
              ctx.strokeStyle = color;
              ctx.fillStyle = color;
              ctx.lineWidth = 1.5 * ratio;
              ctx.setLineDash([]);
              ctx.beginPath();
              ctx.moveTo(x, u.bbox.top);
              ctx.lineTo(x, u.bbox.top + u.bbox.height);
              ctx.stroke();
              ctx.font = `bold ${11 * ratio}px ${cssColor("--font", "sans-serif")}`;
              ctx.textBaseline = "top";
              ctx.fillText(letter, x + 3 * ratio, u.bbox.top + 3 * ratio);
            }
            ctx.restore();
          },
        ],
        setSelect: [
          (u) => {
            // Only the plot the user dragged on reports the zoom (the hook fires on every synced plot).
            if (u.cursor.event != null && u.select.width > 4) {
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
            if (timeRef.current) {
              timeRef.current.textContent = time === null || time === undefined ? "" : `t = ${formatTime(time, 3)}`;
            }
            for (let i = 1; i < u.data.length; i += 1) {
              const el = readoutRefs.current[i - 1];
              if (!el) continue;
              const value = idx === null || idx === undefined ? null : valueNear(u.data[i], idx);
              el.textContent = value === null ? "" : `${reducedRef.current[i - 1] ? "≈ " : ""}${formatValue(value)}`;
            }
          },
        ],
      },
    };
    const plot = new uPlot(options, [[], ...panel.traces.map(() => [])], host);
    plotRef.current = plot;
    let downX: number | null = null;
    let clickTimer: number | undefined;
    const onDown = (e: MouseEvent) => {
      if (e.button === 0) downX = e.clientX;
    };
    const onUp = (e: MouseEvent) => {
      if (e.button !== 0 || downX === null || Math.abs(e.clientX - downX) >= 4) return;
      const idx = plot.cursor.idx;
      if (idx == null) return;
      const t = plot.data[0][idx];
      if (t == null) return;
      const which = e.shiftKey ? "b" : "a";
      window.clearTimeout(clickTimer);
      // Deferred so a double click (reset zoom) does not also set A.
      clickTimer = window.setTimeout(() => live.current.onMeasure(t, which), 250);
    };
    const onDouble = () => {
      window.clearTimeout(clickTimer);
      live.current.onReset();
    };
    const onWheel = (e: WheelEvent) => {
      const list = host.closest(".sl-panels");
      const scrollable = !!list && list.scrollHeight > list.clientHeight + 1;
      if (!e.ctrlKey && scrollable) return; // let the panel list scroll
      e.preventDefault();
      const rect = plot.over.getBoundingClientRect();
      const t = plot.posToVal(e.clientX - rect.left, "x");
      const [x0, x1] = xRange.current;
      const f = e.deltaY < 0 ? 0.8 : 1.25;
      const [s0, s1] = live.current.span;
      const a = Math.max(s0, t - (t - x0) * f);
      const b = Math.min(s1, t + (x1 - t) * f);
      live.current.onWheelZoom(a <= s0 && b >= s1 ? null : [a, b]);
    };
    plot.over.addEventListener("mousedown", onDown);
    plot.over.addEventListener("mouseup", onUp);
    plot.over.addEventListener("dblclick", onDouble);
    plot.over.addEventListener("wheel", onWheel, { passive: false });
    const observer = new ResizeObserver(() => {
      if (host.clientWidth > 0 && host.clientHeight > 0) {
        plot.setSize({ width: host.clientWidth, height: host.clientHeight });
      }
    });
    observer.observe(host);
    return () => {
      observer.disconnect();
      window.clearTimeout(clickTimer);
      plot.over.removeEventListener("mousedown", onDown);
      plot.over.removeEventListener("mouseup", onUp);
      plot.over.removeEventListener("dblclick", onDouble);
      plot.over.removeEventListener("wheel", onWheel);
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
        setDeltas(computeDeltas());
        setStats(result.map((s) => s?.stats ?? null));
        reducedRef.current = result.map((s) => s?.reduced ?? false);
        setReduced(reducedRef.current);
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
  }, [showModes, modes, marker, measure.a, measure.b]);

  useEffect(() => {
    setDeltas(computeDeltas());
  }, [measure.a, measure.b]);

  return (
    <section
      className={`panel sl-panel ${active ? "active" : ""}`}
      onMouseDown={props.onActivate}
      aria-label={`Panel ${index + 1}`}
    >
      <header>
        <div className="sl-panel-title">
          <span className="sl-panel-name">Panel {index + 1}</span>
          {unitList.length >= 3 ? (
            <span className="warn-text sl-panel-units">mixed units: use + Panel</span>
          ) : (
            yUnits && <span className="dim sl-panel-units">{yUnits}</span>
          )}
          {loading && <span className="dim sl-panel-loading">loading…</span>}
          <span className="mono sl-panel-time" ref={timeRef} />
          {measure.a !== null && measure.b !== null && (
            <span className="mono sl-panel-time">
              {`Δt = ${(measure.b - measure.a).toFixed(3)} s`}
              {measure.b !== measure.a && ` (${(1 / Math.abs(measure.b - measure.a)).toFixed(3)} Hz)`}
            </span>
          )}
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
              <li
                key={key}
                className={missing.includes(key) ? "missing" : ""}
                title={reduced[i] ? "Reduced view (min/max per bucket). Zoom in to read every sample." : undefined}
              >
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
                    min {formatValue(st.min)} · avg {formatValue(st.mean)} · max {formatValue(st.max)}
                  </span>
                )}
                {measure.a !== null && measure.b !== null && (
                  <span
                    className="sl-chip-stats mono"
                    title={deltas[i] == null ? "A or B is outside the visible window" : "Value at B minus value at A"}
                  >
                    {deltas[i] == null ? "Δ = n/a" : `Δ = ${formatValue(deltas[i] as number)}`}
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

/** Seconds as m:ss.s (`digits` decimals) on the time axis. */
export function formatTime(seconds: number, digits = 1): string {
  const sign = seconds < 0 ? "-" : "";
  const s = Math.abs(seconds);
  const m = Math.floor(s / 60);
  return `${sign}${m}:${(s - m * 60).toFixed(digits).padStart(3 + digits, "0")}`;
}
