/**
 * Fixed-row-height virtualized list: only rows in view (plus OVERSCAN) are mounted.
 * Props: row count, rowHeight in px, a render(index) callback and an optional className.
 */
import { useEffect, useRef, useState, type ReactNode } from "react";

type Props = {
  count: number;
  rowHeight: number;
  render: (index: number) => ReactNode;
  className?: string;
};

const OVERSCAN = 8;

/** Renders only the rows in view; every row has the same fixed height. */
export function VirtualList({ count, rowHeight, render, className }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  // 600 is a guess used until the container is measured.
  const [height, setHeight] = useState(600);

  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const measure = () => setHeight(element.clientHeight);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  // Visible window from scroll offset and height; rows are absolutely positioned inside a spacer of count * rowHeight.
  const first = Math.max(0, Math.floor(scrollTop / rowHeight) - OVERSCAN);
  const last = Math.min(count, Math.ceil((scrollTop + height) / rowHeight) + OVERSCAN);
  const rows: ReactNode[] = [];
  for (let i = first; i < last; i += 1) {
    rows.push(
      <div key={i} className="vrow" style={{ top: i * rowHeight, height: rowHeight }}>
        {render(i)}
      </div>,
    );
  }

  return (
    <div ref={ref} className={`vlist ${className ?? ""}`} onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}>
      <div style={{ height: count * rowHeight, position: "relative" }}>{rows}</div>
    </div>
  );
}
