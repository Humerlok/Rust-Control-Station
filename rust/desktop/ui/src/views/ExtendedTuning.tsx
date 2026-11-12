/**
 * Extended Tuning (Copter only): curated tuning controls from tuning/copter.ts over the shared paramStore (useParams).
 * Writes only this screen's own pending edits via paramStore.writeParams.
 */
import { useMemo, useState } from "react";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { ParamValueInput } from "../components/ParamValueInput";
import { formatValue } from "../paramUtils";
import { COPTER_GROUPS, latLonTwin, rollPitchPartner, type TuneControl } from "../tuning/copter";
import type { Telemetry } from "../types";
import { paramStore, useParams } from "../useParams";

type Props = { telemetry: Telemetry; notify: (text: string, ok: boolean) => void };

type Resolved = { control: TuneControl; name: string };

export function ExtendedTuning({ telemetry, notify }: Props) {
  const p = useParams();
  const [lockRollPitch, setLockRollPitch] = useState(true);
  const [doubling, setDoubling] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);

  // Only show controls whose parameter exists on this vehicle; `names` lists alternatives (first one found wins).
  const groups = useMemo(
    () =>
      COPTER_GROUPS.map((group) => ({
        ...group,
        controls: group.controls
          .map((control): Resolved | null => {
            const name = control.names.find((n) => p.byName.has(n));
            return name ? { control, name } : null;
          })
          .filter((c): c is Resolved => c !== null),
      })).filter((group) => group.controls.length > 0),
    [p.byName],
  );

  const resolvedById = useMemo(() => {
    const map = new Map<string, string>();
    groups.forEach((g) => g.controls.forEach((c) => map.set(c.control.id, c.name)));
    return map;
  }, [groups]);

  /** Parameters this screen owns: every resolved control plus the LON twins it mirrors to. */
  const ownedNames = useMemo(() => {
    const owned = new Set<string>(resolvedById.values());
    resolvedById.forEach((name) => {
      const twin = latLonTwin(name);
      if (twin && p.byName.has(twin)) owned.add(twin);
    });
    return owned;
  }, [resolvedById, p.byName]);

  const pending = Object.keys(p.edits).filter((name) => ownedNames.has(name));
  const canUseVehicle = p.linkConnected && p.vehicleAlive;

  if (telemetry.vehicleType !== "Copter" && p.linkConnected) {
    return (
      <div className="placeholder">
        <div>
          <h1>Extended Tuning</h1>
          <p>This screen is available for Copter only. The connected vehicle is a {telemetry.vehicleType}.</p>
          <p className="dim">Use the Parameters screen to tune other vehicles.</p>
        </div>
      </div>
    );
  }

  // Edits are staged in paramStore; mirrored controls (roll/pitch lock, lat/lon twin) get the same value.
  function change({ control, name }: Resolved, value: number | null) {
    paramStore.setEdit(name, value);
    if (control.mirror === "rollpitch" && lockRollPitch) {
      const partner = rollPitchPartner(control);
      const partnerName = partner ? resolvedById.get(partner) : undefined;
      if (partnerName) paramStore.setEdit(partnerName, value);
    }
    if (control.mirror === "latlon") {
      const twin = latLonTwin(name);
      if (twin && p.byName.has(twin)) paramStore.setEdit(twin, value);
    }
  }

  // Safety check: edits that more than double the current (non-zero) value need confirmation before writing.
  function doubled(): string[] {
    return pending.filter((name) => {
      const entry = p.byName.get(name);
      return entry && entry.value !== 0 && Math.abs(p.edits[name]) > 2 * Math.abs(entry.value);
    });
  }

  async function write() {
    setDoubling(null);
    setBusy(true);
    try {
      const summary = await paramStore.writeParams(pending);
      notify(
        `Wrote ${summary.written} parameter(s)${summary.failed ? `, ${summary.failed} failed` : ""}${
          summary.rebootRequired ? ". Reboot required." : "."
        }`,
        summary.failed === 0,
      );
    } finally {
      setBusy(false);
    }
  }

  function startWrite() {
    const risky = doubled();
    if (risky.length > 0) setDoubling(risky);
    else void write();
  }

  async function refreshScreen() {
    paramStore.discardEdits([...ownedNames]);
    await paramStore.loadList();
  }

  return (
    <div className="tuning-view">
      <div className="params-toolbar">
        <button className="primary" disabled={!canUseVehicle || p.refreshing || busy} onClick={() => void paramStore.refresh()}>
          {p.refreshing ? "Refreshing…" : "Refresh Params"}
        </button>
        <button disabled={p.list.length === 0 || busy} onClick={() => void refreshScreen()}>
          Refresh Screen
        </button>
        <button className="ok" disabled={!canUseVehicle || pending.length === 0 || busy || p.refreshing} onClick={startWrite}>
          {busy ? "Writing…" : `Write Params${pending.length ? ` (${pending.length})` : ""}`}
        </button>
        <label className="toggle">
          <input type="checkbox" checked={lockRollPitch} onChange={(e) => setLockRollPitch(e.target.checked)} /> Lock roll &amp; pitch
        </label>
        {p.rebootRequired && <span className="badge warn">Reboot required</span>}
      </div>

      {p.list.length === 0 ? (
        <p className="empty dim">
          {canUseVehicle ? (p.refreshing ? "Downloading parameters…" : "No parameters yet. Press Refresh Params.") : "Connect to a vehicle to tune it."}
        </p>
      ) : (
        <div className="tuning-groups">
          {groups.map((group) => (
            <section key={group.id} className="panel tuning-group">
              <header>
                <h2>{group.title}</h2>
              </header>
              <div className="tuning-controls">
                {group.controls.map((resolved) => {
                  const entry = p.byName.get(resolved.name)!;
                  const meta = p.metadata[resolved.name];
                  const edit = p.edits[resolved.name];
                  const error = p.errors[resolved.name];
                  return (
                    <div
                      key={resolved.control.id}
                      className={`tuning-control ${edit !== undefined ? "changed" : ""} ${error ? "failed" : ""}`}
                      title={meta ? `${meta.displayName}${meta.description ? ` — ${meta.description}` : ""}` : resolved.name}
                    >
                      <label>
                        <span>{resolved.control.label}</span>
                        <span className="mono dim">{resolved.name}</span>
                      </label>
                      <ParamValueInput
                        entry={entry}
                        meta={meta}
                        edit={edit}
                        disabled={!canUseVehicle || busy}
                        onChange={(value) => change(resolved, value)}
                      />
                      {meta && (meta.min !== null || meta.max !== null) && (
                        <span className="dim range">
                          {meta.min ?? "…"} – {meta.max ?? "…"} {meta.units}
                        </span>
                      )}
                      {error && <span className="err-text range">{error}</span>}
                    </div>
                  );
                })}
              </div>
            </section>
          ))}
        </div>
      )}

      {doubling && (
        <ConfirmDialog
          title="Large change"
          tone="warn"
          message="These values more than double. Continue?"
          confirmLabel="Write"
          onConfirm={() => void write()}
          onCancel={() => setDoubling(null)}
        >
          <ul className="mono">
            {doubling.map((name) => {
              const entry = p.byName.get(name)!;
              return (
                <li key={name}>
                  {name}: {formatValue(entry.value, entry.paramType)} → {formatValue(p.edits[name], entry.paramType)}
                </li>
              );
            })}
          </ul>
        </ConfirmDialog>
      )}
    </div>
  );
}
