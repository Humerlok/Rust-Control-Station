import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type Unlisten } from "../api";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { PlanGrid } from "../components/plan/PlanGrid";
import { PlanMap } from "../components/plan/PlanMap";
import {
  FRAME_OPTIONS,
  KIND_LABELS,
  addPoint,
  computeStats,
  defaultCommand,
  formatDistance,
  hasHome,
  homeItem,
  insertAt,
  isPositional,
  itemProblems,
  makeItem,
  midpoint,
  moveBy,
  patchAt,
  removeAt,
  renumber,
  totalDistance,
} from "../components/plan/missionModel";
import "../flightplan.css";
import { loadSetting, saveSetting } from "../storage";
import type { MissionCommandInfo, MissionItem, MissionKind, MissionProgress, Telemetry } from "../types";

type Editor = { items: MissionItem[]; dirty: boolean; selected: number | null };
type Editors = Record<MissionKind, Editor>;
type Dialog = { type: "read" } | { type: "write" } | { type: "clear" } | { type: "clear-editor" } | { type: "current"; index: number };

const emptyEditor = (): Editor => ({ items: [], dirty: false, selected: null });
const KINDS: MissionKind[] = ["mission", "fence", "rally"];

type Props = { telemetry: Telemetry; notify: (text: string, ok: boolean) => void };

export function FlightPlan({ telemetry, notify }: Props) {
  const [kind, setKind] = useState<MissionKind>("mission");
  const [editors, setEditors] = useState<Editors>({ mission: emptyEditor(), fence: emptyEditor(), rally: emptyEditor() });
  const [catalog, setCatalog] = useState<MissionCommandInfo[]>([]);
  const [defaultAlt, setDefaultAlt] = useState(() => String(loadSetting<number>("plan.alt", 30)));
  const [defaultFrame, setDefaultFrame] = useState(() => loadSetting<number>("plan.frame", 3));
  const [fitSignal, setFitSignal] = useState(0);
  const [focus, setFocus] = useState<{ index: number; token: number } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [progress, setProgress] = useState<MissionProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const focusToken = useRef(0);

  const editor = editors[kind];
  const connected = telemetry.connected;
  const commands = useMemo(() => new Map(catalog.map((c) => [c.id, c])), [catalog]);
  const stats = useMemo(() => computeStats(editor.items, commands), [editor.items, commands]);
  const distance = useMemo(() => totalDistance(stats), [stats]);
  const wpno = telemetry.fields.wpno;
  const currentSeq = kind === "mission" && connected && wpno !== undefined ? Math.round(wpno) : null;

  // The latest values for callbacks that must stay stable between renders.
  const live = useRef({ kind, catalog, defaultAlt, defaultFrame, telemetry });
  live.current = { kind, catalog, defaultAlt, defaultFrame, telemetry };

  const mutate = useCallback((target: MissionKind, change: (e: Editor) => Editor) => {
    setEditors((prev) => ({ ...prev, [target]: change(prev[target]) }));
  }, []);

  useEffect(() => {
    let cancelled = false;
    api
      .missionCommands()
      .then((list) => !cancelled && setCatalog(list))
      .catch(() => !cancelled && setCatalog([]));
    return () => {
      cancelled = true;
    };
  }, [telemetry.vehicleType, connected]);

  useEffect(() => {
    let off: Unlisten | undefined;
    let dead = false;
    let timer: number | undefined;
    void api
      .onMissionProgress((p) => {
        setProgress(p);
        if (p.error) setError(p.error);
        window.clearTimeout(timer);
        if (p.finished) timer = window.setTimeout(() => setProgress(null), 1500);
      })
      .then((unlisten) => {
        if (dead) unlisten();
        else off = unlisten;
      });
    return () => {
      dead = true;
      window.clearTimeout(timer);
      off?.();
    };
  }, []);

  useEffect(() => {
    saveSetting("plan.alt", Number(defaultAlt) || 30);
  }, [defaultAlt]);
  useEffect(() => {
    saveSetting("plan.frame", defaultFrame);
  }, [defaultFrame]);

  // A mission always has a home row: take it from the vehicle as soon as one is known.
  const missionEmpty = editors.mission.items.length === 0;
  useEffect(() => {
    if (!missionEmpty) return;
    const home = telemetry.home;
    const position = telemetry.position;
    const source = home ?? (position ? { lat: position.lat, lon: position.lon, altM: 0 } : null);
    if (!source) return;
    mutate("mission", (e) =>
      e.items.length > 0 ? e : { items: [homeItem(source.lat, source.lon, source.altM)], dirty: false, selected: null },
    );
    setFitSignal((n) => n + 1);
  }, [missionEmpty, telemetry.home, telemetry.position, mutate]);

  const newPointDefaults = useCallback(() => {
    const { kind: k, catalog: list, defaultAlt: alt, defaultFrame: frame } = live.current;
    const altitude = Number(alt);
    return {
      command: defaultCommand(k, list),
      z: Number.isFinite(altitude) ? altitude : 30,
      // Fence and rally points are always global.
      frame: k === "mission" ? frame : 0,
    };
  }, []);

  const requestFocus = useCallback((index: number) => {
    focusToken.current += 1;
    setFocus({ index, token: focusToken.current });
  }, []);

  const selectFromMap = useCallback((index: number) => mutate(live.current.kind, (e) => ({ ...e, selected: index })), [mutate]);
  const selectFromGrid = useCallback(
    (index: number) => {
      mutate(live.current.kind, (e) => (e.selected === index ? e : { ...e, selected: index }));
      requestFocus(index);
    },
    [mutate, requestFocus],
  );

  const patch = useCallback(
    (index: number, change: Partial<MissionItem>) =>
      mutate(live.current.kind, (e) => ({ ...e, items: patchAt(e.items, index, change), dirty: true })),
    [mutate],
  );

  const move = useCallback(
    (index: number, delta: -1 | 1) =>
      mutate(live.current.kind, (e) => ({ ...e, items: moveBy(e.items, index, delta), selected: index + delta, dirty: true })),
    [mutate],
  );

  const remove = useCallback(
    (index: number) =>
      mutate(live.current.kind, (e) => {
        const selected = e.selected === null ? null : e.selected === index ? null : e.selected > index ? e.selected - 1 : e.selected;
        return { items: removeAt(e.items, index), dirty: true, selected };
      }),
    [mutate],
  );

  const addAtEnd = useCallback(
    (lat: number, lon: number, center: { lat: number; lon: number }) => {
      const k = live.current.kind;
      const defaults = newPointDefaults();
      const home = live.current.telemetry.home;
      mutate(k, (e) => {
        let items = e.items;
        if (hasHome(k) && items.length === 0) {
          items = [home ? homeItem(home.lat, home.lon, home.altM) : homeItem(center.lat, center.lon, 0)];
        }
        items = addPoint(items, makeItem({ ...defaults, x: lat, y: lon }));
        return { items, dirty: true, selected: items.length - 1 };
      });
    },
    [mutate, newPointDefaults],
  );

  const insert = useCallback(
    (index: number, where: "before" | "after") => {
      const defaults = newPointDefaults();
      const cmds = new Map(live.current.catalog.map((c) => [c.id, c]));
      mutate(live.current.kind, (e) => {
        const here = e.items[index];
        if (!here) return e;
        const neighbour = e.items[where === "before" ? index - 1 : index + 1];
        const mid =
          neighbour && isPositional(neighbour, cmds) && isPositional(here, cmds)
            ? midpoint(neighbour, here)
            : { x: here.x, y: here.y + 0.0002, z: here.z };
        const at = where === "before" ? index : index + 1;
        return {
          items: insertAt(e.items, at, makeItem({ ...defaults, x: mid.x, y: mid.y, z: defaults.z })),
          dirty: true,
          selected: at,
        };
      });
    },
    [mutate, newPointDefaults],
  );

  const setHome = useCallback(
    (lat: number, lon: number) =>
      mutate("mission", (e) => ({
        items: e.items.length === 0 ? [homeItem(lat, lon, live.current.telemetry.home?.altM ?? 0)] : patchAt(e.items, 0, { x: lat, y: lon }),
        dirty: true,
        selected: 0,
      })),
    [mutate],
  );

  function switchKind(next: MissionKind) {
    setKind(next);
    setError(null);
    setFitSignal((n) => n + 1);
  }

  async function run<T>(label: string, work: () => Promise<T>): Promise<T | undefined> {
    setBusy(label);
    setError(null);
    setProgress(null);
    try {
      return await work();
    } catch (failure) {
      setError(String(failure));
      notify(`${label} failed: ${String(failure)}`, false);
      return undefined;
    } finally {
      setBusy(null);
    }
  }

  async function doRead() {
    const target = kind;
    const items = await run("Read", () => api.missionDownload(target));
    if (!items) return;
    mutate(target, () => ({ items: renumber(items), dirty: false, selected: null }));
    setFitSignal((n) => n + 1);
    notify(`Read ${items.length} ${items.length === 1 ? "item" : "items"} from the vehicle`, true);
  }

  function uploadItems(): MissionItem[] | null {
    let items = editor.items;
    if (hasHome(kind) && items.length === 0) {
      const home = telemetry.home;
      if (!home) {
        notify("Write: the mission has no home position and the vehicle has not reported one", false);
        return null;
      }
      items = [homeItem(home.lat, home.lon, home.altM)];
    }
    if (items.length === 0) {
      notify("Write: there is nothing to write. Use Clear on vehicle to empty it.", false);
      return null;
    }
    const problems = itemProblems(items, commands);
    if (problems.length > 0) {
      notify(`Write: ${problems[0]}${problems.length > 1 ? ` (+${problems.length - 1} more)` : ""}`, false);
      return null;
    }
    return renumber(items);
  }

  async function doWrite() {
    const items = uploadItems();
    if (!items) return;
    const target = kind;
    const done = await run("Write", async () => {
      await api.missionUpload(target, items);
      return true;
    });
    if (!done) return;
    mutate(target, (e) => ({ ...e, items, dirty: false }));
    notify(`Wrote ${items.length} ${items.length === 1 ? "item" : "items"} to the vehicle`, true);
  }

  async function doClearVehicle() {
    const target = kind;
    const done = await run("Clear", async () => {
      await api.missionClear(target);
      return true;
    });
    if (!done) return;
    mutate(target, (e) => ({ items: hasHome(target) ? e.items.slice(0, 1) : [], dirty: false, selected: null }));
    notify(`${KIND_LABELS[target]} cleared on the vehicle`, true);
  }

  async function doSetCurrent(index: number) {
    try {
      notify(`Current waypoint ${index}: ${await api.setWaypoint(index)}`, true);
    } catch (failure) {
      notify(`Set current waypoint failed: ${String(failure)}`, false);
    }
  }

  async function onFileChosen(file: File | undefined) {
    if (!file) return;
    const target = kind;
    try {
      const items = await api.missionParseFile(await file.text());
      mutate(target, () => ({ items: renumber(items), dirty: true, selected: null }));
      setFitSignal((n) => n + 1);
      setError(null);
      notify(`Loaded ${items.length} items from ${file.name}`, true);
    } catch (failure) {
      setError(String(failure));
      notify(`Load failed: ${String(failure)}`, false);
    }
  }

  async function saveFile() {
    try {
      const text = await api.missionFormatFile(editor.items);
      const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
      const link = document.createElement("a");
      link.href = url;
      link.download = `${kind}.waypoints`;
      document.body.appendChild(link);
      link.click();
      link.remove();
      URL.revokeObjectURL(url);
      notify(`Saved ${editor.items.length} items to ${link.download}`, true);
    } catch (failure) {
      notify(`Save failed: ${String(failure)}`, false);
    }
  }

  function askWrite() {
    if (uploadItems()) setDialog({ type: "write" });
  }

  const percent = progress && progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0;
  const count = editor.items.length;

  return (
    <div className="plan">
      <div className="plan-toolbar">
        <label className="inline" title="Fence and rally editing is basic: new points use one fixed command and the global frame.">
          Type
          <select value={kind} onChange={(event) => switchKind(event.target.value as MissionKind)}>
            {KINDS.map((k) => (
              <option key={k} value={k}>
                {KIND_LABELS[k]}
              </option>
            ))}
          </select>
        </label>
        <label className="inline">
          Default alt (m)
          <input
            className="alt-in"
            type="number"
            min={0}
            value={defaultAlt}
            onChange={(event) => setDefaultAlt(event.target.value)}
            aria-label="Default altitude in meters"
          />
        </label>
        <label className="inline">
          Frame
          <select value={defaultFrame} disabled={kind !== "mission"} onChange={(event) => setDefaultFrame(Number(event.target.value))}>
            {FRAME_OPTIONS.map((f) => (
              <option key={f.value} value={f.value}>
                {f.label}
              </option>
            ))}
          </select>
        </label>
        <span className="sep" />
        <button disabled={!connected || busy !== null} onClick={() => (editor.dirty ? setDialog({ type: "read" }) : void doRead())}>
          Read
        </button>
        <button className="primary" disabled={!connected || busy !== null || (count === 0 && !telemetry.home)} onClick={askWrite}>
          Write
        </button>
        <button className="danger" disabled={!connected || busy !== null} onClick={() => setDialog({ type: "clear" })}>
          Clear on vehicle
        </button>
        <span className="sep" />
        <button onClick={() => fileInput.current?.click()}>Load file</button>
        <button disabled={count === 0} onClick={() => void saveFile()}>
          Save file
        </button>
        <button disabled={count <= (hasHome(kind) ? 1 : 0)} onClick={() => setDialog({ type: "clear-editor" })}>
          Clear editor
        </button>
        <input
          ref={fileInput}
          type="file"
          accept=".waypoints,.txt,.mission"
          hidden
          onChange={(event) => {
            void onFileChosen(event.target.files?.[0]);
            event.target.value = "";
          }}
        />
        <span className="spacer" />
        {(busy || progress) && (
          <div className="progress small" role="progressbar" aria-valuenow={percent} aria-valuemin={0} aria-valuemax={100}>
            <div style={{ width: `${percent}%` }} />
            <span>
              {progress ? `${progress.phase === "upload" ? "Writing" : "Reading"} ${progress.done}/${progress.total}` : `${busy}...`}
            </span>
          </div>
        )}
        <span className="summary dim">
          {count} {count === 1 ? "item" : "items"} · {formatDistance(distance)}
          {editor.dirty ? " · unsaved" : ""}
        </span>
      </div>

      {error && (
        <div className="plan-error" role="alert">
          {error}
          <button onClick={() => setError(null)} aria-label="Dismiss error">
            ✕
          </button>
        </div>
      )}

      <div className="plan-body">
        <PlanMap
          kind={kind}
          items={editor.items}
          commands={commands}
          selected={editor.selected}
          currentSeq={currentSeq}
          telemetry={telemetry}
          fitSignal={fitSignal}
          focus={focus}
          canSetCurrent={connected}
          onSelect={selectFromMap}
          onAdd={addAtEnd}
          onMove={(index, lat, lon) => patch(index, { x: lat, y: lon })}
          onDelete={remove}
          onInsert={insert}
          onSetCurrent={(index) => setDialog({ type: "current", index })}
          onSetHome={setHome}
          onFit={() => setFitSignal((n) => n + 1)}
        />
        <PlanGrid
          kind={kind}
          items={editor.items}
          catalog={catalog}
          commands={commands}
          stats={stats}
          selected={editor.selected}
          currentSeq={currentSeq}
          onSelect={selectFromGrid}
          onPatch={patch}
          onMove={move}
          onDelete={remove}
        />
      </div>

      {dialog?.type === "read" && (
        <ConfirmDialog
          title="Replace the editor content?"
          tone="warn"
          message="The editor has changes that were not written to the vehicle. Reading replaces them."
          confirmLabel="Read"
          onConfirm={() => {
            setDialog(null);
            void doRead();
          }}
          onCancel={() => setDialog(null)}
        />
      )}
      {dialog?.type === "write" && (
        <ConfirmDialog
          title={`Write the ${KIND_LABELS[kind].toLowerCase()} to the vehicle?`}
          tone="warn"
          message={`${uploadItems()?.length ?? count} items will be uploaded. The ${KIND_LABELS[kind].toLowerCase()} stored on the vehicle is replaced.`}
          confirmLabel="Write"
          onConfirm={() => {
            setDialog(null);
            void doWrite();
          }}
          onCancel={() => setDialog(null)}
        />
      )}
      {dialog?.type === "clear" && (
        <ConfirmDialog
          title={`Clear the ${KIND_LABELS[kind].toLowerCase()} on the vehicle?`}
          tone="danger"
          message="Every stored item is deleted from the vehicle. This cannot be undone."
          confirmLabel="Clear"
          onConfirm={() => {
            setDialog(null);
            void doClearVehicle();
          }}
          onCancel={() => setDialog(null)}
        />
      )}
      {dialog?.type === "clear-editor" && (
        <ConfirmDialog
          title="Clear the editor?"
          message="Every item in the editor is removed. The vehicle is not changed."
          confirmLabel="Clear editor"
          onConfirm={() => {
            setDialog(null);
            mutate(kind, () => emptyEditor());
          }}
          onCancel={() => setDialog(null)}
        />
      )}
      {dialog?.type === "current" && (
        <ConfirmDialog
          title={`Set waypoint ${dialog.index} as current?`}
          tone="warn"
          message="In AUTO the vehicle continues the mission from this waypoint."
          confirmLabel="Set current"
          onConfirm={() => {
            const index = dialog.index;
            setDialog(null);
            void doSetCurrent(index);
          }}
          onCancel={() => setDialog(null)}
        />
      )}
    </div>
  );
}
