/**
 * SpeedLOG sidebar: every message type of the log (instances apart) with its fields. Ticking a field
 * plots it in the active panel; a dot marks fields plotted in another panel. Search also looks in the
 * documentation texts, so typing "altitude" finds the altitude fields.
 */
import { forwardRef, useMemo, useState } from "react";
import type { SpeedLogType } from "../../types";

type Props = {
  types: SpeedLogType[];
  /** `TYPE.Field` keys plotted in the active panel. */
  active: Set<string>;
  /** `TYPE.Field` keys plotted in any panel. */
  plotted: Set<string>;
  showDocs: boolean;
  onToggle: (type: string, field: string) => void;
};

export const FieldTree = forwardRef<HTMLInputElement, Props>(function FieldTree(
  { types, active, plotted, showDocs, onToggle },
  searchRef,
) {
  const [filter, setFilter] = useState("");
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const q = filter.trim().toLowerCase();

  const visible = useMemo(() => {
    return types
      .map((type) => {
        const typeHit = !q || type.name.toLowerCase().includes(q) || type.doc.toLowerCase().includes(q);
        const fields = type.fields.filter(
          (f) => typeHit || f.name.toLowerCase().includes(q) || f.doc.toLowerCase().includes(q),
        );
        return { type, fields };
      })
      .filter((entry) => entry.fields.length > 0);
  }, [types, q]);

  const toggleType = (name: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });

  return (
    <div className="sl-tree-wrap">
      <input
        ref={searchRef}
        className="sl-search"
        type="search"
        placeholder="Search fields  ( / )"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        aria-label="Search fields"
      />
      <div className="sl-tree" role="tree">
        {visible.length === 0 && <div className="dim sl-tree-empty">No field matches.</div>}
        {visible.map(({ type, fields }) => {
          const open = q ? true : expanded.has(type.name);
          const plottedHere = fields.some((f) => active.has(`${type.name}.${f.name}`));
          return (
            <div key={type.name} className="sl-type">
              <button
                className={`sl-type-head ${type.timed ? "" : "untimed"}`}
                aria-expanded={open}
                onClick={() => toggleType(type.name)}
                title={(type.doc ? `${type.name}: ${type.doc}` : type.name) + (type.timed ? "" : " (no time field: cannot be plotted)")}
              >
                <span className="sl-caret">{open ? "▾" : "▸"}</span>
                <span className="mono sl-type-name">{type.name}</span>
                {plottedHere && <span className="sl-dot" aria-label="plotted" />}
                <span className="dim sl-type-count">{type.count.toLocaleString("en-US")}</span>
              </button>
              {showDocs && open && type.doc && <div className="sl-doc sl-type-doc">{type.doc}</div>}
              {open && (
                <ul className="sl-fields">
                  {fields.map((f) => {
                    const key = `${type.name}.${f.name}`;
                    const checked = active.has(key);
                    const elsewhere = !checked && plotted.has(key);
                    return (
                      <li key={key}>
                        <label className={`sl-field ${type.timed ? "" : "untimed"}`} title={f.doc || undefined}>
                          <input
                            type="checkbox"
                            checked={checked}
                            disabled={!type.timed}
                            onChange={() => onToggle(type.name, f.name)}
                          />
                          <span className="mono sl-field-name">{f.name}</span>
                          {elsewhere && <span className="sl-dot other" title="Plotted in another panel" />}
                          {f.unit && <span className="dim sl-unit">{f.unit}</span>}
                        </label>
                        {showDocs && f.doc && <div className="sl-doc">{f.doc}</div>}
                      </li>
                    );
                  })}
                </ul>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
});
