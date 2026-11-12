/** Calibration state: one snapshot (`api.calibrationStatus`) then live updates (`api.onCalibration`). */
import { useEffect, useState } from "react";
import { api, type Unlisten } from "./api";
import { idleCalibration, type CalibrationState } from "./types";

export function useCalibration(): CalibrationState {
  const [state, setState] = useState<CalibrationState>(idleCalibration);

  useEffect(() => {
    // `alive` guards against the effect being cleaned up while the async setup is still awaiting
    let alive = true;
    let unlisten: Unlisten | null = null;
    (async () => {
      try {
        const initial = await api.calibrationStatus();
        if (alive) setState(initial);
      } catch {
        // the first event will fill the state
      }
      const stop = await api.onCalibration(setState);
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
