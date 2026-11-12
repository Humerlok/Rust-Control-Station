/** Motor test state via `api.motorStatus` + `api.onMotorTest`; `api.motorWatchEsc` toggles ESC telemetry streaming. */
import { useEffect, useState } from "react";
import { api, type Unlisten } from "./api";
import { idleMotorTest, type MotorTestState } from "./types";

/** Motor test state from the backend; also asks the vehicle for ESC telemetry while `watch` is true. */
export function useMotorTest(watch: boolean): MotorTestState {
  const [state, setState] = useState<MotorTestState>(idleMotorTest);

  useEffect(() => {
    let alive = true;
    let unlisten: Unlisten | null = null;
    (async () => {
      try {
        const initial = await api.motorStatus();
        if (alive) setState(initial);
      } catch {
        // the first event will fill the state
      }
      const stop = await api.onMotorTest(setState);
      if (alive) unlisten = stop;
      else stop();
    })();
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  // ESC telemetry is only requested while the screen is visible, and switched off again on leave
  useEffect(() => {
    if (!watch) return;
    api.motorWatchEsc(true).catch(() => undefined);
    return () => {
      api.motorWatchEsc(false).catch(() => undefined);
    };
  }, [watch]);

  return state;
}
