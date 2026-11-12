/**
 * Right-hand pane of the inspector: fields of the selected message with per-field Graph toggles, Pin and Copy JSON/text.
 * `query` highlights matching field names; `originLabel` names the first tree level (System or Node).
 */
import type { InspectorField, InspectorMessage } from "../../types";
import {
  copyText,
  formatBytes,
  formatRate,
  formatScalar,
  isNumeric,
  isStale,
  messageToJson,
  messageToText,
  watchKey,
} from "./inspectorUtils";

type Props = {
  message: InspectorMessage | undefined;
  pinned: boolean;
  onPin: () => void;
  watched: Set<string>;
  onToggleWatch: (key: string) => void;
  query: string;
  notify: (text: string, ok: boolean) => void;
  /** What the origin of a message is called ("System" for MAVLink, "Node" for DroneCAN). */
  originLabel?: string;
};

function GraphButton({ on, onClick, label }: { on: boolean; onClick: () => void; label: string }) {
  return (
    <button
      className={`insp-graph ${on ? "on" : ""}`}
      aria-pressed={on}
      title={on ? `Remove ${label} from the graph` : `Plot ${label}`}
      onClick={onClick}
    >
      {on ? "Graphed" : "Graph"}
    </button>
  );
}

function FieldRow({
  message,
  field,
  watched,
  onToggleWatch,
  highlight,
}: {
  message: InspectorMessage;
  field: InspectorField;
  watched: Set<string>;
  onToggleWatch: (key: string) => void;
  highlight: boolean;
}) {
  // Array fields get one graph toggle per numeric element; scalars get one toggle.
  const { value } = field;
  if (Array.isArray(value)) {
    return (
      <div className={`insp-field array ${highlight ? "hit" : ""}`}>
        <span className="insp-field-name">
          {field.name}
          <span className="dim"> [{value.length}]</span>
        </span>
        <span className="insp-chips">
          {value.map((element, index) => {
            const key = watchKey(message, field.name, index);
            return (
              <span key={index} className={`insp-chip ${watched.has(key) ? "on" : ""}`}>
                <i>{index}</i>
                <span className="mono">{formatScalar(element)}</span>
                {isNumeric(element) && (
                  <button
                    className="insp-chip-graph"
                    aria-pressed={watched.has(key)}
                    title={watched.has(key) ? `Remove ${field.name}[${index}] from the graph` : `Plot ${field.name}[${index}]`}
                    onClick={() => onToggleWatch(key)}
                  >
                    ~
                  </button>
                )}
              </span>
            );
          })}
        </span>
        <span />
      </div>
    );
  }
  const key = watchKey(message, field.name);
  return (
    <div className={`insp-field ${highlight ? "hit" : ""}`}>
      <span className="insp-field-name">{field.name}</span>
      <span
        className={`insp-value mono ${typeof value === "boolean" ? (value ? "yes" : "no") : ""}`}
        title={typeof value === "number" ? String(value) : undefined}
      >
        {formatScalar(value)}
      </span>
      {isNumeric(value) ? (
        <GraphButton on={watched.has(key)} label={field.name} onClick={() => onToggleWatch(key)} />
      ) : (
        <span />
      )}
    </div>
  );
}

export function MessageDetail({
  message,
  pinned,
  onPin,
  watched,
  onToggleWatch,
  query,
  notify,
  originLabel = "System",
}: Props) {
  if (!message) {
    return (
      <div className="insp-empty">
        <p>Select a message on the left to see its fields.</p>
      </div>
    );
  }

  async function copy(kind: "JSON" | "text") {
    if (!message) return;
    const ok = await copyText(kind === "JSON" ? messageToJson(message) : messageToText(message));
    notify(ok ? `${message.name} copied as ${kind}.` : "Could not copy to the clipboard.", ok);
  }

  const stale = isStale(message);
  return (
    <div className="insp-detail-body">
      <header className="insp-detail-head">
        <div>
          <h3>
            {message.name} <span className="dim">#{message.messageId}</span>
          </h3>
          <div className="insp-meta">
            <span>
              {originLabel} {message.systemId} / {message.componentName}
            </span>
            <span>{formatRate(message.hz)} Hz</span>
            <span>{formatBytes(message.bytesPerSecond)}</span>
            <span>{message.count} received</span>
            {stale && <span className="insp-stale-tag">no recent packets</span>}
          </div>
        </div>
        <div className="insp-actions">
          <button className={pinned ? "primary" : ""} onClick={onPin} aria-pressed={pinned}>
            {pinned ? "Pinned" : "Pin"}
          </button>
          <button onClick={() => void copy("JSON")}>Copy JSON</button>
          <button onClick={() => void copy("text")}>Copy text</button>
        </div>
      </header>
      <div className={`insp-fields ${stale ? "stale" : ""}`}>
        {message.fields.map((field) => (
          <FieldRow
            key={field.name}
            message={message}
            field={field}
            watched={watched}
            onToggleWatch={onToggleWatch}
            highlight={query !== "" && field.name.toLowerCase().includes(query)}
          />
        ))}
      </div>
    </div>
  );
}
