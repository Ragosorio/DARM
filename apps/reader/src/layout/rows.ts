/**
 * The intermediate form between blocks and pages.
 *
 * Every flowing block is flattened into `Row`s: horizontal slices (a line
 * of text, a figure, a widget, a strip of padding) positioned relative to
 * the left edge of the text column and to their own top. The paginator only
 * has to stack rows and decide where a page may end, which is expressed by
 * `keepWithNext`. That single flag implements widows and orphans, headings
 * that stay with their text, figures that stay with their captions and
 * choices that stay whole.
 */
import type { Anim, Color } from "../ir";
import type { DrawItem, Region, SemanticNode } from "./types";

/** Decoration of a container that may be cut by a page break; drawn once per page fragment. */
export interface BoxDeco {
  id: string;
  style: "rule" | "tint" | "bar" | "quote";
  /** Extent relative to the column. */
  x: number;
  w: number;
  /** Padding re-inserted above / below when a page break cuts the box. */
  padTop: number;
  padBottom: number;
  /** Shortens the decoration at both ends (quote rules stop at the ink, not at the line box). */
  inset: number;
  anim?: Anim;
  animBlock?: string;
}

/** What a row contributes to the accessibility outline. Rows with the same key merge into one node. */
export interface RowSem {
  key: string;
  role: SemanticNode["role"];
  level?: 1 | 2 | 3;
  ordered?: boolean;
  /** List item index. */
  item?: number;
  text: string;
  /** The row ends inside a hyphenated word. */
  glue?: boolean;
  block?: string;
}

export interface FixedPayload {
  background?: Color;
  items: DrawItem[];
  regions: Region[];
  semantics: SemanticNode[];
  ids: string[];
}

export interface Row {
  kind: "content" | "break" | "fixed";
  h: number;
  items: DrawItem[];
  regions: Region[];
  /** Block ids this row belongs to, outermost first; a page lists them in `blocks`. */
  ids: string[];
  /** Innermost block, for the page anchor. */
  block: string | null;
  /** Index of this line within its block. */
  line: number;
  /** White space before / after; adjacent gaps collapse to the larger, and vanish at the top of a page. */
  gapBefore: number;
  gapAfter: number;
  keepWithNext: boolean;
  /** Snap the top of the row to the baseline grid. */
  snap: boolean;
  /** Pure white space: dropped at the top or bottom of a page. */
  discardable: boolean;
  /** Enclosing decorated containers, outermost first. */
  boxes: BoxDeco[];
  /** This row is the top / bottom padding strip of a box. */
  padOf?: BoxDeco;
  padEdge?: "top" | "bottom";
  sem?: RowSem[];
  fixed?: FixedPayload;
}

export function makeRow(h: number, ids: string[], block: string | null, boxes: BoxDeco[]): Row {
  return {
    kind: "content",
    h,
    items: [],
    regions: [],
    ids,
    block,
    line: 0,
    gapBefore: 0,
    gapAfter: 0,
    keepWithNext: false,
    snap: false,
    discardable: false,
    boxes,
  };
}

/** Merge row semantics, in order, into outline nodes. */
export function mergeSemantics(rows: RowSem[]): SemanticNode[] {
  const out: SemanticNode[] = [];
  let lastKey: string | null = null;
  let lastGlue = false;
  let lastItem = -1;
  const join = (a: string, b: string): string => (a === "" ? b : lastGlue ? a + b : `${a} ${b}`);
  for (const s of rows) {
    const prev = out[out.length - 1];
    if (s.role === "separator") {
      out.push({ role: "separator" });
      lastKey = null;
    } else if (s.role === "image") {
      out.push(s.block !== undefined ? { role: "image", alt: s.text, block: s.block } : { role: "image", alt: s.text });
      lastKey = null;
    } else if (s.role === "list") {
      if (prev && prev.role === "list" && lastKey === s.key) {
        if (s.item === lastItem && prev.items.length > 0) {
          prev.items[prev.items.length - 1] = join(prev.items[prev.items.length - 1]!, s.text);
        } else prev.items.push(s.text);
      } else {
        const node: SemanticNode = { role: "list", ordered: s.ordered === true, items: [s.text] };
        if (s.block !== undefined) node.block = s.block;
        out.push(node);
      }
      lastKey = s.key;
      lastItem = s.item ?? -1;
    } else if (prev && lastKey === s.key && prev.role === s.role) {
      prev.text = join(prev.text, s.text);
    } else {
      let node: SemanticNode;
      if (s.role === "heading") node = { role: "heading", level: s.level ?? 2, text: s.text };
      else node = { role: s.role, text: s.text };
      if (s.block !== undefined) node.block = s.block;
      out.push(node);
      lastKey = s.key;
    }
    lastGlue = s.glue === true;
  }
  return out;
}
