/**
 * "Display This" dialog: searchable, grouped list of Quick View fields. Double-click or Apply picks one.
 * Props: field catalog from the backend, current key, onPick, onCancel.
 */
import { useMemo, useState } from "react";
import type { FieldInfo } from "../types";

type Props = {
  fields: FieldInfo[];
  current: string;
  onPick: (key: string) => void;
  onCancel: () => void;
};

export function QuickPicker({ fields, current, onPick, onCancel }: Props) {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(current);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    const matching = fields.filter(
      (f) => !q || f.label.toLowerCase().includes(q) || f.key.toLowerCase().includes(q) || f.group.toLowerCase().includes(q),
    );
    // Grouped like the legacy picker, groups in the order the backend lists them.
    const groups = new Map<string, FieldInfo[]>();
    for (const f of matching) {
      const list = groups.get(f.group) ?? [];
      list.push(f);
      groups.set(f.group, list);
    }
    return [...groups];
  }, [fields, query]);

  return (
    <div className="modal-back" onMouseDown={onCancel}>
      <div className="modal picker" role="dialog" aria-modal="true" aria-label="Display this" onMouseDown={(e) => e.stopPropagation()}>
        <h3>Display This</h3>
        <input
          autoFocus
          placeholder="Search fields"
          aria-label="Search fields"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Escape" && onCancel()}
        />
        <div className="picker-list" role="radiogroup">
          {visible.map(([group, list]) => (
            <div key={group} className="picker-group">
              <div className="picker-group-title">{group}</div>
              {list.map((f) => (
                <label key={f.key} className={selected === f.key ? "selected" : ""} onDoubleClick={() => onPick(f.key)}>
                  <input type="radio" name="quick-field" checked={selected === f.key} onChange={() => setSelected(f.key)} />
                  <span>{f.label}</span>
                  <span className="dim mono">{f.unit}</span>
                </label>
              ))}
            </div>
          ))}
          {visible.length === 0 && <p className="dim">No matching fields.</p>}
        </div>
        <div className="buttons">
          <button onClick={onCancel}>Cancel</button>
          <button className="primary" onClick={() => onPick(selected)}>
            Apply
          </button>
        </div>
      </div>
    </div>
  );
}
