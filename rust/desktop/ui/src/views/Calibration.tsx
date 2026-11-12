/**
 * Calibration screen: accel, compass, radio, ESC, gyro and baro. State comes from useCalibration; actions call api.calibration*.
 * Safety: starts are blocked while armed, and ESC/gyro/baro go through a ConfirmDialog (props-off warning).
 */
import { useState } from "react";
import { api } from "../api";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { VehiclePose } from "../components/VehiclePose";
import type { CalibrationKind, StatusText, Telemetry } from "../types";
import { useCalibration } from "../useCalibration";

type Props = {
  telemetry: Telemetry;
  messages: StatusText[];
  notify: (text: string, ok: boolean) => void;
};

const ACCEL_STEPS = ["Level", "Left", "Right", "Nose down", "Nose up", "Back"];
// Vehicle STATUSTEXT lines matching this are shown in the messages panel.
const RELEVANT = /calib|accel|compass|mag|gyro|baro|esc|radio|level|rc\d|position|offset|fitness/i;
// Typical PWM range (us) mapped onto the radio bars.
const RC_MIN = 800;
const RC_MAX = 2200;

type Confirm = { kind: CalibrationKind; title: string; message: string; danger: boolean };

function formatTime(ms: number) {
  return new Date(ms).toLocaleTimeString([], { hour12: false });
}

export function Calibration({ telemetry, messages, notify }: Props) {
  const cal = useCalibration();
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState<Confirm | null>(null);

  const active = cal.state === "running" || cal.state === "waiting";
  // Calibration is refused while disconnected or armed.
  const blockedReason = !telemetry.connected
    ? "Connect to a vehicle first."
    : telemetry.armed
      ? "The vehicle is armed. Disarm it before calibrating."
      : null;
  const canStart = blockedReason === null && !active && !busy;
  const kindIs = (...kinds: string[]) => cal.kind !== null && kinds.includes(cal.kind);

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

  const start = (kind: CalibrationKind, label: string) => run(label, () => api.calibrationStart(kind));

  function ask(kind: CalibrationKind, title: string, message: string, danger = false) {
    setConfirm({ kind, title, message, danger });
  }

  const recent = messages.filter((m) => RELEVANT.test(m.text)).slice(-8).reverse();
  // While radio calibration runs show the backend's recorded min/max; otherwise a live view of channels 1-8.
  const radioRows =
    kindIs("radio") && cal.radio.length > 0
      ? cal.radio
      : Array.from({ length: 8 }, (_, i) => {
          const value = telemetry.fields[`ch${i + 1}in`] ?? 0;
          return { channel: i + 1, min: value, max: value, value };
        });
  const pct = (us: number) => `${Math.max(0, Math.min(100, ((us - RC_MIN) / (RC_MAX - RC_MIN)) * 100))}%`;

  const stateClass = cal.state === "succeeded" ? "ok-text" : cal.state === "failed" ? "err-text" : "dim";

  return (
    <div className="calibration-view">
      <div className="safety-banner" role="note">
        <strong>Props off. Vehicle secured.</strong> Calibrations move motors, sensors or ESC outputs. Never calibrate with
        propellers attached.
      </div>
      {blockedReason && <p className="warn-text status-line">{blockedReason}</p>}
      {(cal.state !== "idle" || cal.message !== "") && (
        <p className={`status-line ${stateClass}`} role="status">
          {cal.state === "idle" ? (
            cal.message
          ) : (
            <>
              {cal.kind ? `${cal.kind.replace("_", " ")}: ` : ""}
              {cal.state}
              {cal.message ? ` — ${cal.message}` : ""}
            </>
          )}
          {active && (
            <>
              {" "}
              <button disabled={busy} onClick={() => void run("Cancel", () => api.calibrationCancel())}>
                Cancel calibration
              </button>
            </>
          )}
        </p>
      )}

      <div className="cal-grid">
        <section className="panel cal-card wide">
          <header>
            <h2>Accelerometer</h2>
          </header>
          <div className="cal-body">
            <div className="row">
              <button disabled={!canStart} onClick={() => void start("accel_full", "Accelerometer")}>
                Full calibration (6 positions)
              </button>
              <button disabled={!canStart} onClick={() => void start("accel_level", "Level")}>
                Level only
              </button>
              <button disabled={!canStart} onClick={() => void start("accel_simple", "Simple accelerometer")}>
                Simple
              </button>
            </div>
            {kindIs("accel_full", "accel_level", "accel_simple") && active && (
              <div className="accel-flow">
                <ol className="steps">
                  {ACCEL_STEPS.map((name, i) => {
                    const current = cal.accelPosition?.name === name;
                    const done = cal.accelPosition ? i + 1 < cal.accelPosition.index : false;
                    return (
                      <li key={name} className={current ? "current" : done ? "done" : ""}>
                        {name}
                      </li>
                    );
                  })}
                </ol>
                {cal.accelPosition && <VehiclePose name={cal.accelPosition.name} />}
                <button
                  className="primary"
                  disabled={cal.state !== "waiting" || busy}
                  onClick={() => void run("Position confirmed", () => api.calibrationAccelConfirm())}
                >
                  Done — vehicle is in position
                </button>
              </div>
            )}
          </div>
        </section>

        <section className="panel cal-card wide">
          <header>
            <h2>Compass</h2>
          </header>
          <div className="cal-body">
            <div className="row">
              <button disabled={!canStart} onClick={() => void start("compass", "Compass")}>
                Start onboard calibration
              </button>
              <button
                className="ok"
                disabled={!(kindIs("compass") && active) || busy}
                onClick={() => void run("Accept", () => api.calibrationCompassAccept())}
              >
                Accept
              </button>
              <button
                className="danger"
                disabled={!(kindIs("compass") && active) || busy}
                onClick={() => void run("Cancel", () => api.calibrationCompassCancel())}
              >
                Cancel
              </button>
            </div>
            <p className="dim">Rotate the vehicle slowly around all axes until every compass reaches 100%.</p>
            {kindIs("compass") && cal.compass.length > 0 && (
              <ul className="compass-list">
                {cal.compass.map((c) => (
                  <li key={c.compassId}>
                    <span>Compass {c.compassId + 1}</span>
                    <div className="progress small" role="progressbar" aria-valuenow={Math.round(c.completionPct)} aria-valuemin={0} aria-valuemax={100}>
                      <div style={{ width: `${c.completionPct}%` }} />
                      <span>{Math.round(c.completionPct)}%</span>
                    </div>
                    <span className="dim">{c.status}</span>
                    {c.fitness !== null && <span className="mono">fitness {c.fitness.toFixed(1)}</span>}
                    {c.autosaved && <span className="badge ok">saved</span>}
                  </li>
                ))}
              </ul>
            )}
          </div>
        </section>

        <section className="panel cal-card wide">
          <header>
            <h2>Radio</h2>
          </header>
          <div className="cal-body">
            <div className="row">
              <button disabled={!canStart} onClick={() => void start("radio", "Radio")}>
                Start radio calibration
              </button>
              <button
                className="primary"
                disabled={!(kindIs("radio") && active) || busy}
                onClick={() => void run("Radio", () => api.radioCalibrationFinish())}
              >
                Finish — save min/max/trim
              </button>
            </div>
            <p className="dim">
              Transmitter on, receiver connected. Start, move every stick and switch to both extremes, centre the sticks, then
              press Finish.
            </p>
            <ul className="radio-list">
              {radioRows.map((row) => (
                <li key={row.channel}>
                  <span className="mono">CH{row.channel}</span>
                  <div className="rc-bar" aria-label={`Channel ${row.channel}`}>
                    {row.max > row.min && <div className="rc-range" style={{ left: pct(row.min), width: `calc(${pct(row.max)} - ${pct(row.min)})` }} />}
                    <div className="rc-marker" style={{ left: pct(row.value) }} />
                  </div>
                  <span className="mono dim">
                    {row.value ? `${Math.round(row.value)}` : "—"}
                    {row.max > row.min ? ` [${Math.round(row.min)}–${Math.round(row.max)}]` : ""}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        </section>

        <section className="panel cal-card">
          <header>
            <h2>ESC</h2>
          </header>
          <div className="cal-body">
            <p className="dim">Sets ESC_CALIBRATION so the ESCs calibrate at the next power-up.</p>
            <button
              className="danger"
              disabled={!canStart}
              onClick={() =>
                ask(
                  "esc",
                  "ESC calibration",
                  "Remove ALL propellers. The ESC calibration spins motors at the next power-up. Power-cycle the vehicle with the battery after confirming.",
                  true,
                )
              }
            >
              ESC calibration…
            </button>
          </div>
        </section>

        <section className="panel cal-card">
          <header>
            <h2>Gyro</h2>
          </header>
          <div className="cal-body">
            <p className="dim">Keep the vehicle completely still while the gyros are zeroed.</p>
            <button disabled={!canStart} onClick={() => ask("gyro", "Calibrate gyros?", "Keep the vehicle still on a level surface.")}>
              Calibrate gyro
            </button>
          </div>
        </section>

        <section className="panel cal-card">
          <header>
            <h2>Barometer</h2>
          </header>
          <div className="cal-body">
            <p className="dim">Resets the ground pressure reference. Do it before flying.</p>
            <button disabled={!canStart} onClick={() => ask("baro", "Calibrate barometer?", "Keep the vehicle still and out of wind.")}>
              Calibrate barometer
            </button>
          </div>
        </section>
      </div>

      <section className="panel cal-messages">
        <header>
          <h2>Vehicle messages</h2>
        </header>
        <ul className="message-list">
          {recent.length === 0 && <li className="dim">No calibration messages yet.</li>}
          {recent.map((m) => (
            <li key={`${m.timeMs}-${m.text}`} className={m.severity <= 3 ? "sev-error" : m.severity === 4 ? "sev-warn" : ""}>
              <time>{formatTime(m.timeMs)}</time> {m.text}
            </li>
          ))}
        </ul>
      </section>

      {confirm && (
        <ConfirmDialog
          title={confirm.title}
          message={confirm.message}
          tone={confirm.danger ? "danger" : "warn"}
          confirmLabel="Start"
          onConfirm={() => {
            const { kind, title } = confirm;
            setConfirm(null);
            void start(kind, title);
          }}
          onCancel={() => setConfirm(null)}
        />
      )}
    </div>
  );
}
