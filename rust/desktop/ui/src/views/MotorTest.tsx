import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { ConfirmDialog } from "../components/ConfirmDialog";
import type { MotorFrame, MotorTestRequest, MotorThrottleType, SpinParam, Telemetry } from "../types";
import { useMotorTest } from "../useMotorTest";
import { useParams } from "../useParams";
import "../motortest.css";

type Props = {
  telemetry: Telemetry;
  active: boolean;
  notify: (text: string, ok: boolean) => void;
};

/** Above this percentage the backend refuses unless the request says the user confirmed it. */
const HIGH_PERCENT = 20;
/** Above this percentage a changed throttle asks for a new confirmation. */
const CONFIRM_PERCENT = 10;
const DIAGRAMS = "https://ardupilot.org/copter/docs/connect-escs-and-motors.html#motor-order-diagrams";

type Pending = { title: string; message: string; danger: boolean; run: (allowHigh: boolean) => void };

const fmt = (value: number | null, digits: number, unit: string) => (value === null ? "—" : `${value.toFixed(digits)} ${unit}`);

export function MotorTest({ telemetry, active, notify }: Props) {
  const connected = telemetry.connected;
  const state = useMotorTest(active && connected);
  const params = useParams();
  const [frame, setFrame] = useState<MotorFrame | null>(null);
  const [throttleType, setThrottleType] = useState<MotorThrottleType>("percent");
  const [throttle, setThrottle] = useState("5");
  const [seconds, setSeconds] = useState("2");
  const [spinArm, setSpinArm] = useState("");
  const [spinMin, setSpinMin] = useState("");
  // Only used for frames that are not in the layout table.
  const [genericCount, setGenericCount] = useState("4");
  const [propsOff, setPropsOff] = useState(false);
  const [confirmedThrottle, setConfirmedThrottle] = useState<number | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [busy, setBusy] = useState(false);

  const frameClass = params.byName.get("FRAME_CLASS")?.value ?? params.byName.get("Q_FRAME_CLASS")?.value ?? null;
  const frameType = params.byName.get("FRAME_TYPE")?.value ?? params.byName.get("Q_FRAME_TYPE")?.value ?? null;

  useEffect(() => {
    if (!connected) {
      setFrame(null);
      return;
    }
    let alive = true;
    api
      .motorFrame(frameClass, frameType, Number.isInteger(Number(genericCount)) && genericCount.trim() !== "" ? Number(genericCount) : null)
      .then((next) => alive && setFrame(next))
      .catch(() => alive && setFrame(null));
    return () => {
      alive = false;
    };
  }, [connected, frameClass, frameType, genericCount]);

  // Show the vehicle's current MOT_SPIN_* values until the user types their own.
  const currentArm = params.byName.get("MOT_SPIN_ARM")?.value;
  const currentMin = params.byName.get("MOT_SPIN_MIN")?.value;
  useEffect(() => {
    if (currentArm !== undefined) setSpinArm(String(Number((currentArm * 100).toFixed(1))));
  }, [currentArm]);
  useEffect(() => {
    if (currentMin !== undefined) setSpinMin(String(Number((currentMin * 100).toFixed(1))));
  }, [currentMin]);

  const count = frame?.motors.length ?? 0;
  const throttleValue = Number(throttle);
  const secondsValue = Number(seconds);
  const percent = throttleType === "percent";
  const inputsValid =
    Number.isFinite(secondsValue) &&
    secondsValue > 0 &&
    (throttleType === "pilot" || (Number.isFinite(throttleValue) && throttle.trim() !== ""));

  const blockedReason = !connected
    ? "Connect to a vehicle first."
    : telemetry.armed
      ? "The vehicle is armed. Disarm it before testing motors (Stop still works)."
      : frame === null
        ? "Reading the frame…"
        : null;
  const canTest = blockedReason === null && inputsValid && !busy && !state.running;

  function request(motor: number, allowHigh: boolean): MotorTestRequest {
    return { motor, throttleType, throttle: throttleType === "pilot" ? 0 : throttleValue, seconds: secondsValue, allowHigh };
  }

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

  /** Every spin goes through here: props-off acknowledgement first, then a high-throttle confirmation. */
  function guarded(label: string, what: string, call: (allowHigh: boolean) => Promise<string>) {
    const high = percent && throttleValue > HIGH_PERCENT;
    const needsThrottleCheck = percent && throttleValue > CONFIRM_PERCENT && confirmedThrottle !== throttleValue;
    const go = (allowHigh: boolean) => {
      if (percent) setConfirmedThrottle(throttleValue);
      void run(label, () => call(allowHigh));
    };
    if (!propsOff || needsThrottleCheck) {
      const parts: string[] = [];
      if (!propsOff) parts.push("Remove ALL propellers and secure the vehicle: the motors will spin.");
      if (needsThrottleCheck) {
        parts.push(`The throttle is ${throttleValue}%${high ? `, above ${HIGH_PERCENT}%` : ""}. Confirm that this is intended.`);
      }
      setPending({
        title: what,
        message: parts.join(" "),
        danger: true,
        run: (allowHigh) => {
          setPropsOff(true);
          go(allowHigh);
        },
      });
      return;
    }
    go(false);
  }

  const startOne = (order: number, name: string) =>
    guarded(`Motor ${name}`, `Test motor ${name}?`, (allowHigh) => api.motorStart(request(order, allowHigh)));
  const startAll = () => guarded("All motors", "Spin all motors?", (allowHigh) => api.motorStartAll(request(0, allowHigh), count));
  const startSequence = () =>
    guarded("Sequence", "Test the motors in sequence?", (allowHigh) => api.motorStartSequence(request(0, allowHigh), count));

  function writeSpin(which: SpinParam, text: string) {
    const value = Number(text);
    void run(`MOT_SPIN_${which.toUpperCase()}`, () => api.motorSetSpin(which, value));
  }

  const escByOutput = useMemo(() => new Map(state.escs.map((e) => [e.index, e])), [state.escs]);
  const statusClass = state.running ? "warn-text" : "dim";

  return (
    <div className="motor-view">
      <div className="safety-banner" role="note">
        <strong>Props off. Vehicle secured.</strong> The motors spin at the throttle you pick. Hold the vehicle down and never
        test with propellers attached.
      </div>

      <div className="motor-top">
        <button className="danger stop-all" disabled={!connected || busy} onClick={() => void run("Stop", () => api.motorStopAll(Math.max(count, 4)))}>
          ■ STOP ALL MOTORS
        </button>
        <span className={`status-line ${statusClass}`} role="status">
          {blockedReason ? <span className="warn-text">{blockedReason}</span> : state.message || "Ready."}
          {state.running && <span className="badge warn"> SPINNING</span>}
        </span>
      </div>

      <div className="motor-grid">
        <section className="panel motor-card">
          <header>
            <h2>Test</h2>
            <span className="dim">
              {frame?.known ? `Class ${frame.className}, type ${frame.typeName}` : ""}
            </span>
          </header>
          <div className="motor-body">
            {frame && !frame.known && (
              <div className="motor-generic">
                <p className="warn-text">
                  {frameClass === null
                    ? "This vehicle has no FRAME_CLASS parameter."
                    : `Frame class ${frameClass}, type ${frameType ?? "?"} is not in the layout table.`}{" "}
                  Motors are generic: the letters follow the test order, but the output number and rotation are unknown.
                </p>
                <label>
                  Number of motors (1 to 12)
                  <input
                    type="number"
                    min={1}
                    max={12}
                    step={1}
                    value={genericCount}
                    onChange={(e) => setGenericCount(e.target.value)}
                  />
                </label>
              </div>
            )}
            <div className="motor-controls">
              <label>
                Throttle type
                <select value={throttleType} onChange={(e) => setThrottleType(e.target.value as MotorThrottleType)}>
                  <option value="percent">Percent</option>
                  <option value="pwm">PWM (µs)</option>
                  <option value="pilot">Pilot stick</option>
                </select>
              </label>
              <label>
                {throttleType === "pwm" ? "Throttle (µs)" : "Throttle (%)"}
                <input
                  type="number"
                  value={throttle}
                  min={throttleType === "pwm" ? 800 : 0}
                  max={throttleType === "pwm" ? 2200 : 100}
                  disabled={throttleType === "pilot"}
                  onChange={(e) => setThrottle(e.target.value)}
                />
              </label>
              <label>
                Duration (s)
                <input type="number" value={seconds} min={1} max={120} onChange={(e) => setSeconds(e.target.value)} />
              </label>
            </div>
            {percent && throttleValue > HIGH_PERCENT && <p className="warn-text">High throttle: you will be asked to confirm it.</p>}

            <div className="motor-buttons">
              {(frame?.motors ?? []).map((slot) => (
                <button key={slot.testOrder} disabled={!canTest} onClick={() => startOne(slot.testOrder, slot.label)}>
                  <strong>Test motor {slot.label}</strong>
                  <small>
                    {slot.output !== null ? `Motor ${slot.output}` : "Motor ?"}
                    {slot.rotation ? `, ${slot.rotation}` : ""}
                  </small>
                </button>
              ))}
            </div>
            <div className="row motor-actions">
              <button disabled={!canTest || count === 0} onClick={startAll}>
                Test all motors
              </button>
              <button disabled={!canTest || count === 0} onClick={startSequence}>
                Test all in sequence
              </button>
            </div>
            <p className="dim motor-note">
              Motors are tested in the ArduPilot test order A, B, C… Motor order diagrams: <span className="mono">{DIAGRAMS}</span>
            </p>
          </div>
        </section>

        <section className="panel motor-card">
          <header>
            <h2>ESC telemetry</h2>
            <span className="dim">{state.escs.length > 0 ? "live" : ""}</span>
          </header>
          <div className="motor-body">
            {state.escs.length === 0 ? (
              <p className="dim">
                No ESC telemetry. It appears when the ESCs report it (DShot telemetry, BLHeli passthrough or DroneCAN ESCs).
              </p>
            ) : (
              <table className="esc-table">
                <thead>
                  <tr>
                    <th>ESC</th>
                    <th>RPM</th>
                    <th>Voltage</th>
                    <th>Current</th>
                    <th>Temp</th>
                  </tr>
                </thead>
                <tbody>
                  {[...escByOutput.values()].map((e) => (
                    <tr key={e.index}>
                      <td className="mono">{e.index}</td>
                      <td className="mono">{e.rpm ?? "—"}</td>
                      <td className="mono">{fmt(e.voltage, 1, "V")}</td>
                      <td className="mono">{fmt(e.current, 1, "A")}</td>
                      <td className="mono">{e.temperature === null ? "—" : `${e.temperature} °C`}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </section>

        <section className="panel motor-card">
          <header>
            <h2>Spin thresholds</h2>
          </header>
          <div className="motor-body">
            <p className="dim">
              Set the minimum output (%) when armed on the ground (MOT_SPIN_ARM) and when flying (MOT_SPIN_MIN). Keep MOT_SPIN_ARM
              below MOT_SPIN_MIN. The vehicle must be disarmed.
            </p>
            <div className="spin-row">
              <label>
                MOT_SPIN_ARM (%)
                <input type="number" step="0.5" min={0} max={30} value={spinArm} onChange={(e) => setSpinArm(e.target.value)} />
              </label>
              <button disabled={!connected || telemetry.armed || busy || spinArm.trim() === ""} onClick={() => writeSpin("arm", spinArm)}>
                Set
              </button>
            </div>
            <div className="spin-row">
              <label>
                MOT_SPIN_MIN (%)
                <input type="number" step="0.5" min={0} max={30} value={spinMin} onChange={(e) => setSpinMin(e.target.value)} />
              </label>
              <button disabled={!connected || telemetry.armed || busy || spinMin.trim() === ""} onClick={() => writeSpin("min", spinMin)}>
                Set
              </button>
            </div>
          </div>
        </section>
      </div>

      {pending && (
        <ConfirmDialog
          title={pending.title}
          message={pending.message}
          tone={pending.danger ? "danger" : "warn"}
          confirmLabel="Spin the motors"
          onConfirm={() => {
            const { run: go } = pending;
            setPending(null);
            go(percent && throttleValue > HIGH_PERCENT);
          }}
          onCancel={() => setPending(null)}
        />
      )}
    </div>
  );
}
