/** Time/value buffers behind the inspector graph. Times are milliseconds from the backend (GraphPanel converts to seconds). */
import type { InspectorPoint } from "../../types";

export type SeriesBuffer = { t: number[]; v: number[] };

/** Points kept per series; the oldest are dropped past this (with some slack to stay cheap). */
export const SERIES_CAP = 60_000;
const SLACK = 5_000;

/**
 * Plot data lives outside React state: snapshots arrive several times per second and the graph
 * redraws itself from here. Only tracked keys are stored, so late points of a removed series
 * are ignored.
 */
export class SeriesStore {
  private buffers = new Map<string, SeriesBuffer>();
  private listeners = new Set<() => void>();

  track(key: string): void {
    if (!this.buffers.has(key)) this.buffers.set(key, { t: [], v: [] });
  }

  untrack(key: string): void {
    if (this.buffers.delete(key)) this.emit();
  }

  get(key: string): SeriesBuffer | undefined {
    return this.buffers.get(key);
  }

  last(key: string): number | undefined {
    const buffer = this.buffers.get(key);
    return buffer && buffer.v.length > 0 ? buffer.v[buffer.v.length - 1] : undefined;
  }

  append(series: Record<string, InspectorPoint[]>): void {
    let changed = false;
    for (const [key, points] of Object.entries(series)) {
      const buffer = this.buffers.get(key);
      if (!buffer || points.length === 0) continue;
      // Time going backwards means the backend restarted its clock: start a fresh session.
      if (buffer.t.length > 0 && points[0][0] < buffer.t[buffer.t.length - 1]) {
        buffer.t.length = 0;
        buffer.v.length = 0;
      }
      for (const [time, value] of points) {
        if (!Number.isFinite(value)) continue;
        buffer.t.push(time);
        buffer.v.push(value);
      }
      if (buffer.t.length > SERIES_CAP + SLACK) {
        const drop = buffer.t.length - SERIES_CAP;
        buffer.t.splice(0, drop);
        buffer.v.splice(0, drop);
      }
      changed = true;
    }
    if (changed) this.emit();
  }

  /** Empties every buffer but keeps the keys tracked. */
  clear(): void {
    for (const buffer of this.buffers.values()) {
      buffer.t.length = 0;
      buffer.v.length = 0;
    }
    this.emit();
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private emit(): void {
    this.listeners.forEach((listener) => listener());
  }
}
