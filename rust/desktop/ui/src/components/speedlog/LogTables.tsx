/** SpeedLOG tables: the parameters of the log (changes marked) and the MSG/ERR/EV messages. */
import { useMemo, useState } from "react";
import type { SpeedLogMessage, SpeedLogParam } from "../../types";
import { formatTime } from "./PlotPanel";

function formatParam(v: number): string {
  return Number.isInteger(v) ? String(v) : String(Number(v.toPrecision(7)));
}

export function ParamsTable({ params }: { params: SpeedLogParam[] | null }) {
  const [filter, setFilter] = useState("");
  const [onlyChanged, setOnlyChanged] = useState(false);
  const q = filter.trim().toLowerCase();
  const rows = useMemo(
    () =>
      (params ?? []).filter(
        (p) =>
          (!onlyChanged || p.changed) &&
          (!q || p.name.toLowerCase().includes(q) || `${p.title} ${p.doc}`.toLowerCase().includes(q)),
      ),
    [params, q, onlyChanged],
  );
  const changedCount = (params ?? []).filter((p) => p.changed).length;

  return (
    <div className="sl-table-view">
      <div className="sl-table-tools">
        <input type="search" placeholder="Search parameters" value={filter} onChange={(e) => setFilter(e.target.value)} aria-label="Search parameters" />
        <label className="sl-check">
          <input type="checkbox" checked={onlyChanged} onChange={(e) => setOnlyChanged(e.target.checked)} />
          Only changed during the log ({changedCount})
        </label>
        <span className="dim sl-table-count">{params === null ? "loading…" : `${rows.length} of ${params.length}`}</span>
      </div>
      <div className="sl-table-scroll">
        <table className="sl-table">
          <thead>
            <tr>
              <th>Parameter</th>
              <th className="num">Value</th>
              <th>Description</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((p) => (
              <tr key={p.name} className={p.changed ? "changed" : ""}>
                <td className="mono">
                  {p.name}
                  {p.changed && <span className="badge warn sl-changed">changed</span>}
                </td>
                <td className="num mono">
                  <div>
                    {formatParam(p.value)}
                    {p.units && <span className="dim"> {p.units}</span>}
                  </div>
                  {p.meaning && <div className="sl-meaning">{p.meaning}</div>}
                  {p.changed && (
                    <div className="dim sl-was">
                      was {formatParam(p.first)}
                      {p.firstMeaning && ` (${p.firstMeaning})`}
                    </div>
                  )}
                </td>
                <td>
                  {p.title && <div className="sl-ptitle">{p.title}</div>}
                  {p.doc && <div className="dim">{p.doc}</div>}
                  {p.range && <div className="dim">Range: {p.range}</div>}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

const KINDS: Array<SpeedLogMessage["kind"]> = ["MSG", "ERR", "EV"];

export function MessagesTable({
  messages,
  onPick,
}: {
  messages: SpeedLogMessage[] | null;
  onPick: (t: number) => void;
}) {
  const [kinds, setKinds] = useState<Set<string>>(new Set(KINDS));
  const [filter, setFilter] = useState("");
  const q = filter.trim().toLowerCase();
  const rows = useMemo(
    () => (messages ?? []).filter((m) => kinds.has(m.kind) && (!q || m.text.toLowerCase().includes(q))),
    [messages, kinds, q],
  );
  const toggle = (kind: string) =>
    setKinds((prev) => {
      const next = new Set(prev);
      if (next.has(kind)) next.delete(kind);
      else next.add(kind);
      return next;
    });

  return (
    <div className="sl-table-view">
      <div className="sl-table-tools">
        {KINDS.map((kind) => (
          <button key={kind} className={kinds.has(kind) ? "primary" : ""} aria-pressed={kinds.has(kind)} onClick={() => toggle(kind)}>
            {kind}
          </button>
        ))}
        <input type="search" placeholder="Search messages" value={filter} onChange={(e) => setFilter(e.target.value)} aria-label="Search messages" />
        <span className="dim sl-table-count">
          {messages === null ? "loading…" : `${rows.length} of ${messages.length} · click a row to mark it on the plots`}
        </span>
      </div>
      <div className="sl-table-scroll">
        <table className="sl-table sl-messages">
          <thead>
            <tr>
              <th className="num">Time</th>
              <th>Type</th>
              <th>Text</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((m, i) => (
              <tr key={`${m.t}-${i}`} className={`clickable kind-${m.kind}`} onClick={() => onPick(m.t)} tabIndex={0} onKeyDown={(e) => e.key === "Enter" && onPick(m.t)}>
                <td className="num mono">{formatTime(m.t)}</td>
                <td>
                  <span className={`sl-kind kind-${m.kind}`}>{m.kind}</span>
                </td>
                <td>{m.text}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
