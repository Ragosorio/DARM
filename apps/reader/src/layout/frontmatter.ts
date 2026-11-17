/**
 * Generated pages: title page, colophon, contents, and the opener that
 * starts a chapter. They are built as rows in the page column, so they go
 * through the same paginator as everything else (a long contents list
 * simply runs on to further pages).
 */
import type { Chapter } from "../ir";
import { baselineOffset, textRows, type FlowEnv, type TextSpec } from "./blocks";
import type { Lx } from "./context";
import { makeRow, type Row } from "./rows";
import { balanceLines, type Piece, type RunStyle } from "./text";
import type { DrawItem, SemanticNode, TocLine } from "./types";

interface Words {
  contents: string;
  published: string;
  identifier: string;
  version: string;
  language: string;
  madeWith: string;
}

// The handful of words the reader itself has to print. Books in other languages fall back to English.
const WORDS: Record<string, Words> = {
  en: { contents: "Contents", published: "Published", identifier: "Identifier", version: "Version", language: "Language", madeWith: "Made with DARM" },
  es: { contents: "Índice", published: "Publicado", identifier: "Identificador", version: "Versión", language: "Idioma", madeWith: "Hecho con DARM" },
  fr: { contents: "Table des matières", published: "Publié", identifier: "Identifiant", version: "Version", language: "Langue", madeWith: "Fait avec DARM" },
  de: { contents: "Inhalt", published: "Erschienen", identifier: "Kennung", version: "Version", language: "Sprache", madeWith: "Erstellt mit DARM" },
  pt: { contents: "Sumário", published: "Publicado", identifier: "Identificador", version: "Versão", language: "Idioma", madeWith: "Feito com DARM" },
  it: { contents: "Indice", published: "Pubblicato", identifier: "Identificativo", version: "Versione", language: "Lingua", madeWith: "Realizzato con DARM" },
};

function words(lx: Lx): Words {
  const lang = String(lx.book.meta.language ?? "").toLowerCase();
  for (const key of Object.keys(WORDS)) if (lang.startsWith(key)) return WORDS[key]!;
  return WORDS.en!;
}

function columnEnv(lx: Lx): FlowEnv {
  const g = lx.geo;
  return { x: 0, w: g.colW, B: g.body, L: g.L, snap: false, ids: [], boxes: [], maxH: g.bottom - g.top, fixed: false, depth: 0 };
}

function piece(lx: Lx, text: string, spec: TextSpec): Piece {
  const st: RunStyle = { font: lx.font(spec), size: spec.size, color: spec.color };
  if (spec.letterSpacing) st.letterSpacing = spec.letterSpacing;
  return { text, st };
}

/** Centred lines of plain text, balanced; returns the y after the last line. */
function centred(lx: Lx, items: DrawItem[], text: string, spec: TextSpec, w: number, y: number, maxW = w): number {
  const font = lx.font(spec);
  const ls = spec.letterSpacing ?? 0;
  const widthOf = (s: string): number => lx.widthSpaced(font, s, ls) - (s ? ls : 0);
  const lines = balanceLines(text, maxW, widthOf);
  const base = baselineOffset(lx, spec);
  for (const line of lines) {
    const it: DrawItem = { kind: "text", x: (w - widthOf(line)) / 2, y: y + base, text: line, font, color: spec.color };
    if (ls) it.letterSpacing = ls;
    items.push(it);
    y += spec.lh;
  }
  return y;
}

function widest(lx: Lx, text: string, spec: TextSpec, maxW: number): number {
  const font = lx.font(spec);
  const ls = spec.letterSpacing ?? 0;
  let m = 0;
  for (const line of balanceLines(text, maxW, (s) => lx.widthSpaced(font, s, ls) - (s ? ls : 0))) m = Math.max(m, lx.widthSpaced(font, line, ls) - ls);
  return m;
}

/** Generated pages are not blocks of the book: their rows carry no block id and no page anchor. */
function anonymous(rows: Row[]): Row[] {
  for (const r of rows) {
    r.ids = [];
    r.block = null;
    for (const it of r.items) delete it.block;
  }
  return rows;
}

// ---------------------------------------------------------------------------
// Title page
// ---------------------------------------------------------------------------

/**
 * Title page: one row as tall as the text block. Title in display
 * lettering in the upper third, subtitle, a small ornament, the authors,
 * and the publisher alone at the foot. Shrinks as a whole when the page is
 * too small for it.
 */
export function titleRows(lx: Lx): { rows: Row[]; semantics: SemanticNode[] } {
  const g = lx.geo;
  const pal = lx.input.palette;
  const meta = lx.book.meta;
  const w = g.colW;
  const H = g.bottom - g.top;
  const title = String(meta.title ?? "");
  const subtitle = typeof meta.subtitle === "string" ? meta.subtitle : "";
  const authors = (Array.isArray(meta.authors) ? meta.authors : []).filter((a) => typeof a === "string" && a.trim() !== "");
  const publisher = typeof meta.publisher === "string" ? meta.publisher : "";

  let items: DrawItem[] = [];
  // Shrink the composition until it fits; on a page too small even for that, the publisher line goes.
  for (let s = 1, showPublisher = publisher !== ""; ; s *= 0.9) {
    if (s < 0.4) {
      if (!showPublisher) break;
      showPublisher = false;
      s = 0.45;
    }
    items = [];
    const B = g.body * s;
    const L = g.L * s;
    const titleSpec: TextSpec = { family: "heading", size: B * 2.6, weight: 500, italic: false, color: pal.ink, lh: Math.round(B * 2.6 * 1.14) };
    const subSpec: TextSpec = { family: "body", size: B * 1.12, weight: 400, italic: true, color: pal.muted, lh: Math.round(B * 1.12 * 1.4) };
    const authorSpec: TextSpec = { family: "body", size: B * 1.06, weight: 400, italic: false, color: pal.ink, lh: Math.round(B * 1.06 * 1.5) };
    const pubSize = B * 0.68;
    const pubSpec: TextSpec = { family: "sans", size: pubSize, weight: 500, italic: false, color: pal.muted, lh: Math.round(pubSize * 1.5), letterSpacing: pubSize * 0.16 };

    let y = H * 0.2 * Math.min(1, s * s * 1.2);
    y = centred(lx, items, title, titleSpec, w, y);
    if (subtitle) y = centred(lx, items, subtitle, subSpec, w, y + L * 0.45, w * 0.86);
    const orn = Math.min(B * 0.95, L * 0.8);
    y += L * 1.1;
    items.push({ kind: "ornament", style: "fleuron", x: (w - orn * 5.6) / 2, y, w: orn * 5.6, h: orn, color: pal.accent });
    y += orn + L * 1.1;
    if (authors.length > 0 && authors.length <= 4) for (const a of authors) y = centred(lx, items, a, authorSpec, w, y);
    else if (authors.length > 4) y = centred(lx, items, authors.join(", "), authorSpec, w, y);

    let footTop = H;
    if (showPublisher) {
      const text = lx.upper(publisher);
      const font = lx.font(pubSpec);
      const ls = pubSpec.letterSpacing ?? 0;
      const lines = balanceLines(text, w, (t) => lx.widthSpaced(font, t, ls) - ls);
      footTop = H - lines.length * pubSpec.lh;
      centred(lx, items, text, pubSpec, w, footTop);
    }
    const tooWide = widest(lx, title, titleSpec, w) > w + 0.5;
    if (!tooWide && y <= footTop - (showPublisher ? L * 0.5 : 0)) break;
  }

  const row = makeRow(H, [], null, []);
  row.items = items;
  const semantics: SemanticNode[] = [{ role: "heading", level: 1, text: title }];
  if (subtitle) semantics.push({ role: "paragraph", text: subtitle });
  if (authors.length > 0) semantics.push({ role: "paragraph", text: authors.join(", ") });
  if (publisher) semantics.push({ role: "paragraph", text: publisher });
  return { rows: [row], semantics };
}

// ---------------------------------------------------------------------------
// Colophon
// ---------------------------------------------------------------------------

/** Copyright page: small type, set at the foot of the page by the paginator (`valign: "bottom"`). */
export function colophonRows(lx: Lx, ch: Chapter): Row[] {
  const g = lx.geo;
  const pal = lx.input.palette;
  const meta = lx.book.meta;
  const wd = words(lx);
  const env = columnEnv(lx);
  const id = ch.id;
  const size = g.body * 0.78;
  const lh = Math.round(size * 1.5);
  const ink: TextSpec = { family: "body", size, weight: 400, italic: false, color: pal.ink, lh };
  const italic: TextSpec = { ...ink, italic: true };
  const muted: TextSpec = { ...ink, color: pal.muted };
  const mono: TextSpec = { family: "mono", size: size * 0.9, weight: 400, italic: false, color: pal.ink, lh };
  const rows: Row[] = [];
  let group = 0;

  const para = (pieces: Piece[], spec: TextSpec, gap: number): void => {
    const part = textRows(lx, env, id, pieces, { spec, align: "left", x: 0, w: g.colW, keepAll: true, sem: { key: `${id}/${group++}`, role: "paragraph" } });
    const first = part[0];
    if (first && rows.length > 0) first.gapBefore = gap;
    for (const r of part) rows.push(r);
  };
  /** Two-column line: label in the muted colour, value beside it (wrapping in its own column). */
  const pair = (label: string, value: Piece[], labelW: number, gap: number): void => {
    const part = textRows(lx, env, id, value, { spec: ink, align: "left", x: labelW, w: Math.max(size * 4, g.colW - labelW), keepAll: true, sem: { key: `${id}/${group++}`, role: "paragraph" }, semPrefix: `${label}: ` });
    const first = part[0];
    if (!first) return;
    first.items.unshift({ kind: "text", x: 0, y: baselineOffset(lx, muted), text: label, font: lx.font(muted), color: pal.muted });
    if (rows.length > 0) first.gapBefore = gap;
    for (const r of part) rows.push(r);
  };
  const str = (v: unknown): string => (typeof v === "string" ? v.trim() : "");
  const block = lh * 0.9;

  const title = str(meta.title);
  if (title) {
    const sub = str(meta.subtitle);
    para([piece(lx, sub ? `${title}: ${sub}` : title, italic)], italic, 0);
  }
  const authors = (Array.isArray(meta.authors) ? meta.authors : []).map(str).filter((a) => a !== "");
  if (authors.length > 0) para([piece(lx, authors.join(", "), ink)], ink, 0);

  const edition = str(meta.edition);
  const publisher = str(meta.publisher);
  const published = str(meta.published);
  let gap = block;
  if (edition) {
    para([piece(lx, edition, ink)], ink, gap);
    gap = 0;
  }
  if (publisher) {
    para([piece(lx, publisher, ink)], ink, gap);
    gap = 0;
  }
  if (published) {
    para([piece(lx, `${wd.published} ${published}`, ink)], ink, gap);
    gap = 0;
  }
  const rights = str(meta.rights);
  if (rights) para([piece(lx, rights, ink)], ink, block);

  const credits = (Array.isArray(meta.credits) ? meta.credits : []).filter((c) => c && typeof c === "object" && str(c.role) !== "" && str(c.name) !== "");
  const mutedFont = lx.font(muted);
  const labels = [...credits.map((c) => str(c.role)), wd.identifier, wd.version, wd.language];
  let labelW = 0;
  for (const l of labels) labelW = Math.max(labelW, lx.width(mutedFont, l));
  labelW = Math.min(g.colW * 0.45, labelW + size * 1.2);
  gap = block;
  for (const c of credits) {
    pair(str(c.role), [piece(lx, str(c.name), ink)], labelW, gap);
    gap = 0;
  }
  gap = block;
  const idText = str(meta.id);
  if (idText) {
    pair(wd.identifier, [piece(lx, idText, mono)], labelW, gap);
    gap = 0;
  }
  const version = str(meta.version);
  if (version) {
    pair(wd.version, [piece(lx, version, ink)], labelW, gap);
    gap = 0;
  }
  const language = str(meta.language);
  if (language) pair(wd.language, [piece(lx, language, ink)], labelW, gap);

  para([piece(lx, wd.madeWith, muted)], muted, block);
  return anonymous(rows);
}

// ---------------------------------------------------------------------------
// Contents
// ---------------------------------------------------------------------------

export function contentsTitle(lx: Lx): string {
  return words(lx).contents;
}

/** Height of the white above the first line of an opener-like page, on the baseline grid. */
function sinkage(lx: Lx, share: number, minimum: number): number {
  const g = lx.geo;
  const target = Math.max(g.pageH * share - g.top, minimum);
  return Math.min(Math.ceil(target / g.L) * g.L, Math.max(0, g.bottom - g.top - 3 * g.L));
}

/**
 * Contents lines with dot leaders and right-aligned page numbers.
 * `pageOf` returns the printed number of an entry, or `null` on the first
 * pass, when the body is not paginated yet; the number column is reserved
 * either way so both passes produce the same pages.
 */
export function contentsRows(lx: Lx, ch: Chapter, toc: TocLine[], pageOf: (line: TocLine) => string | null): { rows: Row[]; semantics: SemanticNode[] } {
  const g = lx.geo;
  const pal = lx.input.palette;
  const env = columnEnv(lx);
  const B = g.body;
  const L = g.L;
  const w = g.colW;
  const rows: Row[] = [];
  const heading = contentsTitle(lx);

  // Heading block, in the manner of a chapter opener but lighter.
  const headSpec: TextSpec = { family: "heading", size: B * 1.55, weight: 500, italic: false, color: pal.ink, lh: Math.round(B * 1.55 * 1.2) };
  const headItems: DrawItem[] = [];
  const y0 = Math.max(0, Math.min(g.pageH * 0.15 - g.top, (g.bottom - g.top) * 0.2));
  let y = centred(lx, headItems, heading, headSpec, w, y0);
  const ornH = Math.max(5, B * 0.42);
  const ornW = Math.min(w * 0.4, B * 4.2);
  y += L * 0.35;
  headItems.push({ kind: "ornament", style: "chapter", x: (w - ornW) / 2, y, w: ornW, h: ornH, color: pal.accent });
  y += ornH;
  const head = makeRow(Math.max(sinkage(lx, 0.27, 0), Math.ceil((y + L * 0.9) / L) * L), [], null, []);
  head.items = headItems;
  head.keepWithNext = true;
  rows.push(head);

  const numFont = lx.font({ family: "body", size: B * 0.92, weight: 400 });
  const numW = Math.max(lx.width(numFont, "000"), B * 1.4);
  const hasNumbers = toc.some((t) => typeof t.number === "number");
  const ordFont = lx.font({ family: "body", size: B, weight: 400 });
  let ordW = 0;
  if (hasNumbers) for (const t of toc) if (typeof t.number === "number") ordW = Math.max(ordW, lx.width(ordFont, `${t.number}.`));
  const gutter = hasNumbers ? ordW + B * 0.55 : 0;
  const dotFont = lx.font({ family: "body", size: B * 0.8, weight: 400 });
  const dotW = Math.max(0.5, lx.width(dotFont, "."));
  const pitch = Math.max(dotW * 2.4, B * 0.42);

  toc.forEach((entry, n) => {
    const major = entry.level !== 2;
    const size = major ? B : B * 0.9;
    const spec: TextSpec = { family: "body", size, weight: 400, italic: false, color: major ? pal.ink : pal.muted, lh: major ? L : Math.round(L * 0.9) };
    const x = major ? gutter : gutter + B * 1.1;
    const textW = Math.max(B * 3, w - x - numW - B * 0.9);
    const part = textRows(lx, env, ch.id, [piece(lx, String(entry.title ?? ""), spec)], { spec, align: "left", x, w: textW, keepAll: true });
    const first = part[0];
    const last = part[part.length - 1];
    if (!first || !last) return;
    const base = baselineOffset(lx, spec);
    if (major && typeof entry.number === "number") {
      const ord = `${entry.number}.`;
      first.items.unshift({ kind: "text", x: gutter - B * 0.55 - lx.width(ordFont, ord), y: base, text: ord, font: ordFont, color: pal.muted });
    }
    // Leader and number sit on the last line of the title.
    let inkEnd = x;
    for (const it of last.items) if (it.kind === "text") inkEnd = Math.max(inkEnd, it.x + lx.width(it.font, it.text));
    const num = pageOf(entry);
    const numText = num ?? "";
    const numX = w - lx.width(numFont, numText);
    if (numText) last.items.push({ kind: "text", x: numX, y: base, text: numText, font: numFont, color: major ? pal.ink : pal.muted });
    // Dots sit on a common grid measured from the right edge, so leaders align down the page.
    const from = inkEnd + B * 0.5;
    const to = w - numW - B * 0.25;
    const count = Math.floor((to - from) / pitch);
    if (count >= 3) {
      const startX = to - count * pitch;
      last.items.push({ kind: "text", x: startX, y: base, text: ".".repeat(count), font: dotFont, color: pal.muted, opacity: 0.5, letterSpacing: pitch - dotW });
    }
    if (major && n > 0) first.gapBefore = L * 0.5;
    const label = String(entry.title ?? "");
    for (const r of part) {
      r.regions.push({ type: "toc", x: 0, y: 0, w, h: r.h, block: ch.id, target: entry.target, label });
      rows.push(r);
    }
  });

  const semantics: SemanticNode[] = [{ role: "heading", level: 1, text: heading }];
  if (toc.length > 0) semantics.push({ role: "list", ordered: false, items: toc.map((t) => String(t.title ?? "")) });
  return { rows: anonymous(rows), semantics };
}

// ---------------------------------------------------------------------------
// Chapter opener
// ---------------------------------------------------------------------------

/**
 * The opener is a single row at the top of the chapter's first page: the
 * numeral, a small ornament and the title, followed by white down to about
 * a third of the page, where the text starts on a grid line.
 */
export function openerRow(lx: Lx, ch: Chapter): { row: Row; semantics: SemanticNode[] } {
  const g = lx.geo;
  const pal = lx.input.palette;
  const B = g.body;
  const L = g.L;
  const w = g.colW;
  const avail = g.bottom - g.top;
  const title = String(ch.title ?? "");
  let items: DrawItem[] = [];
  let y = 0;
  // Shrink the whole opener on pages too small for it.
  for (let s = 1; ; s *= 0.88) {
    items = [];
    const b = B * s;
    y = Math.max(0, Math.min(g.pageH * 0.15 - g.top, avail * 0.2)) * s;
    if (typeof ch.number === "number" && Number.isFinite(ch.number)) {
      const numSpec: TextSpec = { family: "heading", size: b * 1.25, weight: 400, italic: false, color: pal.accent, lh: Math.round(b * 1.25 * 1.2) };
      y = centred(lx, items, String(ch.number), numSpec, w, y);
      y += L * s * 0.2;
    }
    const ornH = Math.max(4, b * 0.42);
    const ornW = Math.min(w * 0.4, b * 4.2);
    items.push({ kind: "ornament", style: "chapter", x: (w - ornW) / 2, y, w: ornW, h: ornH, color: pal.accent });
    y += ornH + L * s * 0.55;
    const size = b * 1.9;
    const titleSpec: TextSpec = { family: "heading", size, weight: 500, italic: false, color: pal.ink, lh: Math.round(size * 1.2) };
    y = centred(lx, items, title, titleSpec, w, y);
    // A very long word must not overflow the measure, nor the opener the page.
    if (s < 0.45 || (widest(lx, title, titleSpec, w) <= w + 0.5 && y <= avail)) break;
  }

  // Text starts about a third of the way down, on a grid line. When the page is too small to
  // hold the opener and two lines of text, the opener keeps the page to itself.
  let h = Math.max(sinkage(lx, 0.33, 0), Math.ceil((y + L * 1.2) / L) * L);
  const alone = h > avail - 2 * L;
  if (alone) h = avail;
  const row = makeRow(Math.max(h, 0), [], null, []);
  row.items = items;
  row.keepWithNext = !alone;
  return { row, semantics: [{ role: "heading", level: 1, text: title }] };
}
