import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { createMockBackend } from "./mockBackend";
import type {
  ActionInfo,
  CalibrationKind,
  CalibrationState,
  ConnectionConfig,
  DroneCanState,
  BoardInfo,
  BoardSummary,
  FirmwareBuild,
  FirmwareQuery,
  FlashProgress,
  FtpEntry,
  LoadedFirmware,
  FtpProgress,
  InspectorSnapshot,
  FieldInfo,
  LinkInfo,
  MissionCommandInfo,
  MissionItem,
  MissionKind,
  MissionProgress,
  MotorFrame,
  MotorTestRequest,
  MotorTestState,
  NodeImageInfo,
  NodeParam,
  NodeParamValue,
  ParamEntry,
  ParamMeta,
  ParamsProgress,
  ParsedParamFile,
  SerialPortInfo,
  SpinParam,
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
  /** The motors to offer for the frame (FRAME_CLASS / FRAME_TYPE values, when known). */
  /** `genericCount` only applies to frames that are not in the layout table. */
  motorFrame(frameClass: number | null, frameType: number | null, genericCount: number | null): Promise<MotorFrame>;
  motorStart(request: MotorTestRequest): Promise<string>;
  motorStartAll(request: MotorTestRequest, count: number): Promise<string>;
  motorStartSequence(request: MotorTestRequest, count: number): Promise<string>;
  motorStopAll(count: number): Promise<string>;
  motorSetSpin(which: SpinParam, percent: number): Promise<string>;
  /** Streams ESC telemetry while the screen is open. */
  motorWatchEsc(enabled: boolean): Promise<void>;
  motorStatus(): Promise<MotorTestState>;
  /** Relays a CAN bus of the autopilot (1 or 2) and speaks DroneCAN on it. */
  dronecanStart(bus: number): Promise<string>;
  dronecanStop(): Promise<string>;
  dronecanStatus(): Promise<DroneCanState>;
  dronecanParams(node: number): Promise<NodeParam[]>;
  /** Resolves with the value the node holds after the write. */
  dronecanSetParam(node: number, name: string, value: NodeParamValue): Promise<NodeParam>;
  dronecanSaveParams(node: number): Promise<string>;
  dronecanRestart(node: number): Promise<string>;
  /** Describes a firmware file (base64 of an `.apj` or a raw `.bin`). */
  dronecanInspectFirmware(dataBase64: string): Promise<NodeImageInfo>;
  /** Starts serving the file to the node; progress arrives in the DroneCAN state. */
  dronecanUpdateFirmware(node: number, dataBase64: string, force: boolean): Promise<string>;
  dronecanCancelUpdate(): Promise<string>;
  dronecanSetAllocation(enabled: boolean): Promise<string>;
  dronecanInspectorEnable(enabled: boolean): Promise<void>;
  /** Key: `node/kind/typeId/field` (kind 0 broadcast, 1 request, 2 response), arrays as `field[3]`. */
  dronecanInspectorWatch(key: string): Promise<void>;
  dronecanInspectorUnwatch(key: string): Promise<void>;
  dronecanInspectorClear(): Promise<void>;
  inspectorEnable(enabled: boolean): Promise<void>;
  inspectorWatch(key: string): Promise<void>;
  inspectorUnwatch(key: string): Promise<void>;
  inspectorClear(): Promise<void>;
  missionDownload(kind: MissionKind): Promise<MissionItem[]>;
  missionUpload(kind: MissionKind, items: MissionItem[]): Promise<void>;
  missionClear(kind: MissionKind): Promise<void>;
  missionCommands(): Promise<MissionCommandInfo[]>;
  missionParseFile(text: string): Promise<MissionItem[]>;
  missionFormatFile(items: MissionItem[]): Promise<string>;
  ftpList(path: string): Promise<FtpEntry[]>;
  /** File contents, base64 encoded. */
  ftpDownload(path: string): Promise<string>;
  ftpUpload(path: string, dataBase64: string, verify: boolean): Promise<void>;
  ftpRemove(path: string): Promise<void>;
  ftpMkdir(path: string): Promise<void>;
  ftpRmdir(path: string): Promise<void>;
  ftpRename(from: string, to: string): Promise<void>;
  /** CRC32 computed by the vehicle, lowercase hex. */
  ftpCrc(path: string): Promise<string>;
  ftpCancel(): Promise<void>;
  firmwareCatalog(): Promise<BoardSummary[]>;
  firmwareBuilds(query: FirmwareQuery): Promise<FirmwareBuild[]>;
  firmwareLoadUrl(url: string): Promise<LoadedFirmware>;
  /** A local `.apj`, base64 encoded. */
  firmwareLoadFile(dataBase64: string): Promise<LoadedFirmware>;
  firmwareRebootBootloader(): Promise<void>;
  firmwareIdentify(port: string): Promise<BoardInfo>;
  firmwareFlash(port: string): Promise<BoardInfo>;
  firmwareCancel(): Promise<void>;
  onTelemetry(cb: (t: Telemetry) => void): Promise<Unlisten>;
  onStatusText(cb: (m: StatusText) => void): Promise<Unlisten>;
  onLink(cb: (l: LinkInfo) => void): Promise<Unlisten>;
  onParamsProgress(cb: (p: ParamsProgress) => void): Promise<Unlisten>;
  onCalibration(cb: (c: CalibrationState) => void): Promise<Unlisten>;
  onMotorTest(cb: (s: MotorTestState) => void): Promise<Unlisten>;
  onDroneCan(cb: (s: DroneCanState) => void): Promise<Unlisten>;
  onDroneCanInspector(cb: (s: InspectorSnapshot) => void): Promise<Unlisten>;
  onInspector(cb: (s: InspectorSnapshot) => void): Promise<Unlisten>;
  onMissionProgress(cb: (p: MissionProgress) => void): Promise<Unlisten>;
  onFtpProgress(cb: (progress: FtpProgress) => void): Promise<Unlisten>;
  onFlashProgress(cb: (progress: FlashProgress) => void): Promise<Unlisten>;
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
  motorFrame: (frameClass, frameType, genericCount) => invoke("motor_frame", { class: frameClass, frameType, genericCount }),
  motorStart: (request) => invoke("motor_start", { request }),
  motorStartAll: (request, count) => invoke("motor_start_all", { request, count }),
  motorStartSequence: (request, count) => invoke("motor_start_sequence", { request, count }),
  motorStopAll: (count) => invoke("motor_stop_all", { count }),
  motorSetSpin: (which, percent) => invoke("motor_set_spin", { which, percent }),
  motorWatchEsc: (enabled) => invoke("motor_watch_esc", { enabled }),
  motorStatus: () => invoke("motor_status"),
  dronecanStart: (bus) => invoke("dronecan_start", { bus }),
  dronecanStop: () => invoke("dronecan_stop"),
  dronecanStatus: () => invoke("dronecan_status"),
  dronecanParams: (node) => invoke("dronecan_params", { node }),
  dronecanSetParam: (node, name, value) => invoke("dronecan_set_param", { node, name, value }),
  dronecanSaveParams: (node) => invoke("dronecan_save_params", { node }),
  dronecanRestart: (node) => invoke("dronecan_restart", { node }),
  dronecanInspectFirmware: (data) => invoke("dronecan_inspect_firmware", { data }),
  dronecanUpdateFirmware: (node, data, force) => invoke("dronecan_update_firmware", { node, data, force }),
  dronecanCancelUpdate: () => invoke("dronecan_cancel_update"),
  dronecanSetAllocation: (enabled) => invoke("dronecan_set_allocation", { enabled }),
  dronecanInspectorEnable: (enabled) => invoke("dronecan_inspector_enable", { enabled }),
  dronecanInspectorWatch: (key) => invoke("dronecan_inspector_watch", { key }),
  dronecanInspectorUnwatch: (key) => invoke("dronecan_inspector_unwatch", { key }),
  dronecanInspectorClear: () => invoke("dronecan_inspector_clear"),
  inspectorEnable: (enabled) => invoke("inspector_enable", { enabled }),
  inspectorWatch: (key) => invoke("inspector_watch", { key }),
  inspectorUnwatch: (key) => invoke("inspector_unwatch", { key }),
  inspectorClear: () => invoke("inspector_clear"),
  missionDownload: (kind) => invoke("mission_download", { kind }),
  missionUpload: (kind, items) => invoke("mission_upload", { kind, items }),
  missionClear: (kind) => invoke("mission_clear", { kind }),
  missionCommands: () => invoke("mission_commands"),
  missionParseFile: (text) => invoke("mission_parse_file", { text }),
  missionFormatFile: (items) => invoke("mission_format_file", { items }),
  ftpList: (path) => invoke("ftp_list", { path }),
  ftpDownload: (path) => invoke("ftp_download", { path }),
  ftpUpload: (path, data, verify) => invoke("ftp_upload", { path, data, verify }),
  ftpRemove: (path) => invoke("ftp_remove", { path }),
  ftpMkdir: (path) => invoke("ftp_mkdir", { path }),
  ftpRmdir: (path) => invoke("ftp_rmdir", { path }),
  ftpRename: (from, to) => invoke("ftp_rename", { from, to }),
  ftpCrc: (path) => invoke("ftp_crc", { path }),
  ftpCancel: () => invoke("ftp_cancel"),
  firmwareCatalog: () => invoke("firmware_catalog"),
  firmwareBuilds: (query) => invoke("firmware_builds", { query }),
  firmwareLoadUrl: (url) => invoke("firmware_load_url", { url }),
  firmwareLoadFile: (data) => invoke("firmware_load_file", { data }),
  firmwareRebootBootloader: () => invoke("firmware_reboot_bootloader"),
  firmwareIdentify: (port) => invoke("firmware_identify", { port }),
  firmwareFlash: (port) => invoke("firmware_flash", { port }),
  firmwareCancel: () => invoke("firmware_cancel"),
  onTelemetry: subscribe<Telemetry>("telemetry"),
  onStatusText: subscribe<StatusText>("statustext"),
  onLink: subscribe<LinkInfo>("link"),
  onParamsProgress: subscribe<ParamsProgress>("params-progress"),
  onCalibration: subscribe<CalibrationState>("calibration"),
  onMotorTest: subscribe<MotorTestState>("motor-test"),
  onDroneCan: subscribe<DroneCanState>("dronecan"),
  onDroneCanInspector: subscribe<InspectorSnapshot>("dronecan-inspector"),
  onInspector: subscribe<InspectorSnapshot>("inspector"),
  onMissionProgress: subscribe<MissionProgress>("mission-progress"),
  onFtpProgress: subscribe<FtpProgress>("ftp-progress"),
  onFlashProgress: subscribe<FlashProgress>("flash-progress"),
};

const useMock =
  typeof window !== "undefined" &&
  (new URLSearchParams(window.location.search).has("mock") || !("__TAURI_INTERNALS__" in window));

export const isMockBackend = useMock;
export const api: Backend = useMock ? createMockBackend() : tauriBackend;
