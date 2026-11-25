/**
 * Mock "Replay (log)": a three-minute timeline over the simulated flight, with play/pause, jumps, speed and the
 * end of the log, behaving like the real replay (crates/link/src/replay.rs). Spread into the mock `Backend` by
 * mockBackend.ts, which advances it from its 10 Hz tick.
 */
import type { Backend } from "./api";
import type { ReplayStatus } from "./types";

type Context = {
  /** Where the simulated vehicle is, as (lat, lon), `seconds` into the replay. */
  pathAt: (seconds: number) => [number, number];
};

type ReplayMock = Pick<
  Backend,
  "pickReplayLog" | "replayStatus" | "replayPlay" | "replayPause" | "replaySeek" | "replaySetSpeed" | "replayTrack" | "onReplay"
> & {
  /** Starts a replay of `path` (the file name is what the UI shows). */
  start(path: string): void;
  /** Ends the replay: listeners get `null`. */
  stop(): void;
  active(): boolean;
  /** Moves the replay on by `dtSeconds` of wall-clock time; returns the position the simulated flight is at. */
  advance(dtSeconds: number): number;
};

const DURATION_S = 180;
/** What the native file dialog "returns" in the mock. */
export const MOCK_LOG_PATH = "C:\\logs\\mock-flight.BIN";

export function createReplayMock({ pathAt }: Context): ReplayMock {
  const listeners = new Set<(status: ReplayStatus | null) => void>();
  let status: ReplayStatus | null = null;

  function publish(next: ReplayStatus | null) {
    status = next;
    listeners.forEach((cb) => cb(next));
  }

  function current(): ReplayStatus {
    if (!status) throw "not a replay link";
    return status;
  }

  const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);

  return {
    pickReplayLog: async () => MOCK_LOG_PATH,
    replayStatus: async () => status,
    async replayPlay() {
      const s = current();
      // At the end of the log Play starts it over, which is a jump of the timeline.
      if (s.ended) publish({ ...s, positionS: 0, ended: false, playing: true, seekEpoch: s.seekEpoch + 1 });
      else publish({ ...s, playing: true });
    },
    async replayPause() {
      publish({ ...current(), playing: false });
    },
    async replaySeek(seconds) {
      const s = current();
      const positionS = clamp(Number.isFinite(seconds) ? seconds : 0, 0, s.durationS);
      const ended = positionS >= s.durationS;
      publish({ ...s, positionS, ended, playing: ended ? false : s.playing, seekEpoch: s.seekEpoch + 1 });
    },
    async replaySetSpeed(speed) {
      publish({ ...current(), speed: clamp(Number.isFinite(speed) ? speed : 1, 0.1, 50) });
    },
    async replayTrack(untilS) {
      const s = current();
      const points: Array<[number, number]> = [];
      for (let second = 0; second <= Math.min(untilS, s.durationS); second += 1) points.push(pathAt(second));
      return points;
    },
    onReplay: async (cb) => {
      listeners.add(cb);
      return () => {
        listeners.delete(cb);
      };
    },
    start(path) {
      const fileName = path.split(/[\\/]/).pop() || path;
      publish({ fileName, playing: true, ended: false, speed: 1, positionS: 0, durationS: DURATION_S, seekEpoch: 0 });
    },
    stop() {
      if (status) publish(null);
    },
    active: () => status !== null,
    advance(dtSeconds) {
      const s = status;
      if (!s) return 0;
      if (!s.playing || s.ended) return s.positionS;
      const positionS = Math.min(s.positionS + dtSeconds * s.speed, s.durationS);
      const ended = positionS >= s.durationS;
      publish({ ...s, positionS, ended, playing: !ended });
      return positionS;
    },
  };
}
