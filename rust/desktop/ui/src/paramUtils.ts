import type { ParamEntry, ParamMeta } from "./types";

const INTEGER_TYPES = new Set(["INT8", "UINT8", "INT16", "UINT16", "INT32", "UINT32", "INT64", "UINT64"]);

export function isIntegerType(paramType: string): boolean {
  return INTEGER_TYPES.has(paramType);
}

/** Shows ints as ints and floats without float32 noise (0.10000000149 -> 0.1). */
export function formatValue(value: number, paramType: string): string {
  if (!Number.isFinite(value)) return String(value);
  if (isIntegerType(paramType)) return String(Math.round(value));
  return String(Number(value.toPrecision(7)));
}

/** Parameters travel as float32, so equality is float32 equality. */
export function sameValue(a: number, b: number): boolean {
  return Math.fround(a) === Math.fround(b);
}

export function naturalCompare(a: string, b: string): number {
  return a.localeCompare(b, "en", { numeric: true });
}

export function isEnableName(name: string): boolean {
  return name.endsWith("ENABLE");
}

export function outOfRange(value: number, meta: ParamMeta | undefined): boolean {
  if (!meta) return false;
  return (meta.min !== null && value < meta.min) || (meta.max !== null && value > meta.max);
}

export function rangeText(meta: ParamMeta | undefined): string {
  if (!meta || (meta.min === null && meta.max === null)) return "";
  return `${meta.min ?? "…"} – ${meta.max ?? "…"}`;
}

export type TreeNode = { key: string; label: string; count: number; children: TreeNode[] };

/**
 * Prefix tree split on `_`. A node exists only where more than one parameter shares the prefix,
 * so `Q_PLT_Y_RATE` and `Q_PLT_Y_RATE_TC` share a node but a lone `Q_FOO_BAR` stays under `Q`.
 */
export function buildTree(entries: ParamEntry[]): TreeNode[] {
  const counts = new Map<string, number>();
  const children = new Map<string, Set<string>>();
  const roots = new Set<string>();
  for (const { name } of entries) {
    const parts = name.split("_");
    let prefix = "";
    for (let i = 0; i < parts.length - 1; i += 1) {
      const next = prefix === "" ? parts[i] : `${prefix}_${parts[i]}`;
      counts.set(next, (counts.get(next) ?? 0) + 1);
      if (prefix === "") roots.add(next);
      else {
        let set = children.get(prefix);
        if (!set) children.set(prefix, (set = new Set()));
        set.add(next);
      }
      prefix = next;
    }
  }
  const make = (key: string): TreeNode => ({
    key,
    label: key.slice(key.lastIndexOf("_") + 1),
    count: counts.get(key) ?? 0,
    children: [...(children.get(key) ?? [])]
      .filter((child) => (counts.get(child) ?? 0) > 1)
      .sort(naturalCompare)
      .map(make),
  });
  return [...roots]
    .filter((root) => (counts.get(root) ?? 0) > 1)
    .sort(naturalCompare)
    .map((root) => ({ ...make(root), label: root }));
}

export function inNode(name: string, node: string | null): boolean {
  return node === null || name === node || name.startsWith(`${node}_`);
}
