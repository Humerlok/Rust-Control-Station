import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { loadSetting, saveSetting } from "../storage";
import type { ConnectionConfig, LinkInfo, SerialPortInfo, Telemetry, VehicleSummary } from "../types";

const BAUDS = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 111100, 115200, 230400, 460800, 500000, 625000, 921600, 1000000, 1500000];

type Kind = "auto" | "serial" | "tcp" | "udp" | "udpcl" | "ws" | "replay";

type Settings = {
  kind: Kind;
  serialPort: string;
  baud: number;
  tcpHost: string;
  tcpPort: string;
  udpPort: string;
  udpclHost: string;
  udpclPort: string;
  wsUrl: string;
  replayPath: string;
};

const DEFAULTS: Settings = {
  kind: "udp",
  serialPort: "",
  baud: 115200,
  tcpHost: "127.0.0.1",
  tcpPort: "5760",
  udpPort: "14550",
  udpclHost: "127.0.0.1",
  udpclPort: "14550",
  wsUrl: "",
  replayPath: "exemplos/Teste 02 - tunning.BIN",
};

const SETTINGS_KEY = "connect.settings";

function parsePort(text: string): number {
  const value = Number(text);
  if (!Number.isInteger(value) || value < 1 || value > 65535) throw `invalid port "${text}"`;
  return value;
}

function buildConfig(s: Settings): ConnectionConfig {
  switch (s.kind) {
    case "auto":
      return { type: "auto" };
    case "serial":
      if (!s.serialPort) throw "select a serial port";
      return { type: "serial", port: s.serialPort, baud: s.baud };
    case "tcp":
      return { type: "tcp", host: s.tcpHost.trim(), port: parsePort(s.tcpPort) };
    case "udp":
      return { type: "udp", port: parsePort(s.udpPort) };
    case "udpcl":
      return { type: "udpcl", host: s.udpclHost.trim(), port: parsePort(s.udpclPort) };
    case "ws":
      if (!s.wsUrl.trim()) throw "enter the WebSocket URL";
      return { type: "ws", url: s.wsUrl.trim() };
    case "replay":
      if (!s.replayPath.trim()) throw "enter the log file path";
      return { type: "replay", path: s.replayPath.trim() };
  }
}

type Props = { link: LinkInfo; telemetry: Telemetry; notify: (text: string, ok: boolean) => void };

/** Legacy-style selector label: `1-1 QUADROTOR`, `1-154 GIMBAL`. */
const vehicleLabel = (v: VehicleSummary) => `${v.systemId}-${v.componentId} ${v.component || v.vehicleType}`;

export function ConnectBar({ link, telemetry, notify }: Props) {
  const [settings, setSettings] = useState<Settings>(() => ({ ...DEFAULTS, ...loadSetting<Partial<Settings>>(SETTINGS_KEY, {}) }));
  const [ports, setPorts] = useState<SerialPortInfo[]>([]);
  const [busy, setBusy] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);

  const active = link.state !== "disconnected";

  // Tell the user once per connection when more than one system shows up, so the choice in
  // the Vehicle selector is not missed (the first autopilot is selected automatically).
  const systems = new Set(telemetry.vehicles.map((v) => v.systemId)).size;
  const announced = useRef(false);
  useEffect(() => {
    if (link.state !== "connected") announced.current = false;
    else if (systems > 1 && !announced.current) {
      announced.current = true;
      notify(`${systems} vehicles detected (different SYSID). Choose one in the Vehicle selector.`, true);
    }
  }, [link.state, systems, notify]);
  const update = (patch: Partial<Settings>) => {
    const next = { ...settings, ...patch };
    setSettings(next);
    saveSetting(SETTINGS_KEY, next);
  };

  const refreshPorts = () => {
    api.listSerialPorts().then(setPorts).catch(() => setPorts([]));
  };
  useEffect(refreshPorts, []);

  const selectValue = settings.kind === "serial" ? `serial:${settings.serialPort}` : settings.kind;
  const portNames = ports.map((p) => p.name);
  const knownSaved = settings.serialPort && !portNames.includes(settings.serialPort);

  function onSelectKind(value: string) {
    if (value.startsWith("serial:")) update({ kind: "serial", serialPort: value.slice("serial:".length) });
    else update({ kind: value as Kind });
  }

  async function toggle() {
    setLocalError(null);
    setBusy(true);
    try {
      if (active) await api.disconnect();
      else await api.connect(buildConfig(settings));
    } catch (error) {
      setLocalError(String(error));
    } finally {
      setBusy(false);
    }
  }

  const heartbeat = telemetry.connected;
  let pillText = "Disconnected";
  let pillClass = "";
  if (link.state === "connecting") pillText = "Connecting…";
  else if (link.state === "connected" && !heartbeat) pillText = "No heartbeat";
  else if (link.state === "connected" && heartbeat) {
    const sys = telemetry.vehicle?.systemId;
    pillText = `${telemetry.armed ? "ARMED" : "Connected"} · SYS ${sys ?? "?"}${telemetry.mode ? ` · ${telemetry.mode}` : ""}`;
    pillClass = telemetry.armed ? "armed" : "online";
  }

  const stats = link.stats;
  const error = localError ?? link.error;

  return (
    <>
      <div className="connect">
        <select
          aria-label="Connection type"
          value={selectValue}
          disabled={active}
          onFocus={refreshPorts}
          onMouseDown={refreshPorts}
          onChange={(event) => onSelectKind(event.target.value)}
        >
          <option value="auto">AUTO</option>
          {knownSaved && <option value={`serial:${settings.serialPort}`}>{settings.serialPort}</option>}
          {ports.map((p) => (
            <option key={p.name} value={`serial:${p.name}`}>
              {p.name} — {p.description}
            </option>
          ))}
          <option value="tcp">TCP</option>
          <option value="udp">UDP</option>
          <option value="udpcl">UDPCl</option>
          <option value="ws">WS</option>
          <option value="replay">Replay (log)</option>
        </select>

        {settings.kind === "serial" && (
          <select aria-label="Baud rate" className="baud" disabled={active} value={settings.baud} onChange={(e) => update({ baud: Number(e.target.value) })}>
            {BAUDS.map((b) => (
              <option key={b} value={b}>
                {b}
              </option>
            ))}
          </select>
        )}
        {settings.kind === "tcp" && (
          <>
            <input className="host" aria-label="TCP host" disabled={active} value={settings.tcpHost} onChange={(e) => update({ tcpHost: e.target.value })} placeholder="host" />
            <input className="port" aria-label="TCP port" disabled={active} value={settings.tcpPort} onChange={(e) => update({ tcpPort: e.target.value })} placeholder="port" />
          </>
        )}
        {settings.kind === "udp" && (
          <input className="port" aria-label="UDP listen port" disabled={active} value={settings.udpPort} onChange={(e) => update({ udpPort: e.target.value })} placeholder="port" />
        )}
        {settings.kind === "udpcl" && (
          <>
            <input className="host" aria-label="UDP remote host" disabled={active} value={settings.udpclHost} onChange={(e) => update({ udpclHost: e.target.value })} placeholder="host" />
            <input className="port" aria-label="UDP remote port" disabled={active} value={settings.udpclPort} onChange={(e) => update({ udpclPort: e.target.value })} placeholder="port" />
          </>
        )}
        {settings.kind === "ws" && (
          <input className="url" aria-label="WebSocket URL" disabled={active} value={settings.wsUrl} onChange={(e) => update({ wsUrl: e.target.value })} placeholder="ws://host:port/path" />
        )}
        {settings.kind === "replay" && (
          <input className="url" aria-label="Log file path" disabled={active} value={settings.replayPath} onChange={(e) => update({ replayPath: e.target.value })} placeholder="path to .BIN log" />
        )}

        <button className="primary" disabled={busy || link.state === "connecting"} onClick={toggle}>
          {link.state === "connecting" ? "Connecting…" : active ? "Disconnect" : "Connect"}
        </button>
      </div>

      <span className={`link-pill ${pillClass}`} title={link.description}>
        <i />
        {pillText}
      </span>

      {telemetry.vehicles.length > 1 && (
        <select
          aria-label="Vehicle"
          title="Vehicle / component that commands, parameters and telemetry refer to"
          value={`${telemetry.vehicle?.systemId}:${telemetry.vehicle?.componentId}`}
          onChange={(event) => {
            const [sys, comp] = event.target.value.split(":").map(Number);
            api.selectVehicle(sys, comp).catch((e) => setLocalError(String(e)));
          }}
        >
          {telemetry.vehicles.map((v) => (
            <option key={`${v.systemId}:${v.componentId}`} value={`${v.systemId}:${v.componentId}`}>
              {vehicleLabel(v)}
            </option>
          ))}
        </select>
      )}

      {link.state === "connected" && (
        <span className="stats" title={`${stats.packetsRx} packets, ${stats.bytesRx} bytes received, ${stats.badCrc} bad CRC`}>
          {stats.packetsPerSecond} pkt/s · lost {stats.packetsLost} · {Math.round(stats.qualityPercent)}%
        </span>
      )}
      {error && (
        <span className="error-line" title={error}>
          {error}
        </span>
      )}
    </>
  );
}
