/**
 * Overlay on the map showing recent important STATUSTEXT messages (warnings, pre-arm failures) with repeat counts, auto-expiry and per-message dismiss.
 * Props: the status text log and the selected system id.
 */
import { useEffect, useRef, useState } from "react";
import type { StatusText } from "../types";

/** How long a message stays on the map after it was last received (legacy HUD: 10 s). */
const SHOW_MS = 10_000;
/** ArduPilot repeats pre-arm failures every 30 s: keep them up between repeats so they do not blink. */
const PREARM_SHOW_MS = 35_000;
const MAX_SHOWN = 5;
/** Distinct texts remembered at once (the most important are shown). */
const MAX_KEPT = 30;

/** Legacy rule: WARNING or worse, plus pre-arm, arming and tuning reports of any severity. */
const PREFIXES = ["PreArm:", "Arm:", "Tuning:"];
export const isBannerMessage = (m: StatusText) => m.severity <= 4 || PREFIXES.some((p) => m.text.startsWith(p));

const isArming = (m: StatusText) => m.text.startsWith("PreArm:") || m.text.startsWith("Arm:");
const holdFor = (m: StatusText) => (isArming(m) ? PREARM_SHOW_MS : SHOW_MS);

/** MAV_SEVERITY → colour class: 0-2 critical (red), 3 error (orange), 4 warning / pre-arm (yellow). */
export function bannerLevel(m: StatusText): "critical" | "error" | "warning" | "info" {
  if (m.severity <= 2) return "critical";
  if (m.severity === 3) return "error";
  if (m.severity === 4 || isArming(m)) return "warning";
  return "info";
}

/** Sort rank: lower is more important. Pre-arm reports rank as warnings whatever their severity. */
const rank = (m: StatusText) => (isArming(m) ? Math.min(m.severity, 4) : m.severity);

const SEVERITY_NAMES = ["EMERGENCY", "ALERT", "CRITICAL", "ERROR", "WARNING", "NOTICE", "INFO", "DEBUG"];

type Shown = { id: number; message: StatusText; count: number; firstSeen: number; lastSeen: number };

type Props = {
  messages: StatusText[];
  /** Only messages from this system are shown (null: no vehicle selected, show nothing). */
  systemId: number | null;
};

/** Critical messages, warnings and pre-arm failures, over the bottom of the map. */
export function MessageBanner({ messages, systemId }: Props) {
  const [shown, setShown] = useState<Shown[]>([]);
  const lastSeen = useRef<StatusText | null>(null);
  const nextId = useRef(0);

  // Drop everything when the selected vehicle changes.
  useEffect(() => setShown([]), [systemId]);

  // Pick up the messages that arrived since the last render.
  useEffect(() => {
    const index = lastSeen.current ? messages.lastIndexOf(lastSeen.current) : -1;
    const fresh = index >= 0 ? messages.slice(index + 1) : messages.filter((m) => !lastSeen.current || m.timeMs > lastSeen.current.timeMs);
    if (messages.length > 0) lastSeen.current = messages[messages.length - 1];
    const relevant = fresh.filter((m) => systemId !== null && m.systemId === systemId && isBannerMessage(m));
    if (relevant.length === 0) return;
    const now = Date.now();
    setShown((current) => {
      const next = [...current];
      for (const message of relevant) {
        const at = next.findIndex((s) => s.message.text === message.text);
        // A repeated message counts up instead of stacking.
        if (at >= 0) next[at] = { ...next[at], message, count: next[at].count + 1, lastSeen: now };
        else next.push({ id: nextId.current++, message, count: 1, firstSeen: now, lastSeen: now });
      }
      // Too many: forget the least important, oldest first.
      return next
        .sort((a, b) => rank(a.message) - rank(b.message) || b.lastSeen - a.lastSeen)
        .slice(0, MAX_KEPT);
    });
  }, [messages, systemId]);

  // Expire messages that were not repeated in time.
  useEffect(() => {
    if (shown.length === 0) return;
    const due = Math.min(...shown.map((s) => s.lastSeen + holdFor(s.message)));
    const timer = window.setTimeout(
      () => setShown((current) => current.filter((s) => Date.now() < s.lastSeen + holdFor(s.message))),
      Math.max(0, due - Date.now()) + 50,
    );
    return () => window.clearTimeout(timer);
  }, [shown]);

  if (shown.length === 0) return null;
  // Most important on top; within a severity, in the order they first appeared (stable lines).
  const ordered = [...shown].sort((a, b) => rank(a.message) - rank(b.message) || a.firstSeen - b.firstSeen || a.id - b.id);
  const visible = ordered.slice(0, MAX_SHOWN);
  const hidden = ordered.length - visible.length;
  return (
    <div className="message-banner" role="status" aria-live="polite" aria-label="Vehicle messages">
      {visible.map((s) => (
        <div key={s.id} className={`banner-msg ${bannerLevel(s.message)}`} title={SEVERITY_NAMES[s.message.severity] ?? ""}>
          <span className="banner-text">{s.message.text}</span>
          {s.count > 1 && <span className="banner-count">×{s.count}</span>}
          <button
            className="banner-close"
            aria-label="Dismiss message"
            title="Dismiss"
            onClick={() => setShown((current) => current.filter((x) => x.id !== s.id))}
          >
            ×
          </button>
        </div>
      ))}
      {hidden > 0 && <div className="banner-more">+{hidden} more in Messages</div>}
    </div>
  );
}
