/**
 * Collapsible tree of parameter groups (prefix hierarchy); selecting a node filters the table, null means all parameters.
 * Expansion state is local to this component.
 */
import { useState } from "react";
import type { TreeNode } from "../paramUtils";

type Props = {
  nodes: TreeNode[];
  total: number;
  selected: string | null;
  onSelect: (key: string | null) => void;
};

// One tree level, indented 14 px per depth.
function Branch({ node, depth, selected, expanded, toggle, onSelect }: {
  node: TreeNode;
  depth: number;
  selected: string | null;
  expanded: Set<string>;
  toggle: (key: string) => void;
  onSelect: (key: string | null) => void;
}) {
  const open = expanded.has(node.key);
  return (
    <>
      <div className={`tree-row ${selected === node.key ? "active" : ""}`} style={{ paddingLeft: 6 + depth * 14 }}>
        <button
          className="tree-toggle"
          aria-label={open ? "Collapse" : "Expand"}
          disabled={node.children.length === 0}
          onClick={() => toggle(node.key)}
        >
          {node.children.length === 0 ? "" : open ? "▾" : "▸"}
        </button>
        <button className="tree-label" onClick={() => onSelect(node.key)}>
          {node.label}
          <span className="dim"> {node.count}</span>
        </button>
      </div>
      {open &&
        node.children.map((child) => (
          <Branch
            key={child.key}
            node={child}
            depth={depth + 1}
            selected={selected}
            expanded={expanded}
            toggle={toggle}
            onSelect={onSelect}
          />
        ))}
    </>
  );
}

export function ParamTree({ nodes, total, selected, onSelect }: Props) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const toggle = (key: string) =>
    setExpanded((current) => {
      const next = new Set(current);
      if (!next.delete(key)) next.add(key);
      return next;
    });

  return (
    <div className="param-tree" role="tree" aria-label="Parameter groups">
      <div className={`tree-row ${selected === null ? "active" : ""}`}>
        <button className="tree-label" onClick={() => onSelect(null)}>
          All parameters<span className="dim"> {total}</span>
        </button>
      </div>
      {nodes.map((node) => (
        <Branch key={node.key} node={node} depth={0} selected={selected} expanded={expanded} toggle={toggle} onSelect={onSelect} />
      ))}
    </div>
  );
}
