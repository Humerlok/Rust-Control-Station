/**
 * Flight Data actions: arm/disarm, flight mode, set waypoint, restart mission and the backend action list.
 * Disarm and flagged actions ask for confirmation; a failed arm/disarm offers a "force" retry that bypasses the vehicle safety checks.
 */
import { useEffect, useState } from "react";
import { api } from "../api";
import type { ActionInfo, Telemetry } from "../types";
import { ConfirmDialog } from "./ConfirmDialog";

type Result = { ok: boolean; text: string };

// Which confirmation dialog is open, if any.
type Pending =
  | { kind: "disarm" }
  | { kind: "force"; arming: boolean; reason: string }
  | { kind: "action"; action: ActionInfo };

type Props = { telemetry: Telemetry };

export function ActionsPanel({ telemetry }: Props) {
  const [actions, setActions] = useState<ActionInfo[]>([]);
  const [actionName, setActionName] = useState("");
  const [modeChoice, setModeChoice] = useState("");
  const [waypoint, setWaypoint] = useState("1");
  const [pending, setPending] = useState<Pending | null>(null);
  const [result, setResult] = useState<Result | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api
      .listActions()
      .then((list) => {
        setActions(list);
        setActionName((current) => current || list[0]?.name || "");
      })
      .catch(() => setActions([]));
  }, []);

  const enabled = telemetry.connected && !busy;
  const mode = modeChoice || telemetry.mode || "";
  const selectedAction = actions.find((a) => a.name === actionName);

  // Runs one command and shows its result line; returns the error text (null on success) so callers can offer a forced retry.
  async function run(label: string, call: () => Promise<string>): Promise<string | null> {
    setBusy(true);
    try {
      const reply = await call();
      setResult({ ok: true, text: `${label}: ${reply}` });
      return null;
    } catch (error) {
      setResult({ ok: false, text: `${label}: ${String(error)}` });
      return String(error);
    } finally {
      setBusy(false);
    }
  }

  async function arm() {
    const failure = await run("Arm", () => api.arm(false));
    if (failure) setPending({ kind: "force", arming: true, reason: failure });
  }

  async function disarm() {
    setPending(null);
    const failure = await run("Disarm", () => api.disarm(false));
    if (failure) setPending({ kind: "force", arming: false, reason: failure });
  }

  async function forceNow(arming: boolean) {
    setPending(null);
    await run(arming ? "Force arm" : "Force disarm", () => (arming ? api.arm(true) : api.disarm(true)));
  }

  // Quick buttons stay disabled when the vehicle does not list that mode (matched case-insensitively).
  function quickMode(name: string) {
    const exact = telemetry.availableModes.find((m) => m.toLowerCase() === name.toLowerCase());
    return exact ? () => run(`Mode ${exact}`, () => api.setMode(exact)) : undefined;
  }

  function startAction() {
    if (!selectedAction) return;
    if (selectedAction.confirm) setPending({ kind: "action", action: selectedAction });
    else void run(selectedAction.label, () => api.doAction(selectedAction.name));
  }

  async function confirmAction(action: ActionInfo) {
    setPending(null);
    await run(action.label, () => api.doAction(action.name));
  }

  const quick = (name: string) => {
    const handler = quickMode(name);
    return (
      <button disabled={!enabled || !handler} onClick={handler}>
        {name}
      </button>
    );
  };

  // Mission item sequence numbers are 16-bit on the wire.
  const seq = Number(waypoint);
  const seqValid = Number.isInteger(seq) && seq >= 0 && seq <= 65535;

  return (
    <section className="panel actions">
      <header>
        <h2>Actions</h2>
      </header>
      <div className="actions-body">
        <div className="row">
          <button className="ok" disabled={!enabled} onClick={arm}>
            Arm
          </button>
          <button className="danger" disabled={!enabled} onClick={() => setPending({ kind: "disarm" })}>
            Disarm
          </button>
        </div>

        <div className="row">
          <select aria-label="Flight mode" value={mode} disabled={!enabled} onChange={(e) => setModeChoice(e.target.value)}>
            {mode && !telemetry.availableModes.includes(mode) && <option value={mode}>{mode}</option>}
            {telemetry.availableModes.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
          <button disabled={!enabled || !mode} onClick={() => run(`Mode ${mode}`, () => api.setMode(mode))}>
            Set Mode
          </button>
        </div>

        <div className="row">
          {quick("Auto")}
          {quick("RTL")}
          {quick("Loiter")}
        </div>

        <div className="row">
          <input
            aria-label="Waypoint number"
            className="wp"
            inputMode="numeric"
            value={waypoint}
            disabled={!enabled}
            onChange={(e) => setWaypoint(e.target.value)}
          />
          <button disabled={!enabled || !seqValid} onClick={() => run(`Set WP ${seq}`, () => api.setWaypoint(seq))}>
            Set WP
          </button>
          <button disabled={!enabled} onClick={() => run("Restart mission", () => api.restartMission())}>
            Restart Mission
          </button>
        </div>

        <div className="row">
          <select aria-label="Action" value={actionName} disabled={!enabled} onChange={(e) => setActionName(e.target.value)}>
            {actions.map((a) => (
              <option key={a.name} value={a.name}>
                {a.label}
              </option>
            ))}
          </select>
          <button className={selectedAction?.danger ? "danger" : ""} disabled={!enabled || !selectedAction} onClick={startAction}>
            Do Action
          </button>
        </div>

        <p className={`result ${result ? (result.ok ? "ok-text" : "err-text") : "dim"}`} role="status">
          {result ? result.text : telemetry.connected ? "Ready." : "Connect to a vehicle to use actions."}
        </p>
      </div>

      {pending?.kind === "disarm" && (
        <ConfirmDialog
          title="Disarm motors?"
          tone="warn"
          message={telemetry.fields.alt > 2 ? "The vehicle appears to be flying. Disarming in flight will drop it." : "Motors will be disarmed."}
          confirmLabel="Disarm"
          onConfirm={disarm}
          onCancel={() => setPending(null)}
        />
      )}
      {pending?.kind === "force" && (
        <ConfirmDialog
          title={pending.arming ? "Arm failed. Force arm?" : "Disarm failed. Force disarm?"}
          tone="danger"
          message={`${pending.reason}. Forcing bypasses the vehicle's safety checks and can be dangerous.`}
          confirmLabel={pending.arming ? "Force arm" : "Force disarm"}
          onConfirm={() => forceNow(pending.arming)}
          onCancel={() => setPending(null)}
        />
      )}
      {pending?.kind === "action" && (
        <ConfirmDialog
          title={`${pending.action.label}?`}
          tone={pending.action.danger ? "danger" : "warn"}
          message="Are you sure?"
          confirmLabel="Do it"
          onConfirm={() => confirmAction(pending.action)}
          onCancel={() => setPending(null)}
        />
      )}
    </section>
  );
}
