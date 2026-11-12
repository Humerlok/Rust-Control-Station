/**
 * "Vehicle logs" dialog of SpeedLOG: lists the connected vehicle's onboard logs (MAVFTP /APM/LOGS when
 * the vehicle offers it, LOG_REQUEST_LIST otherwise), then opens one in the plotter or saves a copy,
 * with a progress bar. The protocol work is in crates/link (LogService).
 */
import { useCallback, useEffect, useState } from "react";
import { api } from "../../api";
import type { SpeedLogInfo, VehicleLogEntry, VehicleLogProgress } from "../../types";
import { formatBytes } from "./speedlogUtils";

type Props = {
  connected: boolean;
  onClose: () => void;
  onOpened: (info: SpeedLogInfo) => void;
  notify: (text: string, ok: boolean) => void;
};

function formatDate(seconds: number | null): string {
  if (!seconds) return "—";
  return new Date(seconds * 1000).toLocaleString();
}

export function VehicleLogsDialog({ connected, onClose, onOpened, notify }: Props) {
  const [entries, setEntries] = useState<VehicleLogEntry[] | null>(null);
  const [progress, setProgress] = useState<VehicleLogProgress | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    api
      .onLogsProgress((p) => setProgress(p))
      .then((u) => {
        if (gone) u();
        else unlisten = u;
      })
      .catch(() => undefined);
    return () => {
      gone = true;
      unlisten?.();
    };
  }, []);

  const refresh = useCallback(async () => {
    setError(null);
    setBusy("Listing the logs…");
    try {
      setEntries(await api.logsList());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }, []);

  useEffect(() => {
    if (connected) void refresh();
  }, [connected, refresh]);

  const open = async (entry: VehicleLogEntry) => {
    setError(null);
    setBusy(`Downloading ${entry.name}…`);
    try {
      const info = await api.logsOpen(entry);
      onOpened(info);
      onClose();
    } catch (e) {
      if (String(e).includes("cancelled")) notify("Download cancelled", false);
      else setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const save = async (entry: VehicleLogEntry) => {
    setError(null);
    setBusy(`Downloading ${entry.name}…`);
    try {
      const path = await api.logsSave(entry);
      if (path) notify(`Saved ${path}`, true);
    } catch (e) {
      if (String(e).includes("cancelled")) notify("Download cancelled", false);
      else setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const downloading = busy !== null && progress?.op === "download" && !progress.finished;
  const fraction = downloading && progress && progress.total > 0 ? progress.done / progress.total : 0;
  // Newest first: the last log is usually the flight the user wants.
  const rows = entries ? [...entries].reverse() : [];

  return (
    <div className="sl-dialog-backdrop" role="presentation" onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <div className="panel sl-dialog" role="dialog" aria-modal="true" aria-label="Vehicle logs">
        <header>
          <h2>Vehicle logs</h2>
          <div className="sl-dialog-tools">
            <button onClick={refresh} disabled={!connected || !!busy}>
              Refresh
            </button>
            <button onClick={onClose} disabled={!!busy} aria-label="Close">
              ×
            </button>
          </div>
        </header>
        <div className="sl-dialog-body">
          {!connected && <p className="dim">Connect to a vehicle to list its onboard logs.</p>}
          {error && <p className="sl-error">{error}</p>}
          {busy && (
            <div className="sl-progress">
              <div className="sl-progress-text">
                <span>{busy}</span>
                {downloading && progress && (
                  <span className="dim mono">
                    {formatBytes(progress.done)} / {formatBytes(progress.total)} · {formatBytes(progress.rate)}/s ·{" "}
                    {progress.method === "ftp" ? "MAVFTP" : "MAVLink LOG"}
                  </span>
                )}
              </div>
              {downloading && (
                <div className="sl-progress-bar" aria-hidden>
                  <div style={{ width: `${Math.round(fraction * 100)}%` }} />
                </div>
              )}
              {downloading && (
                <button onClick={() => api.logsCancel()} className="danger">
                  Cancel
                </button>
              )}
            </div>
          )}
          {connected && entries && entries.length === 0 && !busy && <p className="dim">The vehicle has no logs.</p>}
          {rows.length > 0 && (
            <div className="sl-table-scroll sl-dialog-table">
              <table className="sl-table">
                <thead>
                  <tr>
                    <th>Log</th>
                    <th>Date</th>
                    <th className="num">Size</th>
                    <th>Via</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {rows.map((entry) => (
                    <tr key={entry.name}>
                      <td className="mono">{entry.name}</td>
                      <td>{formatDate(entry.timeUtc)}</td>
                      <td className="num mono">{formatBytes(entry.size)}</td>
                      <td className="dim">{entry.source.type === "ftp" ? "MAVFTP" : "LOG"}</td>
                      <td className="num sl-row-actions">
                        <button className="primary" disabled={!!busy || !connected} onClick={() => open(entry)}>
                          Open
                        </button>
                        <button disabled={!!busy || !connected} onClick={() => save(entry)}>
                          Save…
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
