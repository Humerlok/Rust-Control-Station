/**
 * Mock MAVFTP: an in-memory file tree (base64 contents) with progress events like the real FtpService.
 * Spread into the mock `Backend` by mockBackend.ts.
 */
import type { Backend, Unlisten } from "./api";
import { browserDownload } from "./mockDownload";
import type { FtpEntry, FtpProgress } from "./types";

type FtpMock = Pick<
  Backend,
  | "ftpList"
  | "ftpDownloadSave"
  | "ftpUpload"
  | "ftpRemove"
  | "ftpMkdir"
  | "ftpRmdir"
  | "ftpRename"
  | "ftpCrc"
  | "ftpCancel"
  | "onFtpProgress"
>;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const encode = (text: string) => btoa(text);
const parent = (path: string) => path.slice(0, path.lastIndexOf("/")) || "/";

function crc32(bytes: Uint8Array): string {
  // ArduPilot variant: reflected 0xEDB88320, initial 0, no final xor
  let crc = 0;
  for (const byte of bytes) {
    crc ^= byte;
    for (let i = 0; i < 8; i += 1) crc = crc & 1 ? (crc >>> 1) ^ 0xedb88320 : crc >>> 1;
  }
  return (crc >>> 0).toString(16).padStart(8, "0");
}

export function createFtpMock(): FtpMock {
  const listeners = new Set<(p: FtpProgress) => void>();
  const emit = (p: FtpProgress) => listeners.forEach((listener) => listener(p));
  const now = Math.floor(Date.now() / 1000);
  const files = new Map<string, string>([
    ["/APM/scripts/example.lua", encode('gcs:send_text(6, "hello from Lua")\n')],
    ["/APM/README.txt", encode("Simulated vehicle files.\n")],
  ]);
  const dirs = new Set(["/APM", "/APM/scripts", "/APM/LOGS", "/@SYS"]);
  files.set("/@SYS/uarts.txt", encode("SERIAL0 OTG1 TX=1234 RX=567\n"));

  const size = (b64: string) => atob(b64).length;
  const bytes = (b64: string) => Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));

  // Wraps an operation: emits ftp-style progress (start, then finished or error) around it.
  async function run<T>(op: string, path: string, total: number, body: () => T | Promise<T>): Promise<T> {
    emit({ op, path, done: 0, total, finished: false, error: null });
    await sleep(120);
    try {
      const result = await body();
      emit({ op, path, done: total, total, finished: true, error: null });
      return result;
    } catch (error) {
      emit({ op, path, done: 0, total, finished: true, error: String(error) });
      throw error;
    }
  }

  return {
    ftpList: (path) =>
      run("list", path, 0, () => {
        const dir = path.length > 1 ? path.replace(/\/+$/, "") : path;
        if (dir !== "/" && !dirs.has(dir)) throw `list ${dir}: file or directory not found`;
        const entries: FtpEntry[] = [];
        for (const d of dirs) if (d !== dir && parent(d) === dir) entries.push({ name: d.slice(d.lastIndexOf("/") + 1), isDir: true, size: 0, modified: now });
        for (const [f, data] of files) if (parent(f) === dir) entries.push({ name: f.slice(f.lastIndexOf("/") + 1), isDir: false, size: size(data), modified: now - 3600 });
        return entries;
      }),
    ftpDownloadSave: (path) =>
      run("read", path, 100, () => {
        const data = files.get(path);
        if (data === undefined) throw `read ${path}: file or directory not found`;
        const bytes = Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
        return browserDownload(path.slice(path.lastIndexOf("/") + 1), bytes);
      }),
    ftpUpload: (path, data) =>
      run("write", path, size(data), () => {
        if (!dirs.has(parent(path))) throw `write ${path}: file or directory not found`;
        files.set(path, data);
      }),
    ftpRemove: (path) =>
      run("remove", path, 1, () => {
        if (!files.delete(path)) throw `remove ${path}: file or directory not found`;
      }),
    ftpMkdir: (path) =>
      run("mkdir", path, 1, () => {
        if (dirs.has(path)) throw `mkdir ${path}: the file already exists`;
        if (parent(path) !== "/" && !dirs.has(parent(path))) throw `mkdir ${path}: file or directory not found`;
        dirs.add(path);
      }),
    ftpRmdir: (path) =>
      run("rmdir", path, 1, () => {
        const prefix = `${path}/`;
        const busy = [...files.keys(), ...dirs].some((p) => p.startsWith(prefix));
        if (busy) throw `rmdir ${path}: the vehicle failed the operation (errno 39)`;
        if (!dirs.delete(path)) throw `rmdir ${path}: file or directory not found`;
      }),
    ftpRename: (from, to) =>
      run("rename", from, 1, () => {
        const data = files.get(from);
        if (data === undefined) throw `rename ${from}: file or directory not found`;
        files.delete(from);
        files.set(to, data);
      }),
    ftpCrc: (path) =>
      run("crc", path, 1, () => {
        const data = files.get(path);
        if (data === undefined) throw `crc ${path}: file or directory not found`;
        return crc32(bytes(data));
      }),
    ftpCancel: async () => {},
    onFtpProgress: async (cb): Promise<Unlisten> => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
  };
}
