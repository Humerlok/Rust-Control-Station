/** Pure helpers for the MAVLink / DroneCAN inspector: formatting, search, sorting, grouping, watch keys and clipboard copy. */
import { inspectorKey } from "../../types";
import type { InspectorField, InspectorMessage, InspectorValue } from "../../types";

// A message with no packet for this long is shown dimmed as stale.
export const STALE_MS = 2000;

export type SortMode = "id" | "name" | "rate";

// Row identity: system/component/message id.
export const messageKey = (m: Pick<InspectorMessage, "systemId" | "componentId" | "messageId">) =>
  `${m.systemId}/${m.componentId}/${m.messageId}`;

export const isStale = (m: Pick<InspectorMessage, "ageMs">) => m.ageMs > STALE_MS;

export const isNumeric = (value: unknown): value is number => typeof value === "number" && Number.isFinite(value);

/** Numbers with at most 7 significant digits, no trailing zeros. */
export function formatNumber(value: number): string {
  if (!Number.isFinite(value)) return String(value);
  if (Number.isInteger(value)) return String(value);
  return String(Number(value.toPrecision(7)));
}

export function formatScalar(value: number | string | boolean): string {
  if (typeof value === "number") return formatNumber(value);
  return String(value);
}

export function formatRate(hz: number): string {
  return hz >= 10 ? hz.toFixed(0) : hz.toFixed(1);
}

export function formatBytes(bytesPerSecond: number): string {
  return bytesPerSecond >= 1024 ? `${(bytesPerSecond / 1024).toFixed(1)} kB/s` : `${Math.round(bytesPerSecond)} B/s`;
}

export function valueToText(value: InspectorValue): string {
  return Array.isArray(value) ? value.map((v) => formatScalar(v)).join(", ") : formatScalar(value);
}

export function fieldMatches(field: InspectorField, query: string): boolean {
  return field.name.toLowerCase().includes(query) || valueToText(field.value).toLowerCase().includes(query);
}

/** Search by message name, field name or field value (query is already lower case). */
export function messageMatches(m: InspectorMessage, query: string): boolean {
  if (!query) return true;
  return m.name.toLowerCase().includes(query) || m.fields.some((f) => fieldMatches(f, query));
}

export function sortMessages(list: InspectorMessage[], mode: SortMode): InspectorMessage[] {
  const sorted = [...list];
  if (mode === "id") sorted.sort((a, b) => a.messageId - b.messageId);
  else if (mode === "name") sorted.sort((a, b) => a.name.localeCompare(b.name));
  else sorted.sort((a, b) => b.hz - a.hz || a.messageId - b.messageId);
  return sorted;
}

export type ComponentGroup = {
  key: string;
  componentId: number;
  componentName: string;
  messages: InspectorMessage[];
};
export type SystemGroup = { key: string; systemId: number; components: ComponentGroup[] };

// System > component > messages tree; ids ascending, messages ordered by `mode`.
export function groupMessages(messages: InspectorMessage[], mode: SortMode): SystemGroup[] {
  const systems = new Map<number, Map<number, ComponentGroup>>();
  for (const m of messages) {
    let components = systems.get(m.systemId);
    if (!components) systems.set(m.systemId, (components = new Map()));
    let group = components.get(m.componentId);
    if (!group) {
      components.set(
        m.componentId,
        (group = {
          key: `c:${m.systemId}/${m.componentId}`,
          componentId: m.componentId,
          componentName: m.componentName,
          messages: [],
        }),
      );
    }
    group.messages.push(m);
  }
  return [...systems.entries()]
    .sort(([a], [b]) => a - b)
    .map(([systemId, components]) => ({
      key: `s:${systemId}`,
      systemId,
      components: [...components.values()]
        .sort((a, b) => a.componentId - b.componentId)
        .map((c) => ({ ...c, messages: sortMessages(c.messages, mode) })),
    }));
}

/** Watch key of a field, or of one element of an array field. */
export const watchKey = (m: InspectorMessage, field: string, index?: number) =>
  inspectorKey(m, index === undefined ? field : `${field}[${index}]`);

/** Human label of a watch key, using the message name when it is known. */
export function watchLabel(key: string, names: Map<string, string>): string {
  const [system, component, id, ...rest] = key.split("/");
  const field = rest.join("/");
  const name = names.get(`${system}/${component}/${id}`) ?? `#${id}`;
  return `${name}.${field}`;
}

export function messageToJson(m: InspectorMessage): string {
  return JSON.stringify(
    {
      name: m.name,
      messageId: m.messageId,
      systemId: m.systemId,
      componentId: m.componentId,
      fields: Object.fromEntries(m.fields.map((f) => [f.name, f.value])),
    },
    null,
    2,
  );
}

export function messageToText(m: InspectorMessage): string {
  const header = `${m.name} (#${m.messageId}) from system ${m.systemId} / component ${m.componentId}`;
  return [header, ...m.fields.map((f) => `${f.name}: ${valueToText(f.value)}`)].join("\n");
}

export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // clipboard API unavailable or denied: fall back to a hidden textarea
  }
  try {
    const area = document.createElement("textarea");
    area.value = text;
    area.setAttribute("readonly", "");
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.select();
    const ok = document.execCommand("copy");
    area.remove();
    return ok;
  } catch {
    return false;
  }
}
