/**
 * Live uPlot graph of the watched inspector fields (x axis in seconds). Props: series store, watch keys and labels, paused, onRemove, onClear.
 * Redraws are throttled to REFRESH_MS; while paused the plot is frozen but the store keeps recording.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import { formatNumber } from "./inspectorUtils";
import type { SeriesStore } from "./seriesStore";

const WINDOWS = [
  { label: "10 s", seconds: 10 },
  { label: "30 s", seconds: 30 },
  { label: "60 s", seconds: 60 },
  { label: "All", seconds: 0 },
];
// Minimum interval between redraws, to coalesce bursts of snapshots.
const REFRESH_MS = 200;

// Frost and Aurora tokens from theme.css; the fallbacks are the same Nord values.
const PALETTE_VARS: Array<[string, string]> = [
  ["--nord8", "#88c0d0"],
  ["--nord14", "#a3be8c"],
  ["--nord13", "#ebcb8b"],
  ["--nord12", "#d08770"],
  ["--nord15", "#b48ead"],
  ["--nord9", "#81a1c1"],
  ["--nord11", "#bf616a"],
  ["--nord7", "#8fbcbb"],
];

function cssColor(name: string, fallback: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value || fallback;
}

// Series colour by position, cycling through the palette.
export const seriesColor = (index: number): string => {
  const [name, fallback] = PALETTE_VARS[index % PALETTE_VARS.length];
  return cssColor(name, fallback);
};

/** First index whose value is >= x. */
function lowerBound(values: number[], x: number): number {
  let low = 0;
  let high = values.length;
  while (low < high) {
    const mid = (low + high) >> 1;
    if (values[mid] < x) low = mid + 1;
    else high = mid;
  }
  return low;
}

type Props = {
  store: SeriesStore;
  keys: string[];
  labels: Record<string, string>;
  paused: boolean;
  onRemove: (key: string) => void;
  onClear: () => void;
};

export function GraphPanel({ store, keys, labels, paused, onRemove, onClear }: Props) {
  const hostRef = useRef<HTMLDivElement>(null);
  const plotRef = useRef<uPlot | null>(null);
  const [windowSeconds, setWindowSeconds] = useState(30);
  const [last, setLast] = useState<Record<string, number | undefined>>({});

  // Refs give the uPlot callbacks and the refresh timer the latest values without rebuilding the plot.
  const windowRef = useRef(windowSeconds);
  const pausedRef = useRef(paused);
  const keysRef = useRef(keys);
  windowRef.current = windowSeconds;
  pausedRef.current = paused;
  keysRef.current = keys;

  const signature = keys.join("|");
  const colors = useMemo(() => keys.map((_, index) => seriesColor(index)), [keys]);

  // Aligned data for uPlot from the store, restricted to the selected time window.
  const buildData = useRef((): uPlot.AlignedData => {
    const current = keysRef.current;
    const buffers = current.map((key) => store.get(key));
    let latest = -Infinity;
    for (const buffer of buffers) {
      if (buffer && buffer.t.length > 0) latest = Math.max(latest, buffer.t[buffer.t.length - 1]);
    }
    const from = windowRef.current > 0 && latest > -Infinity ? latest - windowRef.current * 1000 : -Infinity;
    const tables = buffers.map((buffer): uPlot.AlignedData => {
      if (!buffer || buffer.t.length === 0) return [[], []];
      const start = from === -Infinity ? 0 : lowerBound(buffer.t, from);
      return [
        buffer.t.slice(start).map((t) => t / 1000),
        buffer.v.slice(start),
      ];
    });
    if (tables.length === 0) return [[]];
    if (tables.length === 1) return tables[0];
    return uPlot.join(tables);
  }).current;

  useEffect(() => {
    const host = hostRef.current;
    if (!host || keys.length === 0) return;
    const text = cssColor("--text-dim", "#9aa5b8");
    const grid = "rgba(76, 86, 106, 0.55)";
    const options: uPlot.Options = {
      width: Math.max(host.clientWidth, 100),
      height: Math.max(host.clientHeight, 100),
      legend: { show: false },
      cursor: { drag: { x: false, y: false } },
      scales: {
        x: {
          time: false,
          // Fixed-width sliding window ending at the newest sample; "All" (0) shows everything.
          range: (_plot, min, max) => {
            const span = windowRef.current;
            const hi = max > min ? max : min + 1;
            return span > 0 ? [hi - span, hi] : [min, hi];
          },
        },
      },
      axes: [
        {
          stroke: text,
          grid: { stroke: grid, width: 1 },
          ticks: { stroke: grid, width: 1 },
          values: (_plot, splits) => splits.map((v) => `${Number(v.toFixed(1))}s`),
        },
        {
          stroke: text,
          grid: { stroke: grid, width: 1 },
          ticks: { stroke: grid, width: 1 },
          size: 56,
          values: (_plot, splits) => splits.map((v) => formatNumber(v)),
        },
      ],
      series: [
        {},
        ...keys.map((key, index) => ({
          label: labels[key] ?? key,
          stroke: colors[index],
          width: 1.5,
          spanGaps: true,
          points: { show: false },
        })),
      ],
    };
    const plot = new uPlot(options, buildData(), host);
    plotRef.current = plot;

    const observer = new ResizeObserver(() => {
      if (host.clientWidth > 0 && host.clientHeight > 0) {
        plot.setSize({ width: host.clientWidth, height: host.clientHeight });
      }
    });
    observer.observe(host);
    return () => {
      observer.disconnect();
      plot.destroy();
      plotRef.current = null;
    };
    // The plot is rebuilt only when the set of series changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature]);

  useEffect(() => {
    let timer: number | undefined;
    const refresh = () => {
      timer = undefined;
      if (pausedRef.current) return;
      plotRef.current?.setData(buildData());
      setLast(Object.fromEntries(keysRef.current.map((key) => [key, store.last(key)])));
    };
    // Debounce: the first store update arms a timer, later ones within REFRESH_MS ride on it.
    const schedule = () => {
      if (timer === undefined) timer = window.setTimeout(refresh, REFRESH_MS);
    };
    const unsubscribe = store.subscribe(schedule);
    refresh();
    return () => {
      unsubscribe();
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [store, buildData, signature, paused, windowSeconds]);

  return (
    <section className="panel insp-graph-panel" aria-label="Graph">
      <header>
        <h2>Graph</h2>
        <div className="insp-graph-tools">
          <span className="dim">Window</span>
          {WINDOWS.map((w) => (
            <button
              key={w.label}
              className={windowSeconds === w.seconds ? "primary" : ""}
              onClick={() => setWindowSeconds(w.seconds)}
            >
              {w.label}
            </button>
          ))}
          <button
            onClick={onClear}
            title="Empties every recorded series and resets the message counters"
          >
            Clear
          </button>
        </div>
      </header>
      {keys.length === 0 ? (
        <div className="insp-hint insp-graph-empty">
          Press <strong>Graph</strong> next to a numeric field to plot it live. Add several fields to compare them.
        </div>
      ) : (
        <>
          <ul className="insp-legend">
            {keys.map((key, index) => (
              <li key={key}>
                <span className="insp-swatch" style={{ background: colors[index] }} />
                <span className="insp-legend-label">{labels[key] ?? key}</span>
                <span className="mono insp-legend-value">
                  {last[key] === undefined ? "—" : formatNumber(last[key] as number)}
                </span>
                <button
                  className="insp-legend-remove"
                  aria-label={`Remove ${labels[key] ?? key}`}
                  title="Remove from the graph"
                  onClick={() => onRemove(key)}
                >
                  ×
                </button>
              </li>
            ))}
          </ul>
          <div className="insp-plot" ref={hostRef} />
          {paused && <div className="insp-paused-tag">Paused: the graph is frozen, data is still being recorded</div>}
        </>
      )}
    </section>
  );
}
