import { useEffect, useState } from "react";

export type CompareRow = { name: string; current: string; file: string; value: number };

type Props = {
  rows: CompareRow[];
  missing: number;
  onApply: (rows: CompareRow[]) => void;
  onClose: () => void;
};

export function ParamCompareDialog({ rows, missing, onApply, onClose }: Props) {
  const [selected, setSelected] = useState<Set<string>>(() => new Set(rows.map((r) => r.name)));

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const toggle = (name: string) =>
    setSelected((current) => {
      const next = new Set(current);
      if (!next.delete(name)) next.add(name);
      return next;
    });
  const allSelected = selected.size === rows.length;

  return (
    <div className="modal-back" onMouseDown={onClose}>
      <div className="modal wide" role="dialog" aria-modal="true" aria-label="Compare parameters" onMouseDown={(e) => e.stopPropagation()}>
        <h3>Compare with file</h3>
        <p className="dim">
          {rows.length === 0 ? "No differences." : `${rows.length} parameter(s) differ.`}
          {missing > 0 && ` ${missing} in the file are not on the vehicle.`}
        </p>
        {rows.length > 0 && (
          <div className="compare-list">
            <div className="compare-row head">
              <label>
                <input
                  type="checkbox"
                  checked={allSelected}
                  onChange={() => setSelected(allSelected ? new Set() : new Set(rows.map((r) => r.name)))}
                />
              </label>
              <span>Name</span>
              <span>Current</span>
              <span>File</span>
            </div>
            {rows.map((row) => (
              <div key={row.name} className="compare-row">
                <label>
                  <input type="checkbox" checked={selected.has(row.name)} onChange={() => toggle(row.name)} />
                </label>
                <span className="mono">{row.name}</span>
                <span className="mono dim">{row.current}</span>
                <span className="mono">{row.file}</span>
              </div>
            ))}
          </div>
        )}
        <div className="buttons">
          <button onClick={onClose}>Close</button>
          <button
            className="primary"
            disabled={selected.size === 0}
            onClick={() => onApply(rows.filter((r) => selected.has(r.name)))}
          >
            Apply selected ({selected.size})
          </button>
        </div>
      </div>
    </div>
  );
}
