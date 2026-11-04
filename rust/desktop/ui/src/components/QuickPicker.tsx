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
    return fields.filter((f) => !q || f.label.toLowerCase().includes(q) || f.key.toLowerCase().includes(q));
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
          {visible.map((f) => (
            <label key={f.key} className={selected === f.key ? "selected" : ""} onDoubleClick={() => onPick(f.key)}>
              <input type="radio" name="quick-field" checked={selected === f.key} onChange={() => setSelected(f.key)} />
              <span>{f.label}</span>
              <span className="dim mono">{f.unit}</span>
            </label>
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
