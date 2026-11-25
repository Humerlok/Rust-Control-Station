/** Replay state via `api.replayStatus` + `api.onReplay`: `status` is null unless the link is a log replay. */
import { useEffect, useState } from "react";
import { api, type Unlisten } from "./api";
import type { ReplayStatus } from "./types";

type State = { status: ReplayStatus | null; session: number };

/**
 * `resetKey` changes for every new replay and for every jump in its timeline: what the screen accumulated
 * (map track, messages) no longer matches the new moment and is rebuilt. The backend's `seekEpoch` restarts at 0
 * for each replay, so the key also counts the replays (a second log must not inherit the first one's track).
 *
 * `onReset` is called, from the event itself, when a replay starts and on every jump. It runs before the events
 * that follow, so what arrives after the jump (the log's opening messages, say) is kept, unlike a reset that
 * waited for a render.
 */
export function useReplay(onReset: () => void): { status: ReplayStatus | null; resetKey: string } {
  const [state, setState] = useState<State>({ status: null, session: 0 });

  useEffect(() => {
    let alive = true;
    let sawEvent = false;
    let unlisten: Unlisten | null = null;
    let session = 0;
    let previous: ReplayStatus | null = null;
    const apply = (next: ReplayStatus | null) => {
      const started = next !== null && previous === null;
      const jumped = next !== null && previous !== null && next.seekEpoch !== previous.seekEpoch;
      if (started) session += 1;
      previous = next;
      if (started || jumped) onReset();
      setState({ status: next, session });
    };
    (async () => {
      const stop = await api.onReplay((next) => {
        sawEvent = true;
        if (alive) apply(next);
      });
      if (alive) unlisten = stop;
      else stop();
      try {
        const initial = await api.replayStatus();
        // An event that arrived while asking is newer than the answer.
        if (alive && !sawEvent) apply(initial);
      } catch {
        // the first event will fill the state
      }
    })();
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [onReset]);

  return { status: state.status, resetKey: `${state.session}:${state.status?.seekEpoch ?? 0}` };
}
