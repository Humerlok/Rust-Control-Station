/**
 * Playback bar of a log replay, under the map in Flight Data: play/pause, a timeline to drag back and forth like a
 * video, elapsed/total time and the playback speed. Everything is a command to the backend (`api.replay*`); the bar
 * only shows the status it is given, except while the user drags, when the thumb follows the pointer.
 */
import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { api } from "../api";
import { formatClock } from "../timeFormat";
import type { ReplayStatus } from "../types";

const SPEEDS = [0.5, 1, 1.5, 2];
// While dragging, the backend is asked for the new position at most this often (the map and HUD follow like a video preview).
const SEEK_THROTTLE_MS = 100;
// After letting go, the thumb stays where it was dropped until the status catches up, so it does not jump back.
const SETTLE_HOLD_MS = 250;

type Props = {
  status: ReplayStatus;
  notify: (text: string, ok: boolean) => void;
};

export function ReplayBar({ status, notify }: Props) {
  // Position under the pointer while dragging (and just after); null means "show the status".
  const [scrub, setScrub] = useState<number | null>(null);
  const pending = useRef<number | null>(null);
  const lastSent = useRef(0);
  const sendTimer = useRef<number | null>(null);
  const holdTimer = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (sendTimer.current !== null) window.clearTimeout(sendTimer.current);
      if (holdTimer.current !== null) window.clearTimeout(holdTimer.current);
    },
    [],
  );

  const report = (error: unknown) => notify(`Replay: ${String(error)}`, false);
  const seek = (seconds: number, settle: boolean) => api.replaySeek(seconds, settle).catch(report);

  function sendPending() {
    if (sendTimer.current !== null) {
      window.clearTimeout(sendTimer.current);
      sendTimer.current = null;
    }
    if (pending.current === null) return;
    lastSent.current = performance.now();
    void seek(pending.current, false);
  }

  function drag(seconds: number) {
    if (holdTimer.current !== null) {
      window.clearTimeout(holdTimer.current);
      holdTimer.current = null;
    }
    setScrub(seconds);
    pending.current = seconds;
    const wait = SEEK_THROTTLE_MS - (performance.now() - lastSent.current);
    if (wait <= 0) sendPending();
    else if (sendTimer.current === null) sendTimer.current = window.setTimeout(sendPending, wait);
  }

  // The final position: unlike the ones sent while dragging, it also clears what the telemetry had accumulated.
  function release() {
    if (pending.current === null) return;
    if (sendTimer.current !== null) {
      window.clearTimeout(sendTimer.current);
      sendTimer.current = null;
    }
    void seek(pending.current, true);
    pending.current = null;
    holdTimer.current = window.setTimeout(() => {
      holdTimer.current = null;
      setScrub(null);
    }, SETTLE_HOLD_MS);
  }

  function togglePlay() {
    (status.playing ? api.replayPause() : api.replayPlay()).catch(report);
  }

  // Space plays/pauses from the timeline too (a range input does nothing with it).
  function onKeyDown(event: KeyboardEvent<HTMLElement>) {
    if (event.key === " " && event.target instanceof HTMLInputElement) {
      event.preventDefault();
      togglePlay();
    }
  }

  const duration = status.durationS;
  const shown = Math.min(Math.max(scrub ?? status.positionS, 0), duration);
  const percent = duration > 0 ? (shown / duration) * 100 : 0;
  // A speed the backend runs at that is not offered (set from outside) still shows, instead of a blank select.
  const speeds = SPEEDS.includes(status.speed) ? SPEEDS : [...SPEEDS, status.speed].sort((a, b) => a - b);
  const playLabel = status.ended ? "Play again" : status.playing ? "Pause" : "Play";

  return (
    <div className="panel replay-bar" role="group" aria-label={`Replay of ${status.fileName}`} onKeyDown={onKeyDown}>
      <button className="replay-play" title={playLabel} aria-label={playLabel} onClick={togglePlay}>
        <svg viewBox="0 0 16 16" aria-hidden="true">
          {status.playing ? <path d="M3 2h4v12H3z M9 2h4v12H9z" /> : <path d="M4 2 L14 8 L4 14 Z" />}
        </svg>
      </button>
      <input
        type="range"
        className="replay-slider"
        aria-label="Replay position"
        aria-valuetext={`${formatClock(shown)} of ${formatClock(duration)}`}
        min={0}
        max={duration}
        step="any"
        value={shown}
        style={{ "--progress": `${percent}%` } as CSSProperties}
        onChange={(event) => drag(Number(event.target.value))}
        onPointerUp={release}
        onPointerCancel={release}
        onKeyUp={release}
        onBlur={release}
      />
      <span className="replay-time" title={status.fileName}>
        {formatClock(shown)} / {formatClock(duration)}
      </span>
      <select
        aria-label="Playback speed"
        title="Playback speed"
        value={status.speed}
        onChange={(event) => api.replaySetSpeed(Number(event.target.value)).catch(report)}
      >
        {speeds.map((speed) => (
          <option key={speed} value={speed}>
            {speed}×
          </option>
        ))}
      </select>
    </div>
  );
}
