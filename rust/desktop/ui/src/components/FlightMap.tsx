import L, { type LatLngExpression, type LatLngTuple } from "leaflet";
import "leaflet/dist/leaflet.css";
import { useEffect, useMemo, useRef, useState } from "react";
import { MapContainer, Marker, Polyline, TileLayer, useMap, useMapEvents } from "react-leaflet";
import { api } from "../api";
import { loadSetting, saveSetting } from "../storage";
import type { Telemetry } from "../types";
import { ConfirmDialog } from "./ConfirmDialog";

// Tiles are fetched live for viewing only. Never bulk-download from tile.openstreetmap.org (AGENTS.md).
const BASE_LAYERS = {
  sat: {
    label: "Satellite",
    url: "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}",
    attribution: "Tiles © Esri — Source: Esri, Maxar, Earthstar Geographics, and the GIS User Community",
  },
  osm: {
    label: "Streets",
    url: "https://tile.openstreetmap.org/{z}/{x}/{y}.png",
    attribution: "© OpenStreetMap contributors",
  },
} as const;
type BaseKey = keyof typeof BASE_LAYERS;

const MAX_TRACK_POINTS = 2000;
const MENU_WIDTH = 160;
const MENU_HEIGHT = 110;
const TAKEOFF_KEY = "takeoff.alt";

function vehicleIcon(heading: number): L.DivIcon {
  const svg =
    `<svg viewBox="0 0 34 34" width="34" height="34" style="transform: rotate(${heading}deg)">` +
    `<path d="M17 3 L27 29 L17 23 L7 29 Z" fill="#88c0d0" stroke="#2e3440" stroke-width="2" stroke-linejoin="round"/></svg>`;
  return L.divIcon({ className: "vehicle-icon", html: svg, iconSize: [34, 34], iconAnchor: [17, 17] });
}

const homeIcon = L.divIcon({ className: "home-icon", html: "<span>H</span>", iconSize: [22, 22], iconAnchor: [11, 11] });

type MenuState = { x: number; y: number; lat: number; lon: number };
type MapAction = { kind: "takeoff" | "flyto" | "home"; lat: number; lon: number };

type BehaviorProps = {
  position: LatLngTuple | null;
  follow: boolean;
  onUserDrag: () => void;
  onContext: (menu: MenuState) => void;
  onDismiss: () => void;
};

function MapBehavior({ position, follow, onUserDrag, onContext, onDismiss }: BehaviorProps) {
  const map = useMap();
  const centered = useRef(false);
  const lat = position?.[0];
  const lon = position?.[1];

  useEffect(() => {
    if (lat === undefined || lon === undefined) return;
    if (!centered.current) {
      centered.current = true;
      map.setView([lat, lon], 17, { animate: false });
    } else if (follow) {
      map.panTo([lat, lon], { animate: false });
    }
  }, [map, lat, lon, follow]);

  useEffect(() => {
    const observer = new ResizeObserver(() => map.invalidateSize());
    observer.observe(map.getContainer());
    return () => observer.disconnect();
  }, [map]);

  useMapEvents({
    dragstart: () => {
      onUserDrag();
      onDismiss();
    },
    zoomstart: onDismiss,
    click: onDismiss,
    contextmenu: (event) => onContext({ x: event.containerPoint.x, y: event.containerPoint.y, lat: event.latlng.lat, lon: event.latlng.lng }),
  });
  return null;
}

type Props = {
  telemetry: Telemetry;
  notify: (text: string, ok: boolean) => void;
};

export function FlightMap({ telemetry, notify }: Props) {
  const [base, setBase] = useState<BaseKey>(() => loadSetting<BaseKey>("map.base", "sat"));
  const [follow, setFollow] = useState(true);
  const [track, setTrack] = useState<LatLngTuple[]>([]);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [action, setAction] = useState<MapAction | null>(null);
  const [altitude, setAltitude] = useState("10");
  const wrapRef = useRef<HTMLDivElement>(null);

  const position: LatLngTuple | null = telemetry.position ? [telemetry.position.lat, telemetry.position.lon] : null;
  const home: LatLngTuple | null = telemetry.home ? [telemetry.home.lat, telemetry.home.lon] : null;
  const heading = Math.round(telemetry.fields.heading ?? telemetry.fields.yaw ?? 0);
  const icon = useMemo(() => vehicleIcon(heading), [heading]);
  const lat = position?.[0];
  const lon = position?.[1];

  useEffect(() => {
    if (lat === undefined || lon === undefined) return;
    setTrack((points) => {
      const last = points[points.length - 1];
      if (last && last[0] === lat && last[1] === lon) return points;
      const next = [...points, [lat, lon] as LatLngTuple];
      return next.length > MAX_TRACK_POINTS ? next.slice(next.length - MAX_TRACK_POINTS) : next;
    });
  }, [lat, lon]);

  function chooseBase(next: BaseKey) {
    setBase(next);
    saveSetting("map.base", next);
  }

  function openAction(kind: MapAction["kind"]) {
    if (!menu) return;
    const remembered = String(loadSetting<number>(TAKEOFF_KEY, 10));
    const current = Math.round(telemetry.fields.alt ?? 0);
    setAltitude(kind === "flyto" && current > 0 ? String(current) : remembered);
    setAction({ kind, lat: menu.lat, lon: menu.lon });
    setMenu(null);
  }

  const altitudeValue = Number(altitude);
  const altitudeValid = Number.isFinite(altitudeValue) && altitudeValue > 0 && altitudeValue <= 10000;

  async function confirmAction() {
    if (!action) return;
    const current = action;
    setAction(null);
    try {
      let reply: string;
      if (current.kind === "takeoff") {
        saveSetting(TAKEOFF_KEY, altitudeValue);
        reply = await api.takeoff(altitudeValue);
      } else if (current.kind === "flyto") {
        reply = await api.flyTo(current.lat, current.lon, altitudeValue);
      } else {
        reply = await api.setHomeHere(current.lat, current.lon, telemetry.home?.altM ?? telemetry.fields.altasl ?? 0);
      }
      notify(`${titleOf(current.kind)}: ${reply}`, true);
    } catch (error) {
      notify(`${titleOf(current.kind)} failed: ${String(error)}`, false);
    }
  }

  const layer = BASE_LAYERS[base];
  const center: LatLngExpression = [20, 0];

  return (
    <div className="map-wrap" ref={wrapRef}>
      <MapContainer center={center} zoom={3} minZoom={2} maxZoom={19} zoomControl worldCopyJump className="map">
        <TileLayer key={base} url={layer.url} attribution={layer.attribution} maxZoom={19} />
        {track.length > 1 && <Polyline positions={track} pathOptions={{ color: "#ebcb8b", weight: 3 }} />}
        {home && <Marker position={home} icon={homeIcon} interactive={false} />}
        {position && <Marker position={position} icon={icon} interactive={false} />}
        <MapBehavior
          position={position}
          follow={follow}
          onUserDrag={() => setFollow(false)}
          onContext={setMenu}
          onDismiss={() => setMenu(null)}
        />
      </MapContainer>

      <div className="map-controls">
        {(Object.keys(BASE_LAYERS) as BaseKey[]).map((key) => (
          <button key={key} className={base === key ? "primary" : ""} onClick={() => chooseBase(key)}>
            {BASE_LAYERS[key].label}
          </button>
        ))}
        <button className={follow ? "primary" : ""} aria-pressed={follow} onClick={() => setFollow((value) => !value)}>
          Follow
        </button>
        <button onClick={() => setTrack([])} disabled={track.length === 0}>
          Clear track
        </button>
      </div>

      {menu && (
        <ul
          className="context-menu"
          style={{
            left: Math.max(0, Math.min(menu.x, (wrapRef.current?.clientWidth ?? Infinity) - MENU_WIDTH)),
            top: Math.max(0, Math.min(menu.y, (wrapRef.current?.clientHeight ?? Infinity) - MENU_HEIGHT)),
          }}
          role="menu"
        >
          <li>
            <button role="menuitem" disabled={!telemetry.connected} onClick={() => openAction("takeoff")}>
              Takeoff
            </button>
          </li>
          <li>
            <button role="menuitem" disabled={!telemetry.connected} onClick={() => openAction("flyto")}>
              Fly To Here
            </button>
          </li>
          <li>
            <button role="menuitem" disabled={!telemetry.connected} onClick={() => openAction("home")}>
              Set Home Here
            </button>
          </li>
        </ul>
      )}

      {action && (
        <ConfirmDialog
          title={`${titleOf(action.kind)}?`}
          tone="warn"
          message={
            action.kind === "takeoff"
              ? "The vehicle will switch to GUIDED and take off. Make sure the area is clear."
              : action.kind === "flyto"
                ? `Fly to ${action.lat.toFixed(6)}, ${action.lon.toFixed(6)}.`
                : `Set the home position to ${action.lat.toFixed(6)}, ${action.lon.toFixed(6)}.`
          }
          confirmLabel={titleOf(action.kind)}
          onConfirm={() => altitudeValid || action.kind === "home" ? void confirmAction() : undefined}
          onCancel={() => setAction(null)}
        >
          {action.kind !== "home" && (
            <label className="field">
              Altitude (m, relative)
              <input
                type="number"
                min={1}
                max={10000}
                value={altitude}
                aria-label="Altitude in meters"
                onChange={(event) => setAltitude(event.target.value)}
              />
            </label>
          )}
        </ConfirmDialog>
      )}
    </div>
  );
}

function titleOf(kind: MapAction["kind"]): string {
  return kind === "takeoff" ? "Takeoff" : kind === "flyto" ? "Fly To Here" : "Set Home Here";
}
