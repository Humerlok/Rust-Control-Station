import { useEffect, useState } from "react";
import { formatValue, outOfRange } from "../paramUtils";
import type { ParamEntry, ParamMeta } from "../types";
import { ConfirmDialog } from "./ConfirmDialog";

type Props = {
  entry: ParamEntry;
  meta?: ParamMeta;
  /** Pending (unwritten) value, if the user changed this parameter. */
  edit?: number;
  disabled?: boolean;
  onChange: (value: number | null) => void;
};

const bitIsSet = (value: number, bit: number) => Math.floor(value / 2 ** bit) % 2 === 1;

function NumberField({ value, paramType, invalid, disabled, onChange, title }: {
  value: number;
  paramType: string;
  invalid: boolean;
  disabled?: boolean;
  onChange: (value: number) => void;
  title?: string;
}) {
  const [text, setText] = useState(formatValue(value, paramType));
  useEffect(() => setText(formatValue(value, paramType)), [value, paramType]);

  const commit = () => {
    const parsed = Number(text);
    if (text.trim() === "" || !Number.isFinite(parsed)) setText(formatValue(value, paramType));
    else onChange(parsed);
  };

  return (
    <input
      className={`pv-input ${invalid ? "invalid" : ""}`}
      value={text}
      disabled={disabled}
      title={title}
      inputMode="decimal"
      onChange={(event) => setText(event.target.value)}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === "Enter") event.currentTarget.blur();
        if (event.key === "Escape") setText(formatValue(value, paramType));
      }}
    />
  );
}

function BitmaskDialog({ name, meta, value, onApply, onCancel }: {
  name: string;
  meta: ParamMeta;
  value: number;
  onApply: (value: number) => void;
  onCancel: () => void;
}) {
  const [draft, setDraft] = useState(value);
  const toggle = (bit: number) => setDraft((v) => (bitIsSet(v, bit) ? v - 2 ** bit : v + 2 ** bit));
  return (
    <ConfirmDialog title={name} confirmLabel="Apply" onConfirm={() => onApply(draft)} onCancel={onCancel}>
      <div className="bitmask-list">
        {meta.bitmask.map(({ bit, label }) => (
          <label key={bit}>
            <input type="checkbox" checked={bitIsSet(draft, bit)} onChange={() => toggle(bit)} />
            <span className="mono dim">{bit}</span> {label}
          </label>
        ))}
      </div>
      <p className="dim mono">Value: {draft}</p>
    </ConfirmDialog>
  );
}

/** Edits one parameter: a select for enumerations, bit checkboxes for bitmasks, a number otherwise. */
export function ParamValueInput({ entry, meta, edit, disabled, onChange }: Props) {
  const [bitsOpen, setBitsOpen] = useState(false);
  const shown = edit ?? entry.value;
  const readOnly = disabled || meta?.readOnly;
  const invalid = outOfRange(shown, meta);
  const title = meta?.readOnly ? "Read-only parameter" : undefined;

  if (meta && meta.values.length > 0) {
    const known = meta.values.some((v) => v.code === shown);
    return (
      <select
        className={`pv-input ${invalid ? "invalid" : ""}`}
        value={String(shown)}
        disabled={readOnly}
        title={title}
        onChange={(event) => onChange(Number(event.target.value))}
      >
        {!known && <option value={String(shown)}>{formatValue(shown, entry.paramType)}</option>}
        {meta.values.map(({ code, label }) => (
          <option key={code} value={String(code)}>
            {code}: {label}
          </option>
        ))}
      </select>
    );
  }

  return (
    <span className="pv-wrap">
      <NumberField
        value={shown}
        paramType={entry.paramType}
        invalid={invalid}
        disabled={readOnly}
        title={title}
        onChange={onChange}
      />
      {meta && meta.bitmask.length > 0 && (
        <button className="pv-bits" disabled={readOnly} title="Edit bits" onClick={() => setBitsOpen(true)}>
          bits
        </button>
      )}
      {bitsOpen && meta && (
        <BitmaskDialog
          name={entry.name}
          meta={meta}
          value={shown}
          onApply={(value) => {
            setBitsOpen(false);
            onChange(value);
          }}
          onCancel={() => setBitsOpen(false)}
        />
      )}
    </span>
  );
}
