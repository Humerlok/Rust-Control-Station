import { useSyncExternalStore } from "react";
import { api } from "./api";
import { isEnableName, naturalCompare, sameValue } from "./paramUtils";
import { loadSetting, saveSetting } from "./storage";
import type { ParamEntry, ParamMeta, ParamsProgress } from "./types";

export type ParamState = {
  list: ParamEntry[];
  byName: Map<string, ParamEntry>;
  metadata: Record<string, ParamMeta>;
  /** Values typed by the user and not yet written to the vehicle. */
  edits: Record<string, number>;
  /** Last write failure per parameter. */
  errors: Record<string, string>;
  favorites: Set<string>;
  progress: ParamsProgress | null;
  refreshing: boolean;
  refreshError: string | null;
  writing: boolean;
  rebootRequired: boolean;
  linkConnected: boolean;
  vehicleAlive: boolean;
  vehicleType: string;
};

export type WriteSummary = { written: number; failed: number; rebootRequired: boolean };

const FAVORITES_KEY = "params.favorites";
const listeners = new Set<() => void>();
let initialized = false;
let metadataRequested = false;
/** `sys:comp` of the vehicle the list belongs to, to notice when the user switches vehicles. */
let vehicleKey: string | null = null;
/** The selection changed while a download was running: download again once it ends. */
let switchPending = false;

let state: ParamState = {
  list: [],
  byName: new Map(),
  metadata: {},
  edits: {},
  errors: {},
  favorites: new Set(loadSetting<string[]>(FAVORITES_KEY, [])),
  progress: null,
  refreshing: false,
  refreshError: null,
  writing: false,
  rebootRequired: false,
  linkConnected: false,
  vehicleAlive: false,
  vehicleType: "Unknown",
};

function update(patch: Partial<ParamState>) {
  state = { ...state, ...patch };
  listeners.forEach((listener) => listener());
}

function without<T>(record: Record<string, T>, names: string[]): Record<string, T> {
  const next = { ...record };
  names.forEach((name) => delete next[name]);
  return next;
}

function setList(list: ParamEntry[]) {
  const sorted = [...list].sort((a, b) => naturalCompare(a.name, b.name));
  update({ list: sorted, byName: new Map(sorted.map((entry) => [entry.name, entry])) });
}

function ensureInit() {
  if (initialized) return;
  initialized = true;
  void api.onParamsProgress((progress) => {
    update({ progress: progress.done ? null : progress });
    // Any finished download (also one started elsewhere) makes the list and metadata current.
    if (progress.done && !progress.error && !switchPending) {
      void loadList()
        .then(() => loadMetadata())
        .catch(() => undefined);
    }
  });
}

async function loadList(): Promise<void> {
  setList(await api.paramsList());
}

async function loadMetadata(): Promise<void> {
  if (metadataRequested) return;
  metadataRequested = true;
  try {
    const metadata = await api.paramsMetadata();
    update({ metadata });
    // Empty means no vehicle yet (or no data): ask again after the next download.
    if (Object.keys(metadata).length === 0) metadataRequested = false;
  } catch {
    metadataRequested = false;
  }
}

async function refresh(): Promise<void> {
  if (state.refreshing) return;
  update({ refreshing: true, refreshError: null, progress: { received: 0, total: 0, done: false, error: null } });
  try {
    await api.paramsRefresh();
    // A download for the previously selected vehicle: its list is not wanted any more.
    if (switchPending) return;
    await loadList();
    void loadMetadata();
  } catch (error) {
    if (!switchPending) update({ refreshError: String(error) });
  } finally {
    update({ refreshing: false, progress: null });
    if (switchPending) {
      switchPending = false;
      clearVehicleData();
      void refresh();
    }
  }
}

function clearVehicleData() {
  metadataRequested = false;
  update({
    list: [],
    byName: new Map(),
    metadata: {},
    edits: {},
    errors: {},
    progress: null,
    refreshError: null,
    rebootRequired: false,
  });
}

function setConnection(linkConnected: boolean, vehicleAlive: boolean, vehicleType: string, selected: string | null = null) {
  ensureInit();
  const wasConnected = state.linkConnected;
  update({ vehicleAlive, vehicleType });
  // Another vehicle/component selected on the same link: its parameters are a different set
  // (like the legacy planner, download them again).
  const switched = linkConnected && wasConnected && selected !== null && vehicleKey !== null && selected !== vehicleKey;
  if (linkConnected && selected !== null) vehicleKey = selected;
  if (!linkConnected) vehicleKey = null;
  if (switched) {
    clearVehicleData();
    if (state.refreshing) switchPending = true;
    else void refresh();
    return;
  }
  if (linkConnected === wasConnected) return;
  update({ linkConnected });
  if (linkConnected) {
    void loadList()
      .then(() => {
        if (state.list.length === 0 && state.linkConnected) void refresh();
      })
      .catch(() => undefined);
    void loadMetadata();
  } else {
    switchPending = false;
    clearVehicleData();
    update({ refreshing: false });
  }
}

function setEdit(name: string, value: number | null) {
  const entry = state.byName.get(name);
  const edits = { ...state.edits };
  if (value === null || !Number.isFinite(value) || (entry && sameValue(entry.value, value))) delete edits[name];
  else edits[name] = value;
  update({ edits, errors: without(state.errors, [name]) });
}

function discardEdits(names?: string[]) {
  update({
    edits: names ? without(state.edits, names) : {},
    errors: names ? without(state.errors, names) : {},
  });
}

/** Writes edits one by one (ENABLE parameters last); failures stay modified with their reason. */
async function writeParams(only?: string[]): Promise<WriteSummary> {
  const wanted = only ? new Set(only) : null;
  const names = Object.keys(state.edits)
    .filter((name) => !wanted || wanted.has(name))
    .sort((a, b) => Number(isEnableName(a)) - Number(isEnableName(b)) || naturalCompare(a, b));
  let written = 0;
  let failed = 0;
  let rebootRequired = false;
  update({ writing: true });
  try {
    for (const name of names) {
      const value = state.edits[name];
      try {
        const echoed = await api.paramSet(name, value);
        const entry = state.byName.get(name);
        if (entry) {
          const next = { ...entry, value: echoed };
          const byName = new Map(state.byName).set(name, next);
          update({ byName, list: state.list.map((e) => (e.name === name ? next : e)) });
        }
        update({ edits: without(state.edits, [name]), errors: without(state.errors, [name]) });
        if (state.metadata[name]?.rebootRequired) rebootRequired = true;
        written += 1;
      } catch (error) {
        update({ errors: { ...state.errors, [name]: String(error) } });
        failed += 1;
      }
    }
  } finally {
    update({ writing: false, rebootRequired: state.rebootRequired || rebootRequired });
  }
  return { written, failed, rebootRequired };
}

function toggleFavorite(name: string) {
  const favorites = new Set(state.favorites);
  if (!favorites.delete(name)) favorites.add(name);
  saveSetting(FAVORITES_KEY, [...favorites]);
  update({ favorites });
}

function clearRebootNotice() {
  update({ rebootRequired: false });
}

export const paramStore = {
  setConnection,
  refresh,
  loadList,
  setEdit,
  discardEdits,
  writeParams,
  toggleFavorite,
  clearRebootNotice,
};

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => listeners.delete(listener);
};

export function useParams(): ParamState {
  return useSyncExternalStore(subscribe, () => state);
}
