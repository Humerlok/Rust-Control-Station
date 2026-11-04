import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { createMockBackend } from "./mockBackend";
import type {
  ActionInfo,
  CalibrationKind,
  CalibrationState,
  ConnectionConfig,
  FieldInfo,
  LinkInfo,
  ParamEntry,
  ParamMeta,
  ParamsProgress,
  ParsedParamFile,
  SerialPortInfo,
  StatusText,
  Telemetry,
} from "./types";

export type Unlisten = () => void;

export interface Backend {
  listSerialPorts(): Promise<SerialPortInfo[]>;
  connect(config: ConnectionConfig): Promise<void>;
  disconnect(): Promise<void>;
  linkStatus(): Promise<LinkInfo>;
  telemetrySnapshot(): Promise<Telemetry>;
  selectVehicle(systemId: number, componentId: number): Promise<void>;
  quickFields(): Promise<FieldInfo[]>;
  listActions(): Promise<ActionInfo[]>;
  arm(force: boolean): Promise<string>;
  disarm(force: boolean): Promise<string>;
  setMode(mode: string): Promise<string>;
  setWaypoint(seq: number): Promise<string>;
  restartMission(): Promise<string>;
  doAction(name: string): Promise<string>;
  reboot(): Promise<string>;
  takeoff(altitudeM: number): Promise<string>;
  flyTo(lat: number, lon: number, altitudeM: number): Promise<string>;
  setHomeHere(lat: number, lon: number, altitudeM: number): Promise<string>;
  paramsRefresh(): Promise<number>;
  paramsList(): Promise<ParamEntry[]>;
  paramSet(name: string, value: number): Promise<number>;
  paramsMetadata(): Promise<Record<string, ParamMeta>>;
  paramsParseFile(text: string): Promise<ParsedParamFile>;
  paramsFormatFile(): Promise<string>;
  calibrationStart(kind: CalibrationKind): Promise<string>;
  calibrationAccelConfirm(): Promise<string>;
  calibrationCompassAccept(): Promise<string>;
  calibrationCompassCancel(): Promise<string>;
  calibrationCancel(): Promise<string>;
  radioCalibrationFinish(): Promise<string>;
  calibrationStatus(): Promise<CalibrationState>;
  onTelemetry(cb: (t: Telemetry) => void): Promise<Unlisten>;
  onStatusText(cb: (m: StatusText) => void): Promise<Unlisten>;
  onLink(cb: (l: LinkInfo) => void): Promise<Unlisten>;
  onParamsProgress(cb: (p: ParamsProgress) => void): Promise<Unlisten>;
  onCalibration(cb: (c: CalibrationState) => void): Promise<Unlisten>;
}

const subscribe =
  <T>(event: string) =>
  (cb: (payload: T) => void): Promise<Unlisten> =>
    listen<T>(event, (e) => cb(e.payload));

const tauriBackend: Backend = {
  listSerialPorts: () => invoke("list_serial_ports"),
  connect: (config) => invoke("connect", { config }),
  disconnect: () => invoke("disconnect"),
  linkStatus: () => invoke("link_status"),
  telemetrySnapshot: () => invoke("telemetry_snapshot"),
  selectVehicle: (systemId, componentId) => invoke("select_vehicle", { systemId, componentId }),
  quickFields: () => invoke("quick_fields"),
  listActions: () => invoke("list_actions"),
  arm: (force) => invoke("arm", { force }),
  disarm: (force) => invoke("disarm", { force }),
  setMode: (mode) => invoke("set_mode", { mode }),
  setWaypoint: (seq) => invoke("set_waypoint", { seq }),
  restartMission: () => invoke("restart_mission"),
  doAction: (name) => invoke("do_action", { name }),
  reboot: () => invoke("reboot"),
  takeoff: (altitudeM) => invoke("takeoff", { altitudeM }),
  flyTo: (lat, lon, altitudeM) => invoke("fly_to", { lat, lon, altitudeM }),
  setHomeHere: (lat, lon, altitudeM) => invoke("set_home_here", { lat, lon, altitudeM }),
  paramsRefresh: () => invoke("params_refresh"),
  paramsList: () => invoke("params_list"),
  paramSet: (name, value) => invoke("param_set", { name, value }),
  paramsMetadata: () => invoke("params_metadata"),
  paramsParseFile: (text) => invoke("params_parse_file", { text }),
  paramsFormatFile: () => invoke("params_format_file"),
  calibrationStart: (kind) => invoke("calibration_start", { kind }),
  calibrationAccelConfirm: () => invoke("calibration_accel_confirm"),
  calibrationCompassAccept: () => invoke("calibration_compass_accept"),
  calibrationCompassCancel: () => invoke("calibration_compass_cancel"),
  calibrationCancel: () => invoke("calibration_cancel"),
  radioCalibrationFinish: () => invoke("radio_calibration_finish"),
  calibrationStatus: () => invoke("calibration_status"),
  onTelemetry: subscribe<Telemetry>("telemetry"),
  onStatusText: subscribe<StatusText>("statustext"),
  onLink: subscribe<LinkInfo>("link"),
  onParamsProgress: subscribe<ParamsProgress>("params-progress"),
  onCalibration: subscribe<CalibrationState>("calibration"),
};

const useMock =
  typeof window !== "undefined" &&
  (new URLSearchParams(window.location.search).has("mock") || !("__TAURI_INTERNALS__" in window));

export const isMockBackend = useMock;
export const api: Backend = useMock ? createMockBackend() : tauriBackend;
