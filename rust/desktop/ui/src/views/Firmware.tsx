import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import { ConfirmDialog } from "../components/ConfirmDialog";
import type { BoardInfo, BoardSummary, FirmwareBuild, FlashProgress, LoadedFirmware, SerialPortInfo } from "../types";
import "../firmware.css";

type Props = { connected: boolean; notify: (text: string, ok: boolean) => void };

const VEHICLES = ["Copter", "Plane", "Rover", "Sub", "Heli", "Blimp", "AntennaTracker"];
const CHANNELS = [
  { id: "OFFICIAL", label: "Stable" },
  { id: "BETA", label: "Beta" },
  { id: "DEV", label: "Latest dev" },
];
const PHASES: Record<string, string> = {
  sync: "Syncing with the bootloader",
  identify: "Reading the board",
  erase: "Erasing flash",
  program: "Programming",
  verify: "Verifying",
  reboot: "Rebooting the board",
  done: "Done",
};

// A board id that matches, or fmuv2 firmware on a board that reports 33 (same rule as the backend).
const compatible = (board: number, firmware: number) => board === firmware || (board === 33 && firmware === 9);

function toBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let text = "";
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(text);
}

type Pending = "bootloader" | "flash";

export function Firmware({ connected, notify }: Props) {
  const [boards, setBoards] = useState<BoardSummary[] | null>(null);
  const [boardId, setBoardId] = useState<number | null>(null);
  const [vehicle, setVehicle] = useState("Copter");
  const [channel, setChannel] = useState("OFFICIAL");
  const [builds, setBuilds] = useState<FirmwareBuild[]>([]);
  const [buildUrl, setBuildUrl] = useState("");
  const [loaded, setLoaded] = useState<LoadedFirmware | null>(null);
  const [ports, setPorts] = useState<SerialPortInfo[]>([]);
  const [port, setPort] = useState("");
  const [board, setBoard] = useState<BoardInfo | null>(null);
  const [progress, setProgress] = useState<FlashProgress | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [flashed, setFlashed] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let off: (() => void) | undefined;
    let cancelled = false;
    void api.onFlashProgress((p) => setProgress(p)).then((unlisten) => {
      if (cancelled) unlisten();
      else off = unlisten;
    });
    return () => {
      cancelled = true;
      off?.();
    };
  }, []);

  async function refreshPorts() {
    const list = await api.listSerialPorts();
    setPorts(list);
    setPort((current) => (list.some((p) => p.name === current) ? current : (list[0]?.name ?? "")));
  }
  useEffect(() => {
    void refreshPorts();
  }, []);

  // Builds for the chosen board, vehicle and channel (needs the catalog first).
  useEffect(() => {
    if (!boards || boardId === null) {
      setBuilds([]);
      return;
    }
    let stale = false;
    void api.firmwareBuilds({ boardId, vehicleType: vehicle, versionType: channel }).then((list) => {
      if (stale) return;
      const apj = list.filter((b) => b.format === "apj");
      setBuilds(apj);
      setBuildUrl(apj[0]?.url ?? "");
    });
    return () => {
      stale = true;
    };
  }, [boards, boardId, vehicle, channel]);

  async function run<T>(label: string, action: () => Promise<T>): Promise<T | undefined> {
    setBusy(label);
    setError(null);
    try {
      return await action();
    } catch (e) {
      setError(String(e));
      return undefined;
    } finally {
      setBusy(null);
    }
  }

  const loadCatalog = () =>
    run("catalog", async () => {
      const list = await api.firmwareCatalog();
      setBoards(list);
      setBoardId((current) => current ?? list[0]?.boardId ?? null);
    });

  const download = () =>
    run("download", async () => {
      setLoaded(await api.firmwareLoadUrl(buildUrl));
      setFlashed(false);
    });

  const pickFile = (file: File) =>
    run("file", async () => {
      setLoaded(await api.firmwareLoadFile(toBase64(await file.arrayBuffer())));
      setFlashed(false);
    });

  const identify = () =>
    run("identify", async () => {
      setBoard(await api.firmwareIdentify(port));
    });

  const rebootToBootloader = async () => {
    setPending(null);
    const done = await run("bootloader", async () => {
      await api.firmwareRebootBootloader();
      return true;
    });
    if (done) {
      notify("Rebooting into the bootloader. Pick the new serial port and identify the board.", true);
      setTimeout(() => void refreshPorts(), 3000);
    }
  };

  const flash = async () => {
    setPending(null);
    setProgress(null);
    setFlashed(false);
    const info = await run("flash", () => api.firmwareFlash(port));
    if (info) {
      setBoard(info);
      setFlashed(true);
      notify("Firmware installed. The board is rebooting.", true);
    }
  };

  const mismatch = loaded && board ? !compatible(board.boardId, loaded.boardId) : false;
  const canFlash = !!loaded && !!board && !mismatch && !busy && port !== "";
  const selectedBuild = useMemo(() => builds.find((b) => b.url === buildUrl), [builds, buildUrl]);
  const percent = progress && progress.total > 0 ? Math.min(100, (progress.done / progress.total) * 100) : 0;
  const flashing = busy === "flash";

  return (
    <div className="fw">
      <section className="fw-card">
        <h2>1. Firmware</h2>
        <div className="fw-row">
          <button disabled={!!busy} onClick={() => void loadCatalog()}>
            {busy === "catalog" ? "Loading..." : boards ? "Reload list" : "Load official list"}
          </button>
          <select value={boardId ?? ""} disabled={!boards} onChange={(e) => setBoardId(Number(e.target.value))} aria-label="Board">
            {!boards && <option value="">Board</option>}
            {boards?.map((b) => (
              <option key={`${b.platform}-${b.boardId}`} value={b.boardId}>
                {b.platform} ({b.boardId})
              </option>
            ))}
          </select>
          <select value={vehicle} onChange={(e) => setVehicle(e.target.value)} aria-label="Vehicle">
            {VEHICLES.map((v) => (
              <option key={v}>{v}</option>
            ))}
          </select>
          <select value={channel} onChange={(e) => setChannel(e.target.value)} aria-label="Release channel">
            {CHANNELS.map((c) => (
              <option key={c.id} value={c.id}>
                {c.label}
              </option>
            ))}
          </select>
        </div>
        <div className="fw-row">
          <select className="fw-builds" value={buildUrl} disabled={builds.length === 0} onChange={(e) => setBuildUrl(e.target.value)} aria-label="Build">
            {builds.length === 0 && <option value="">{boards ? "No build for this selection" : "Load the list first"}</option>}
            {builds.map((b) => (
              <option key={b.url} value={b.url}>
                {b.vehicleType} {b.version} {b.latest ? "(latest)" : ""} {b.gitSha ? `- ${b.gitSha.slice(0, 8)}` : ""}
              </option>
            ))}
          </select>
          <button className="primary" disabled={!selectedBuild || !!busy} onClick={() => void download()}>
            {busy === "download" ? "Downloading..." : "Download"}
          </button>
          <span className="sep" />
          <button disabled={!!busy} onClick={() => fileInput.current?.click()}>
            Use a local .apj...
          </button>
          <input
            ref={fileInput}
            type="file"
            accept=".apj"
            hidden
            onChange={(e) => {
              const file = e.target.files?.[0];
              e.target.value = "";
              if (file) void pickFile(file);
            }}
          />
        </div>
        {loaded ? (
          <div className="fw-info">
            <b>{loaded.description || "Firmware"}</b> {loaded.version} · board <b>{loaded.boardId}</b> · {(loaded.size / 1024).toFixed(0)} KB
            {loaded.gitHash && <span className="dim"> · {loaded.gitHash.slice(0, 10)}</span>}
          </div>
        ) : (
          <div className="fw-info dim">No firmware loaded. Downloads come only from firmware.ardupilot.org.</div>
        )}
      </section>

      <section className="fw-card">
        <h2>2. Board</h2>
        <p className="dim">
          The board must be in its bootloader. Either reboot a connected vehicle into it, or unplug it and plug it in again
          (some boards need the bootloader button held). Disconnect the vehicle link before identifying.
        </p>
        <div className="fw-row">
          <button disabled={!connected || !!busy} onClick={() => setPending("bootloader")}>
            Reboot vehicle into bootloader
          </button>
          <span className="sep" />
          <select value={port} onChange={(e) => setPort(e.target.value)} aria-label="Serial port">
            {ports.length === 0 && <option value="">No serial ports</option>}
            {ports.map((p) => (
              <option key={p.name} value={p.name}>
                {p.name} {p.description ? `- ${p.description}` : ""}
              </option>
            ))}
          </select>
          <button disabled={!!busy} onClick={() => void refreshPorts()}>
            Refresh ports
          </button>
          <button className="primary" disabled={port === "" || !!busy || connected} onClick={() => void identify()} title={connected ? "Disconnect first" : ""}>
            {busy === "identify" ? "Reading..." : "Identify board"}
          </button>
        </div>
        {board && (
          <div className="fw-info">
            Board <b>{board.boardId}</b> rev {board.boardRev} · bootloader v{board.bootloaderRev} · flash {(board.flashSize / 1024).toFixed(0)} KB
            {board.chipDescription && <> · {board.chipDescription}</>}
            {board.serial && <span className="dim"> · {board.serial}</span>}
          </div>
        )}
        {mismatch && loaded && board && (
          <div className="fw-error">
            This firmware is for board {loaded.boardId} but the connected board is {board.boardId}. Flashing is blocked.
          </div>
        )}
      </section>

      <section className="fw-card">
        <h2>3. Install</h2>
        <div className="fw-row">
          <button className="danger" disabled={!canFlash} onClick={() => setPending("flash")}>
            Flash firmware
          </button>
          {flashing && <button onClick={() => void api.firmwareCancel()}>Cancel</button>}
          {!loaded && <span className="dim">Load a firmware first.</span>}
          {loaded && !board && <span className="dim">Identify the board first.</span>}
        </div>
        {progress && !progress.finished && (
          <div className="fw-status">
            <span>{PHASES[progress.phase] ?? progress.phase}</span>
            <div className="progress small">
              <div style={{ width: `${percent}%` }} />
            </div>
            <span className="dim">{progress.total > 0 ? `${percent.toFixed(0)}%` : ""}</span>
          </div>
        )}
        {flashed && <div className="fw-ok">Firmware installed and verified. The board is rebooting; reconnect to it in a few seconds.</div>}
      </section>

      {error && (
        <div className="fw-error">
          <span>{error}</span>
          <button onClick={() => setError(null)} aria-label="Dismiss">
            x
          </button>
        </div>
      )}

      {pending === "bootloader" && (
        <ConfirmDialog
          title="Reboot into bootloader?"
          message="The vehicle will reboot and the link will be closed. Make sure it is disarmed and on the ground with propellers off."
          tone="warn"
          confirmLabel="Reboot"
          onConfirm={() => void rebootToBootloader()}
          onCancel={() => setPending(null)}
        />
      )}
      {pending === "flash" && loaded && board && (
        <ConfirmDialog
          title="Flash firmware?"
          tone="danger"
          confirmLabel="Erase and flash"
          onConfirm={() => void flash()}
          onCancel={() => setPending(null)}
        >
          <p>
            This erases the board ({board.boardId}) on <b>{port}</b> and installs {loaded.description || "the firmware"} {loaded.version}.
            Do not unplug the board or close the app until it finishes. If it fails, the board stays in its bootloader and you can flash again.
            Parameters are kept unless the new firmware resets them. Propellers must be off.
          </p>
        </ConfirmDialog>
      )}
    </div>
  );
}
