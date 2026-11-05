import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../api";
import type { DroneCanNode, NodeParam, NodeParamValue } from "../types";

type Props = {
  node: DroneCanNode;
  progress: { node: number; received: number } | null;
  notify: (text: string, ok: boolean) => void;
  onClose: () => void;
};

function show(value: NodeParamValue | null): string {
  if (value === null || value.type === "empty") return "—";
  if (value.type === "boolean") return value.value ? "true" : "false";
  if (value.type === "real") return String(Number(value.value.toPrecision(7)));
  return String(value.value);
}

/** The value typed by the user, as the same type the node uses; null when it does not parse. */
function parse(original: NodeParamValue, text: string): NodeParamValue | null {
  const trimmed = text.trim();
  switch (original.type) {
    case "integer":
      return /^-?\d+$/.test(trimmed) ? { type: "integer", value: Number(trimmed) } : null;
    case "real":
      return trimmed !== "" && Number.isFinite(Number(trimmed)) ? { type: "real", value: Number(trimmed) } : null;
    case "boolean":
      return trimmed === "true" || trimmed === "false" ? { type: "boolean", value: trimmed === "true" } : null;
    case "text":
      return { type: "text", value: text };
    default:
      return null;
  }
}

const sameValue = (a: NodeParamValue, b: NodeParamValue) =>
  a.type === b.type && (a.type === "empty" || (a as { value: unknown }).value === (b as { value: unknown }).value ||
    (a.type === "real" && Math.abs(a.value - (b as { value: number }).value) < 1e-6));

export function NodeParamsPanel({ node, progress, notify, onClose }: Props) {
  const [params, setParams] = useState<NodeParam[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [edits, setEdits] = useState<Record<string, string>>({});
  const [failed, setFailed] = useState<Record<string, string>>({});
  const [search, setSearch] = useState("");
  const [modifiedOnly, setModifiedOnly] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setParams(await api.dronecanParams(node.id));
      setEdits({});
      setFailed({});
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [node.id]);

  useEffect(() => {
    void load();
  }, [load]);

  const changes = useMemo(() => {
    if (!params) return [];
    return params.flatMap((p) => {
      const text = edits[p.name];
      if (text === undefined) return [];
      const next = parse(p.value, text);
      return next && !sameValue(next, p.value) ? [{ param: p, next }] : [];
    });
  }, [params, edits]);

  const visible = useMemo(() => {
    if (!params) return [];
    const needle = search.trim().toLowerCase();
    return params.filter((p) => {
      if (needle && !p.name.toLowerCase().includes(needle)) return false;
      if (modifiedOnly && p.default && sameValue(p.value, p.default)) return false;
      return true;
    });
  }, [params, search, modifiedOnly]);

  async function write() {
    // *_ENABLE parameters go last, so the settings they enable are already in place.
    const ordered = [...changes].sort((a, b) => Number(/_ENABLE$/.test(a.param.name)) - Number(/_ENABLE$/.test(b.param.name)));
    setBusy(true);
    const nextFailed: Record<string, string> = {};
    let written = 0;
    for (const { param, next } of ordered) {
      try {
        const echoed = await api.dronecanSetParam(node.id, param.name, next);
        setParams((list) => list?.map((p) => (p.name === param.name ? { ...p, value: echoed.value } : p)) ?? null);
        if (sameValue(echoed.value, next)) written += 1;
        else nextFailed[param.name] = `the node kept ${show(echoed.value)}`;
      } catch (e) {
        nextFailed[param.name] = String(e);
      }
    }
    setFailed(nextFailed);
    setEdits((current) => Object.fromEntries(Object.entries(current).filter(([name]) => name in nextFailed)));
    try {
      if (written > 0) notify(`${written} parameter${written === 1 ? "" : "s"} written. ${await api.dronecanSaveParams(node.id)}`, true);
    } catch (e) {
      notify(`Written, but saving to flash failed: ${String(e)}`, false);
    }
    if (Object.keys(nextFailed).length > 0) notify(`${Object.keys(nextFailed).length} parameter(s) were not accepted`, false);
    setBusy(false);
  }

  async function commit() {
    setBusy(true);
    try {
      notify(await api.dronecanSaveParams(node.id), true);
    } catch (e) {
      notify(String(e), false);
    } finally {
      setBusy(false);
    }
  }

  const label = node.name ?? `node ${node.id}`;
  const received = progress?.node === node.id ? progress.received : null;

  return (
    <section className="panel dc-params" aria-label={`Parameters of node ${node.id}`}>
      <header>
        <h2>
          Parameters: {label} <span className="dim mono">#{node.id}</span>
        </h2>
        <button onClick={onClose}>Close</button>
      </header>
      <div className="dc-params-body">
        <div className="params-toolbar">
          <input className="search" placeholder="Search…" value={search} onChange={(e) => setSearch(e.target.value)} />
          <label className="toggle">
            <input type="checkbox" checked={modifiedOnly} onChange={(e) => setModifiedOnly(e.target.checked)} /> Non-default only
          </label>
          <button disabled={loading || busy} onClick={() => void load()}>
            Reload
          </button>
          <button className="primary" disabled={busy || loading || changes.length === 0} onClick={() => void write()}>
            Write {changes.length > 0 ? `${changes.length} change${changes.length === 1 ? "" : "s"}` : "changes"}
          </button>
          <button disabled={busy || loading} onClick={() => void commit()} title="Make the node write its parameters to flash">
            Commit to flash
          </button>
          {changes.length > 0 && (
            <button disabled={busy} onClick={() => setEdits({})}>
              Discard
            </button>
          )}
        </div>

        {loading && (
          <p className="status-line dim" role="status">
            Reading parameters{received !== null ? `… ${received}` : "…"}
          </p>
        )}
        {error && <p className="status-line err-text">{error}</p>}

        {params && (
          <div className="dc-ptable" role="table">
            <div className="dc-prow head" role="row">
              <span>Name</span>
              <span>Value</span>
              <span>Default</span>
              <span>Min</span>
              <span>Max</span>
              <span />
            </div>
            {visible.map((p) => {
              const text = edits[p.name];
              const next = text === undefined ? null : parse(p.value, text);
              const invalid = text !== undefined && next === null;
              const changed = next !== null && !sameValue(next, p.value);
              return (
                <div key={p.name} className={`dc-prow ${changed ? "modified" : ""} ${failed[p.name] ? "failed" : ""}`} role="row">
                  <span className="mono ellipsis" title={p.name}>
                    {p.name}
                  </span>
                  {p.value.type === "boolean" ? (
                    <select value={text ?? show(p.value)} onChange={(e) => setEdits({ ...edits, [p.name]: e.target.value })}>
                      <option value="true">true</option>
                      <option value="false">false</option>
                    </select>
                  ) : (
                    <input
                      className={`pv-input ${invalid ? "invalid" : ""}`}
                      value={text ?? show(p.value)}
                      disabled={p.value.type === "empty"}
                      onChange={(e) => setEdits({ ...edits, [p.name]: e.target.value })}
                      aria-label={p.name}
                    />
                  )}
                  <span className="mono dim">{show(p.default)}</span>
                  <span className="mono dim">{p.min ?? "—"}</span>
                  <span className="mono dim">{p.max ?? "—"}</span>
                  <span className="err-text ellipsis" title={failed[p.name]}>
                    {failed[p.name] ?? ""}
                  </span>
                </div>
              );
            })}
            {visible.length === 0 && <p className="empty dim">No parameters match.</p>}
          </div>
        )}
      </div>
    </section>
  );
}
