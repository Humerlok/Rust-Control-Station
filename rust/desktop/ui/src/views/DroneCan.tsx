import { useState } from "react";
import { api } from "../api";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { NodeParamsPanel } from "../components/NodeParamsPanel";
import type { DroneCanNode, Telemetry } from "../types";
import { useDroneCan } from "../useDroneCan";
import "../dronecan.css";

type Props = {
  telemetry: Telemetry;
  notify: (text: string, ok: boolean) => void;
};

function uptime(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  return h > 0 ? `${h}h ${String(m).padStart(2, "0")}m ${String(s).padStart(2, "0")}s` : `${m}m ${String(s).padStart(2, "0")}s`;
}

const HEALTH_CLASS: Record<string, string> = { OK: "ok", WARNING: "warn", ERROR: "danger", CRITICAL: "danger" };
const LEVEL_CLASS: Record<string, string> = { ERROR: "sev-error", WARNING: "sev-warn", DEBUG: "sev-dim" };

export function DroneCan({ telemetry, notify }: Props) {
  const state = useDroneCan();
  const [bus, setBus] = useState(1);
  const [busy, setBusy] = useState(false);
  const [paramsNode, setParamsNode] = useState<number | null>(null);
  const [restart, setRestart] = useState<DroneCanNode | null>(null);

  const connected = telemetry.connected;
  const selected = state.nodes.find((n) => n.id === paramsNode) ?? null;

  async function run(label: string, call: () => Promise<string>) {
    setBusy(true);
    try {
      notify(`${label}: ${await call()}`, true);
    } catch (error) {
      notify(`${label}: ${String(error)}`, false);
    } finally {
      setBusy(false);
    }
  }

  function toggle() {
    if (state.active) {
      setParamsNode(null);
      void run("DroneCAN", () => api.dronecanStop());
    } else {
      void run("DroneCAN", () => api.dronecanStart(bus));
    }
  }

  return (
    <div className="dc-view">
      <div className="dc-connect panel">
        <label>
          CAN bus
          <select value={state.bus ?? bus} disabled={state.active} onChange={(e) => setBus(Number(e.target.value))}>
            <option value={1}>CAN1</option>
            <option value={2}>CAN2</option>
          </select>
        </label>
        <button className={state.active ? "" : "primary"} disabled={busy || (!connected && !state.active)} onClick={toggle}>
          {state.active ? "Disconnect" : "Connect"}
        </button>
        <span className="dim dc-stats mono">
          {state.active ? `CAN${state.bus} · ${state.framesRx} frames in · ${state.framesTx} out · ${state.nodes.length} node(s)` : "Not connected to a CAN bus"}
        </span>
      </div>
      {!connected && !state.active && <p className="warn-text status-line">Connect to the vehicle first: DroneCAN uses its MAVLink link.</p>}
      <p className="dim dc-note">
        The autopilot relays one of its CAN buses over MAVLink while this screen is connected. The ground station appears on the bus
        as node 127. The CAN port on the autopilot must have a DroneCAN driver enabled (CAN_Pn_DRIVER).
      </p>

      <section className="panel">
        <header>
          <h2>Nodes</h2>
          <span className="dim">{state.nodes.filter((n) => n.online).length} online</span>
        </header>
        <div className="dc-table-wrap">
          <table className="dc-table">
            <thead>
              <tr>
                <th>ID</th>
                <th>Name</th>
                <th>Mode</th>
                <th>Health</th>
                <th>Uptime</th>
                <th>HW</th>
                <th>SW</th>
                <th>SW CRC</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {state.nodes.map((n) => (
                <tr key={n.id} className={n.online ? "" : "offline"}>
                  <td className="mono">{n.id}</td>
                  <td title={n.uniqueId ? `Unique ID ${n.uniqueId}` : undefined}>{n.name ?? <span className="dim">asking…</span>}</td>
                  <td>{n.online ? n.mode : "OFFLINE"}</td>
                  <td>
                    <span className={`badge ${HEALTH_CLASS[n.health] ?? ""}`}>{n.health}</span>
                  </td>
                  <td className="mono">{uptime(n.uptimeSec)}</td>
                  <td className="mono">{n.hwVersion ?? "—"}</td>
                  <td className="mono">{n.swVersion ?? "—"}</td>
                  <td className="mono dim">{n.swCrc ?? "—"}</td>
                  <td className="dc-actions">
                    <button disabled={!n.online || busy} onClick={() => setParamsNode(n.id)}>
                      Parameters
                    </button>
                    <button className="danger" disabled={!n.online || busy} onClick={() => setRestart(n)}>
                      Restart
                    </button>
                  </td>
                </tr>
              ))}
              {state.nodes.length === 0 && (
                <tr>
                  <td colSpan={9} className="empty dim">
                    {state.active ? "Waiting for nodes… check the wiring, termination and CAN_Pn_DRIVER." : "Connect to a CAN bus to see its nodes."}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </section>

      {selected && <NodeParamsPanel key={selected.id} node={selected} progress={state.progress} notify={notify} onClose={() => setParamsNode(null)} />}

      <section className="panel">
        <header>
          <h2>Node messages</h2>
          <span className="dim">{state.logs.length} shown</span>
        </header>
        <ul className="message-list dc-logs">
          {state.logs.length === 0 && <li className="dim">No messages from the nodes yet.</li>}
          {state.logs.map((m, i) => (
            <li key={`${m.timeMs}-${i}`} className={LEVEL_CLASS[m.level] ?? ""}>
              <time>#{m.node}</time> <span className="dim">{m.level}</span> <span className="mono">{m.source}</span> {m.text}
            </li>
          ))}
        </ul>
      </section>

      {restart && (
        <ConfirmDialog
          title={`Restart node ${restart.id}?`}
          message={`${restart.name ?? "This node"} will reboot and drop off the bus for a few seconds. Do not do this in flight if the vehicle depends on it (GPS, ESC, compass).`}
          tone={telemetry.armed ? "danger" : "warn"}
          confirmLabel="Restart node"
          onConfirm={() => {
            const node = restart;
            setRestart(null);
            void run(`Node ${node.id}`, () => api.dronecanRestart(node.id));
          }}
          onCancel={() => setRestart(null)}
        />
      )}
    </div>
  );
}
