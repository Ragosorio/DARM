/**
 * Shared state of one `layoutBook` run: page geometry, font strings and the
 * measurement cache. Nothing here is specific to a block type.
 */
import type { Book, Resource, Value } from "../ir";
import type { LayoutInput } from "./types";

export type Family = "body" | "heading" | "mono" | "sans";

export interface FontSpec {
  family: Family;
  size: number;
  weight?: number;
  italic?: boolean;
}

export interface Metrics {
  ascent: number;
  descent: number;
  xHeight: number;
  capHeight: number;
}

export interface Geometry {
  pageW: number;
  pageH: number;
  /** Top and bottom of the text block. `bottom - top` is a whole number of body lines. */
  top: number;
  bottom: number;
  /** Side margins. In single-page mode both are equal. */
  inner: number;
  outer: number;
  /** The measure: width of the text column. */
  colW: number;
  /** Body type size and line height (the baseline grid), px. */
  body: number;
  L: number;
  /** Line-height factor chosen by the reader (1.38 / 1.5 / 1.68). */
  leading: number;
  /** Vertical snapping step for body lines. */
  grid: number;
}

/** Page width / body size at fontScale 1 (before clamping to 14.5-21 px). */
export const BODY_DIVISOR = 34.5;

const LEADING = { compact: 1.38, normal: 1.5, relaxed: 1.68 } as const;
const SIDE_SCALE = { narrow: 0.7, normal: 1, wide: 1.3 } as const;
const VERT_SCALE = { narrow: 0.84, normal: 1, wide: 1.16 } as const;

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

function finite(v: number, fallback: number): number {
  return Number.isFinite(v) ? v : fallback;
}

export function computeGeometry(input: LayoutInput): Geometry {
  const pageW = Math.max(40, finite(input.page.width, 400));
  const pageH = Math.max(60, finite(input.page.height, 600));
  const s = input.settings;
  const side = SIDE_SCALE[s.margins] ?? 1;
  const vert = VERT_SCALE[s.margins] ?? 1;

  // Inner margins are wider than outer ones on a spread because the gutter
  // visually swallows part of them; a single page is symmetric.
  let outer = pageW * 0.105 * side;
  let inner = pageW * 0.125 * side;
  if (!input.spread) outer = inner = pageW * 0.1 * side;
  outer = Math.max(outer, 14);
  inner = Math.max(inner, 14);
  // Absurdly small pages: never give more than 30% of the width to margins.
  const maxSides = pageW * 0.3;
  if (inner + outer > maxSides) {
    const k = maxSides / (inner + outer);
    inner *= k;
    outer *= k;
  }

  const fontScale = clamp(finite(s.fontScale, 1), 0.5, 3);
  // The nominal rule is width/29, but with the built-in serif (Literata, ~0.48em per character)
  // that yields a 46-character measure. width/34.5 lands the default spread page at ~56
  // characters per line, inside the 55-70 range a book column wants.
  const body = Math.round(clamp(pageW / BODY_DIVISOR, 14.5, 21) * fontScale * 100) / 100;
  const leading = LEADING[s.lineHeight] ?? 1.5;
  const L = Math.max(4, Math.round(body * leading));

  let top = Math.max(pageH * 0.09 * vert, Math.min(30, pageH * 0.12));
  let bottomMargin = Math.max(pageH * 0.11 * vert, Math.min(36, pageH * 0.14));
  // Guarantee at least two lines of text on any page.
  const need = Math.min(2 * L, pageH * 0.7);
  if (pageH - top - bottomMargin < need) {
    const k = Math.max(0, pageH - need) / (top + bottomMargin);
    top *= k;
    bottomMargin *= k;
  }
  top = Math.round(top);
  const lines = Math.max(1, Math.floor((pageH - top - bottomMargin + 0.01) / L));
  const bottom = Math.min(pageH, top + lines * L);

  return {
    pageW,
    pageH,
    top,
    bottom,
    inner,
    outer,
    colW: Math.max(20, pageW - inner - outer),
    body,
    L,
    leading,
    grid: input.book.theme.paragraph === "spaced" ? L / 2 : L,
  };
}

/** Own-property lookup: state and ui records are plain objects built from untrusted names. */
export function own<T>(rec: Record<string, T> | undefined, key: string): T | undefined {
  return rec && Object.prototype.hasOwnProperty.call(rec, key) ? rec[key] : undefined;
}

export class Lx {
  readonly input: LayoutInput;
  readonly book: Book;
  readonly geo: Geometry;
  readonly justify: boolean;
  readonly hyphenate: ((word: string) => string[]) | undefined;
  readonly resources = new Map<string, Resource>();
  /** Hyphenation points per word (indices into the word), shared by all paragraphs. */
  readonly hyphenCache = new Map<string, number[]>();
  /** First page of the body, set by `layoutBook` before the first chapter is paginated. */
  startPage: number | null = null;

  /** `width` as a free function, for the typesetter. */
  readonly measure = (font: string, text: string): number => this.width(font, text);

  private readonly widths = new Map<string, Map<string, number>>();
  private readonly mets = new Map<string, Metrics>();
  private readonly fontStrings = new Map<string, string>();

  constructor(input: LayoutInput) {
    this.input = input;
    this.book = input.book;
    this.geo = computeGeometry(input);
    this.justify = input.settings.justify ?? input.book.theme.justify;
    const wantHyphens = input.settings.hyphenate ?? input.book.theme.hyphenate;
    this.hyphenate = wantHyphens ? input.hyphenate : undefined;
    for (const r of input.book.resources) if (!this.resources.has(r.name)) this.resources.set(r.name, r);
  }

  /** CSS `font` shorthand. Sizes are rounded so that equal styles share cache entries. */
  font(spec: FontSpec): string {
    const size = Math.round(spec.size * 100) / 100;
    const weight = spec.weight ?? 400;
    const key = `${spec.family}|${size}|${weight}|${spec.italic ? 1 : 0}`;
    let f = this.fontStrings.get(key);
    if (f === undefined) {
      f = `${spec.italic ? "italic " : ""}${weight} ${size}px ${this.input.fonts[spec.family]}`;
      this.fontStrings.set(key, f);
    }
    return f;
  }

  width(font: string, text: string): number {
    if (text === "") return 0;
    let m = this.widths.get(font);
    if (!m) {
      m = new Map();
      this.widths.set(font, m);
    }
    let w = m.get(text);
    if (w === undefined) {
      w = this.input.measurer.width(font, text);
      if (!Number.isFinite(w) || w < 0) w = 0;
      m.set(text, w);
    }
    return w;
  }

  /** Width including letter spacing (the canvas adds it after every character). */
  widthSpaced(font: string, text: string, letterSpacing: number): number {
    return this.width(font, text) + (letterSpacing ? letterSpacing * text.length : 0);
  }

  metrics(font: string): Metrics {
    let m = this.mets.get(font);
    if (!m) {
      const raw = this.input.measurer.metrics(font);
      const size = fontSize(font);
      const ascent = raw.ascent > 0 && Number.isFinite(raw.ascent) ? raw.ascent : size * 0.9;
      const descent = raw.descent >= 0 && Number.isFinite(raw.descent) ? raw.descent : size * 0.25;
      const xHeight = raw.xHeight > 0 && Number.isFinite(raw.xHeight) ? raw.xHeight : size * 0.48;
      const cap = raw.capHeight;
      const capHeight = cap !== undefined && cap > 0 && Number.isFinite(cap) ? cap : Math.min(size * 0.74, xHeight * 1.42);
      m = { ascent, descent, xHeight, capHeight };
      this.mets.set(font, m);
    }
    return m;
  }

  /** Left edge of the text column on a page. Even indices are recto pages: their inner margin is on the left. */
  left(pageIndex: number): number {
    if (!this.input.spread) return this.geo.inner;
    return pageIndex % 2 === 0 ? this.geo.inner : this.geo.outer;
  }

  stateValue(name: string): Value | undefined {
    return own(this.input.state, name);
  }

  /** Upper-casing that survives a malformed language tag. */
  upper(text: string): string {
    try {
      return text.toLocaleUpperCase(this.book.meta.language);
    } catch {
      return text.toUpperCase();
    }
  }
}

/** Pixel size out of a CSS font shorthand built by `Lx.font`. */
export function fontSize(font: string): number {
  const m = /(\d+(?:\.\d+)?)px/.exec(font);
  return m ? Number(m[1]) : 16;
}
