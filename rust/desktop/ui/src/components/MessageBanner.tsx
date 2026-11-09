import { useEffect, useRef, useState } from "react";
import type { StatusText } from "../types";

/** How long a message stays on the map (legacy HUD: 10 s). */
const SHOW_MS = 10_000;
const MAX_SHOWN = 3;

/** Legacy rule: WARNING or worse, plus pre-arm, arming and tuning reports of any severity. */
const PREFIXES = ["PreArm:", "Arm:", "Tuning:"];
export const isBannerMessage = (m: StatusText) => m.severity <= 4 || PREFIXES.some((p) => m.text.startsWith(p));

/** MAV_SEVERITY → colour class: 0-2 critical (red), 3 error (orange), 4 warning / pre-arm (yellow). */
export function bannerLevel(m: StatusText): "critical" | "error" | "warning" | "info" {
  if (m.severity <= 2) return "critical";
  if (m.severity === 3) return "error";
  if (m.severity === 4 || m.text.startsWith("PreArm:") || m.text.startsWith("Arm:")) return "warning";
  return "info";
}

const SEVERITY_NAMES = ["EMERGENCY", "ALERT", "CRITICAL", "ERROR", "WARNING", "NOTICE", "INFO", "DEBUG"];

type Shown = { id: number; message: StatusText; count: number; shownAt: number };

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
      let next = [...current];
      for (const message of relevant) {
        const same = next.find((s) => s.message.text === message.text);
        if (same) {
          // A repeated message counts up and moves to the bottom instead of stacking.
          next = [...next.filter((s) => s !== same), { ...same, message, count: same.count + 1, shownAt: now }];
        } else {
          next.push({ id: nextId.current++, message, count: 1, shownAt: now });
        }
      }
      return next.slice(-MAX_SHOWN);
    });
  }, [messages, systemId]);

  // Expire old messages.
  useEffect(() => {
    if (shown.length === 0) return;
    const oldest = Math.min(...shown.map((s) => s.shownAt));
    const timer = window.setTimeout(
      () => setShown((current) => current.filter((s) => Date.now() - s.shownAt < SHOW_MS)),
      Math.max(0, oldest + SHOW_MS - Date.now()) + 50,
    );
    return () => window.clearTimeout(timer);
  }, [shown]);

  if (shown.length === 0) return null;
  return (
    <div className="message-banner" role="status" aria-live="polite" aria-label="Vehicle messages">
      {shown.map((s) => (
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
    </div>
  );
}
