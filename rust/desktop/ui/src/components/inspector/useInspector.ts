import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Unlisten } from "../../api";
import type { InspectorMessage, InspectorSnapshot } from "../../types";
import { SeriesStore } from "./seriesStore";

type Notify = (text: string, ok: boolean) => void;

/** Where an inspector gets its snapshots from: the MAVLink link or the DroneCAN bus. */
export type InspectorSource = {
  on(cb: (snapshot: InspectorSnapshot) => void): Promise<Unlisten>;
  enable(enabled: boolean): Promise<void>;
  watch(key: string): Promise<void>;
  unwatch(key: string): Promise<void>;
  clear(): Promise<void>;
};

export const mavlinkSource: InspectorSource = {
  on: (cb) => api.onInspector(cb),
  enable: (enabled) => api.inspectorEnable(enabled),
  watch: (key) => api.inspectorWatch(key),
  unwatch: (key) => api.inspectorUnwatch(key),
  clear: () => api.inspectorClear(),
};

export const droneCanSource: InspectorSource = {
  on: (cb) => api.onDroneCanInspector(cb),
  enable: (enabled) => api.dronecanInspectorEnable(enabled),
  watch: (key) => api.dronecanInspectorWatch(key),
  unwatch: (key) => api.dronecanInspectorUnwatch(key),
  clear: () => api.dronecanInspectorClear(),
};

/**
 * Talks to the backend while the screen is active: enables the stream, keeps the watched series
 * in a [`SeriesStore`] (outside React state) and releases every watch when the screen goes away.
 *
 * Pausing freezes the message list; the graph freezes with it (see GraphPanel) but series keep
 * being recorded, so resuming shows the whole history without a gap.
 */
export function useInspector(active: boolean, notify: Notify, source: InspectorSource = mavlinkSource) {
  const [store] = useState(() => new SeriesStore());
  const [messages, setMessages] = useState<InspectorMessage[]>([]);
  const [paused, setPaused] = useState(false);
  const [watched, setWatched] = useState<string[]>([]);

  const pausedRef = useRef(false);
  const watchedRef = useRef<string[]>([]);
  const activeRef = useRef(active);
  const notifyRef = useRef(notify);
  notifyRef.current = notify;
  activeRef.current = active;

  useEffect(() => {
    if (!active) return;
    let disposed = false;
    let unlisten: Unlisten | null = null;
    // The backend restarts its time base every time the inspector is enabled.
    store.clear();

    const handle = (snapshot: InspectorSnapshot) => {
      store.append(snapshot.series);
      if (!pausedRef.current) setMessages(snapshot.messages);
    };

    (async () => {
      try {
        const off = await source.on(handle);
        if (disposed) {
          off();
          return;
        }
        unlisten = off;
        await source.enable(true);
        for (const key of watchedRef.current) {
          store.track(key);
          await source.watch(key);
        }
      } catch (error) {
        notifyRef.current(`Inspector: ${String(error)}`, false);
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
      void source.enable(false).catch(() => undefined);
      for (const key of watchedRef.current) void source.unwatch(key).catch(() => undefined);
    };
  }, [active, store, source]);

  const togglePause = useCallback(() => {
    pausedRef.current = !pausedRef.current;
    setPaused(pausedRef.current);
  }, []);

  const removeWatch = useCallback(
    (key: string) => {
      watchedRef.current = watchedRef.current.filter((k) => k !== key);
      setWatched(watchedRef.current);
      store.untrack(key);
      void source.unwatch(key).catch(() => undefined);
    },
    [store, source],
  );

  const toggleWatch = useCallback(
    (key: string) => {
      if (watchedRef.current.includes(key)) {
        removeWatch(key);
        return;
      }
      watchedRef.current = [...watchedRef.current, key];
      setWatched(watchedRef.current);
      store.track(key);
      if (activeRef.current) {
        void source.watch(key).catch((error) => notifyRef.current(`Graph: ${String(error)}`, false));
      }
    },
    [removeWatch, store, source],
  );

  const clearAll = useCallback(() => {
    void source.clear().catch((error) => notifyRef.current(`Clear: ${String(error)}`, false));
    store.clear();
    setMessages([]);
  }, [store, source]);

  return { store, messages, paused, watched, togglePause, toggleWatch, removeWatch, clearAll };
}
