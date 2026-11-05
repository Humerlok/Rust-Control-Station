import { useEffect, useState } from "react";
import { api, type Unlisten } from "./api";
import { idleDroneCan, type DroneCanState } from "./types";

/** DroneCAN node table and logs from the backend. */
export function useDroneCan(): DroneCanState {
  const [state, setState] = useState<DroneCanState>(idleDroneCan);

  useEffect(() => {
    let alive = true;
    let unlisten: Unlisten | null = null;
    (async () => {
      try {
        const initial = await api.dronecanStatus();
        if (alive) setState(initial);
      } catch {
        // the first event will fill the state
      }
      const stop = await api.onDroneCan(setState);
      if (alive) unlisten = stop;
      else stop();
    })();
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  return state;
}
