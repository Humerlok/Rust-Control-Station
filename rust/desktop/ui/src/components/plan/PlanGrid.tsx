import { memo, useEffect, useState } from "react";
import type { MissionCommandInfo, MissionItem, MissionKind } from "../../types";
import { VirtualList } from "../VirtualList";
import {
  FRAME_OPTIONS,
  firstFreeIndex,
  frameLabel,
  hasHome,
  KIND_LABELS,
  validLat,
  validLon,
  type CommandMap,
  type RowStats,
} from "./missionModel";

const ROW_HEIGHT = 34;

/** A number input that keeps what the user is typing and commits every valid number at once. */
function NumCell({
  value,
  decimals,
  disabled,
  invalid,
  title,
  onCommit,
}: {
  value: number;
  decimals: number;
  disabled?: boolean;
  invalid?: (v: number) => boolean;
  title?: string;
  onCommit: (v: number) => void;
}) {
  const shown = String(+value.toFixed(decimals));
  const [text, setText] = useState(shown);
  const [focused, setFocused] = useState(false);

  useEffect(() => {
    if (!focused) setText(shown);
  }, [shown, focused]);

  const parsed = text.trim() === "" ? Number.NaN : Number(text);
  const bad = !Number.isFinite(parsed) || (invalid ? invalid(parsed) : false);
  return (
    <input
      className={`cell-num ${bad ? "bad" : ""}`}
      value={text}
      disabled={disabled}
      title={bad ? "Invalid value" : title}
      inputMode="decimal"
      onFocus={() => setFocused(true)}
      onBlur={() => {
        setFocused(false);
        setText(shown);
      }}
      onChange={(event) => {
        setText(event.target.value);
        const next = event.target.value.trim() === "" ? Number.NaN : Number(event.target.value);
        if (Number.isFinite(next)) onCommit(next);
      }}
    />
  );
}

type RowProps = {
  kind: MissionKind;
  index: number;
  item: MissionItem;
  info: MissionCommandInfo | undefined;
  catalog: MissionCommandInfo[];
  stats: RowStats;
  selected: boolean;
  current: boolean;
  last: boolean;
  onSelect: (index: number) => void;
  onPatch: (index: number, patch: Partial<MissionItem>) => void;
  onMove: (index: number, delta: -1 | 1) => void;
  onDelete: (index: number) => void;
};

const PlanRow = memo(function PlanRow(p: RowProps) {
  const { index, item, info, kind } = p;
  const home = hasHome(kind) && index === 0;
  const free = firstFreeIndex(kind);
  // Unknown commands keep every field editable.
  const used = (label: string | null | undefined) => !info || label !== null;
  const known = p.catalog.some((c) => c.id === item.command);
  const frameKnown = FRAME_OPTIONS.some((f) => f.value === item.frame);

  const param = (field: "param1" | "param2" | "param3" | "param4", label: string | null | undefined) => (
    <NumCell value={item[field]} decimals={4} disabled={!used(label)} onCommit={(v) => p.onPatch(index, { [field]: v })} />
  );

  return (
    <div
      className={`plan-row ${p.selected ? "selected" : ""} ${p.current ? "current" : ""} ${home ? "home" : ""}`}
      onMouseDown={() => p.onSelect(index)}
    >
      <span className="idx">{home ? "H" : index}</span>
      {home ? (
        <span className="home-label">Home</span>
      ) : (
        <select value={item.command} onChange={(event) => p.onPatch(index, { command: Number(event.target.value) })}>
          {!known && <option value={item.command}>CMD {item.command}</option>}
          {p.catalog.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
      )}
      {param("param1", info?.p1)}
      {param("param2", info?.p2)}
      {param("param3", info?.p3)}
      {param("param4", info?.p4)}
      <NumCell value={item.x} decimals={7} disabled={!used(info?.x)} invalid={(v) => !validLat(v)} onCommit={(v) => p.onPatch(index, { x: v })} />
      <NumCell value={item.y} decimals={7} disabled={!used(info?.y)} invalid={(v) => !validLon(v)} onCommit={(v) => p.onPatch(index, { y: v })} />
      <NumCell value={item.z} decimals={2} disabled={!used(info?.z)} onCommit={(v) => p.onPatch(index, { z: v })} />
      {home ? (
        <span className="home-label">{frameLabel(0)}</span>
      ) : (
        <select value={item.frame} onChange={(event) => p.onPatch(index, { frame: Number(event.target.value) })}>
          {!frameKnown && <option value={item.frame}>Frame {item.frame}</option>}
          {FRAME_OPTIONS.map((f) => (
            <option key={f.value} value={f.value}>
              {f.label}
            </option>
          ))}
        </select>
      )}
      <span className="calc">{p.stats.dist === null ? "" : Math.round(p.stats.dist)}</span>
      <span className="calc">{p.stats.az === null ? "" : Math.round(p.stats.az)}</span>
      <span className="calc">{p.stats.grade === null ? "" : p.stats.grade.toFixed(1)}</span>
      <span className="row-buttons">
        <button title="Move up" disabled={index <= free} onClick={() => p.onMove(index, -1)}>
          ▲
        </button>
        <button title="Move down" disabled={home || p.last} onClick={() => p.onMove(index, 1)}>
          ▼
        </button>
        <button className="danger" title="Delete" disabled={home} onClick={() => p.onDelete(index)}>
          ✕
        </button>
      </span>
    </div>
  );
});

type Props = {
  kind: MissionKind;
  items: MissionItem[];
  catalog: MissionCommandInfo[];
  commands: CommandMap;
  stats: RowStats[];
  selected: number | null;
  currentSeq: number | null;
  onSelect: (index: number) => void;
  onPatch: (index: number, patch: Partial<MissionItem>) => void;
  onMove: (index: number, delta: -1 | 1) => void;
  onDelete: (index: number) => void;
};

export function PlanGrid({ kind, items, catalog, commands, stats, selected, currentSeq, onSelect, onPatch, onMove, onDelete }: Props) {
  // Headers follow the command of the selected row, like the legacy grid.
  const headerInfo = selected !== null && items[selected] ? commands.get(items[selected].command) : undefined;
  const label = (value: string | null | undefined, fallback: string) => value ?? fallback;

  return (
    <div className="plan-grid">
      <div className="plan-head">
        <span>#</span>
        <span>Command</span>
        <span title={headerInfo?.description}>{label(headerInfo?.p1, "P1")}</span>
        <span>{label(headerInfo?.p2, "P2")}</span>
        <span>{label(headerInfo?.p3, "P3")}</span>
        <span>{label(headerInfo?.p4, "P4")}</span>
        <span>{label(headerInfo?.x, "Lat")}</span>
        <span>{label(headerInfo?.y, "Lon")}</span>
        <span>{label(headerInfo?.z, "Alt")}</span>
        <span>Frame</span>
        <span title="Distance from the previous positional item (m)">Dist</span>
        <span title="Azimuth from the previous positional item (deg)">Az</span>
        <span title="Gradient between items in the same frame (%)">Grad %</span>
        <span />
      </div>
      {items.length === 0 ? (
        <p className="plan-empty dim">
          No items. Click the map to add a point, or Read the {KIND_LABELS[kind].toLowerCase()} from the vehicle.
        </p>
      ) : (
        <VirtualList
          count={items.length}
          rowHeight={ROW_HEIGHT}
          className="plan-list"
          render={(i) => (
            <PlanRow
              kind={kind}
              index={i}
              item={items[i]}
              info={commands.get(items[i].command)}
              catalog={catalog}
              stats={stats[i]}
              selected={selected === i}
              current={currentSeq === i}
              last={i === items.length - 1}
              onSelect={onSelect}
              onPatch={onPatch}
              onMove={onMove}
              onDelete={onDelete}
            />
          )}
        />
      )}
    </div>
  );
}
