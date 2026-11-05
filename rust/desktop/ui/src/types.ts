export type VehicleRef = { systemId: number; componentId: number };
export type VehicleSummary = VehicleRef & { vehicleType: string };
export type GeoPoint = { lat: number; lon: number; altM: number };

export type Telemetry = {
  vehicle: VehicleRef | null;
  connected: boolean;
  armed: boolean;
  mode: string | null;
  vehicleType: string;
  availableModes: string[];
  fields: Record<string, number>;
  position: GeoPoint | null;
  home: GeoPoint | null;
  vehicles: VehicleSummary[];
};

export type LinkStats = {
  bytesRx: number;
  bytesTx: number;
  packetsRx: number;
  packetsLost: number;
  badCrc: number;
  packetsPerSecond: number;
  bytesPerSecond: number;
  qualityPercent: number;
};

export type LinkInfo = {
  state: "disconnected" | "connecting" | "connected";
  description: string;
  error: string | null;
  stats: LinkStats;
};

export type StatusText = {
  timeMs: number;
  severity: number;
  text: string;
  systemId: number;
  componentId: number;
};

export type SerialPortInfo = { name: string; description: string; vid?: number | null; pid?: number | null };
export type FieldInfo = { key: string; label: string; unit: string };
export type ActionInfo = { name: string; label: string; confirm: boolean; danger: boolean };

export type ConnectionConfig =
  | { type: "serial"; port: string; baud: number }
  | { type: "tcp"; host: string; port: number }
  | { type: "udp"; port: number }
  | { type: "udpcl"; host: string; port: number }
  | { type: "ws"; url: string }
  | { type: "auto" }
  | { type: "replay"; path: string; speed?: number };

export const emptyStats: LinkStats = {
  bytesRx: 0,
  bytesTx: 0,
  packetsRx: 0,
  packetsLost: 0,
  badCrc: 0,
  packetsPerSecond: 0,
  bytesPerSecond: 0,
  qualityPercent: 0,
};

export const emptyLink: LinkInfo = { state: "disconnected", description: "", error: null, stats: emptyStats };

export const emptyTelemetry: Telemetry = {
  vehicle: null,
  connected: false,
  armed: false,
  mode: null,
  vehicleType: "Unknown",
  availableModes: [],
  fields: {},
  position: null,
  home: null,
  vehicles: [],
};

export type ParamEntry = { name: string; value: number; paramType: string; index: number };

export type ParamMeta = {
  displayName: string;
  description: string;
  units: string;
  min: number | null;
  max: number | null;
  increment: number | null;
  values: Array<{ code: number; label: string }>;
  bitmask: Array<{ bit: number; label: string }>;
  rebootRequired: boolean;
  readOnly: boolean;
  user: "Standard" | "Advanced";
};

export type ParamsProgress = { received: number; total: number; done: boolean; error: string | null };
export type ParsedParamFile = { params: Array<{ name: string; value: number }>; skipped: string[] };

export type CalibrationKind =
  | "accel_full"
  | "accel_level"
  | "accel_simple"
  | "gyro"
  | "baro"
  | "compass"
  | "radio"
  | "esc";

export type CalibrationState = {
  kind: string | null;
  state: "idle" | "running" | "waiting" | "succeeded" | "failed";
  message: string;
  accelPosition: { index: number; name: string } | null;
  compass: Array<{ compassId: number; completionPct: number; status: string; fitness: number | null; autosaved: boolean }>;
  radio: Array<{ channel: number; min: number; max: number; value: number }>;
};

export const idleCalibration: CalibrationState = {
  kind: null,
  state: "idle",
  message: "",
  accelPosition: null,
  compass: [],
  radio: [],
};

// ---- Phase 5: motor test ----

export type MotorThrottleType = "percent" | "pwm" | "pilot";
export type SpinParam = "arm" | "min";

export type MotorTestRequest = {
  /** 1-based position in the test order (motor A is 1). Ignored by "all" and "sequence". */
  motor: number;
  throttleType: MotorThrottleType;
  /** Percent, or microseconds for PWM; unused for "pilot". */
  throttle: number;
  seconds: number;
  /** The user confirmed a throttle above 20 %. */
  allowHigh: boolean;
};

export type MotorSlot = {
  label: string;
  testOrder: number;
  output: number | null;
  rotation: string | null;
};

export type MotorFrame = {
  known: boolean;
  className: string | null;
  typeName: string | null;
  motors: MotorSlot[];
};

export type EscReading = {
  index: number;
  rpm: number | null;
  voltage: number | null;
  current: number | null;
  temperature: number | null;
};

export type MotorTestState = {
  escs: EscReading[];
  running: boolean;
  message: string;
};

export const idleMotorTest: MotorTestState = { escs: [], running: false, message: "" };

// ---- Phase 6: DroneCAN ----

export type DroneCanNode = {
  id: number;
  online: boolean;
  name: string | null;
  health: string;
  mode: string;
  uptimeSec: number;
  vendorStatus: number;
  swVersion: string | null;
  swCrc: string | null;
  hwVersion: string | null;
  uniqueId: string | null;
};

export type DroneCanLog = { timeMs: number; node: number; level: string; source: string; text: string };

export type UpdatePhase = "starting" | "sending" | "done" | "failed";

export type FirmwareUpdateState = {
  node: number;
  phase: UpdatePhase;
  sent: number;
  total: number;
  message: string;
};

/** What a firmware file is, before it is sent to a node. */
export type NodeImageInfo = {
  format: "apj" | "bin";
  size: number;
  /** The image CRC from the application descriptor, 16 hex digits; null when the file has none. */
  crc: string | null;
  description: string | null;
};

export type DroneCanAllocation = { uniqueId: string; nodeId: number };

export type DroneCanState = {
  active: boolean;
  /** 1 or 2: the autopilot's CAN bus being relayed. */
  bus: number | null;
  nodes: DroneCanNode[];
  /** Newest first. */
  logs: DroneCanLog[];
  framesRx: number;
  framesTx: number;
  progress: { node: number; received: number } | null;
  update: FirmwareUpdateState | null;
  allocation: { enabled: boolean; recent: DroneCanAllocation[] };
};

export const idleDroneCan: DroneCanState = {
  active: false,
  bus: null,
  nodes: [],
  logs: [],
  framesRx: 0,
  framesTx: 0,
  progress: null,
  update: null,
  allocation: { enabled: false, recent: [] },
};

export type NodeParamValue =
  | { type: "empty" }
  | { type: "integer"; value: number }
  | { type: "real"; value: number }
  | { type: "boolean"; value: boolean }
  | { type: "text"; value: string };

export type NodeParam = {
  index: number;
  name: string;
  value: NodeParamValue;
  default: NodeParamValue | null;
  min: number | null;
  max: number | null;
};

// ---- Phase 3: MAVLink inspector ----

export type InspectorValue = number | string | boolean | Array<number | string | boolean>;
export type InspectorField = { name: string; value: InspectorValue };
export type InspectorMessage = {
  systemId: number;
  componentId: number;
  componentName: string;
  messageId: number;
  name: string;
  count: number;
  hz: number;
  bytesPerSecond: number;
  ageMs: number;
  fields: InspectorField[];
};
/** [timeMs since the inspector was enabled, value] */
export type InspectorPoint = [number, number];
export type InspectorSnapshot = {
  messages: InspectorMessage[];
  /** New points since the previous snapshot, keyed by watch key (see inspectorKey). */
  series: Record<string, InspectorPoint[]>;
};

/** Watch key: `${systemId}/${componentId}/${messageId}/${field}`, array elements as `field[3]`. */
export const inspectorKey = (m: { systemId: number; componentId: number; messageId: number }, field: string) =>
  `${m.systemId}/${m.componentId}/${m.messageId}/${field}`;

// ---- Phase 3: missions ----

export type MissionKind = "mission" | "fence" | "rally";

/** A MISSION_ITEM_INT in friendly units: x/y are degrees for global frames, z is metres. */
export type MissionItem = {
  seq: number;
  /** MAV_FRAME number: 0 global (absolute), 3 relative to home, 10 terrain. */
  frame: number;
  /** MAV_CMD number. */
  command: number;
  param1: number;
  param2: number;
  param3: number;
  param4: number;
  x: number;
  y: number;
  z: number;
  autocontinue: boolean;
};

/** Labels of the grid columns for one command; null means the field is unused by the command. */
export type MissionCommandInfo = {
  id: number;
  name: string;
  description: string;
  p1: string | null;
  p2: string | null;
  p3: string | null;
  p4: string | null;
  x: string | null;
  y: string | null;
  z: string | null;
};

export type MissionProgress = {
  kind: MissionKind;
  phase: "download" | "upload";
  done: number;
  total: number;
  finished: boolean;
  error: string | null;
};

export type FtpEntry = {
  name: string;
  isDir: boolean;
  size: number;
  /** Seconds since the Unix epoch, when the vehicle reports one. */
  modified: number | null;
};

export type FtpProgress = {
  op: string;
  path: string;
  done: number;
  total: number;
  finished: boolean;
  error: string | null;
};

export type BoardSummary = { boardId: number; platform: string; brandName: string; manufacturer: string };

export type FirmwareBuild = {
  boardId: number | null;
  platform: string;
  vehicleType: string;
  format: string;
  url: string;
  version: string;
  /** OFFICIAL, BETA or DEV. */
  versionType: string;
  latest: boolean;
  gitSha: string;
  brandName: string;
};

export type FirmwareQuery = {
  boardId?: number;
  vehicleType?: string;
  versionType?: string;
  latestOnly?: boolean;
};

export type LoadedFirmware = {
  boardId: number;
  description: string;
  version: string;
  gitHash: string;
  size: number;
};

export type BoardInfo = {
  bootloaderRev: number;
  boardId: number;
  boardRev: number;
  flashSize: number;
  extfSize: number;
  chip: number | null;
  chipDescription: string | null;
  serial: string | null;
};

export type FlashProgress = {
  /** sync, identify, erase, program, verify, reboot, done */
  phase: string;
  done: number;
  total: number;
  finished: boolean;
  error: string | null;
};
