/**
 * Tools > SpeedLOG: offline DataFlash log plotter (port of SpeedLOG). Open a `.bin` from disk or from the
 * connected vehicle, tick fields to plot them in panels, zoom by dragging (the time axis is shared),
 * shade flight modes, browse the parameters and the messages. Parsing, series, decimation and the
 * descriptions live in crates/speedlog; this screen only draws.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import { FieldTree } from "../components/speedlog/FieldTree";
import { MessagesTable, ParamsTable } from "../components/speedlog/LogTables";
import { MAX_TRACES, Panel, PlotPanel } from "../components/speedlog/PlotPanel";
import { VehicleLogsDialog } from "../components/speedlog/VehicleLogsDialog";
import { formatBytes, formatDuration, PRESET_LABELS, traceKey } from "../components/speedlog/speedlogUtils";
import { loadSetting, saveSetting } from "../storage";
import type {
  SpeedLogInfo,
  SpeedLogLang,
  SpeedLogMessage,
  SpeedLogMode,
  SpeedLogParam,
  SpeedLogPreset,
  SpeedLogTrace,
  SpeedLogType,
} from "../types";
import "../speedlog.css";

type Tab = "plot" | "params" | "messages";

type Props = {
  active: boolean;
  connected: boolean;
  notify: (text: string, ok: boolean) => void;
};

let nextPanelId = 1;
const newPanel = (traces: SpeedLogTrace[] = []): Panel => ({ id: nextPanelId++, traces });

export function SpeedLog({ active, connected, notify }: Props) {
  const [info, setInfo] = useState<SpeedLogInfo | null>(null);
  const [types, setTypes] = useState<SpeedLogType[]>([]);
  const [modes, setModes] = useState<SpeedLogMode[]>([]);
  const [presets, setPresets] = useState<SpeedLogPreset[]>([]);
  const [params, setParams] = useState<SpeedLogParam[] | null>(null);
  const [messages, setMessages] = useState<SpeedLogMessage[] | null>(null);
  const [panels, setPanels] = useState<Panel[]>(() => [newPanel()]);
  const [activeId, setActiveId] = useState<number>(() => panels[0].id);
  const [range, setRange] = useState<[number, number] | null>(null);
  const [marker, setMarker] = useState<number | null>(null);
  const [tab, setTab] = useState<Tab>("plot");
  const [busy, setBusy] = useState<string | null>(null);
  const [lang, setLang] = useState<SpeedLogLang>(() => loadSetting<SpeedLogLang>("speedlog.lang", "en"));
  const [showModes, setShowModes] = useState(() => loadSetting("speedlog.modes", true));
  const [showDocs, setShowDocs] = useState(() => loadSetting("speedlog.docs", true));
  const [sidebar, setSidebar] = useState(() => loadSetting("speedlog.sidebar", true));
  const [presetsOpen, setPresetsOpen] = useState(false);
  const [logsOpen, setLogsOpen] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const panelsRef = useRef<HTMLDivElement>(null);

  useEffect(() => saveSetting("speedlog.lang", lang), [lang]);
  useEffect(() => saveSetting("speedlog.modes", showModes), [showModes]);
  useEffect(() => saveSetting("speedlog.docs", showDocs), [showDocs]);
  useEffect(() => saveSetting("speedlog.sidebar", sidebar), [sidebar]);

  const span = useMemo<[number, number]>(
    () => (info && info.tEnd > info.tStart ? [info.tStart, info.tEnd] : [0, 1]),
    [info],
  );

  const units = useMemo(() => {
    const out: Record<string, string | null> = {};
    for (const t of types) for (const f of t.fields) out[`${t.name}.${f.name}`] = f.unit;
    return out;
  }, [types]);

  /** A log was opened (from disk or from the vehicle): reset the view and load what the plotter needs. */
  const loaded = useCallback(
    async (next: SpeedLogInfo) => {
      setInfo(next);
      setRange(null);
      setMarker(null);
      setParams(null);
      setMessages(null);
      const first = newPanel();
      setPanels([first]);
      setActiveId(first.id);
      setBusy("Reading the log…");
      try {
        const [tree, m, p] = await Promise.all([api.speedlogTree(lang), api.speedlogModes(), api.speedlogPresets()]);
        setTypes(tree);
        setModes(m);
        setPresets(p);
      } catch (error) {
        notify(`Could not read the log: ${String(error)}`, false);
      } finally {
        setBusy(null);
      }
    },
    [lang, notify],
  );

  // A log opened before a reload of the UI is still open on the Rust side.
  useEffect(() => {
    api
      .speedlogInfo()
      .then((current) => {
        if (current) void loaded(current);
      })
      .catch(() => undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Texts follow the language: reload the tree, and the tables when they were loaded.
  const langRef = useRef(lang);
  useEffect(() => {
    if (langRef.current === lang || !info) {
      langRef.current = lang;
      return;
    }
    langRef.current = lang;
    api.speedlogTree(lang).then(setTypes).catch(() => undefined);
    setParams(null);
    setMessages(null);
  }, [lang, info]);

  // Tables are loaded when their tab is first opened.
  useEffect(() => {
    if (!info) return;
    if (tab === "params" && params === null) {
      api.speedlogParams(lang).then(setParams).catch((e) => notify(`Could not read the parameters: ${String(e)}`, false));
    }
    if (tab === "messages" && messages === null) {
      api.speedlogMessages(lang).then(setMessages).catch((e) => notify(`Could not read the messages: ${String(e)}`, false));
    }
  }, [tab, info, lang, params, messages, notify]);

  const openFile = async () => {
    setBusy("Opening…");
    try {
      const next = await api.speedlogOpen();
      if (next) await loaded(next);
    } catch (error) {
      notify(String(error), false);
    } finally {
      setBusy(null);
    }
  };

  const activePanel = panels.find((p) => p.id === activeId) ?? panels[0];
  const activeKeys = useMemo(() => new Set((activePanel?.traces ?? []).map(traceKey)), [activePanel]);
  const plottedKeys = useMemo(() => new Set(panels.flatMap((p) => p.traces.map(traceKey))), [panels]);

  const addPanel = useCallback((traces: SpeedLogTrace[] = []) => {
    const panel = newPanel(traces);
    setPanels((prev) => [...prev, panel]);
    setActiveId(panel.id);
    window.setTimeout(() => panelsRef.current?.lastElementChild?.scrollIntoView({ block: "nearest", behavior: "smooth" }), 50);
    return panel;
  }, []);

  const toggleTrace = (type: string, field: string) => {
    if (!activePanel) return;
    const key = `${type}.${field}`;
    setPanels((prev) =>
      prev.map((p) => {
        if (p.id !== activePanel.id) return p;
        if (p.traces.some((t) => traceKey(t) === key)) return { ...p, traces: p.traces.filter((t) => traceKey(t) !== key) };
        if (p.traces.length >= MAX_TRACES) {
          notify(`A panel holds up to ${MAX_TRACES} traces: add a panel (P) for more.`, false);
          return p;
        }
        return { ...p, traces: [...p.traces, { type, field }] };
      }),
    );
  };

  const applyPreset = (preset: SpeedLogPreset) => {
    setPresetsOpen(false);
    setTab("plot");
    const fields = preset.fields.slice(0, MAX_TRACES);
    if (activePanel && activePanel.traces.length === 0) {
      setPanels((prev) => prev.map((p) => (p.id === activePanel.id ? { ...p, traces: fields } : p)));
    } else {
      addPanel(fields);
    }
  };

  const updatePanel = (id: number, change: (p: Panel) => Panel) => setPanels((prev) => prev.map((p) => (p.id === id ? change(p) : p)));

  const closePanel = (id: number) => {
    setPanels((prev) => {
      const rest = prev.filter((p) => p.id !== id);
      if (rest.length === 0) {
        const panel = newPanel();
        setActiveId(panel.id);
        return [panel];
      }
      if (id === activeId) setActiveId(rest[rest.length - 1].id);
      return rest;
    });
  };

  const movePanel = (id: number, step: number) =>
    setPanels((prev) => {
      const i = prev.findIndex((p) => p.id === id);
      const j = i + step;
      if (i < 0 || j < 0 || j >= prev.length) return prev;
      const next = [...prev];
      [next[i], next[j]] = [next[j], next[i]];
      return next;
    });

  const zoom = (next: [number, number]) => setRange(next);
  const resetZoom = () => setRange(null);

  const pickMessage = (t: number) => {
    setMarker(t);
    setTab("plot");
    // Keep the marker in view: centre the window on it when it is outside
    setRange((current) => {
      if (!current || (t >= current[0] && t <= current[1])) return current;
      const half = (current[1] - current[0]) / 2;
      return [t - half, t + half];
    });
  };

  // Keyboard shortcuts while the screen is visible (not while typing).
  useEffect(() => {
    if (!active) return;
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      if (target && (target.tagName === "INPUT" || target.tagName === "SELECT" || target.tagName === "TEXTAREA")) {
        if (e.key === "Escape") target.blur();
        return;
      }
      if (!info) return;
      switch (e.key) {
        case "/":
          e.preventDefault();
          setSidebar(true);
          setTab("plot");
          window.setTimeout(() => searchRef.current?.focus(), 0);
          break;
        case "p":
        case "P":
          setTab("plot");
          addPanel();
          break;
        case "m":
        case "M":
          setShowModes((v) => !v);
          break;
        case "r":
        case "R":
          resetZoom();
          break;
        case "s":
        case "S":
          setSidebar((v) => !v);
          break;
        default:
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active, info, addPanel]);

  return (
    <div className="sl-view">
      <div className="sl-toolbar">
        <button className="primary" onClick={openFile} disabled={!!busy}>
          Open log…
        </button>
        <button
          onClick={() => setLogsOpen(true)}
          disabled={!connected || !!busy}
          title={connected ? "List and download the connected vehicle's logs" : "Connect to a vehicle to list its logs"}
        >
          Vehicle logs…
        </button>
        {info && (
          <span className="sl-file" title={[info.fileName, info.firmware, info.board, info.os].filter(Boolean).join("\n")}>
            <strong className="mono">{info.fileName}</strong>
            <span className="dim">
              {formatBytes(info.size)} · {formatDuration(info.duration)}
              {info.firmware ? ` · ${info.firmware}` : ` · ${info.vehicle}`}
            </span>
          </span>
        )}
        {busy && <span className="dim">{busy}</span>}
        <div className="sl-spacer" />
        {info && (
          <>
            <div className="sl-tabs" role="tablist">
              {(["plot", "params", "messages"] as Tab[]).map((t) => (
                <button key={t} role="tab" aria-selected={tab === t} className={tab === t ? "primary" : ""} onClick={() => setTab(t)}>
                  {t === "plot" ? "Plot" : t === "params" ? "Parameters" : "Messages"}
                </button>
              ))}
            </div>
            {tab === "plot" && (
              <>
                <button onClick={() => addPanel()} title="Add a panel (P)">
                  + Panel
                </button>
                <div className="sl-menu-wrap">
                  <button onClick={() => setPresetsOpen((v) => !v)} aria-expanded={presetsOpen} disabled={presets.length === 0}>
                    Presets ▾
                  </button>
                  {presetsOpen && (
                    <ul className="sl-menu" role="menu">
                      {presets.map((p) => (
                        <li key={p.id}>
                          <button role="menuitem" onClick={() => applyPreset(p)}>
                            {PRESET_LABELS[p.id] ?? p.id}
                            <span className="dim"> {p.fields.map((f) => f.field).join(", ")}</span>
                          </button>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
                <button className={showModes ? "primary" : ""} aria-pressed={showModes} onClick={() => setShowModes((v) => !v)} title="Shade flight modes (M)">
                  Modes
                </button>
                <button onClick={resetZoom} disabled={!range} title="Back to the whole log (R, or double click a plot)">
                  Reset zoom
                </button>
                {marker !== null && (
                  <button onClick={() => setMarker(null)} title="Remove the message marker">
                    Clear marker
                  </button>
                )}
              </>
            )}
          </>
        )}
        <button onClick={() => setLang((l) => (l === "en" ? "pt" : "en"))} title="Language of the descriptions and decoded messages">
          {lang === "en" ? "EN → PT" : "PT → EN"}
        </button>
      </div>

      {!info ? (
        <div className="panel sl-empty">
          <h1>SpeedLOG</h1>
          <p>Offline viewer for ArduPilot DataFlash (.bin) logs. Nothing is plotted until you pick it.</p>
          <p className="dim">
            Open a log from disk, or download one from the connected vehicle. Then tick fields in the sidebar, drag on a plot to
            zoom (all panels follow) and double click to go back. Shortcuts: <kbd>/</kbd> search, <kbd>P</kbd> new panel,{" "}
            <kbd>M</kbd> modes, <kbd>R</kbd> reset zoom, <kbd>S</kbd> sidebar.
          </p>
          <div className="sl-empty-actions">
            <button className="primary" onClick={openFile} disabled={!!busy}>
              Open log…
            </button>
            <button onClick={() => setLogsOpen(true)} disabled={!connected || !!busy}>
              Vehicle logs…
            </button>
          </div>
        </div>
      ) : tab === "params" ? (
        <ParamsTable params={params} />
      ) : tab === "messages" ? (
        <MessagesTable messages={messages} onPick={pickMessage} />
      ) : (
        <div className={`sl-body ${sidebar ? "" : "no-side"}`}>
          {sidebar && (
            <aside className="panel sl-side">
              <header>
                <h2>Fields</h2>
                <div className="sl-side-tools">
                  <label className="sl-check" title="Show what each message and field means">
                    <input type="checkbox" checked={showDocs} onChange={(e) => setShowDocs(e.target.checked)} />
                    Descriptions
                  </label>
                  <button onClick={() => setSidebar(false)} title="Hide the sidebar (S)" aria-label="Hide the sidebar">
                    ‹
                  </button>
                </div>
              </header>
              <FieldTree ref={searchRef} types={types} active={activeKeys} plotted={plottedKeys} showDocs={showDocs} onToggle={toggleTrace} />
            </aside>
          )}
          <div className="sl-panels" ref={panelsRef}>
            {!sidebar && (
              <button className="sl-side-show" onClick={() => setSidebar(true)} title="Show the field sidebar (S)">
                › Fields
              </button>
            )}
            {showModes && modes.length > 0 && (
              <div className="sl-mode-legend">
                {[...new Map(modes.map((m) => [m.name, m])).values()].map((m) => (
                  <span key={m.name} className={`sl-mode-chip slot-${m.slot}`}>
                    {m.name}
                  </span>
                ))}
              </div>
            )}
            {panels.map((panel, index) => (
              <PlotPanel
                key={panel.id}
                panel={panel}
                index={index}
                count={panels.length}
                active={panel.id === activePanel?.id}
                range={range}
                span={span}
                units={units}
                modes={modes}
                showModes={showModes}
                marker={marker}
                onActivate={() => setActiveId(panel.id)}
                onRemoveTrace={(trace) =>
                  updatePanel(panel.id, (p) => ({ ...p, traces: p.traces.filter((t) => traceKey(t) !== traceKey(trace)) }))
                }
                onZoom={zoom}
                onReset={resetZoom}
                onClose={() => closePanel(panel.id)}
                onClear={() => updatePanel(panel.id, (p) => ({ ...p, traces: [] }))}
                onMove={(step) => movePanel(panel.id, step)}
                notify={notify}
              />
            ))}
          </div>
        </div>
      )}
      {logsOpen && (
        <VehicleLogsDialog
          connected={connected}
          onClose={() => setLogsOpen(false)}
          onOpened={(next) => {
            setTab("plot");
            void loaded(next);
          }}
          notify={notify}
        />
      )}
    </div>
  );
}
