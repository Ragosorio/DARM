/**
 * Rows → pages.
 *
 * `Stack` places rows one under another (gaps collapse, body lines snap to
 * the baseline grid, container decorations are drawn once per fragment);
 * `paginate` drives it, starting a new page whenever the next unbreakable
 * run of rows ("chunk") does not fit.
 */
import type { ChapterKind } from "../ir";
import type { Lx } from "./context";
import { mergeSemantics, type BoxDeco, type Row, type RowSem } from "./rows";
import { ellipsize } from "./text";
import type { DrawItem, PageAnchor, PageLayout, Region, SemanticNode } from "./types";

const FIT_EPS = 0.5;

export function translateItem(it: DrawItem, dx: number, dy: number): DrawItem {
  if (it.kind === "line") return { ...it, x1: it.x1 + dx, y1: it.y1 + dy, x2: it.x2 + dx, y2: it.y2 + dy };
  return { ...it, x: it.x + dx, y: it.y + dy };
}

/** Vertical stacking of rows inside one page (or one frame of a fixed page). */
export class Stack {
  readonly items: DrawItem[] = [];
  readonly deco: DrawItem[] = [];
  readonly regions: Region[] = [];
  readonly ids: string[] = [];
  readonly sems: RowSem[] = [];
  y: number;
  last: Row | null = null;
  anchor: { block: string | null; line: number } | null = null;

  private readonly idSet = new Set<string>();
  private readonly open = new Map<BoxDeco, { y0: number; y1: number; closed: boolean }>();

  constructor(
    private readonly lx: Lx,
    /** Page x of the column's left edge. */
    readonly left: number,
    /** Page y where the stack starts; also the origin of the baseline grid. */
    readonly top: number,
    private readonly grid: number,
  ) {
    this.y = top;
  }

  get empty(): boolean {
    return this.last === null;
  }

  addIds(ids: string[]): void {
    for (const id of ids) {
      if (this.idSet.has(id)) continue;
      this.idSet.add(id);
      this.ids.push(id);
    }
  }

  /** Top of `row` if it were placed after `prev` at `y`. */
  private topFor(row: Row, y: number, prev: Row | null): number {
    let t = y;
    if (prev) t += Math.max(prev.gapAfter, row.gapBefore);
    else {
      // A box cut by the page break: give its continuation the box's top padding again.
      for (const b of row.boxes) if (!(row.padOf === b && row.padEdge === "top")) t += b.padTop;
    }
    if (row.snap && this.grid > 0) t = this.top + Math.ceil((t - this.top) / this.grid - 1e-6) * this.grid;
    return t;
  }

  /** Space to keep free under `row` if the page ends after it (bottom padding of boxes that continue). */
  private static reserve(row: Row): number {
    let r = 0;
    for (const b of row.boxes) if (!(row.padOf === b && row.padEdge === "bottom")) r += b.padBottom;
    return r;
  }

  /** Bottom edge reached after placing rows[from, to), without placing them. */
  measure(rows: Row[], from: number, to: number): number {
    let y = this.y;
    let prev = this.last;
    for (let k = from; k < to; k++) {
      const row = rows[k]!;
      y = this.topFor(row, y, prev) + row.h;
      prev = row;
    }
    return prev && to > from ? y + Stack.reserve(prev) : y;
  }

  add(row: Row): void {
    const top = this.topFor(row, this.y, this.last);
    const continuing = this.last === null;
    for (let bi = 0; bi < row.boxes.length; bi++) {
      const b = row.boxes[bi]!;
      let ext = this.open.get(b);
      if (!ext) {
        // First row of this box on the page. When the box continues from the previous page its
        // fragment starts above the row, by its own top padding and that of the boxes inside it.
        let y0 = top;
        if (continuing) {
          for (let k = row.boxes.length - 1; k >= bi; k--) {
            const inner = row.boxes[k]!;
            if (!(row.padOf === inner && row.padEdge === "top")) y0 -= inner.padTop;
          }
        }
        ext = { y0, y1: top, closed: false };
        this.open.set(b, ext);
      }
      ext.y1 = top + row.h;
      if (row.padOf === b && row.padEdge === "bottom") ext.closed = true;
    }
    for (const it of row.items) this.items.push(translateItem(it, this.left, top));
    for (const r of row.regions) this.regions.push({ ...r, x: r.x + this.left, y: r.y + top });
    if (row.sem) for (const s of row.sem) this.sems.push(s);
    this.addIds(row.ids);
    if (this.anchor === null && row.block !== null) this.anchor = { block: row.block, line: row.line };
    this.y = top + row.h;
    this.last = row;
  }

  /** Emit the decoration of every container fragment on this page. Call once, after the last row. */
  finishDeco(): void {
    const pal = this.lx.input.palette;
    for (const [b, ext] of this.open) {
      let y0 = ext.y0;
      let y1 = ext.closed ? ext.y1 : ext.y1 + b.padBottom;
      y0 += b.inset;
      y1 -= b.inset;
      if (y1 - y0 < 1) continue;
      const x = this.left + b.x;
      let it: DrawItem;
      if (b.style === "tint") it = { kind: "rect", x, y: y0, w: b.w, h: y1 - y0, fill: pal.tint, radius: 4 };
      else if (b.style === "rule") it = { kind: "rect", x: x + 0.5, y: y0 + 0.5, w: b.w - 1, h: y1 - y0 - 1, stroke: pal.rule, lineWidth: 1, radius: 3 };
      else if (b.style === "bar") it = { kind: "rect", x, y: y0, w: 3, h: y1 - y0, fill: pal.accent, radius: 1.5 };
      else it = { kind: "rect", x, y: y0, w: b.w, h: y1 - y0, fill: pal.accent, radius: b.w / 2 };
      it.block = b.animBlock ?? b.id;
      if (b.anim) it.anim = b.anim;
      this.deco.push(it);
    }
    this.open.clear();
  }
}

export interface PaginateOpts {
  chapter: { id: string; kind: ChapterKind; title: string };
  firstIndex: number;
  /** Folio and running head (body chapters only). */
  furniture: boolean;
  /** The first row is a chapter opener: that page has no running head and no block anchor. */
  opener?: boolean;
  /** Push the content of a single page down to the foot of the text block (colophon). */
  valign?: "bottom";
  /** Extra outline nodes placed before the rows of the first page. */
  leadSemantics?: SemanticNode[];
}

/** Lay rows out on as many pages as needed. Always returns at least one page. */
export function paginate(lx: Lx, rows: Row[], o: PaginateOpts): PageLayout[] {
  const g = lx.geo;
  const pages: PageLayout[] = [];
  let pending: string[] = [];
  let usedBottom = g.top;

  const open = (): Stack => new Stack(lx, lx.left(o.firstIndex + pages.length), g.top, g.grid);
  let st = open();
  let fromBreak = false;

  const close = (): void => {
    const index = o.firstIndex + pages.length;
    const first = pages.length === 0;
    st.finishDeco();
    if (pending.length > 0) {
      st.addIds(pending);
      pending = [];
    }
    const items: DrawItem[] = [];
    if (o.furniture) for (const it of furniture(lx, o, index, first)) items.push(it);
    for (const it of st.deco) items.push(it);
    for (const it of st.items) items.push(it);
    const anchor: PageAnchor =
      st.anchor && !(first && o.opener) ? { chapterId: o.chapter.id, block: st.anchor.block, line: st.anchor.line } : { chapterId: o.chapter.id, block: null, line: 0 };
    const semantics = mergeSemantics(st.sems);
    pages.push({
      index,
      chapterId: o.chapter.id,
      chapterKind: o.chapter.kind,
      fixed: false,
      items,
      regions: st.regions,
      anchor,
      blocks: st.ids,
      semantics: first && o.leadSemantics ? [...o.leadSemantics, ...semantics] : semantics,
      animated: items.some((it) => it.anim !== undefined),
    });
    usedBottom = st.y;
    st = open();
    fromBreak = false;
  };

  let i = 0;
  // Every iteration places a row, drops one, or closes a non-empty page; the budget is a last line of defence.
  let budget = rows.length * 4 + 16;
  while (i < rows.length && budget-- > 0) {
    const row = rows[i]!;
    if (row.kind === "break") {
      st.addIds(row.ids);
      // A break on a page that is empty only because the previous one filled up starts nothing new;
      // a second break in a row does force a blank page.
      if (!st.empty || fromBreak) {
        close();
        fromBreak = true;
      }
      i++;
      continue;
    }
    if (row.kind === "fixed") {
      if (!st.empty) close();
      const f = row.fixed;
      const index = o.firstIndex + pages.length;
      const ids = [...pending, ...row.ids, ...(f ? f.ids : [])];
      pending = [];
      const page: PageLayout = {
        index,
        chapterId: o.chapter.id,
        chapterKind: o.chapter.kind,
        fixed: true,
        items: f ? f.items : [],
        regions: f ? f.regions : [],
        anchor: { chapterId: o.chapter.id, block: row.block, line: 0 },
        blocks: Array.from(new Set(ids)),
        semantics: f ? f.semantics : [],
        animated: f ? f.items.some((it) => it.anim !== undefined) : false,
      };
      if (f && f.background !== undefined) page.background = f.background;
      pages.push(page);
      st = open();
      fromBreak = false;
      i++;
      continue;
    }
    if (row.discardable && st.empty) {
      pending.push(...row.ids);
      i++;
      continue;
    }

    // The chunk: this row and everything chained to it by keepWithNext.
    let j = i + 1;
    while (j < rows.length && rows[j - 1]!.keepWithNext && rows[j]!.kind === "content") j++;

    if (st.measure(rows, i, j) <= g.bottom + FIT_EPS) {
      for (let k = i; k < j; k++) st.add(rows[k]!);
      i = j;
      continue;
    }
    if (row.discardable) {
      // White space that does not fit ends the page and is not carried over.
      pending.push(...row.ids);
      i++;
      if (!st.empty) close();
      continue;
    }
    if (!st.empty) {
      close();
      continue;
    }
    // Even an empty page cannot hold the chunk: place what fits (at least one row) and go on.
    let placed = 0;
    for (let k = i; k < j; k++) {
      if (placed > 0 && st.measure(rows, k, k + 1) > g.bottom + FIT_EPS) break;
      st.add(rows[k]!);
      placed++;
    }
    i += placed;
    if (i < j) close();
  }

  if (!st.empty || pages.length === 0) close();
  else {
    // Ids of blocks that left no ink after the last page (a trailing break or space).
    const lastPage = pages[pages.length - 1]!;
    for (const id of [...pending, ...st.ids]) if (!lastPage.blocks.includes(id)) lastPage.blocks.push(id);
  }

  if (o.valign === "bottom" && pages.length === 1) {
    const dy = Math.floor(g.bottom - usedBottom);
    const page = pages[0]!;
    if (dy > 0) {
      page.items = page.items.map((it) => translateItem(it, 0, dy));
      page.regions = page.regions.map((r) => ({ ...r, y: r.y + dy }));
    }
  }
  return pages;
}

/** Folio and running head of a body page. */
function furniture(lx: Lx, o: PaginateOpts, index: number, firstOfChapter: boolean): DrawItem[] {
  const g = lx.geo;
  const pal = lx.input.palette;
  const spread = lx.input.spread;
  const left = lx.left(index);
  const right = left + g.colW;
  const recto = index % 2 === 0;
  const out: DrawItem[] = [];

  // Running head: letterspaced capitals in the UI face. Book title on the left-hand page, chapter title on the right.
  const headSize = Math.max(8.5, Math.min(11.5, g.body * 0.6));
  if (!(firstOfChapter && o.opener) && g.top >= headSize * 2.6) {
    const font = lx.font({ family: "sans", size: headSize, weight: 500 });
    const ls = headSize * 0.14;
    const raw = spread && !recto ? lx.book.meta.title : o.chapter.title;
    const text = ellipsize(lx.upper(String(raw ?? "")), g.colW, (s) => lx.widthSpaced(font, s, ls));
    if (text !== "") {
      const w = lx.widthSpaced(font, text, ls) - ls;
      const y = Math.round(g.top - Math.max(headSize * 1.5, g.top * 0.36));
      out.push({ kind: "text", x: left + (g.colW - w) / 2, y, text, font, color: pal.muted, letterSpacing: ls });
    }
  }

  // Folio: outer corner on a spread, centred on a single page. Pages are numbered from the first body page.
  const start = lx.startPage;
  const marginBelow = g.pageH - g.bottom;
  const folioSize = Math.max(9.5, Math.min(13.5, g.body * 0.74));
  if (start !== null && index >= start && marginBelow >= folioSize * 2.2) {
    const font = lx.font({ family: "body", size: folioSize, weight: 400 });
    const text = String(index - start + 1);
    const w = lx.width(font, text);
    const y = Math.round(g.bottom + Math.min(marginBelow * 0.56, marginBelow - folioSize * 0.6));
    const x = !spread ? left + (g.colW - w) / 2 : recto ? right - w : left;
    out.push({ kind: "text", x, y, text, font, color: pal.muted });
  }
  return out;
}
