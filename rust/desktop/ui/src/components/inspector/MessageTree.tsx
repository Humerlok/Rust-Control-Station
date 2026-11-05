import { memo } from "react";
import type { InspectorMessage } from "../../types";
import {
  formatBytes,
  formatRate,
  isStale,
  messageKey,
  type SystemGroup,
} from "./inspectorUtils";

function PinIcon({ filled }: { filled: boolean }) {
  return (
    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
      <path
        d="M9.5 1.5 14.5 6.5l-1.8.6-2.7 2.7.3 3.2-1 1-2.7-3.6L2.2 14.2l-.4-.4 3.8-3.4L2 7.7l1-1 3.2.3L8.9 4.3z"
        fill={filled ? "currentColor" : "none"}
        stroke="currentColor"
        strokeWidth="1"
        strokeLinejoin="round"
      />
    </svg>
  );
}

type RowProps = {
  m: InspectorMessage;
  selected: boolean;
  pinned: boolean;
  showOrigin?: boolean;
  onSelect: (key: string) => void;
  onPin: (key: string) => void;
};

const MessageRow = memo(
  function MessageRow({ m, selected, pinned, showOrigin, onSelect, onPin }: RowProps) {
    const key = messageKey(m);
    const stale = isStale(m);
    return (
      <div
        className={`insp-row ${selected ? "active" : ""} ${stale ? "stale" : ""}`}
        onClick={() => onSelect(key)}
        role="treeitem"
        aria-selected={selected}
        title={`${m.name} (#${m.messageId})${stale ? " - no recent packets" : ""}`}
      >
        <button
          className={`insp-pin ${pinned ? "on" : ""}`}
          aria-label={pinned ? "Unpin message" : "Pin message"}
          aria-pressed={pinned}
          title={pinned ? "Unpin" : "Pin to the top"}
          onClick={(event) => {
            event.stopPropagation();
            onPin(key);
          }}
        >
          <PinIcon filled={pinned} />
        </button>
        <span className="insp-name">
          {m.name}
          {showOrigin && (
            <span className="dim insp-origin">
              {" "}
              {m.systemId}/{m.componentId}
            </span>
          )}
        </span>
        <span className="insp-num">{formatRate(m.hz)} Hz</span>
        <span className="insp-num dim">{formatBytes(m.bytesPerSecond)}</span>
        <span className="insp-count dim">{m.count}</span>
      </div>
    );
  },
  (a, b) =>
    a.selected === b.selected &&
    a.pinned === b.pinned &&
    a.showOrigin === b.showOrigin &&
    a.m.name === b.m.name &&
    a.m.count === b.m.count &&
    a.m.hz === b.m.hz &&
    a.m.bytesPerSecond === b.m.bytesPerSecond &&
    isStale(a.m) === isStale(b.m) &&
    a.onSelect === b.onSelect &&
    a.onPin === b.onPin,
);

type Props = {
  groups: SystemGroup[];
  pinnedMessages: InspectorMessage[];
  missingPinned: number;
  searching: boolean;
  collapsed: Set<string>;
  onToggle: (key: string) => void;
  selectedKey: string | null;
  onSelect: (key: string) => void;
  pinnedKeys: Set<string>;
  onPin: (key: string) => void;
  /** What the first level of the tree is called ("System" for MAVLink, "Node" for DroneCAN). */
  originLabel?: string;
};

export function MessageTree({
  groups,
  pinnedMessages,
  missingPinned,
  searching,
  collapsed,
  onToggle,
  selectedKey,
  onSelect,
  pinnedKeys,
  onPin,
  originLabel = "System",
}: Props) {
  const isOpen = (key: string) => searching || !collapsed.has(key);
  return (
    <div className="insp-tree" role="tree" aria-label="Messages">
      {(pinnedMessages.length > 0 || missingPinned > 0) && (
        <div className="insp-section">
          <div className="insp-section-title">Pinned</div>
          {pinnedMessages.map((m) => (
            <MessageRow
              key={messageKey(m)}
              m={m}
              showOrigin
              selected={selectedKey === messageKey(m)}
              pinned
              onSelect={onSelect}
              onPin={onPin}
            />
          ))}
          {missingPinned > 0 && (
            <div className="insp-hint">{missingPinned} pinned message(s) not being received</div>
          )}
        </div>
      )}
      {groups.map((system) => (
        <div key={system.key} className="insp-section">
          <button
            className="insp-node"
            aria-expanded={isOpen(system.key)}
            onClick={() => onToggle(system.key)}
          >
            <span className="insp-caret">{isOpen(system.key) ? "▾" : "▸"}</span>
            {originLabel} {system.systemId}
          </button>
          {isOpen(system.key) &&
            system.components.map((component) => (
              <div key={component.key}>
                <button
                  className="insp-node comp"
                  aria-expanded={isOpen(component.key)}
                  onClick={() => onToggle(component.key)}
                >
                  <span className="insp-caret">{isOpen(component.key) ? "▾" : "▸"}</span>
                  {component.componentName}
                  <span className="dim"> {component.messages.length}</span>
                </button>
                {isOpen(component.key) &&
                  component.messages.map((m) => (
                    <MessageRow
                      key={messageKey(m)}
                      m={m}
                      selected={selectedKey === messageKey(m)}
                      pinned={pinnedKeys.has(messageKey(m))}
                      onSelect={onSelect}
                      onPin={onPin}
                    />
                  ))}
              </div>
            ))}
        </div>
      ))}
    </div>
  );
}
