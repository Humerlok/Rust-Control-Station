import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";

type ParameterValue = { Int: number } | { Float: number } | { Text: string };
type Parameter = { name: string; value: ParameterValue; component_id: number };
type Telemetry = { vehicle: { system_id: number; component_id: number }; connected: boolean; mode?: string; armed: boolean; groundspeed_m_s?: number; battery_percent?: number };
type CalibrationKind = "Accelerometer" | "Compass" | "Gyroscope" | "Level" | "Pressure" | "Radio" | "Esc";

const calibrationKinds: CalibrationKind[] = ["Accelerometer", "Compass", "Gyroscope", "Level", "Pressure", "Radio", "Esc"];
const valueText = (value: ParameterValue) => Object.values(value)[0]?.toString() ?? "";

function App() {
  const [telemetry, setTelemetry] = useState<Telemetry>();
  const [parameters, setParameters] = useState<Parameter[]>([]);
  const [query, setQuery] = useState("");
  const [notice, setNotice] = useState("Ready to connect to an ArduPilot vehicle.");

  const refresh = async () => {
    const [nextTelemetry, nextParameters] = await Promise.all([invoke<Telemetry>("telemetry"), invoke<Parameter[]>("parameters")]);
    setTelemetry(nextTelemetry); setParameters(nextParameters);
  };
  useEffect(() => { void refresh(); const timer = window.setInterval(() => void refresh(), 1000); return () => window.clearInterval(timer); }, []);
  const visibleParameters = useMemo(() => parameters.filter((p) => p.name.toLowerCase().includes(query.toLowerCase())), [parameters, query]);

  async function changeParameter(parameter: Parameter, value: string) {
    const numeric = Number(value); const typedValue: ParameterValue = Number.isFinite(numeric) ? { Float: numeric } : { Text: value };
    await invoke("set_parameter", { request: { target: telemetry?.vehicle, parameter: { ...parameter, value: typedValue } } });
    setNotice(`${parameter.name} sent to system ${telemetry?.vehicle.system_id ?? "?"}.`); await refresh();
  }
  async function calibrate(kind: CalibrationKind) { const session = await invoke<{ instruction: string }>("start_calibration", { kind }); setNotice(`${kind}: ${session.instruction}`); }

  return <main>
    <header><div><p className="eyebrow">ARDUPILOT CONTROL STATION</p><h1>Flight console</h1></div><div className={`connection ${telemetry?.connected ? "online" : "offline"}`}><span />{telemetry?.connected ? `Connected · SYS ${telemetry.vehicle.system_id}` : "Disconnected"}</div></header>
    <section className="metrics">
      <Metric label="Flight mode" value={telemetry?.mode ?? "Waiting for heartbeat"} />
      <Metric label="Armed" value={telemetry?.armed ? "ARMED" : "Safe"} warn={telemetry?.armed} />
      <Metric label="Ground speed" value={telemetry?.groundspeed_m_s ? `${telemetry.groundspeed_m_s.toFixed(1)} m/s` : "—"} />
      <Metric label="Battery" value={telemetry?.battery_percent ? `${telemetry.battery_percent}%` : "—"} />
    </section>
    <section className="layout">
      <article className="map panel"><div className="map-grid" /><div className="hud"><span>HDG —</span><strong>ATTITUDE</strong><span>ALT —</span></div><p className="attribution">Map source must be configured for offline use. © OpenStreetMap contributors</p></article>
      <aside className="panel status"><h2>Vehicle</h2><dl><dt>Target</dt><dd>SYS {telemetry?.vehicle.system_id ?? "—"} / COMP {telemetry?.vehicle.component_id ?? "—"}</dd><dt>Link</dt><dd>{telemetry?.connected ? "Heartbeat active" : "No connection"}</dd></dl><p className="notice">{notice}</p></aside>
    </section>
    <section className="workspace">
      <article className="panel parameters"><div className="panel-heading"><div><h2>Parameters</h2><p>Changes are sent immediately to the selected vehicle.</p></div><input aria-label="Search parameters" placeholder="Search parameters" value={query} onChange={(event) => setQuery(event.target.value)} /></div>
        <table><thead><tr><th>Name</th><th>Component</th><th>Value</th></tr></thead><tbody>{visibleParameters.map((parameter) => <tr key={`${parameter.component_id}-${parameter.name}`}><td>{parameter.name}</td><td>{parameter.component_id}</td><td><input defaultValue={valueText(parameter.value)} onBlur={(event) => void changeParameter(parameter, event.currentTarget.value)} /></td></tr>)}</tbody></table>
      </article>
      <article className="panel calibration"><h2>Calibration</h2><p>Run only with the vehicle secured and follow its audible or on-screen prompts.</p><div className="calibration-grid">{calibrationKinds.map((kind) => <button key={kind} onClick={() => void calibrate(kind)}>{kind}</button>)}</div></article>
    </section>
  </main>;
}
function Metric({ label, value, warn = false }: { label: string; value: string; warn?: boolean }) { return <article className={`metric ${warn ? "warning" : ""}`}><span>{label}</span><strong>{value}</strong></article>; }
createRoot(document.getElementById("root")!).render(<App />);
