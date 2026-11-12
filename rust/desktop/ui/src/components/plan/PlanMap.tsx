/**
 * Mission planning map: numbered draggable waypoint pins, route polyline (closed for fences), vehicle marker and a context menu.
 * Click adds a point, right-click opens the menu, dragging a pin moves it; the home pin is fixed. Reuses the "map.base" setting.
 */
import L, { type LatLngTuple } from "leaflet";
import "leaflet/dist/leaflet.css";
import { useEffect, useMemo, useRef, useState } from "react";
import { MapContainer, Marker, Polyline, TileLayer, useMap, useMapEvents } from "react-leaflet";
import { loadSetting, saveSetting } from "../../storage";
import type { MissionItem, MissionKind, Telemetry } from "../../types";
import { hasHome, isPositional, type CommandMap } from "./missionModel";

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

// Approximate context-menu size in px, used to keep the menu inside the map.
const MENU_WIDTH = 190;
const MENU_HEIGHT = 150;

// DivIcons are cached per label/state so markers do not rebuild their DOM on every render.
const pinCache = new Map<string, L.DivIcon>();
function pinIcon(label: string, flags: { current: boolean; selected: boolean; home: boolean }): L.DivIcon {
  const key = `${label}|${flags.current}|${flags.selected}|${flags.home}`;
  let icon = pinCache.get(key);
  if (!icon) {
    const classes = ["wp-pin", flags.home ? "home" : "", flags.current ? "current" : "", flags.selected ? "selected" : ""].join(" ");
    icon = L.divIcon({ className: "wp-icon", html: `<span class="${classes}">${label}</span>`, iconSize: [28, 28], iconAnchor: [14, 14] });
    pinCache.set(key, icon);
  }
  return icon;
}

function vehicleIcon(heading: number): L.DivIcon {
  const svg =
    `<svg viewBox="0 0 34 34" width="30" height="30" style="transform: rotate(${heading}deg)">` +
    `<path d="M17 3 L27 29 L17 23 L7 29 Z" fill="#88c0d0" stroke="#2e3440" stroke-width="2" stroke-linejoin="round"/></svg>`;
  return L.divIcon({ className: "vehicle-icon", html: svg, iconSize: [30, 30], iconAnchor: [15, 15] });
}

export type MapMenu = { x: number; y: number } & ({ target: "map"; lat: number; lon: number } | { target: "marker"; index: number });

type BehaviorProps = {
  fallback: LatLngTuple | null;
  points: LatLngTuple[];
  fitSignal: number;
  focus: { index: number; token: number } | null;
  focusPoint: LatLngTuple | null;
  menuOpen: boolean;
  onAdd: (lat: number, lon: number, center: { lat: number; lon: number }) => void;
  onContext: (menu: MapMenu) => void;
  onDismiss: () => void;
};

// Imperative Leaflet behaviour: resize, initial view, fit and focus requests, click and context events.
function MapBehavior({ fallback, points, fitSignal, focus, focusPoint, menuOpen, onAdd, onContext, onDismiss }: BehaviorProps) {
  const map = useMap();
  const placed = useRef(false);
  const pointsRef = useRef(points);
  pointsRef.current = points;

  useEffect(() => {
    // Leaflet does not notice container resizes, so re-measure.
    const observer = new ResizeObserver(() => map.invalidateSize());
    observer.observe(map.getContainer());
    return () => observer.disconnect();
  }, [map]);

  // First sensible view: the mission if there is one, otherwise where the vehicle is.
  useEffect(() => {
    if (placed.current) return;
    if (pointsRef.current.length > 0) {
      placed.current = true;
      fit(map, pointsRef.current);
    } else if (fallback) {
      placed.current = true;
      map.setView(fallback, 16, { animate: false });
    }
  }, [map, fallback, points.length]);

  useEffect(() => {
    if (fitSignal > 0 && pointsRef.current.length > 0) fit(map, pointsRef.current);
  }, [map, fitSignal]);

  useEffect(() => {
    if (focus && focusPoint) map.panTo(focusPoint, { animate: true });
    // Only a new focus request pans the map; moving the focused marker must not.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [map, focus?.token]);

  useMapEvents({
    click: (event) => {
      if (menuOpen) {
        onDismiss();
        return;
      }
      const center = map.getCenter();
      onAdd(event.latlng.lat, event.latlng.lng, { lat: center.lat, lon: center.lng });
    },
    contextmenu: (event) =>
      onContext({ x: event.containerPoint.x, y: event.containerPoint.y, target: "map", lat: event.latlng.lat, lon: event.latlng.lng }),
    dragstart: onDismiss,
    zoomstart: onDismiss,
  });
  return null;
}

function fit(map: L.Map, points: LatLngTuple[]) {
  if (points.length === 1) {
    map.setView(points[0], 17, { animate: false });
    return;
  }
  map.fitBounds(L.latLngBounds(points), { padding: [50, 50], maxZoom: 18, animate: false });
}

type Props = {
  kind: MissionKind;
  items: MissionItem[];
  commands: CommandMap;
  selected: number | null;
  currentSeq: number | null;
  telemetry: Telemetry;
  fitSignal: number;
  focus: { index: number; token: number } | null;
  canSetCurrent: boolean;
  onSelect: (index: number) => void;
  onAdd: (lat: number, lon: number, center: { lat: number; lon: number }) => void;
  onMove: (index: number, lat: number, lon: number) => void;
  onDelete: (index: number) => void;
  onInsert: (index: number, where: "before" | "after") => void;
  onSetCurrent: (index: number) => void;
  onSetHome: (lat: number, lon: number) => void;
  onFit: () => void;
};

export function PlanMap(props: Props) {
  const { kind, items, commands, selected, currentSeq, telemetry, fitSignal, focus, canSetCurrent } = props;
  const [base, setBase] = useState<BaseKey>(() => loadSetting<BaseKey>("map.base", "sat"));
  const [menu, setMenu] = useState<MapMenu | null>(null);
  const wrapRef = useRef<HTMLDivElement>(null);

  const positional = useMemo(
    () => items.map((item, index) => ({ item, index })).filter(({ item }) => isPositional(item, commands)),
    [items, commands],
  );
  const points = useMemo<LatLngTuple[]>(() => positional.map(({ item }) => [item.x, item.y]), [positional]);
  const vehicle: LatLngTuple | null = telemetry.position ? [telemetry.position.lat, telemetry.position.lon] : null;
  const homePoint: LatLngTuple | null = telemetry.home ? [telemetry.home.lat, telemetry.home.lon] : null;
  const fallback = vehicle ?? homePoint;
  const heading = Math.round(telemetry.fields.heading ?? telemetry.fields.yaw ?? 0);
  const vIcon = useMemo(() => vehicleIcon(heading), [heading]);
  const focusItem = focus ? items[focus.index] : undefined;
  const focusPoint: LatLngTuple | null = focusItem ? [focusItem.x, focusItem.y] : null;
  // A fence polygon is drawn closed once it has three vertices.
  const closeFence = kind === "fence" && points.length >= 3;

  function chooseBase(next: BaseKey) {
    setBase(next);
    saveSetting("map.base", next);
  }

  const layer = BASE_LAYERS[base];
  const menuLeft = menu ? Math.max(0, Math.min(menu.x, (wrapRef.current?.clientWidth ?? Infinity) - MENU_WIDTH)) : 0;
  const menuTop = menu ? Math.max(0, Math.min(menu.y, (wrapRef.current?.clientHeight ?? Infinity) - MENU_HEIGHT)) : 0;
  // First editable index: mission item 0 is the fixed home.
  const free = kind === "mission" ? 1 : 0;

  function run(action: () => void) {
    setMenu(null);
    action();
  }

  return (
    <div className="plan-map" ref={wrapRef}>
      <MapContainer center={[20, 0]} zoom={3} minZoom={2} maxZoom={19} zoomControl worldCopyJump className="map">
        <TileLayer key={base} url={layer.url} attribution={layer.attribution} maxZoom={19} />
        {points.length > 1 && <Polyline positions={points} pathOptions={{ color: "#ebcb8b", weight: 3 }} />}
        {closeFence && (
          <Polyline positions={[points[points.length - 1], points[0]]} pathOptions={{ color: "#ebcb8b", weight: 2, dashArray: "6 6" }} />
        )}
        {positional.map(({ item, index }) => {
          const isHome = hasHome(kind) && index === 0;
          const icon = pinIcon(isHome ? "H" : String(index), {
            home: isHome,
            current: currentSeq === index,
            selected: selected === index,
          });
          return (
            <Marker
              key={`${kind}-${index}`}
              position={[item.x, item.y]}
              icon={icon}
              draggable={!isHome}
              zIndexOffset={selected === index ? 1000 : 0}
              eventHandlers={{
                click: () => props.onSelect(index),
                dragend: (event) => {
                  const at = (event.target as L.Marker).getLatLng();
                  props.onMove(index, at.lat, at.lng);
                },
                contextmenu: (event) => {
                  props.onSelect(index);
                  setMenu({ x: event.containerPoint.x, y: event.containerPoint.y, target: "marker", index });
                },
              }}
            />
          );
        })}
        {vehicle && <Marker position={vehicle} icon={vIcon} interactive={false} zIndexOffset={2000} />}
        <MapBehavior
          fallback={fallback}
          points={points}
          fitSignal={fitSignal}
          focus={focus}
          focusPoint={focusPoint}
          menuOpen={menu !== null}
          onAdd={props.onAdd}
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
        <button onClick={props.onFit} disabled={points.length === 0} title="Zoom to the whole mission">
          Fit to mission
        </button>
      </div>

      {menu && (
        <ul className="context-menu" style={{ left: menuLeft, top: menuTop }} role="menu">
          {menu.target === "map" ? (
            <>
              <li>
                <button role="menuitem" onClick={() => run(() => props.onAdd(menu.lat, menu.lon, { lat: menu.lat, lon: menu.lon }))}>
                  Add waypoint here
                </button>
              </li>
              {hasHome(kind) && (
                <li>
                  <button role="menuitem" onClick={() => run(() => props.onSetHome(menu.lat, menu.lon))}>
                    Set home here
                  </button>
                </li>
              )}
            </>
          ) : menu.index < free ? (
            <li>
              <button role="menuitem" disabled>
                Home cannot be changed here
              </button>
            </li>
          ) : (
            <>
              <li>
                <button role="menuitem" onClick={() => run(() => props.onDelete(menu.index))}>
                  Delete
                </button>
              </li>
              <li>
                <button role="menuitem" onClick={() => run(() => props.onInsert(menu.index, "before"))}>
                  Insert waypoint before
                </button>
              </li>
              <li>
                <button role="menuitem" onClick={() => run(() => props.onInsert(menu.index, "after"))}>
                  Insert waypoint after
                </button>
              </li>
              {kind === "mission" && (
                <li>
                  <button role="menuitem" disabled={!canSetCurrent} onClick={() => run(() => props.onSetCurrent(menu.index))}>
                    Set as current waypoint
                  </button>
                </li>
              )}
            </>
          )}
        </ul>
      )}
    </div>
  );
}
