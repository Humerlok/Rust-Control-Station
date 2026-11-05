import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import { ConfirmDialog } from "../components/ConfirmDialog";
import type { FtpEntry, FtpProgress } from "../types";
import "../mavftp.css";

type Props = { connected: boolean; notify: (text: string, ok: boolean) => void };

const PLACES = ["/", "/APM", "/APM/scripts", "/APM/logs", "/APM/lua"];

const join = (dir: string, name: string) => (dir === "/" ? `/${name}` : `${dir.replace(/\/+$/, "")}/${name}`);
const parentOf = (dir: string) => dir.replace(/\/+$/, "").slice(0, dir.replace(/\/+$/, "").lastIndexOf("/")) || "/";
const clean = (dir: string) => (dir.trim() === "" ? "/" : dir.length > 1 ? dir.trim().replace(/\/+$/, "") : dir.trim());

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function formatTime(seconds: number | null): string {
  if (!seconds) return "";
  return new Date(seconds * 1000).toLocaleString();
}

function toBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let text = "";
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(text);
}

function save(filename: string, base64: string) {
  const bytes = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
  const url = URL.createObjectURL(new Blob([bytes], { type: "application/octet-stream" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

type Pending =
  | { kind: "delete"; entry: FtpEntry }
  | { kind: "overwrite"; name: string; data: string }
  | { kind: "rename"; entry: FtpEntry }
  | { kind: "mkdir" };

export function MavFtp({ connected, notify }: Props) {
  const [dir, setDir] = useState("/APM");
  const [pathText, setPathText] = useState("/APM");
  const [entries, setEntries] = useState<FtpEntry[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<FtpProgress | null>(null);
  const [verify, setVerify] = useState(true);
  const [pending, setPending] = useState<Pending | null>(null);
  const [info, setInfo] = useState<string | null>(null);
  const [nameValue, setNameValue] = useState("");
  const fileInput = useRef<HTMLInputElement>(null);
  const token = useRef(0);

  useEffect(() => {
    let off: (() => void) | undefined;
    let cancelled = false;
    void api.onFtpProgress((p) => setProgress(p)).then((unlisten) => {
      if (cancelled) unlisten();
      else off = unlisten;
    });
    return () => {
      cancelled = true;
      off?.();
    };
  }, []);

  const selectedEntry = useMemo(() => entries.find((e) => e.name === selected) ?? null, [entries, selected]);

  const load = useCallback(
    async (target: string) => {
      const mine = ++token.current;
      setBusy(true);
      setError(null);
      try {
        const list = await api.ftpList(target);
        if (mine !== token.current) return;
        list.sort((a, b) => Number(b.isDir) - Number(a.isDir) || a.name.localeCompare(b.name, undefined, { numeric: true }));
        setEntries(list);
        setDir(target);
        setPathText(target);
        setSelected(null);
      } catch (e) {
        if (mine === token.current) setError(String(e));
      } finally {
        if (mine === token.current) setBusy(false);
      }
    },
    [],
  );

  // Load once when the vehicle becomes available; the user drives it from there.
  useEffect(() => {
    if (connected) void load("/APM");
    else {
      setEntries([]);
      setError(null);
    }
  }, [connected, load]);

  async function run(label: string, action: () => Promise<unknown>, refresh = true) {
    setBusy(true);
    setError(null);
    try {
      await action();
      notify(label, true);
      if (refresh) await load(dir);
    } catch (e) {
      setError(String(e));
      notify(String(e), false);
    } finally {
      setBusy(false);
    }
  }

  const download = () =>
    selectedEntry &&
    !selectedEntry.isDir &&
    run(`Downloaded ${selectedEntry.name}`, async () => save(selectedEntry.name, await api.ftpDownload(join(dir, selectedEntry.name))), false);

  const checksum = () =>
    selectedEntry &&
    !selectedEntry.isDir &&
    run(
      `CRC32 of ${selectedEntry.name}`,
      async () => setInfo(`${selectedEntry.name}: CRC32 ${await api.ftpCrc(join(dir, selectedEntry.name))}`),
      false,
    );

  async function upload(name: string, data: string) {
    await run(`Uploaded ${name}`, () => api.ftpUpload(join(dir, name), data, verify));
  }

  async function onPickFile(file: File | undefined) {
    if (!file) return;
    if (file.size > 16 * 1024 * 1024) {
      setError("That file is larger than 16 MB; MAVFTP uploads are slow and this is probably a mistake.");
      return;
    }
    const data = toBase64(await file.arrayBuffer());
    if (entries.some((e) => e.name === file.name && !e.isDir)) setPending({ kind: "overwrite", name: file.name, data });
    else await upload(file.name, data);
  }

  function confirm() {
    const p = pending;
    setPending(null);
    if (!p) return;
    if (p.kind === "delete") {
      const path = join(dir, p.entry.name);
      void run(`Deleted ${p.entry.name}`, () => (p.entry.isDir ? api.ftpRmdir(path) : api.ftpRemove(path)));
    } else if (p.kind === "overwrite") void upload(p.name, p.data);
    else if (p.kind === "rename") {
      const to = nameValue.trim();
      if (to && to !== p.entry.name) void run(`Renamed to ${to}`, () => api.ftpRename(join(dir, p.entry.name), join(dir, to)));
    } else {
      const name = nameValue.trim();
      if (name) void run(`Created ${name}`, () => api.ftpMkdir(join(dir, name)));
    }
  }

  const needsName = pending?.kind === "rename" || pending?.kind === "mkdir";
  const nameInvalid = needsName && (nameValue.trim() === "" || nameValue.includes("/"));

  const disabled = !connected || busy;
  const active = progress && !progress.finished ? progress : null;
  const percent = active && active.total > 0 ? Math.min(100, (active.done / active.total) * 100) : null;

  return (
    <div className="ftp">
      <div className="ftp-toolbar">
        <button disabled={disabled || dir === "/"} onClick={() => void load(parentOf(dir))} title="Up one level">
          Up
        </button>
        <input
          className="ftp-path"
          value={pathText}
          disabled={!connected}
          onChange={(e) => setPathText(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void load(clean(pathText))}
          aria-label="Directory"
          spellCheck={false}
        />
        <button disabled={disabled} onClick={() => void load(dir)}>
          Refresh
        </button>
        <span className="sep" />
        <button
          disabled={disabled}
          onClick={() => {
            setNameValue("");
            setPending({ kind: "mkdir" });
          }}
        >
          New folder
        </button>
        <button disabled={disabled} onClick={() => fileInput.current?.click()}>
          Upload…
        </button>
        <label className="inline" title="Ask the vehicle for the CRC32 of the uploaded file and compare it">
          <input type="checkbox" checked={verify} onChange={(e) => setVerify(e.target.checked)} /> Verify CRC
        </label>
        <span className="sep" />
        <button disabled={disabled || !selectedEntry || selectedEntry.isDir} onClick={() => void download()}>
          Download
        </button>
        <button disabled={disabled || !selectedEntry || selectedEntry.isDir} onClick={() => void checksum()}>
          CRC32
        </button>
        <button
          disabled={disabled || !selectedEntry}
          onClick={() => {
            if (!selectedEntry) return;
            setNameValue(selectedEntry.name);
            setPending({ kind: "rename", entry: selectedEntry });
          }}
        >
          Rename
        </button>
        <button className="danger" disabled={disabled || !selectedEntry} onClick={() => selectedEntry && setPending({ kind: "delete", entry: selectedEntry })}>
          Delete
        </button>
        <span className="spacer" />
        {busy && (
          <button onClick={() => void api.ftpCancel()} title="Stop the running transfer">
            Cancel
          </button>
        )}
        <input ref={fileInput} type="file" hidden onChange={(e) => { void onPickFile(e.target.files?.[0]); e.target.value = ""; }} />
      </div>

      {error && (
        <div className="ftp-error" role="alert">
          <span>{error}</span>
          <button onClick={() => setError(null)} aria-label="Dismiss">✕</button>
        </div>
      )}

      <div className="ftp-body">
        <div className="ftp-places" aria-label="Places">
          <div className="group">Places</div>
          {PLACES.map((place) => (
            <button key={place} className={dir === place ? "active" : ""} disabled={disabled} onClick={() => void load(place)}>
              {place}
            </button>
          ))}
        </div>
        <div className="ftp-files">
          <div className="ftp-head">
            <span>Name</span>
            <span style={{ textAlign: "right" }}>Size</span>
            <span>Modified</span>
          </div>
          {!connected && <p className="ftp-empty dim">Connect to a vehicle to browse its files.</p>}
          {connected && entries.length === 0 && !busy && !error && <p className="ftp-empty dim">This directory is empty.</p>}
          {entries.map((entry) => (
            <div
              key={entry.name}
              className={`ftp-row ${selected === entry.name ? "selected" : ""}`}
              onClick={() => setSelected(entry.name)}
              onDoubleClick={() => (entry.isDir ? void load(join(dir, entry.name)) : void download())}
            >
              <span className={`name ${entry.isDir ? "dir" : ""}`}>{entry.isDir ? `▸ ${entry.name}/` : entry.name}</span>
              <span className="num">{entry.isDir ? "" : formatSize(entry.size)}</span>
              <span className="when">{formatTime(entry.modified)}</span>
            </div>
          ))}
        </div>
      </div>

      <div className="ftp-status dim">
        {active && (
          <>
            <span>
              {active.op} {active.path}
              {active.total > 0 && ` — ${formatSize(active.done)} / ${formatSize(active.total)}`}
            </span>
            {percent !== null && (
              <div className="progress small" role="progressbar" aria-valuenow={Math.round(percent)} aria-valuemin={0} aria-valuemax={100}>
                <div style={{ width: `${percent}%` }} />
              </div>
            )}
          </>
        )}
        {!active && info && <span>{info}</span>}
        {!active && !info && connected && <span>{entries.length} item(s) in {dir}</span>}
      </div>

      {pending && (
        <ConfirmDialog
          title={
            pending.kind === "delete" ? "Delete from the vehicle" : pending.kind === "overwrite" ? "Replace file" : pending.kind === "rename" ? "Rename" : "Create folder"
          }
          message={
            pending.kind === "delete"
              ? `Delete ${join(dir, pending.entry.name)}? ${pending.entry.isDir ? "Only an empty folder can be removed. " : ""}This cannot be undone.`
              : pending.kind === "overwrite"
                ? `${join(dir, pending.name)} already exists on the vehicle and will be replaced.`
                : pending.kind === "rename"
                  ? `Rename ${pending.entry.name} in ${dir} to:`
                  : `New folder in ${dir}:`
          }
          tone={pending.kind === "delete" || pending.kind === "overwrite" ? "danger" : "default"}
          confirmLabel={pending.kind === "delete" ? "Delete" : pending.kind === "overwrite" ? "Replace" : "OK"}
          onConfirm={nameInvalid ? () => undefined : confirm}
          onCancel={() => setPending(null)}
        >
          {needsName && (
            <input
              value={nameValue}
              onChange={(e) => setNameValue(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && !nameInvalid && confirm()}
              aria-label="Name"
              aria-invalid={nameInvalid}
              spellCheck={false}
              autoFocus
              style={{ width: "100%", fontFamily: "var(--mono)" }}
            />
          )}
        </ConfirmDialog>
      )}
    </div>
  );
}
