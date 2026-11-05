import type { Backend, Unlisten } from "./api";
import type { BoardInfo, FirmwareBuild, FlashProgress, LoadedFirmware } from "./types";

type FirmwareMock = Pick<
  Backend,
  | "firmwareCatalog"
  | "firmwareBuilds"
  | "firmwareLoadUrl"
  | "firmwareLoadFile"
  | "firmwareRebootBootloader"
  | "firmwareIdentify"
  | "firmwareFlash"
  | "firmwareCancel"
  | "onFlashProgress"
>;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

const BOARD: BoardInfo = {
  bootloaderRev: 5,
  boardId: 140,
  boardRev: 0,
  flashSize: 2080768,
  extfSize: 0,
  chip: 0x450,
  chipDescription: "STM32H7xx",
  serial: "0031002A3338510B37363934",
};

export function createFirmwareMock(): FirmwareMock {
  const listeners = new Set<(p: FlashProgress) => void>();
  const emit = (p: FlashProgress) => listeners.forEach((listener) => listener(p));
  let loaded: LoadedFirmware | null = null;
  let cancelled = false;

  const build = (vehicleType: string, version: string, versionType: string, latest: boolean): FirmwareBuild => ({
    boardId: 140,
    platform: "CubeOrange",
    vehicleType,
    format: "apj",
    url: `https://firmware.ardupilot.org/${vehicleType}/stable-${version}/CubeOrange/arducopter.apj`,
    version,
    versionType,
    latest,
    gitSha: "0123abcd",
    brandName: "Cube Orange",
  });
  const builds = [
    build("Copter", "4.5.7", "OFFICIAL", true),
    build("Copter", "4.6.0-beta1", "BETA", true),
    build("Plane", "4.5.7", "OFFICIAL", true),
  ];

  return {
    firmwareCatalog: async () => {
      await sleep(300);
      return [{ boardId: 140, platform: "CubeOrange", brandName: "Cube Orange", manufacturer: "Hex/ProfiCNC" }];
    },
    firmwareBuilds: async (query) =>
      builds.filter(
        (b) =>
          (!query.vehicleType || b.vehicleType === query.vehicleType) &&
          (!query.versionType || b.versionType === query.versionType),
      ),
    firmwareLoadUrl: async (url) => {
      await sleep(400);
      const found = builds.find((b) => b.url === url);
      loaded = { boardId: 140, description: "Simulated build", version: found?.version ?? "?", gitHash: "0123abcd", size: 1_500_000 };
      return loaded;
    },
    firmwareLoadFile: async () => {
      loaded = { boardId: 140, description: "Local file (simulated)", version: "local", gitHash: "", size: 1_500_000 };
      return loaded;
    },
    firmwareRebootBootloader: async () => {
      await sleep(300);
    },
    firmwareIdentify: async () => {
      await sleep(300);
      return BOARD;
    },
    firmwareFlash: async () => {
      if (!loaded) throw new Error("no firmware loaded");
      cancelled = false;
      emit({ phase: "erase", done: 0, total: 1, finished: false, error: null });
      await sleep(600);
      emit({ phase: "erase", done: 1, total: 1, finished: true, error: null });
      const total = loaded.size;
      for (let done = 0; done < total; done += total / 20) {
        if (cancelled) {
          emit({ phase: "done", done: 0, total: 0, finished: true, error: "cancelled" });
          throw new Error("cancelled");
        }
        emit({ phase: "program", done: Math.min(done, total), total, finished: false, error: null });
        await sleep(100);
      }
      emit({ phase: "verify", done: 1, total: 1, finished: true, error: null });
      emit({ phase: "done", done: 1, total: 1, finished: true, error: null });
      return BOARD;
    },
    firmwareCancel: async () => {
      cancelled = true;
    },
    onFlashProgress: async (cb): Promise<Unlisten> => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
  };
}
