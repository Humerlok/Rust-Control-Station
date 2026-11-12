/** Flight Data message log: all STATUSTEXT lines, newest first, colour-coded by MAV_SEVERITY, with a Clear button. */
import type { StatusText } from "../types";

const pad = (n: number) => String(n).padStart(2, "0");

function clock(timeMs: number): string {
  const d = new Date(timeMs);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

function severityClass(severity: number): string {
  if (severity <= 3) return "sev-error";
  if (severity === 4) return "sev-warn";
  if (severity >= 7) return "sev-dim";
  return "";
}

type Props = { messages: StatusText[]; onClear: () => void };

export function MessagesPanel({ messages, onClear }: Props) {
  return (
    <section className="panel messages">
      <header>
        <h2>Messages</h2>
        <button onClick={onClear} disabled={messages.length === 0}>
          Clear
        </button>
      </header>
      <ul className="message-list mono" aria-label="Vehicle messages">
        {messages.length === 0 && <li className="dim">No messages yet.</li>}
        {[...messages].reverse().map((m, i) => (
          <li key={`${m.timeMs}-${messages.length - i}`} className={severityClass(m.severity)}>
            <time>{clock(m.timeMs)}</time> {m.text}
          </li>
        ))}
      </ul>
    </section>
  );
}
