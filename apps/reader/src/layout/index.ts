/**
 * `layoutBook`: IR book → pages of positioned draw items.
 *
 * Pure and deterministic: the result depends only on the input (and on what
 * the injected `Measurer` and `hyphenate` return). No DOM, no canvas.
 */
import { holds } from "../core/state";
import type { Block, Book, Chapter, State } from "../ir";
import { flowRows } from "./blocks";
import { Lx } from "./context";
import { colophonRows, contentsRows, openerRow, titleRows } from "./frontmatter";
import { paginate } from "./paginate";
import type { BookLayout, ChapterSpan, LayoutInput, PageLayout, TocLine } from "./types";

export type * from "./types";
export { COMFORT_STRETCH, LOOSE_SPACE_EM, MAX_SPACE_EM, MAX_SPACE_STRETCH, MIN_SPACE_SHRINK, maxSpaceWidth, resetTypesetStats, typesetStats } from "./text";
export { MIN_TARGET, audioWidgetLayout, buttonLabelSize, buttonRadius, galleryWidgetLayout, rowWidgetLayout, videoWidgetLayout } from "./widgetMetrics";
export type { Rect } from "./widgetMetrics";
export { computeGeometry } from "./context";
export type { Geometry } from "./context";

/** Ids of every block that is part of the book in the given state (conditions evaluated). */
function visibleIds(book: Book, state: State): Set<string> {
  const ids = new Set<string>();
  const walk = (blocks: Block[] | undefined, depth: number): void => {
    if (!Array.isArray(blocks) || depth > 32) return;
    for (const b of blocks) {
      if (!b || typeof b !== "object" || typeof b.id !== "string") continue;
      if (b.type === "group" && !holds(b.when, state)) continue;
      ids.add(b.id);
      if (b.type === "group" || b.type === "box" || b.type === "page") walk(b.blocks, depth + 1);
    }
  };
  for (const ch of book.chapters) if (holds(ch.when, state)) walk(ch.blocks, 0);
  return ids;
}

export function layoutBook(input: LayoutInput): BookLayout {
  const lx = new Lx(input);
  const book = input.book;
  const state = input.state;
  const pages: PageLayout[] = [];
  const spans: ChapterSpan[] = [];
  const chapters: Chapter[] = (Array.isArray(book.chapters) ? book.chapters : []).filter(
    (ch) => ch && typeof ch === "object" && typeof ch.id === "string" && holds(ch.when, state),
  );

  // Contents entries are known before pagination: an entry is listed when its own condition
  // holds and its target (a visible chapter or a visible block of one) exists.
  const blockIds = visibleIds(book, state);
  const chapterIds = new Set(chapters.map((c) => c.id));
  const anchorsIr = book.anchors && typeof book.anchors === "object" ? book.anchors : {};
  const resolvable = (target: string): boolean => {
    if (chapterIds.has(target)) return true;
    if (!Object.prototype.hasOwnProperty.call(anchorsIr, target)) return false;
    const a = anchorsIr[target];
    if (!a || !chapterIds.has(a.chapter)) return false;
    return a.block === undefined || blockIds.has(a.block);
  };
  const toc: TocLine[] = [];
  for (const e of Array.isArray(book.toc) ? book.toc : []) {
    if (!e || typeof e.target !== "string" || !holds(e.when, state) || !resolvable(e.target)) continue;
    const line: TocLine = { title: String(e.title ?? ""), target: e.target, level: e.level === 2 ? 2 : 1, page: 0 };
    if (typeof e.number === "number") line.number = e.number;
    toc.push(line);
  }

  let contents: { chapter: Chapter; first: number; count: number } | null = null;
  // The first body chapter fixes where page 1 is; chapters laid out after it need it for their folios.
  for (const ch of chapters) {
    const first = pages.length;
    const meta = { id: ch.id, kind: ch.kind, title: String(ch.title ?? "") };
    let made: PageLayout[];
    if (ch.kind === "title") {
      const t = titleRows(lx);
      made = paginate(lx, t.rows, { chapter: meta, firstIndex: first, furniture: false, leadSemantics: t.semantics });
    } else if (ch.kind === "colophon") {
      made = paginate(lx, colophonRows(lx, ch), { chapter: meta, firstIndex: first, furniture: false, valign: "bottom" });
    } else if (ch.kind === "contents") {
      // First pass: the number column is reserved but empty. The pages are replaced once every target has a page.
      const c = contentsRows(lx, ch, toc, () => null);
      made = paginate(lx, c.rows, { chapter: meta, firstIndex: first, furniture: false, opener: true, leadSemantics: c.semantics });
      if (!contents) contents = { chapter: ch, first, count: made.length };
    } else {
      if (lx.startPage === null) lx.startPage = first;
      const flow = flowRows(lx, ch.blocks);
      let rows = flow.rows;
      let lead;
      const hasOpener = ch.opener === true;
      if (hasOpener) {
        const op = openerRow(lx, ch);
        rows = [op.row, ...rows];
        lead = op.semantics;
      }
      made = paginate(lx, rows, { chapter: meta, firstIndex: first, furniture: true, opener: hasOpener, leadSemantics: lead });
      // Blocks that never produced a row (hidden by nothing, just empty) still resolve to a page.
      const lastPage = made[made.length - 1];
      if (lastPage) for (const id of flow.pending) if (!lastPage.blocks.includes(id)) lastPage.blocks.push(id);
    }
    for (const p of made) pages.push(p);
    spans.push({ id: ch.id, kind: ch.kind, title: meta.title, firstPage: first, pageCount: made.length });
  }

  const startPage = lx.startPage ?? Math.max(0, pages.length - 1);

  const blockPages = new Map<string, number>();
  for (const p of pages) for (const id of p.blocks) if (!blockPages.has(id)) blockPages.set(id, p.index);

  const anchors = new Map<string, number>();
  const firstPageOf = new Map<string, number>();
  for (const s of spans) if (!firstPageOf.has(s.id)) firstPageOf.set(s.id, s.firstPage);
  for (const name of Object.keys(anchorsIr)) {
    const a = anchorsIr[name];
    if (!a || typeof a !== "object") continue;
    const chapterPage = firstPageOf.get(a.chapter);
    if (chapterPage === undefined) continue;
    if (a.block === undefined) anchors.set(name, chapterPage);
    else {
      const p = blockPages.get(a.block);
      if (p !== undefined) anchors.set(name, p);
    }
  }
  for (const [id, p] of firstPageOf) if (!anchors.has(id)) anchors.set(id, p);

  const lines: TocLine[] = [];
  for (const line of toc) {
    const p = anchors.get(line.target);
    if (p !== undefined) lines.push({ ...line, page: p });
  }

  if (contents) {
    const { chapter, first, count } = contents;
    const c = contentsRows(lx, chapter, lines, (line) => (line.page >= startPage ? String(line.page - startPage + 1) : null));
    const made = paginate(lx, c.rows, {
      chapter: { id: chapter.id, kind: chapter.kind, title: String(chapter.title ?? "") },
      firstIndex: first,
      furniture: false,
      opener: true,
      leadSemantics: c.semantics,
    });
    // Same rows, same heights: the page count cannot differ. If it ever did, the numberless pages stay.
    if (made.length === count) for (let i = 0; i < count; i++) pages[first + i] = made[i]!;
  }

  return {
    pages,
    chapters: spans,
    anchors,
    blockPages,
    toc: lines,
    startPage,
    contentsPage: contents ? contents.first : null,
  };
}
