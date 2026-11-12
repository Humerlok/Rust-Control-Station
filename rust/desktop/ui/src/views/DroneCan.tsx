/**
 * DroneCAN screen: node table, node parameters, restart, firmware update and bus inspector, all through the MAVLink CAN relay.
 * Uses useDroneCan (api.dronecan* + event). Update is blocked while armed and always goes through a danger ConfirmDialog.
 */
import { useRef, useState } from "react";
import { api } from "../api";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { NodeParamsPanel } from "../components/NodeParamsPanel";
import { droneCanSource } from "../components/inspector/useInspector";
import type { DroneCanNode, NodeImageInfo, Telemetry } from "../types";
import { useDroneCan } from "../useDroneCan";
import { InspectorPanel } from "./Inspector";
import "../dronecan.css";

type Props = {
  telemetry: Telemetry;
  /** The screen is the one being shown. */
  active: boolean;
  notify: (text: string, ok: boolean) => void;
};

function uptime(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  return h > 0 ? `${h}h ${String(m).padStart(2, "0")}m ${String(s).padStart(2, "0")}s` : `${m}m ${String(s).padStart(2, "0")}s`;
}

// Chunked so String.fromCharCode does not overflow the call stack on large firmware files.
function toBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let text = "";
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(text);
}

type PendingUpdate = { node: DroneCanNode; fileName: string; data: string; info: NodeImageInfo };

const HEALTH_CLASS: Record<string, string> = { OK: "ok", WARNING: "warn", ERROR: "danger", CRITICAL: "danger" };
const LEVEL_CLASS: Record<string, string> = { ERROR: "sev-error", WARNING: "sev-warn", DEBUG: "sev-dim" };

export function DroneCan({ telemetry, active, notify }: Props) {
  const state = useDroneCan();
  const [tab, setTab] = useState<"nodes" | "inspector">("nodes");
  const [bus, setBus] = useState(1);
  const [busy, setBusy] = useState(false);
  const [paramsNode, setParamsNode] = useState<number | null>(null);
  const [restart, setRestart] = useState<DroneCanNode | null>(null);
  const [pendingUpdate, setPendingUpdate] = useState<PendingUpdate | null>(null);
  const [force, setForce] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);
  const updateTarget = useRef<DroneCanNode | null>(null);

  const connected = telemetry.connected;
  // Firmware transfer in flight: disables Update/restart-sensitive actions and shows the do-not-unplug warning.
  const updating = state.update !== null && (state.update.phase === "starting" || state.update.phase === "sending");

  function chooseFirmware(node: DroneCanNode) {
    updateTarget.current = node;
    fileInput.current?.click();
  }

  // Reads the chosen file, asks the backend to inspect it (format, CRC), then opens the confirm dialog; nothing is sent yet.
  async function onFirmwareFile(file: File | undefined) {
    const node = updateTarget.current;
    if (fileInput.current) fileInput.current.value = "";
    if (!file || !node) return;
    try {
      const data = toBase64(await file.arrayBuffer());
      const info = await api.dronecanInspectFirmware(data);
      setForce(false);
      setPendingUpdate({ node, fileName: file.name, data, info });
    } catch (error) {
      notify(`Firmware file: ${String(error)}`, false);
    }
  }
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
        <label className="toggle" title="Hand out node ids to devices that ask for one (dynamic node allocation)">
          <input
            type="checkbox"
            checked={state.allocation.enabled}
            disabled={!state.active || busy}
            onChange={(e) => void run("Node ids", () => api.dronecanSetAllocation(e.target.checked))}
          />{" "}
          Assign node IDs to new devices
        </label>
        <span className="dim dc-stats mono">
          {state.active ? `CAN${state.bus} · ${state.framesRx} frames in · ${state.framesTx} out · ${state.nodes.length} node(s)` : "Not connected to a CAN bus"}
        </span>
      </div>
      {!connected && !state.active && <p className="warn-text status-line">Connect to the vehicle first: DroneCAN uses its MAVLink link.</p>}
      <p className="dim dc-note">
        The autopilot relays one of its CAN buses over MAVLink while this screen is connected. The ground station appears on the bus
        as node 127. The CAN port on the autopilot must have a DroneCAN driver enabled (CAN_Pn_DRIVER).
      </p>

      <div className="dc-tabs" role="tablist" aria-label="DroneCAN views">
        <button role="tab" aria-selected={tab === "nodes"} className={tab === "nodes" ? "active" : ""} onClick={() => setTab("nodes")}>
          Nodes
        </button>
        <button role="tab" aria-selected={tab === "inspector"} className={tab === "inspector" ? "active" : ""} onClick={() => setTab("inspector")}>
          Inspector
        </button>
      </div>

      {tab === "inspector" && (
        <div className="dc-inspector">
          <InspectorPanel
            active={active && tab === "inspector" && state.active}
            notify={notify}
            source={droneCanSource}
            originLabel="Node"
            emptyText={state.active ? "Waiting for traffic on the CAN bus…" : "Connect to a CAN bus to see its traffic."}
            storagePrefix="dronecan.inspector"
          />
        </div>
      )}

      {tab === "nodes" && (
      <>
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
                    <button disabled={!n.online || busy || updating || telemetry.armed} onClick={() => chooseFirmware(n)} title={telemetry.armed ? "Disarm the vehicle first" : "Send a firmware file (.bin or .apj) to this node"}>
                      Update
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

      <input
        ref={fileInput}
        type="file"
        accept=".bin,.apj"
        hidden
        onChange={(e) => void onFirmwareFile(e.target.files?.[0])}
        aria-label="Node firmware file"
      />

      {state.update && (
        <section className="panel dc-update" aria-label="Node firmware update">
          <header>
            <h2>
              Firmware update: node {state.update.node}
              {state.nodes.find((n) => n.id === state.update?.node)?.name ? ` (${state.nodes.find((n) => n.id === state.update?.node)?.name})` : ""}
            </h2>
            {updating && (
              <button className="danger" disabled={busy} onClick={() => void run("Update", () => api.dronecanCancelUpdate())}>
                Cancel
              </button>
            )}
          </header>
          <div className="dc-update-body">
            <div
              className="progress"
              role="progressbar"
              aria-valuenow={state.update.total > 0 ? Math.round((state.update.sent / state.update.total) * 100) : 0}
              aria-valuemin={0}
              aria-valuemax={100}
            >
              <div style={{ width: `${state.update.total > 0 ? (state.update.sent / state.update.total) * 100 : 0}%` }} />
              <span>
                {state.update.total > 0 ? Math.round((state.update.sent / state.update.total) * 100) : 0}% · {state.update.sent} / {state.update.total} bytes
              </span>
            </div>
            <p className={`status-line ${state.update.phase === "failed" ? "err-text" : state.update.phase === "done" ? "ok-text" : "dim"}`} role="status">
              {state.update.phase === "failed" ? "Failed: " : ""}
              {state.update.message}
            </p>
            {updating && <p className="warn-text status-line">Do not power off the vehicle or unplug the CAN bus until this finishes.</p>}
          </div>
        </section>
      )}

      {pendingUpdate && (
        <ConfirmDialog
          title={`Update node ${pendingUpdate.node.id}?`}
          tone="danger"
          confirmLabel="Update node"
          onConfirm={() => {
            const { node, data } = pendingUpdate;
            const sendAnyway = force;
            setPendingUpdate(null);
            void run(`Node ${node.id}`, () => api.dronecanUpdateFirmware(node.id, data, sendAnyway));
          }}
          onCancel={() => setPendingUpdate(null)}
        >
          <ul className="dc-update-facts">
            <li>
              Node: <strong>{pendingUpdate.node.name ?? `node ${pendingUpdate.node.id}`}</strong>, running {pendingUpdate.node.swVersion ?? "an unknown version"}
            </li>
            <li>
              File: <span className="mono">{pendingUpdate.fileName}</span> ({pendingUpdate.info.format}, {pendingUpdate.info.size} bytes
              {pendingUpdate.info.description ? `, ${pendingUpdate.info.description}` : ""})
            </li>
            <li>
              Image CRC: <span className="mono">{pendingUpdate.info.crc ?? "none"}</span>; the node reports{" "}
              <span className="mono">{pendingUpdate.node.swCrc ?? "none"}</span>
            </li>
          </ul>
          {pendingUpdate.info.crc === null && (
            <p className="warn-text">This file has no ArduPilot application descriptor. Make sure it is firmware for this exact device.</p>
          )}
          {pendingUpdate.info.crc !== null && pendingUpdate.info.crc === pendingUpdate.node.swCrc && (
            <label className="toggle">
              <input type="checkbox" checked={force} onChange={(e) => setForce(e.target.checked)} /> The node already runs this image. Send it anyway
            </label>
          )}
          <p className="warn-text">
            The wrong firmware, or an interrupted transfer, can leave the device unusable until it is recovered through its bootloader. Keep the
            vehicle disarmed, powered and connected until the update finishes.
          </p>
        </ConfirmDialog>
      )}

      {state.allocation.enabled && state.allocation.recent.length > 0 && (
        <section className="panel">
          <header>
            <h2>Node ids assigned</h2>
          </header>
          <ul className="message-list">
            {state.allocation.recent.map((a) => (
              <li key={a.uniqueId}>
                <span className="mono">#{a.nodeId}</span> <span className="mono dim">{a.uniqueId}</span>
              </li>
            ))}
          </ul>
        </section>
      )}

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
      </>
      )}

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
