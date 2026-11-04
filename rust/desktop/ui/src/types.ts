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

export type SerialPortInfo = { name: string; description: string };
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
