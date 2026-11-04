import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Unlisten } from "../../api";
import type { InspectorMessage, InspectorSnapshot } from "../../types";
import { SeriesStore } from "./seriesStore";

type Notify = (text: string, ok: boolean) => void;

/**
 * Talks to the backend while the screen is active: enables the stream, keeps the watched series
 * in a [`SeriesStore`] (outside React state) and releases every watch when the screen goes away.
 *
 * Pausing freezes the message list; the graph freezes with it (see GraphPanel) but series keep
 * being recorded, so resuming shows the whole history without a gap.
 */
export function useInspector(active: boolean, notify: Notify) {
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
        const off = await api.onInspector(handle);
        if (disposed) {
          off();
          return;
        }
        unlisten = off;
        await api.inspectorEnable(true);
        for (const key of watchedRef.current) {
          store.track(key);
          await api.inspectorWatch(key);
        }
      } catch (error) {
        notifyRef.current(`Inspector: ${String(error)}`, false);
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
      void api.inspectorEnable(false).catch(() => undefined);
      for (const key of watchedRef.current) void api.inspectorUnwatch(key).catch(() => undefined);
    };
  }, [active, store]);

  const togglePause = useCallback(() => {
    pausedRef.current = !pausedRef.current;
    setPaused(pausedRef.current);
  }, []);

  const removeWatch = useCallback(
    (key: string) => {
      watchedRef.current = watchedRef.current.filter((k) => k !== key);
      setWatched(watchedRef.current);
      store.untrack(key);
      void api.inspectorUnwatch(key).catch(() => undefined);
    },
    [store],
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
        void api.inspectorWatch(key).catch((error) => notifyRef.current(`Graph: ${String(error)}`, false));
      }
    },
    [removeWatch, store],
  );

  const clearAll = useCallback(() => {
    void api.inspectorClear().catch((error) => notifyRef.current(`Clear: ${String(error)}`, false));
    store.clear();
    setMessages([]);
  }, [store]);

  return { store, messages, paused, watched, togglePause, toggleWatch, removeWatch, clearAll };
}
