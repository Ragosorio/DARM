/**
 * IR blocks → rows. One function per block type; all of them work in the
 * coordinate system of a column (`FlowEnv.x`, `FlowEnv.w`), so the same
 * code sets a paragraph in the page column, inside a box, inside a quote or
 * inside a frame of a fixed page.
 */
import { displayValue, holds } from "../core/state";
import type { Align, Anim, Block, BlockOf, Color, Link, Span, TextStyle } from "../ir";
import { own, type Family, type Lx } from "./context";
import { buildFixed } from "./fixed";
import { makeRow, type BoxDeco, type Row, type RowSem } from "./rows";
import { breakLines, ellipsize, placeLine, tokenize, type Piece, type RunStyle } from "./text";
import type { DrawItem, Region, Widget } from "./types";
import { MIN_TARGET, buttonLabelSize, galleryStripHeight, rowWidgetLayout } from "./widgetMetrics";

export interface FlowEnv {
  /** Column box available to the block, relative to the page column. */
  x: number;
  w: number;
  /** Body size and body line height in this context (fixed frames shrink them). */
  B: number;
  L: number;
  /** Body lines may snap to the baseline grid (false inside boxes, quotes and frames). */
  snap: boolean;
  /** Ids of the enclosing blocks, outermost first. */
  ids: string[];
  boxes: BoxDeco[];
  /** Entrance animation inherited from an enclosing block. */
  anim?: { anim: Anim; block: string };
  /** Tallest unbreakable thing that still fits a page (or the frame). */
  maxH: number;
  fixed: boolean;
  depth: number;
}

export interface Flow {
  rows: Row[];
  /** Ids of blocks that produced no row yet; they attach to the next row. */
  pending: string[];
  /** The previous block was an ordinary paragraph: the next one gets a first-line indent. */
  prevPara: boolean;
  /** The previous block was a paragraph of small print. */
  prevSmall: boolean;
}

export function newFlow(): Flow {
  return { rows: [], pending: [], prevPara: false, prevSmall: false };
}

export interface TextSpec {
  family: Family;
  size: number;
  weight: number;
  italic: boolean;
  color: Color;
  /** Line height, px. */
  lh: number;
  letterSpacing?: number;
}

const MAX_DEPTH = 24;
const HEX = /^#(?:[0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/i;

function colorOf(c: Color | undefined, fallback: Color): Color {
  return typeof c === "string" && HEX.test(c) ? c : fallback;
}

function scaleOf(b: { scale?: number }): number {
  const s = b.scale;
  return typeof s === "number" && Number.isFinite(s) ? Math.max(0.25, Math.min(8, s)) : 1;
}

function withAnim(env: FlowEnv, b: Block): FlowEnv {
  const a = b.anim;
  if (!a || typeof a !== "object") return env;
  return { ...env, anim: { anim: a, block: b.id } };
}

function commit(flow: Flow, env: FlowEnv, rows: Row[]): void {
  const first = rows[0];
  if (!first) return;
  if (flow.pending.length > 0) {
    first.ids = [...first.ids, ...flow.pending];
    flow.pending = [];
  }
  if (env.anim) {
    for (const r of rows) {
      for (const it of r.items) {
        if (it.anim) continue;
        it.anim = env.anim.anim;
        it.block = env.anim.block;
      }
    }
  }
  for (const r of rows) flow.rows.push(r);
}

/**
 * Give a set-off block (figure, heading, box…) a total height, gaps
 * included, that is a whole number of body lines, so the text after it
 * falls back on the baseline grid without a visible jump. `bias` is the
 * share of the rounding slack that goes above the block.
 */
function slot(env: FlowEnv, rows: Row[], gapBefore: number, gapAfter: number, bias = 0.5): void {
  const first = rows[0];
  const last = rows[rows.length - 1];
  if (!first || !last) return;
  let before = gapBefore;
  let after = gapAfter;
  if (env.snap) {
    let total = before + after;
    for (let i = 0; i < rows.length; i++) {
      const r = rows[i]!;
      total += r.h;
      const next = rows[i + 1];
      if (next) total += Math.max(r.gapAfter, next.gapBefore);
    }
    const extra = Math.ceil((total - 0.01) / env.L) * env.L - total;
    before += extra * bias;
    after += extra * (1 - bias);
  }
  first.gapBefore = before;
  last.gapAfter = after;
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

/** Resolve spans (bold, italic, code, links, `{var}`) against a base style. */
export function toPieces(lx: Lx, spans: Span[] | undefined, spec: TextSpec): Piece[] {
  const out: Piece[] = [];
  if (!Array.isArray(spans)) return out;
  const styles = new Map<string, RunStyle>();
  // Consecutive spans with the same target are one link (it may contain bold or italic words);
  // the same target used again later in the paragraph is another link.
  let run = 0;
  let runKey = "";
  let runLink: Link | undefined;
  const labels = new Map<Link, string>();
  for (const sp of spans) {
    if (!sp || typeof sp !== "object") continue;
    const raw = sp.var !== undefined ? displayValue(lx.stateValue(String(sp.var))) : sp.text;
    if (typeof raw !== "string" || raw === "") {
      if (sp.break) out.push("br");
      continue;
    }
    let link: Link | undefined;
    let linkKey = "";
    if (sp.link && typeof sp.link === "object") {
      const target = sp.link.kind === "anchor" ? `a:${sp.link.target}` : `u:${(sp.link as { href?: string }).href ?? ""}`;
      if (target !== runKey || !runLink) {
        run++;
        runKey = target;
        runLink = { ...sp.link };
      }
      link = runLink;
      linkKey = `L${run}`;
      labels.set(link, (labels.get(link) ?? "") + raw);
    } else {
      runKey = "";
      runLink = undefined;
    }
    const key = `${sp.bold ? 1 : 0}${sp.italic ? 1 : 0}${sp.code ? 1 : 0}|${linkKey}`;
    let st = styles.get(key);
    if (!st) {
      const size = sp.code ? spec.size * 0.88 : spec.size;
      st = {
        font: lx.font({
          family: sp.code ? "mono" : spec.family,
          size,
          weight: sp.bold ? (spec.weight >= 600 ? 800 : 700) : spec.weight,
          // Italic inside italic text turns upright, as in print.
          italic: sp.italic ? !spec.italic : spec.italic,
        }),
        size,
        color: link ? lx.input.palette.accent : spec.color,
      };
      // A monospace word space is 0.6 em: far too wide inside a line of prose. Code keeps the text's own space.
      if (sp.code && spec.family !== "mono") st.spaceWidth = lx.width(lx.font(spec), " ") * 1.1;
      if (link) st.link = link;
      if (spec.letterSpacing) st.letterSpacing = spec.letterSpacing;
      styles.set(key, st);
    }
    out.push({ text: raw, st });
    // `break` marks a hard line break AFTER the span's text.
    if (sp.break) out.push("br");
  }
  for (const st of styles.values()) if (st.link) st.linkLabel = (labels.get(st.link) ?? "").replace(/\s+/g, " ").trim();
  return out;
}

export function plainText(lx: Lx, spans: Span[] | undefined): string {
  if (!Array.isArray(spans)) return "";
  let s = "";
  for (const sp of spans) {
    if (!sp || typeof sp !== "object") continue;
    const raw = sp.var !== undefined ? displayValue(lx.stateValue(String(sp.var))) : sp.text;
    if (typeof raw === "string") s += raw;
    if (sp.break) s += " ";
  }
  return s.replace(/\s+/g, " ").trim();
}

function plainPiece(lx: Lx, text: string, spec: TextSpec): Piece {
  const st: RunStyle = { font: lx.font(spec), size: spec.size, color: spec.color };
  if (spec.letterSpacing) st.letterSpacing = spec.letterSpacing;
  return { text, st };
}

/** Distance from the top of a line box to its baseline: the font's ascent/descent centred in the leading. */
export function baselineOffset(lx: Lx, spec: TextSpec): number {
  const m = lx.metrics(lx.font(spec));
  return (spec.lh - (m.ascent + m.descent)) / 2 + m.ascent;
}

export interface ParaOpts {
  spec: TextSpec;
  align: Align;
  /** Line box, column coordinates. */
  x: number;
  w: number;
  indent?: number;
  /** Indent of lines that continue a wrapped line (verse). */
  hang?: number;
  hyphenate?: boolean;
  snap?: boolean;
  /** Never split across pages. */
  keepAll?: boolean;
  lineStart?: number;
  /** Extra left inset of a line (the lines beside a drop capital). */
  shape?: (line: number) => number;
  sem?: Omit<RowSem, "text" | "glue">;
  /** Prepended to the semantic text of the first line. */
  semPrefix?: string;
}

/** Set one paragraph: one row per line, with link regions and widow/orphan flags. */
export function textRows(lx: Lx, env: FlowEnv, id: string, pieces: Piece[], o: ParaOpts): Row[] {
  const spec = o.spec;
  const base = baselineOffset(lx, spec);
  const indent = o.indent ?? 0;
  const hang = o.hang ?? 0;
  const inset = (i: number, cont: boolean): number => (i === 0 ? indent : cont ? hang : 0) + (o.shape ? o.shape(i) : 0);
  const lines = breakLines(tokenize(pieces, lx.measure), (i, cont) => o.w - inset(i, cont), {
    measure: lx.measure,
    justify: o.align === "justify",
    hyphenate: o.hyphenate ? lx.hyphenate : undefined,
    hyphenCache: lx.hyphenCache,
  });
  const ids = [...env.ids, id];
  const rows: Row[] = [];
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!;
    const prev = lines[i - 1];
    const cont = prev !== undefined && (prev.end === "soft" || prev.end === "hyphen");
    const dx = inset(i, cont);
    const placed = placeLine(line, o.x + dx, base, Math.max(1, o.w - dx), o.align, id);
    const row = makeRow(spec.lh, ids, id, env.boxes);
    row.items = placed.items;
    row.line = (o.lineStart ?? 0) + i;
    row.snap = o.snap === true && env.snap;
    for (const f of placed.links) {
      row.regions.push({ type: "link", x: f.x, y: 0, w: f.w, h: spec.lh, block: id, link: f.link, label: f.label });
    }
    if (o.sem) {
      const text = i === 0 && o.semPrefix ? o.semPrefix + placed.text : placed.text;
      row.sem = [{ ...o.sem, text, glue: placed.glue || undefined }];
    }
    rows.push(row);
  }
  const n = rows.length;
  if (o.keepAll || n < 4) {
    for (let i = 0; i < n - 1; i++) rows[i]!.keepWithNext = true;
  } else {
    // Widows and orphans: at least two lines on either side of a page break.
    rows[0]!.keepWithNext = true;
    rows[n - 2]!.keepWithNext = true;
  }
  return rows;
}

interface StyleInfo {
  spec: TextSpec;
  align: Align;
  hyphenate: boolean;
  snap: boolean;
}

function styleInfo(lx: Lx, env: FlowEnv, style: TextStyle, scale: number, color: Color | undefined): StyleInfo {
  const pal = lx.input.palette;
  const B = env.B * scale;
  const L = env.L * scale;
  const ink = colorOf(color, pal.ink);
  const just: Align = lx.justify ? "justify" : "left";
  const onGrid = scale === 1;
  switch (style) {
    case "lead":
      return { spec: { family: "body", size: B * 1.18, weight: 400, italic: false, color: ink, lh: Math.round(L * 1.2) }, align: just, hyphenate: true, snap: false };
    case "small":
      return { spec: { family: "body", size: B * 0.86, weight: 400, italic: false, color: ink, lh: Math.round(L * 0.86) }, align: just, hyphenate: true, snap: false };
    case "caption":
      return {
        spec: { family: "body", size: B * 0.82, weight: 400, italic: false, color: colorOf(color, pal.muted), lh: Math.round(L * 0.84) },
        align: "left",
        hyphenate: false,
        snap: false,
      };
    case "verse":
      return { spec: { family: "body", size: B, weight: 400, italic: false, color: ink, lh: L }, align: "left", hyphenate: false, snap: false };
    case "mono":
      return { spec: { family: "mono", size: B * 0.84, weight: 400, italic: false, color: ink, lh: L }, align: "left", hyphenate: false, snap: onGrid };
    case "display": {
      const size = B * 2.3;
      return { spec: { family: "heading", size, weight: 600, italic: false, color: ink, lh: Math.round(size * 1.16) }, align: "center", hyphenate: false, snap: false };
    }
    case "body":
    case "dropcap":
    default:
      return { spec: { family: "body", size: B, weight: 400, italic: false, color: ink, lh: L }, align: just, hyphenate: true, snap: onGrid };
  }
}

/** Justified text needs a reasonable measure; narrow columns are set ragged. */
function fitAlign(align: Align, w: number, size: number): Align {
  return align === "justify" && w < size * 17 ? "left" : align;
}

function paragraph(lx: Lx, flow: Flow, env: FlowEnv, b: BlockOf<"paragraph">): void {
  const theme = lx.book.theme;
  const style: TextStyle = b.style;
  const info = styleInfo(lx, env, style, scaleOf(b), b.color);
  const spec = info.spec;
  let align: Align = b.align ?? info.align;
  if (style === "verse" && align === "justify") align = "left";
  align = fitAlign(align, env.w, spec.size);
  const pieces = toPieces(lx, b.spans, spec);
  const sem: ParaOpts["sem"] = { key: b.id, role: style === "caption" ? "caption" : "paragraph", block: b.id };

  const flowing = style === "body" || style === "dropcap" || style === undefined;
  // Consecutive paragraphs of small print run on like body text: indent, no extra space.
  const smallRun = style === "small" && flow.prevSmall && theme.paragraph === "indent" && !env.fixed;
  const indented =
    (smallRun || (style === "body" && flow.prevPara)) && theme.paragraph === "indent" && !env.fixed && (align === "left" || align === "justify");
  const base: ParaOpts = {
    spec,
    align,
    x: env.x,
    w: env.w,
    hyphenate: info.hyphenate,
    snap: info.snap,
    sem,
  };

  let rows: Row[] | null = null;
  if (style === "dropcap") rows = dropcapRows(lx, env, b, pieces, base);
  if (!rows) {
    rows = textRows(lx, env, b.id, pieces, {
      ...base,
      indent: indented ? spec.size * 1.3 : 0,
      hang: style === "verse" ? spec.size * 1.5 : 0,
    });
  }
  if (rows.length === 0) {
    flow.pending.push(b.id);
    return;
  }

  const L = env.L;
  const first = rows[0]!;
  const last = rows[rows.length - 1]!;
  if (flowing || style === "mono") {
    if (theme.paragraph === "spaced" || env.fixed) {
      first.gapBefore = L / 2;
      last.gapAfter = L / 2;
    }
  } else if (style === "small") {
    const prev = flow.rows[flow.rows.length - 1];
    if (smallRun && prev) prev.gapAfter = 0;
    else first.gapBefore = L * 0.3;
    last.gapAfter = L * 0.3;
  } else if (style === "lead") slot(env, rows, L * 0.5, L * 0.5, 0.35);
  else if (style === "verse") {
    first.gapBefore = L / 2;
    last.gapAfter = L / 2;
  } else if (style === "display") slot(env, rows, L * 0.5, L * 0.75);
  else slot(env, rows, L * 0.25, L * 0.25);

  commit(flow, env, rows);
  flow.prevPara = flowing;
  flow.prevSmall = style === "small";
}

const DROPCAP_LINES = 3;
const DROPCAP_START = /^(\s*)(["'\u201C\u2018\u201E\u201A\u00AB\u2039\u00BF\u00A1(\[]*)(\p{L})/u;

/**
 * A three-line drop capital: the top of the letter sits at the cap height of
 * the first line and its foot on the baseline of the third. Returns `null`
 * (plain paragraph) when the text does not start with a letter or is too
 * short to wrap around the capital.
 *
 * Opening quotation marks stay at text size beside the top of the capital;
 * `¿` and `¡` are enlarged with the letter, since they belong to the word.
 */
function dropcapRows(lx: Lx, env: FlowEnv, b: BlockOf<"paragraph">, pieces: Piece[], base: ParaOpts): Row[] | null {
  const first = pieces[0];
  if (!first || first === "br") return null;
  const m = DROPCAP_START.exec(first.text);
  if (!m) return null;
  const lead = m[2]!;
  const small = lead.replace(/[\u00BF\u00A1]/g, "");
  const big = lead.replace(/[^\u00BF\u00A1]/g, "") + m[3]!;
  const rest = first.text.slice(m[0].length);

  const spec = base.spec;
  const pal = lx.input.palette;
  const bodyFont = lx.font(spec);
  const bodyMetrics = lx.metrics(bodyFont);
  const height = (DROPCAP_LINES - 1) * spec.lh + bodyMetrics.capHeight;
  const probe = lx.metrics(lx.font({ family: "heading", size: 100, weight: 500 }));
  const capSize = height / Math.max(0.3, probe.capHeight / 100);
  const capFont = lx.font({ family: "heading", size: capSize, weight: 500 });
  const capW = lx.width(capFont, big);
  const smallW = small ? lx.width(bodyFont, small) : 0;
  const gap = spec.size * 0.14;
  const dx = smallW + capW + gap;
  if (dx > env.w * 0.45) return null;
  // "A man": the capital is a whole word, so the word space follows it.
  const wordSpace = /^\s/.test(rest) ? lx.width(bodyFont, " ") : 0;

  const rows = textRows(lx, env, b.id, [{ text: rest, st: first.st }, ...pieces.slice(1)], {
    ...base,
    shape: (i) => (i < DROPCAP_LINES ? dx + (i === 0 ? wordSpace : 0) : 0),
    semPrefix: small + big + (wordSpace > 0 ? " " : ""),
  });
  if (rows.length < DROPCAP_LINES) return null;

  const baseline = baselineOffset(lx, spec);
  const color = colorOf(b.color, pal.accent);
  const row0 = rows[0]!;
  const extra: DrawItem[] = [];
  if (small) extra.push({ kind: "text", x: env.x, y: baseline, text: small, font: bodyFont, color, block: b.id });
  extra.push({ kind: "text", x: env.x + smallW, y: baseline + (DROPCAP_LINES - 1) * spec.lh, text: big, font: capFont, color, block: b.id });
  row0.items = [...extra, ...row0.items];
  for (let i = 0; i < DROPCAP_LINES - 1; i++) rows[i]!.keepWithNext = true;
  return rows;
}

function heading(lx: Lx, flow: Flow, env: FlowEnv, b: BlockOf<"heading">): void {
  const s = scaleOf(b);
  const major = b.level !== 3;
  const size = env.B * s * (major ? 1.36 : 1.1);
  const spec: TextSpec = {
    family: "heading",
    size,
    weight: 600,
    italic: false,
    color: colorOf(b.color, lx.input.palette.ink),
    lh: major ? Math.round(size * 1.24) : Math.max(Math.round(env.L * s), Math.round(size * 1.3)),
  };
  const align: Align = b.align === undefined || b.align === "justify" ? "left" : b.align;
  const rows = textRows(lx, env, b.id, toPieces(lx, b.spans, spec), {
    spec,
    align,
    x: env.x,
    w: env.w,
    keepAll: true,
    sem: { key: b.id, role: "heading", level: major ? 2 : 3, block: b.id },
  });
  if (rows.length === 0) {
    flow.pending.push(b.id);
    return;
  }
  // A heading is never the last thing on a page: it stays with what follows.
  for (const r of rows) r.keepWithNext = true;
  if (env.fixed) slot(env, rows, 0, env.L * 0.25);
  else slot(env, rows, env.L * (major ? 1.2 : 0.9), env.L * (major ? 0.4 : 0.1), 0.7);
  commit(flow, env, rows);
  flow.prevPara = flow.prevSmall = false;
}

function quote(lx: Lx, flow: Flow, env: FlowEnv, b: BlockOf<"quote">): void {
  const pal = lx.input.palette;
  const s = scaleOf(b);
  const B = env.B * s;
  const roomy = env.w > B * 16;
  const ruleX = env.x + (roomy ? B * 0.9 : B * 0.2);
  const textX = ruleX + B * 0.95;
  const textW = Math.max(B * 3, env.x + env.w - textX - (roomy ? B * 0.9 : 0));
  const spec: TextSpec = { family: "body", size: B * 0.95, weight: 400, italic: false, color: colorOf(b.color, pal.ink), lh: Math.round(env.L * s * 0.96) };
  const deco: BoxDeco = { id: b.id, style: "quote", x: ruleX, w: Math.max(1.5, B * 0.09), padTop: 0, padBottom: 0, inset: spec.lh * 0.17 };
  if (env.anim) {
    deco.anim = env.anim.anim;
    deco.animBlock = env.anim.block;
  }
  const inner: FlowEnv = { ...env, boxes: [...env.boxes, deco], snap: false };
  const align = fitAlign(lx.justify ? "justify" : "left", textW, spec.size);

  const rows: Row[] = [];
  const paragraphs = Array.isArray(b.paragraphs) ? b.paragraphs : [];
  for (let p = 0; p < paragraphs.length; p++) {
    const part = textRows(lx, inner, b.id, toPieces(lx, paragraphs[p], spec), {
      spec,
      align,
      x: textX,
      w: textW,
      hyphenate: true,
      lineStart: rows.length,
      sem: { key: `${b.id}/${p}`, role: "quote", block: b.id },
    });
    const head = part[0];
    if (head && rows.length > 0) head.gapBefore = env.L * 0.35;
    for (const r of part) rows.push(r);
  }
  if (b.attribution && b.attribution.length > 0) {
    const by: TextSpec = { family: "body", size: B * 0.86, weight: 400, italic: true, color: pal.muted, lh: Math.round(env.L * s * 0.9) };
    const pieces: Piece[] = [plainPiece(lx, "\u2014\u2009", by), ...toPieces(lx, b.attribution, by)];
    const part = textRows(lx, inner, b.id, pieces, {
      spec: by,
      align: "right",
      x: textX,
      w: textW,
      keepAll: true,
      lineStart: rows.length,
      sem: { key: `${b.id}/by`, role: "quote", block: b.id },
    });
    const head = part[0];
    const prev = rows[rows.length - 1];
    if (head && prev) {
      head.gapBefore = env.L * 0.25;
      prev.keepWithNext = true;
    }
    for (const r of part) rows.push(r);
  }
  if (rows.length === 0) {
    flow.pending.push(b.id);
    return;
  }
  slot(env, rows, env.L * 0.7, env.L * 0.7);
  commit(flow, env, rows);
  flow.prevPara = flow.prevSmall = false;
}

function list(lx: Lx, flow: Flow, env: FlowEnv, b: BlockOf<"list">): void {
  const pal = lx.input.palette;
  const s = scaleOf(b);
  const B = env.B * s;
  const spec: TextSpec = { family: "body", size: B, weight: 400, italic: false, color: colorOf(b.color, pal.ink), lh: Math.round(env.L * s) };
  const font = lx.font(spec);
  const items = Array.isArray(b.items) ? b.items : [];
  const markers = items.map((_, i) => (b.ordered ? `${i + 1}.` : "\u2022"));
  let markerW = 0;
  for (const mk of markers) markerW = Math.max(markerW, lx.width(font, mk));
  // Numerals are right-aligned in a gutter so that their points line up.
  const gutter = Math.max(B * 1.55, markerW + B * 0.5);
  const textX = env.x + gutter + B * 0.5;
  const textW = Math.max(B * 3, env.x + env.w - textX);
  const base = baselineOffset(lx, spec);

  const rows: Row[] = [];
  for (let i = 0; i < items.length; i++) {
    const part = textRows(lx, env, b.id, toPieces(lx, items[i], spec), {
      spec,
      align: "left",
      x: textX,
      w: textW,
      hyphenate: false,
      lineStart: rows.length,
      sem: { key: b.id, role: "list", ordered: b.ordered === true, item: i, block: b.id },
    });
    const head = part[0];
    if (!head) continue;
    const mk = markers[i]!;
    head.items.unshift({
      kind: "text",
      // Bullets sit a little inside the gutter; numerals end flush with it.
      x: env.x + gutter - lx.width(font, mk) - (b.ordered ? 0 : B * 0.12),
      y: base,
      text: mk,
      font,
      color: b.ordered ? spec.color : pal.muted,
      block: b.id,
    });
    for (const r of part) rows.push(r);
  }
  if (rows.length === 0) {
    flow.pending.push(b.id);
    return;
  }
  // Half a line above and below: the list sits between grid lines and the text after it is back on the grid.
  rows[0]!.gapBefore = env.L / 2;
  rows[rows.length - 1]!.gapAfter = env.L / 2;
  commit(flow, env, rows);
  flow.prevPara = flow.prevSmall = false;
}

// ---------------------------------------------------------------------------
// Figures
// ---------------------------------------------------------------------------

function captionRows(lx: Lx, env: FlowEnv, id: string, caption: Span[] | undefined): Row[] {
  if (!Array.isArray(caption) || caption.length === 0) return [];
  const info = styleInfo(lx, env, "caption", 1, undefined);
  const inset = env.w > env.B * 20 ? env.w * 0.06 : 0;
  const rows = textRows(lx, env, id, toPieces(lx, caption, info.spec), {
    spec: info.spec,
    align: "center",
    x: env.x + inset,
    w: env.w - 2 * inset,
    keepAll: true,
    sem: { key: `${id}/caption`, role: "caption", block: id },
  });
  const first = rows[0];
  if (first) first.gapBefore = env.L * 0.4;
  return rows;
}

function rowsHeight(rows: Row[]): number {
  let h = 0;
  for (let i = 0; i < rows.length; i++) {
    const r = rows[i]!;
    h += r.h;
    if (i > 0) h += Math.max(rows[i - 1]!.gapAfter, r.gapBefore);
    else h += r.gapBefore;
  }
  return h;
}

/** A figure is its picture row followed by its caption; the two never part. */
function figure(lx: Lx, flow: Flow, env: FlowEnv, id: string, main: Row | null, caption: Row[]): void {
  const rows: Row[] = main ? [main, ...caption] : caption;
  if (rows.length === 0) {
    flow.pending.push(id);
    return;
  }
  if (!main) {
    const first = rows[0];
    if (first) first.gapBefore = 0;
  }
  for (let i = 0; i < rows.length - 1; i++) rows[i]!.keepWithNext = true;
  slot(env, rows, env.L * 0.7, env.L * 0.7);
  commit(flow, env, rows);
  flow.prevPara = flow.prevSmall = false;
}

function alignX(env: FlowEnv, w: number, align: Align | undefined): number {
  if (align === "left") return env.x;
  if (align === "right") return env.x + env.w - w;
  return env.x + (env.w - w) / 2;
}

function aspectOf(lx: Lx, resource: string, fallback: number): number {
  const r = lx.resources.get(resource);
  if (r && typeof r.width === "number" && typeof r.height === "number" && r.width > 0 && r.height > 0) {
    return Math.max(0.1, Math.min(10, r.width / r.height));
  }
  return fallback;
}

function image(lx: Lx, flow: Flow, env: FlowEnv, b: BlockOf<"image">): void {
  const caption = captionRows(lx, env, b.id, b.caption);
  const res = lx.resources.get(b.resource);
  let main: Row | null = null;
  // An unknown or non-image resource draws nothing; the caption still stands.
  if (res && res.kind === "image") {
    const frac = typeof b.width === "number" && Number.isFinite(b.width) ? Math.max(0.05, Math.min(1, b.width)) : 1;
    const aspect = aspectOf(lx, b.resource, 1.5);
    let w = env.w * frac;
    let h = w / aspect;
    const maxH = Math.max(env.L, env.maxH - rowsHeight(caption));
    if (h > maxH) {
      h = maxH;
      w = h * aspect;
    }
    const x = alignX(env, w, b.align);
    main = makeRow(h, [...env.ids, b.id], b.id, env.boxes);
    main.items.push({ kind: "image", x, y: 0, w, h, resource: b.resource, fit: b.fit === "cover" ? "cover" : "contain", radius: 2, block: b.id });
    if (!env.fixed) {
      const region: Region = { type: "image", x, y: 0, w, h, block: b.id, resource: b.resource };
      const cap = plainText(lx, b.caption);
      if (cap) region.caption = cap;
      main.regions.push(region);
    }
    main.sem = [{ key: b.id, role: "image", text: res.alt ?? "", block: b.id }];
  }
  figure(lx, flow, env, b.id, main, caption);
}

function gallery(lx: Lx, flow: Flow, env: FlowEnv, b: BlockOf<"gallery">): void {
  const caption = captionRows(lx, env, b.id, b.caption);
  const resources = (Array.isArray(b.resources) ? b.resources : []).filter((r) => lx.resources.get(r)?.kind === "image");
  let main: Row | null = null;
  const firstRes = resources[0];
  if (firstRes !== undefined) {
    const strip = galleryStripHeight(env.w);
    const aspect = Math.max(1, Math.min(2.2, aspectOf(lx, firstRes, 1.5)));
    let w = env.w;
    let h = w / aspect;
    const maxH = Math.max(env.L, Math.min(env.maxH * 0.62, env.maxH - rowsHeight(caption)) - strip);
    if (h > maxH) {
      h = maxH;
      w = Math.max(env.w * 0.5, h * aspect);
    }
    const x = alignX(env, w, b.align);
    main = makeRow(h + strip, [...env.ids, b.id], b.id, env.boxes);
    const widget: Widget = { type: "gallery", resources, count: resources.length };
    main.items.push({ kind: "widget", widget, x, y: 0, w, h: h + strip, block: b.id });
    const region: Region = { type: "gallery", x, y: 0, w, h: h + strip, block: b.id, resources };
    const cap = plainText(lx, b.caption);
    if (cap) region.caption = cap;
    main.regions.push(region);
    main.sem = resources.map((r, i) => ({ key: `${b.id}/${i}`, role: "image" as const, text: lx.resources.get(r)?.alt ?? "", block: b.id }));
  }
  figure(lx, flow, env, b.id, main, caption);
}

function video(lx: Lx, flow: Flow, env: FlowEnv, b: BlockOf<"video">): void {
  const caption = captionRows(lx, env, b.id, b.caption);
  const res = lx.resources.get(b.resource);
  let main: Row | null = null;
  if (res && res.kind === "video") {
    const aspect = Math.max(0.5, Math.min(3, aspectOf(lx, b.resource, 16 / 9)));
    let w = env.w;
    let h = w / aspect;
    const maxH = Math.max(env.L, Math.min(env.maxH * 0.7, env.maxH - rowsHeight(caption)));
    if (h > maxH) {
      h = maxH;
      w = h * aspect;
    }
    const x = alignX(env, w, b.align);
    main = makeRow(h, [...env.ids, b.id], b.id, env.boxes);
    const widget: Widget = { type: "video", resource: b.resource };
    if (res.poster && lx.resources.get(res.poster)?.kind === "image") widget.poster = res.poster;
    main.items.push({ kind: "widget", widget, x, y: 0, w, h, block: b.id });
    const region: Region = { type: "video", x, y: 0, w, h, block: b.id, resource: b.resource, autoplay: b.autoplay === true, loop: b.loop === true, muted: b.muted === true };
    const cap = plainText(lx, b.caption);
    if (cap) region.caption = cap;
    main.regions.push(region);
    main.sem = [{ key: b.id, role: "image", text: res.alt ?? res.title ?? "", block: b.id }];
  }
  figure(lx, flow, env, b.id, main, caption);
}

function audio(lx: Lx, flow: Flow, env: FlowEnv, b: BlockOf<"audio">): void {
  const caption = captionRows(lx, env, b.id, b.caption);
  const res = lx.resources.get(b.resource);
  let main: Row | null = null;
  if (res && res.kind === "audio") {
    const h = Math.min(env.maxH, Math.max(56, Math.round(env.B * 3.3)));
    const w = Math.min(env.w, Math.max(220, env.B * 24));
    const x = alignX(env, w, b.align);
    main = makeRow(h, [...env.ids, b.id], b.id, env.boxes);
    const title = res.title ?? "";
    main.items.push({ kind: "widget", widget: { type: "audio", resource: b.resource, title }, x, y: 0, w, h, block: b.id });
    const region: Region = { type: "audio", x, y: 0, w, h, block: b.id, resource: b.resource };
    const cap = plainText(lx, b.caption);
    if (cap) region.caption = cap;
    main.regions.push(region);
    if (title) main.sem = [{ key: b.id, role: "note", text: title, block: b.id }];
  }
  figure(lx, flow, env, b.id, main, caption);
}

// ---------------------------------------------------------------------------
// Rules and white space
// ---------------------------------------------------------------------------

function divider(lx: Lx, flow: Flow, env: FlowEnv, b: BlockOf<"divider">): void {
  const pal = lx.input.palette;
  const L = env.L;
  const row = makeRow(L, [...env.ids, b.id], b.id, env.boxes);
  if (b.style === "rule") {
    const w = env.w * 0.3;
    const x = env.x + (env.w - w) / 2;
    const y = Math.round(L / 2) + 0.5;
    row.items.push({ kind: "line", x1: x, y1: y, x2: x + w, y2: y, color: pal.rule, width: 1, block: b.id });
  } else if (b.style === "blank") {
    row.discardable = true;
  } else {
    const h = Math.min(L * 0.9, env.B * 1.12);
    const w = Math.min(env.w * 0.5, h * 5.6);
    row.items.push({ kind: "ornament", style: "fleuron", x: env.x + (env.w - w) / 2, y: (L - h) / 2, w, h, color: pal.accent, block: b.id });
  }
  row.sem = [{ key: b.id, role: "separator", text: "" }];
  row.gapBefore = L / 2;
  row.gapAfter = L / 2;
  // A rule or ornament closes what came before: it may end a page but never open one.
  const before = flow.rows[flow.rows.length - 1];
  if (b.style !== "blank" && before && before.kind === "content" && !before.discardable) before.keepWithNext = true;
  commit(flow, env, [row]);
  flow.prevPara = flow.prevSmall = false;
}

function space(lx: Lx, flow: Flow, env: FlowEnv, b: BlockOf<"space">): void {
  const lines = typeof b.lines === "number" && Number.isFinite(b.lines) ? Math.max(0, Math.min(40, b.lines)) : 1;
  if (lines <= 0) {
    flow.pending.push(b.id);
    return;
  }
  const row = makeRow(Math.min(lines * env.L, env.maxH), [...env.ids, b.id], b.id, env.boxes);
  row.discardable = true;
  commit(flow, env, [row]);
  flow.prevPara = flow.prevSmall = false;
}

// ---------------------------------------------------------------------------
// Boxes
// ---------------------------------------------------------------------------

function box(lx: Lx, flow: Flow, env: FlowEnv, b: BlockOf<"box">): void {
  const pal = lx.input.palette;
  const B = env.B;
  const L = env.L;
  const style = b.style === "tint" || b.style === "bar" ? b.style : "rule";
  const padL = style === "bar" ? B * 1.05 : B * 0.95;
  const padR = style === "bar" ? 0 : B * 0.95;
  const padY = style === "bar" ? L * 0.14 : L * 0.6;
  const deco: BoxDeco = { id: b.id, style, x: env.x, w: env.w, padTop: padY, padBottom: padY, inset: 0 };
  if (env.anim) {
    deco.anim = env.anim.anim;
    deco.animBlock = env.anim.block;
  }
  const ids = [...env.ids, b.id];
  const boxes = [...env.boxes, deco];
  const inner: FlowEnv = {
    ...env,
    x: env.x + padL,
    w: Math.max(B * 2, env.w - padL - padR),
    snap: false,
    ids,
    boxes,
    depth: env.depth + 1,
    maxH: Math.max(L, env.maxH - 2 * padY - L),
  };

  const sub = newFlow();
  if (typeof b.title === "string" && b.title.trim() !== "") {
    // Small letterspaced capitals in the UI face: a label, not a heading.
    const size = B * 0.66;
    const spec: TextSpec = { family: "sans", size, weight: 600, italic: false, color: pal.accent, lh: Math.round(L * 0.8), letterSpacing: size * 0.09 };
    const rows = textRows(lx, inner, b.id, [plainPiece(lx, lx.upper(b.title.trim()), spec)], {
      spec,
      align: "left",
      x: inner.x,
      w: inner.w,
      keepAll: true,
      sem: { key: `${b.id}/title`, role: "note", block: b.id },
    });
    for (const r of rows) r.keepWithNext = true;
    const lastTitle = rows[rows.length - 1];
    if (lastTitle) lastTitle.gapAfter = L * 0.2;
    commit(sub, inner, rows);
  }
  if (env.depth < MAX_DEPTH && Array.isArray(b.blocks)) {
    for (const child of b.blocks) addBlock(lx, sub, inner, child);
  }
  const content = sub.rows;
  const firstContent = content[0];
  const lastContent = content[content.length - 1];
  if (!firstContent || !lastContent) {
    flow.pending.push(b.id, ...sub.pending);
    return;
  }
  firstContent.gapBefore = 0;
  lastContent.gapAfter = 0;
  lastContent.keepWithNext = true;

  const top = makeRow(padY, ids, b.id, boxes);
  top.padOf = deco;
  top.padEdge = "top";
  top.keepWithNext = true;
  const bottom = makeRow(padY, ids, b.id, boxes);
  bottom.padOf = deco;
  bottom.padEdge = "bottom";
  const rows = [top, ...content, bottom];
  slot(env, rows, L * 0.7, L * 0.7);
  if (sub.pending.length > 0) bottom.ids = [...bottom.ids, ...sub.pending];
  // Children carry their own animation already; the frame rows have no ink.
  commit(flow, { ...env, anim: undefined }, rows);
  flow.prevPara = flow.prevSmall = false;
}

// ---------------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------------

/** The label of a button, centred in its box. Shrinks, then truncates, a label that is too long. */
export function buttonItems(lx: Lx, id: string, label: string, x: number, y: number, w: number, h: number): DrawItem[] {
  const pal = lx.input.palette;
  const items: DrawItem[] = [{ kind: "widget", widget: { type: "button", label }, x, y, w, h, block: id }];
  let size = buttonLabelSize(h);
  const room = Math.max(4, w - Math.min(h * 0.5, w * 0.16));
  let font = lx.font({ family: "heading", size, weight: 600 });
  let ls = size * 0.012;
  let tw = lx.widthSpaced(font, label, ls) - ls;
  if (tw > room) {
    size = Math.max(size * 0.7, size * (room / tw));
    font = lx.font({ family: "heading", size, weight: 600 });
    ls = size * 0.012;
    tw = lx.widthSpaced(font, label, ls) - ls;
  }
  let text = label;
  if (tw > room) {
    text = ellipsize(label, room, (s) => lx.widthSpaced(font, s, ls));
    tw = lx.widthSpaced(font, text, ls) - ls;
  }
  if (text !== "") {
    const cap = lx.metrics(font).capHeight;
    items.push({ kind: "text", x: x + (w - tw) / 2, y: y + h / 2 + cap / 2 - 0.5, text, font, color: pal.paper, letterSpacing: ls, block: id });
  }
  return items;
}

function button(lx: Lx, flow: Flow, env: FlowEnv, b: BlockOf<"button">): void {
  const label = typeof b.label === "string" ? b.label : "";
  const h = Math.min(env.maxH, Math.max(MIN_TARGET, Math.round(env.B * 2.35)));
  const size = buttonLabelSize(h);
  const font = lx.font({ family: "heading", size, weight: 600 });
  const natural = lx.widthSpaced(font, label, size * 0.012) + h * 1.15;
  const w = b.align === "justify" ? env.w : Math.min(env.w, Math.max(Math.min(env.w, h * 2.6), natural));
  const x = alignX(env, w, b.align);
  const row = makeRow(h, [...env.ids, b.id], b.id, env.boxes);
  row.items = buttonItems(lx, b.id, label, x, 0, w, h);
  row.regions.push({ type: "button", x, y: 0, w, h, block: b.id, label, actions: Array.isArray(b.actions) ? b.actions : [] });
  slot(env, [row], env.L * 0.7, env.L * 0.7);
  commit(flow, env, [row]);
  flow.prevPara = flow.prevSmall = false;
}

/** One row of a choice or a quiz: the control drawn by the painter plus its wrapped label. */
function controlRow(lx: Lx, env: FlowEnv, id: string, widget: Widget, label: string, dim: boolean): Row {
  const pal = lx.input.palette;
  const w = env.w;
  const lay = rowWidgetLayout(w, MIN_TARGET);
  const size = env.B * 0.95;
  const spec: TextSpec = { family: "body", size, weight: 400, italic: false, color: pal.ink, lh: Math.round(size * 1.34) };
  const lines = breakLines(tokenize([plainPiece(lx, label, spec)], lx.measure), () => lay.textW, {
    measure: lx.measure,
    justify: false,
    hyphenate: lx.hyphenate,
    hyphenCache: lx.hyphenCache,
  });
  const textH = lines.length * spec.lh;
  const h = Math.max(MIN_TARGET, Math.round(textH + size * 0.9));
  const row = makeRow(h, [...env.ids, id], id, env.boxes);
  row.items.push({ kind: "widget", widget, x: env.x, y: 0, w, h, block: id });
  const top = (h - textH) / 2;
  const base = baselineOffset(lx, spec);
  for (let i = 0; i < lines.length; i++) {
    const placed = placeLine(lines[i]!, env.x + lay.textX, top + i * spec.lh + base, lay.textW, "left", id);
    for (const it of placed.items) {
      if (dim) it.opacity = 0.5;
      row.items.push(it);
    }
  }
  return row;
}

function promptRows(lx: Lx, env: FlowEnv, id: string, spans: Span[] | undefined, weight: number): Row[] {
  const spec: TextSpec = { family: "body", size: env.B, weight, italic: false, color: lx.input.palette.ink, lh: env.L };
  const rows = textRows(lx, env, id, toPieces(lx, spans, spec), {
    spec,
    align: "left",
    x: env.x,
    w: env.w,
    keepAll: true,
    sem: { key: `${id}/prompt`, role: "paragraph", block: id },
  });
  for (const r of rows) r.keepWithNext = true;
  const last = rows[rows.length - 1];
  if (last) last.gapAfter = env.L * 0.45;
  return rows;
}

function choice(lx: Lx, flow: Flow, env: FlowEnv, b: BlockOf<"choice">): void {
  const rows = promptRows(lx, env, b.id, b.prompt, 400);
  const prompt = plainText(lx, b.prompt);
  const picked = own(lx.input.ui?.choice, b.id);
  const options = Array.isArray(b.options) ? b.options : [];
  const gap = Math.max(8, env.L * 0.32);
  let shown = 0;
  for (let i = 0; i < options.length; i++) {
    const opt = options[i]!;
    // Options whose condition is false are not part of the book right now.
    if (!holds(opt.when, lx.input.state)) continue;
    const label = typeof opt.label === "string" ? opt.label : "";
    const selected = picked === i;
    const row = controlRow(lx, env, b.id, { type: "option", label, selected }, label, false);
    row.regions.push({ type: "option", x: env.x, y: 0, w: env.w, h: row.h, block: b.id, index: i, label, prompt, selected, actions: Array.isArray(opt.actions) ? opt.actions : [] });
    if (shown > 0) row.gapBefore = gap;
    row.line = rows.length;
    const prev = rows[rows.length - 1];
    if (prev) prev.keepWithNext = true;
    rows.push(row);
    shown++;
  }
  if (rows.length === 0) {
    flow.pending.push(b.id);
    return;
  }
  slot(env, rows, env.L * 0.7, env.L * 0.7);
  commit(flow, env, rows);
  flow.prevPara = flow.prevSmall = false;
}

function quiz(lx: Lx, flow: Flow, env: FlowEnv, b: BlockOf<"quiz">): void {
  const rows = promptRows(lx, env, b.id, b.question, 600);
  const question = plainText(lx, b.question);
  const record = own(lx.input.ui?.quiz, b.id);
  const answered = record !== undefined && record !== null && typeof record === "object";
  const answers = Array.isArray(b.answers) ? b.answers : [];
  const gap = Math.max(8, env.L * 0.32);
  for (let i = 0; i < answers.length; i++) {
    const ans = answers[i]!;
    const label = typeof ans.label === "string" ? ans.label : "";
    let state: "idle" | "correct" | "wrong" | "missed" | "dimmed" = "idle";
    if (answered) {
      // Colour is never the only signal: the painter adds a tick or a cross.
      if (record.picked === i) state = ans.correct ? "correct" : "wrong";
      else if (ans.correct) state = "missed";
      else state = "dimmed";
    }
    const row = controlRow(lx, env, b.id, { type: "answer", label, state }, label, state === "dimmed");
    row.regions.push({ type: "answer", x: env.x, y: 0, w: env.w, h: row.h, block: b.id, index: i, label, question, answered });
    if (i > 0) row.gapBefore = gap;
    row.line = rows.length;
    const prev = rows[rows.length - 1];
    if (prev) prev.keepWithNext = true;
    rows.push(row);
  }
  if (answered && Array.isArray(b.explanation) && b.explanation.length > 0) {
    const info = styleInfo(lx, env, "small", 1, undefined);
    const spec: TextSpec = { ...info.spec, italic: true };
    const part = textRows(lx, env, b.id, toPieces(lx, b.explanation, spec), {
      spec,
      align: "left",
      x: env.x,
      w: env.w,
      keepAll: true,
      lineStart: rows.length,
      sem: { key: `${b.id}/explanation`, role: "note", block: b.id },
    });
    const head = part[0];
    const prev = rows[rows.length - 1];
    if (head && prev) {
      head.gapBefore = env.L * 0.45;
      prev.keepWithNext = true;
    }
    for (const r of part) rows.push(r);
  }
  if (rows.length === 0) {
    flow.pending.push(b.id);
    return;
  }
  slot(env, rows, env.L * 0.7, env.L * 0.7);
  commit(flow, env, rows);
  flow.prevPara = flow.prevSmall = false;
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

export function addBlock(lx: Lx, flow: Flow, outer: FlowEnv, b: Block): void {
  if (!b || typeof b !== "object" || typeof b.id !== "string") return;
  const env = withAnim(outer, b);
  switch (b.type) {
    case "paragraph":
      return paragraph(lx, flow, env, b);
    case "heading":
      return heading(lx, flow, env, b);
    case "quote":
      return quote(lx, flow, env, b);
    case "list":
      return list(lx, flow, env, b);
    case "image":
      return image(lx, flow, env, b);
    case "gallery":
      return gallery(lx, flow, env, b);
    case "video":
      return video(lx, flow, env, b);
    case "audio":
      return audio(lx, flow, env, b);
    case "divider":
      return divider(lx, flow, env, b);
    case "space":
      return space(lx, flow, env, b);
    case "page_break": {
      if (env.fixed) {
        flow.pending.push(b.id);
        return;
      }
      const row = makeRow(0, [...env.ids, b.id], b.id, []);
      row.kind = "break";
      commit(flow, env, [row]);
      flow.prevPara = flow.prevSmall = false;
      return;
    }
    case "box":
      return box(lx, flow, env, b);
    case "button":
      return button(lx, flow, env, b);
    case "choice":
      return choice(lx, flow, env, b);
    case "quiz":
      return quiz(lx, flow, env, b);
    case "group": {
      // Conditional content: an `if` whose condition is false is simply not in the book.
      if (!holds(b.when, lx.input.state)) return;
      if (env.depth >= MAX_DEPTH || !Array.isArray(b.blocks)) {
        flow.pending.push(b.id);
        return;
      }
      const before = flow.rows.length;
      const inner: FlowEnv = { ...env, ids: [...env.ids, b.id], depth: env.depth + 1 };
      for (const child of b.blocks) addBlock(lx, flow, inner, child);
      if (flow.rows.length === before) flow.pending.push(b.id);
      return;
    }
    case "page": {
      if (env.fixed) {
        flow.pending.push(b.id);
        return;
      }
      const row = makeRow(0, [...env.ids, b.id], b.id, []);
      row.kind = "fixed";
      row.fixed = buildFixed(lx, b);
      commit(flow, { ...env, anim: undefined }, [row]);
      flow.prevPara = flow.prevSmall = false;
      return;
    }
    default:
      // A block type from a newer IR: keep its id addressable, draw nothing.
      flow.pending.push((b as { id: string }).id);
  }
}

/** Rows of a chapter body in the page column. */
export function flowRows(lx: Lx, blocks: Block[] | undefined): Flow {
  const g = lx.geo;
  const flow = newFlow();
  const env: FlowEnv = { x: 0, w: g.colW, B: g.body, L: g.L, snap: true, ids: [], boxes: [], maxH: g.bottom - g.top, fixed: false, depth: 0 };
  if (Array.isArray(blocks)) for (const b of blocks) addBlock(lx, flow, env, b);
  return flow;
}
