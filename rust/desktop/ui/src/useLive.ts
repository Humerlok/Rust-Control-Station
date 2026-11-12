/** App-wide live data: telemetry, link info and the STATUSTEXT log, from api snapshots plus onTelemetry/onLink/onStatusText events. */
import { useCallback, useEffect, useState } from "react";
import { api, type Unlisten } from "./api";
import { emptyLink, emptyTelemetry, type LinkInfo, type StatusText, type Telemetry } from "./types";

// Status-text history is capped so a long session does not grow memory without bound.
const MAX_MESSAGES = 500;

export function useLive() {
  const [telemetry, setTelemetry] = useState<Telemetry>(emptyTelemetry);
  const [link, setLink] = useState<LinkInfo>(emptyLink);
  const [messages, setMessages] = useState<StatusText[]>([]);

  useEffect(() => {
    let alive = true;
    const unlisteners: Unlisten[] = [];
    // Listener registration is async: if the effect was already cleaned up, unlisten immediately.
    const keep = (unlisten: Unlisten) => {
      if (alive) unlisteners.push(unlisten);
      else unlisten();
    };

    (async () => {
      try {
        const [initialTelemetry, initialLink] = await Promise.all([api.telemetrySnapshot(), api.linkStatus()]);
        if (!alive) return;
        setTelemetry(initialTelemetry);
        setLink(initialLink);
      } catch {
        // backend not ready yet: the first event will fill the state
      }
      keep(await api.onTelemetry(setTelemetry));
      keep(await api.onLink(setLink));
      keep(await api.onStatusText((message) => setMessages((all) => [...all.slice(-(MAX_MESSAGES - 1)), message])));
    })();

    return () => {
      alive = false;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  const clearMessages = useCallback(() => setMessages([]), []);
  return { telemetry, link, messages, clearMessages };
}
