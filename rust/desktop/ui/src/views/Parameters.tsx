import { useMemo, useRef, useState } from "react";
import { api } from "../api";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { ParamCompareDialog, type CompareRow } from "../components/ParamCompareDialog";
import { ParamTree } from "../components/ParamTree";
import { ParamValueInput } from "../components/ParamValueInput";
import { VirtualList } from "../components/VirtualList";
import { buildTree, formatValue, inNode, rangeText, sameValue } from "../paramUtils";
import { paramStore, useParams } from "../useParams";

const ROW_HEIGHT = 30;
const CONFIRM_ABOVE = 20;

type Props = { notify: (text: string, ok: boolean) => void };

function download(filename: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

export function Parameters({ notify }: Props) {
  const p = useParams();
  const [selectedNode, setSelectedNode] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [modifiedOnly, setModifiedOnly] = useState(false);
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  const [advanced, setAdvanced] = useState(true);
  const [confirmWrite, setConfirmWrite] = useState(false);
  const [compare, setCompare] = useState<{ rows: CompareRow[]; missing: number } | null>(null);
  const [info, setInfo] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const fileMode = useRef<"load" | "compare">("load");

  const tree = useMemo(() => buildTree(p.list), [p.list]);
  const modifiedCount = Object.keys(p.edits).length;
  const failedCount = Object.keys(p.errors).length;
  const canUseVehicle = p.linkConnected && p.vehicleAlive;
  const search = query.trim().toLowerCase();

  const rows = useMemo(
    () =>
      p.list.filter((entry) => {
        if (!inNode(entry.name, selectedNode)) return false;
        if (modifiedOnly && !(entry.name in p.edits)) return false;
        if (favoritesOnly && !p.favorites.has(entry.name)) return false;
        const meta = p.metadata[entry.name];
        if (!advanced && meta?.user === "Advanced") return false;
        if (search) {
          const haystack = `${entry.name} ${meta?.displayName ?? ""} ${meta?.description ?? ""}`.toLowerCase();
          if (!haystack.includes(search)) return false;
        }
        return true;
      }),
    [p.list, p.edits, p.favorites, p.metadata, selectedNode, modifiedOnly, favoritesOnly, advanced, search],
  );

  async function write() {
    setConfirmWrite(false);
    const summary = await paramStore.writeParams();
    const text = `Wrote ${summary.written} parameter(s)${summary.failed ? `, ${summary.failed} failed` : ""}.`;
    setInfo(text);
    notify(text, summary.failed === 0);
  }

  async function save() {
    try {
      const text = await api.paramsFormatFile();
      download(`${p.vehicleType.toLowerCase()}.param`, text);
      setInfo(`Saved ${text.split("\n").filter((l) => l.trim()).length} parameters.`);
    } catch (error) {
      setInfo(`Save failed: ${String(error)}`);
    }
  }

  function pickFile(mode: "load" | "compare") {
    fileMode.current = mode;
    fileInput.current?.click();
  }

  async function onFile(file: File | undefined) {
    if (!file) return;
    try {
      const parsed = await api.paramsParseFile(await file.text());
      const diffs: CompareRow[] = [];
      let missing = 0;
      for (const { name, value } of parsed.params) {
        const entry = p.byName.get(name);
        if (!entry) missing += 1;
        else if (!sameValue(entry.value, value)) {
          diffs.push({
            name,
            current: formatValue(entry.value, entry.paramType),
            file: formatValue(value, entry.paramType),
            value,
          });
        }
      }
      if (fileMode.current === "compare") {
        setCompare({ rows: diffs, missing });
      } else {
        diffs.forEach((row) => paramStore.setEdit(row.name, row.value));
        setInfo(
          `Loaded ${parsed.params.length} value(s): ${diffs.length} differ, ${missing} not on the vehicle, ${parsed.skipped.length} skipped.`,
        );
      }
    } catch (error) {
      setInfo(`Could not read the file: ${String(error)}`);
    } finally {
      if (fileInput.current) fileInput.current.value = "";
    }
  }

  const progressPct = p.progress && p.progress.total > 0 ? Math.min(100, (p.progress.received / p.progress.total) * 100) : 0;

  return (
    <div className="params-view">
      <div className="params-toolbar">
        <button className="primary" disabled={!canUseVehicle || p.refreshing || p.writing} onClick={() => void paramStore.refresh()}>
          {p.refreshing ? "Refreshing…" : "Refresh"}
        </button>
        <button
          className="ok"
          disabled={!canUseVehicle || modifiedCount === 0 || p.writing || p.refreshing}
          onClick={() => (modifiedCount > CONFIRM_ABOVE ? setConfirmWrite(true) : void write())}
        >
          {p.writing ? "Writing…" : `Write Params${modifiedCount ? ` (${modifiedCount})` : ""}`}
        </button>
        <button disabled={p.list.length === 0} onClick={() => void save()}>
          Save to file
        </button>
        <button disabled={p.list.length === 0} onClick={() => pickFile("load")}>
          Load from file
        </button>
        <button disabled={p.list.length === 0} onClick={() => pickFile("compare")}>
          Compare
        </button>
        <button disabled={modifiedCount === 0 && failedCount === 0} onClick={() => paramStore.discardEdits()}>
          Discard edits
        </button>
        <input
          className="search"
          aria-label="Search parameters"
          placeholder="Search name or description"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <label className="toggle">
          <input type="checkbox" checked={modifiedOnly} onChange={(e) => setModifiedOnly(e.target.checked)} /> Modified
        </label>
        <label className="toggle">
          <input type="checkbox" checked={favoritesOnly} onChange={(e) => setFavoritesOnly(e.target.checked)} /> Favorites
        </label>
        <label className="toggle">
          <input type="checkbox" checked={advanced} onChange={(e) => setAdvanced(e.target.checked)} /> Advanced
        </label>
        <input
          ref={fileInput}
          type="file"
          accept=".param,.parm,.txt"
          hidden
          onChange={(event) => void onFile(event.target.files?.[0])}
        />
      </div>

      {p.refreshing && (
        <div className="progress" role="progressbar" aria-valuenow={Math.round(progressPct)} aria-valuemin={0} aria-valuemax={100}>
          <div style={{ width: `${progressPct}%` }} />
          <span>
            {p.progress && p.progress.total > 0 ? `${p.progress.received} / ${p.progress.total}` : "Requesting parameters…"}
          </span>
        </div>
      )}
      {p.refreshError && <p className="err-text status-line">Refresh failed: {p.refreshError}</p>}
      {p.rebootRequired && (
        <p className="warn-banner status-line" role="status">
          Reboot required for some of the written parameters to take effect.{" "}
          <button onClick={() => paramStore.clearRebootNotice()}>Dismiss</button>
        </p>
      )}
      {info && (
        <p className="status-line dim" role="status">
          {info}
        </p>
      )}
      {failedCount > 0 && (
        <p className="err-text status-line">
          {failedCount} parameter(s) could not be written; see the red rows.
        </p>
      )}

      <div className="params-body">
        <aside className="panel params-tree-panel">
          <ParamTree nodes={tree} total={p.list.length} selected={selectedNode} onSelect={setSelectedNode} />
        </aside>
        <section className="panel params-table">
          <div className="ptable-row ptable-head">
            <span title="Favorite">★</span>
            <span>Name</span>
            <span>Value</span>
            <span>Units</span>
            <span>Range</span>
            <span>Description</span>
          </div>
          {p.list.length === 0 ? (
            <p className="empty dim">
              {canUseVehicle ? (p.refreshing ? "Downloading parameters…" : "No parameters yet. Press Refresh.") : "Connect to a vehicle to see its parameters."}
            </p>
          ) : rows.length === 0 ? (
            <p className="empty dim">No parameters match the filters.</p>
          ) : (
            <VirtualList
              count={rows.length}
              rowHeight={ROW_HEIGHT}
              render={(index) => {
                const entry = rows[index];
                const meta = p.metadata[entry.name];
                const edit = p.edits[entry.name];
                const error = p.errors[entry.name];
                const modified = edit !== undefined;
                return (
                  <div className={`ptable-row ${modified ? "modified" : ""} ${error ? "failed" : ""}`}>
                    <button
                      className={`star ${p.favorites.has(entry.name) ? "on" : ""}`}
                      aria-label={p.favorites.has(entry.name) ? "Remove favorite" : "Add favorite"}
                      onClick={() => paramStore.toggleFavorite(entry.name)}
                    >
                      {p.favorites.has(entry.name) ? "★" : "☆"}
                    </button>
                    <span className="mono ellipsis" title={meta?.displayName || entry.name}>
                      {entry.name}
                    </span>
                    <ParamValueInput
                      entry={entry}
                      meta={meta}
                      edit={edit}
                      disabled={!canUseVehicle || p.writing}
                      onChange={(value) => paramStore.setEdit(entry.name, value)}
                    />
                    <span className="dim ellipsis">{meta?.units}</span>
                    <span className="dim ellipsis mono">{rangeText(meta)}</span>
                    <span
                      className={`ellipsis ${error ? "err-text" : "dim"}`}
                      title={error ?? `${meta?.displayName ?? ""}${meta?.description ? ` — ${meta.description}` : ""}`}
                    >
                      {error ?? (meta ? `${meta.displayName}${meta.description ? ` — ${meta.description}` : ""}` : "")}
                    </span>
                  </div>
                );
              }}
            />
          )}
        </section>
      </div>

      {confirmWrite && (
        <ConfirmDialog
          title={`Write ${modifiedCount} parameters?`}
          tone="warn"
          message="Changing many parameters at once is risky. Make sure the vehicle is disarmed and you reviewed the list."
          confirmLabel="Write"
          onConfirm={() => void write()}
          onCancel={() => setConfirmWrite(false)}
        />
      )}
      {compare && (
        <ParamCompareDialog
          rows={compare.rows}
          missing={compare.missing}
          onClose={() => setCompare(null)}
          onApply={(chosen) => {
            chosen.forEach((row) => paramStore.setEdit(row.name, row.value));
            setInfo(`${chosen.length} value(s) from the file marked as modified.`);
            setCompare(null);
          }}
        />
      )}
    </div>
  );
}
