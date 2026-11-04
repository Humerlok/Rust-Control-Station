import { useCallback, useMemo, useState } from "react";
import { GraphPanel } from "../components/inspector/GraphPanel";
import {
  groupMessages,
  messageKey,
  messageMatches,
  sortMessages,
  watchLabel,
  type SortMode,
} from "../components/inspector/inspectorUtils";
import { MessageDetail } from "../components/inspector/MessageDetail";
import { MessageTree } from "../components/inspector/MessageTree";
import { useInspector } from "../components/inspector/useInspector";
import { loadSetting, saveSetting } from "../storage";
import "../inspector.css";

type Props = { active: boolean; notify: (text: string, ok: boolean) => void };

const COLLAPSED_KEY = "inspector.collapsed";
const PINNED_KEY = "inspector.pinned";
const SORT_KEY = "inspector.sort";

function toggleIn(list: string[], key: string): string[] {
  return list.includes(key) ? list.filter((k) => k !== key) : [...list, key];
}

export function Inspector({ active, notify }: Props) {
  const { store, messages, paused, watched, togglePause, toggleWatch, removeWatch, clearAll } = useInspector(
    active,
    notify,
  );
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<SortMode>(() => loadSetting<SortMode>(SORT_KEY, "id"));
  const [collapsed, setCollapsed] = useState<string[]>(() => loadSetting<string[]>(COLLAPSED_KEY, []));
  const [pinned, setPinned] = useState<string[]>(() => loadSetting<string[]>(PINNED_KEY, []));
  const [selectedKey, setSelectedKey] = useState<string | null>(null);

  const search = query.trim().toLowerCase();
  const collapsedSet = useMemo(() => new Set(collapsed), [collapsed]);
  const pinnedSet = useMemo(() => new Set(pinned), [pinned]);
  const watchedSet = useMemo(() => new Set(watched), [watched]);

  const visible = useMemo(() => messages.filter((m) => messageMatches(m, search)), [messages, search]);
  const groups = useMemo(() => groupMessages(visible, sort), [visible, sort]);
  const byKey = useMemo(() => new Map(messages.map((m) => [messageKey(m), m])), [messages]);

  const pinnedMessages = useMemo(
    () =>
      sortMessages(
        visible.filter((m) => pinnedSet.has(messageKey(m))),
        sort,
      ),
    [visible, pinnedSet, sort],
  );
  const missingPinned = useMemo(
    () => (search ? 0 : pinned.filter((key) => !byKey.has(key)).length),
    [pinned, byKey, search],
  );

  const names = useMemo(() => new Map(messages.map((m) => [messageKey(m), m.name])), [messages]);
  const labels = useMemo(
    () => Object.fromEntries(watched.map((key) => [key, watchLabel(key, names)])),
    [watched, names],
  );

  // Without an explicit choice, show the first message in the tree.
  const firstKey = pinnedMessages[0]
    ? messageKey(pinnedMessages[0])
    : groups[0]?.components[0]?.messages[0]
      ? messageKey(groups[0].components[0].messages[0])
      : null;
  const effectiveKey = selectedKey && byKey.has(selectedKey) ? selectedKey : firstKey;
  const selected = effectiveKey ? byKey.get(effectiveKey) : undefined;

  const onToggleNode = useCallback((key: string) => {
    setCollapsed((current) => {
      const next = toggleIn(current, key);
      saveSetting(COLLAPSED_KEY, next);
      return next;
    });
  }, []);

  const onPin = useCallback((key: string) => {
    setPinned((current) => {
      const next = toggleIn(current, key);
      saveSetting(PINNED_KEY, next);
      return next;
    });
  }, []);

  const changeSort = (mode: SortMode) => {
    setSort(mode);
    saveSetting(SORT_KEY, mode);
  };

  const systems = new Set(messages.map((m) => m.systemId)).size;

  return (
    <div className="insp-view">
      <div className="insp-toolbar">
        <button
          className={paused ? "primary" : ""}
          aria-pressed={paused}
          onClick={togglePause}
          title="Freezes the message list and the graph. Graph data keeps being recorded, so nothing is lost when you resume."
        >
          {paused ? "Resume" : "Pause"}
        </button>
        <button onClick={clearAll} title="Resets the message counters and empties the graph">
          Clear
        </button>
        <input
          className="search"
          type="search"
          placeholder="Search message, field or value"
          aria-label="Search messages"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <label className="insp-sort">
          Sort
          <select value={sort} onChange={(event) => changeSort(event.target.value as SortMode)}>
            <option value="id">By message id</option>
            <option value="name">By name</option>
            <option value="rate">By rate</option>
          </select>
        </label>
        <span className="dim insp-summary">
          {messages.length === 0
            ? "No messages"
            : `${messages.length} message type${messages.length === 1 ? "" : "s"} from ${systems} system${systems === 1 ? "" : "s"}`}
          {search && messages.length > 0 ? ` · ${visible.length} match` : ""}
          {paused ? " · paused" : ""}
        </span>
      </div>

      <div className="insp-body">
        <aside className="panel insp-left">
          {messages.length === 0 ? (
            <div className="insp-empty">
              <p>Connect to a vehicle to see MAVLink traffic.</p>
            </div>
          ) : visible.length === 0 ? (
            <div className="insp-empty">
              <p>No message matches "{query}".</p>
            </div>
          ) : (
            <MessageTree
              groups={groups}
              pinnedMessages={pinnedMessages}
              missingPinned={missingPinned}
              searching={search !== ""}
              collapsed={collapsedSet}
              onToggle={onToggleNode}
              selectedKey={effectiveKey}
              onSelect={setSelectedKey}
              pinnedKeys={pinnedSet}
              onPin={onPin}
            />
          )}
        </aside>

        <div className="insp-right">
          <section className="panel insp-detail">
            <MessageDetail
              message={selected}
              pinned={effectiveKey ? pinnedSet.has(effectiveKey) : false}
              onPin={() => effectiveKey && onPin(effectiveKey)}
              watched={watchedSet}
              onToggleWatch={toggleWatch}
              query={search}
              notify={notify}
            />
          </section>
          <GraphPanel
            store={store}
            keys={watched}
            labels={labels}
            paused={paused}
            onRemove={removeWatch}
            onClear={clearAll}
          />
        </div>
      </div>
    </div>
  );
}
