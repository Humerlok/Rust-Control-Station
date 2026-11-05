import { useCallback, useEffect, useRef, useState } from "react";
import { isMockBackend } from "./api";
import { ConnectBar } from "./components/ConnectBar";
import { RebootButton } from "./components/RebootButton";
import { paramStore } from "./useParams";
import { useLive } from "./useLive";
import { Calibration } from "./views/Calibration";
import { ExtendedTuning } from "./views/ExtendedTuning";
import { DroneCan } from "./views/DroneCan";
import { FlightData } from "./views/FlightData";
import { FlightPlan } from "./views/FlightPlan";
import { Inspector } from "./views/Inspector";
import { Firmware } from "./views/Firmware";
import { MavFtp } from "./views/MavFtp";
import { MotorTest } from "./views/MotorTest";
import { Parameters } from "./views/Parameters";
import { Placeholder } from "./views/Placeholder";

// Screens that stay mounted while hidden so unsaved edits and scroll positions survive navigation.
const LIVE_VIEWS = ["flight-data", "flight-plan", "calibration", "parameters", "ext-tuning", "mavftp", "inspector", "firmware", "motor-test", "dronecan"];

type NavItem = { id: string; label: string };
const NAV: Array<{ group: string; items: NavItem[] }> = [
  { group: "Flight", items: [{ id: "flight-data", label: "Flight Data" }, { id: "flight-plan", label: "Flight Plan" }] },
  {
    group: "Setup",
    items: [
      { id: "calibration", label: "Calibration" },
      { id: "motor-test", label: "Motor Test" },
      { id: "dronecan", label: "DroneCAN" },
      { id: "firmware", label: "Firmware" },
    ],
  },
  {
    group: "Config",
    items: [
      { id: "parameters", label: "Parameters" },
      { id: "ext-tuning", label: "Extended Tuning" },
      { id: "mavftp", label: "MAVFTP" },
    ],
  },
  { group: "Tools", items: [{ id: "inspector", label: "Inspector" }] },
];

type Toast = { text: string; ok: boolean; id: number };

export function App() {
  const { telemetry, link, messages, clearMessages } = useLive();
  const [view, setView] = useState("flight-data");
  const [toast, setToast] = useState<Toast | null>(null);
  const toastId = useRef(0);

  const notify = useCallback((text: string, ok: boolean) => {
    toastId.current += 1;
    setToast({ text, ok, id: toastId.current });
  }, []);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 5000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  const label = NAV.flatMap((g) => g.items).find((i) => i.id === view)?.label ?? "";
  const connected = link.state === "connected" && telemetry.connected;

  useEffect(() => {
    paramStore.setConnection(link.state === "connected", telemetry.connected, telemetry.vehicleType);
  }, [link.state, telemetry.connected, telemetry.vehicleType]);

  return (
    <div className="shell">
      <header className="topbar">
        <div className="brand">
          <span>ARDUPILOT</span>
          <strong>Control Station</strong>
        </div>
        <ConnectBar link={link} telemetry={telemetry} />
        <div className="spacer" />
        {isMockBackend && <span className="badge warn" title="No Tauri backend: showing simulated data">SIMULATED</span>}
        <RebootButton connected={connected} armed={telemetry.armed} notify={notify} />
      </header>

      <nav className="nav" aria-label="Screens">
        {NAV.map((group) => (
          <div key={group.group}>
            <div className="group">{group.group}</div>
            {group.items.map((item) => (
              <button key={item.id} className={view === item.id ? "active" : ""} onClick={() => setView(item.id)}>
                {item.label}
              </button>
            ))}
          </div>
        ))}
      </nav>

      <main className="content">
        <div className="view" hidden={view !== "flight-data"}>
          <FlightData telemetry={telemetry} messages={messages} onClearMessages={clearMessages} notify={notify} />
        </div>
        <div className="view" hidden={view !== "flight-plan"}>
          <FlightPlan telemetry={telemetry} notify={notify} />
        </div>
        <div className="view" hidden={view !== "mavftp"}>
          <MavFtp connected={connected} notify={notify} />
        </div>
        <div className="view" hidden={view !== "firmware"}>
          <Firmware connected={connected} notify={notify} />
        </div>
        <div className="view" hidden={view !== "inspector"}>
          <Inspector active={view === "inspector"} notify={notify} />
        </div>
        <div className="view" hidden={view !== "calibration"}>
          <Calibration telemetry={telemetry} messages={messages} notify={notify} />
        </div>
        <div className="view" hidden={view !== "motor-test"}>
          <MotorTest telemetry={telemetry} active={view === "motor-test"} notify={notify} />
        </div>
        <div className="view" hidden={view !== "dronecan"}>
          <DroneCan telemetry={telemetry} notify={notify} />
        </div>
        <div className="view" hidden={view !== "parameters"}>
          <Parameters notify={notify} />
        </div>
        <div className="view" hidden={view !== "ext-tuning"}>
          <ExtendedTuning telemetry={telemetry} notify={notify} />
        </div>
        {!LIVE_VIEWS.includes(view) && <Placeholder title={label} />}
      </main>

      {toast && (
        <div key={toast.id} className={`toast ${toast.ok ? "" : "err"}`} role="status">
          {toast.text}
        </div>
      )}
    </div>
  );
}
