/**
 * Contract between the layout engine, the painter and the rest of the reader.
 *
 * Pipeline:
 *
 *   IR Book ──layoutBook()──▶ BookLayout (pages of positioned draw items)
 *                                │
 *              paintPage(ctx) ◀──┘   → a canvas, uploaded as a texture and
 *                                      mapped onto the page mesh by the
 *                                      WebGL renderer (or drawn directly by
 *                                      the Canvas 2D fallback).
 *
 * Everything in a BookLayout is plain data in CSS pixels relative to the
 * top-left corner of ONE page. Nothing here touches the DOM, so layout can
 * run under Node in unit tests with a fake `Measurer`.
 *
 * Interactive content is drawn on the canvas (so that it bends with the
 * page while it turns) and described again as `regions`; the app puts
 * transparent, focusable DOM controls over those regions while a page is at
 * rest.
 */
import type { Action, Anim, Book, ChapterKind, Color, Covers, Link, State } from "../ir";

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

export interface ReaderSettings {
  /** Type size multiplier, 0.8 … 1.6 (1 = the book's nominal size). */
  fontScale: number;
  lineHeight: "compact" | "normal" | "relaxed";
  margins: "narrow" | "normal" | "wide";
  /** `null` = follow the book's theme. */
  justify: boolean | null;
  hyphenate: boolean | null;
}

/** Colours after resolving the book theme against the reader's light/dark mode. */
export interface ResolvedPalette {
  dark: boolean;
  paper: Color;
  ink: Color;
  accent: Color;
  /** Secondary text: captions, folios, running heads. */
  muted: Color;
  /** Hairlines and box borders. */
  rule: Color;
  /** Tinted box / widget background. */
  tint: Color;
}

/** CSS `font-family` values that are already loaded and usable on a canvas. */
export interface FontSet {
  body: string;
  heading: string;
  mono: string;
  /** UI lettering inside pages: buttons, players, folios when the body face is unsuitable. */
  sans: string;
}

export interface FontMetrics {
  /** Distances from the baseline for a font of the given CSS `font` shorthand, in px. */
  ascent: number;
  descent: number;
  /** Height of a lower-case x, in px. */
  xHeight: number;
  /**
   * Height of a capital H, in px. Optional: when absent the layout estimates
   * it from `xHeight`. Drop capitals and widget labels are aligned with it.
   */
  capHeight?: number;
}

/** Text measurement. The browser implementation wraps a canvas context. */
export interface Measurer {
  /** Advance width in px of `text` set in the CSS `font` shorthand. */
  width(font: string, text: string): number;
  metrics(font: string): FontMetrics;
}

/** What the reader remembers about interactive blocks (persisted per book). */
export interface UiRecords {
  /** quiz block id → first picked answer and whether it was correct. */
  quiz: Record<string, { picked: number; correct: boolean }>;
  /** choice block id → index of the selected option (into the IR `options` array). */
  choice: Record<string, number>;
}

export interface LayoutInput {
  book: Book;
  /** Size of one page in CSS px. For `fixed` books the app already matched `meta.page_aspect`. */
  page: { width: number; height: number };
  /** Two-page spread: even page indices are right-hand (recto) pages and inner/outer margins differ. */
  spread: boolean;
  settings: ReaderSettings;
  palette: ResolvedPalette;
  fonts: FontSet;
  measurer: Measurer;
  state: State;
  ui: UiRecords;
  /** Split a word at its legal hyphenation points (`["hy","phen","ate"]`). Absent = no hyphenation. */
  hyphenate?: (word: string) => string[];
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

export interface BookLayout {
  /** Pages in reading order: generated front matter, then every chapter whose `when` holds. */
  pages: PageLayout[];
  chapters: ChapterSpan[];
  /** Anchor name → page index, for every IR anchor and chapter id that is currently visible. */
  anchors: Map<string, number>;
  /** Block id → index of the page where the block starts (search hits, bookmarks). */
  blockPages: Map<string, number>;
  /** Visible table-of-contents entries with the page they point at. */
  toc: TocLine[];
  /** First page after the front matter (jump target `start`). */
  startPage: number;
  /** Page of the generated contents (jump target `contents`), if the book has one. */
  contentsPage: number | null;
}

export interface ChapterSpan {
  id: string;
  kind: ChapterKind;
  title: string;
  firstPage: number;
  pageCount: number;
}

export interface TocLine {
  title: string;
  target: string;
  level: 1 | 2;
  number?: number;
  page: number;
}

/** Where a page starts, so the reading position survives a re-layout. */
export interface PageAnchor {
  chapterId: string;
  /** First block with a fragment on the page; `null` for generated pages and chapter openers. */
  block: string | null;
  /** Index of the block's first line on this page (0 when the block starts here). */
  line: number;
}

export interface PageLayout {
  index: number;
  chapterId: string;
  chapterKind: ChapterKind;
  /** A fixed `page` composition (no margins, no folio). */
  fixed: boolean;
  /** Page background override (fixed pages); otherwise the palette's paper. */
  background?: Color;
  /** Draw items in paint order. */
  items: DrawItem[];
  /** Interactive areas. May overlap draw items; never overlap each other. */
  regions: Region[];
  anchor: PageAnchor;
  /** Ids of blocks with at least one fragment on this page, in order. */
  blocks: string[];
  /** Reading-order outline of the page for the accessibility mirror. */
  semantics: SemanticNode[];
  /** True when any item carries an entrance animation. */
  animated: boolean;
}

export type SemanticNode =
  | { role: "heading"; level: 1 | 2 | 3; text: string; block?: string }
  | { role: "paragraph" | "quote" | "caption" | "note"; text: string; block?: string }
  | { role: "list"; ordered: boolean; items: string[]; block?: string }
  | { role: "image"; alt: string; block?: string }
  | { role: "separator" };

interface ItemBase {
  /** Owning block id; items of one block animate together. */
  block?: string;
  anim?: Anim;
  /** 0 … 1, default 1. */
  opacity?: number;
}

export type DrawItem = ItemBase &
  (
    | {
        kind: "text";
        /** Left edge and baseline. */
        x: number;
        y: number;
        text: string;
        /** CSS `font` shorthand, e.g. `italic 600 18px "Literata Variable"`. */
        font: string;
        color: Color;
        /** Extra space between letters, px. */
        letterSpacing?: number;
        underline?: boolean;
      }
    | { kind: "rect"; x: number; y: number; w: number; h: number; fill?: Color; stroke?: Color; lineWidth?: number; radius?: number }
    | { kind: "line"; x1: number; y1: number; x2: number; y2: number; color: Color; width: number }
    | { kind: "image"; x: number; y: number; w: number; h: number; resource: string; fit: "contain" | "cover"; radius?: number }
    | { kind: "ornament"; style: OrnamentStyle; x: number; y: number; w: number; h: number; color: Color }
    | { kind: "widget"; widget: Widget; x: number; y: number; w: number; h: number }
  );

export type OrnamentStyle =
  /** Section break: a small typographic flourish centred in its box. */
  | "fleuron"
  /** Three spaced asterisks. */
  | "asterism"
  /** Decoration under a chapter number on an opener. */
  | "chapter";

/** Controls drawn on the page. Their look depends on the reader's records, already resolved here. */
export type Widget =
  | { type: "button"; label: string }
  | { type: "option"; label: string; selected: boolean }
  | { type: "answer"; label: string; state: "idle" | "correct" | "wrong" | "missed" | "dimmed" }
  | { type: "video"; resource: string; poster?: string }
  | { type: "audio"; resource: string; title: string }
  | { type: "gallery"; resources: string[]; count: number };

interface RegionBase {
  x: number;
  y: number;
  w: number;
  h: number;
  /** Block that owns the region. */
  block: string;
}

export type Region = RegionBase &
  (
    | { type: "link"; link: Link; label: string }
    | { type: "button"; label: string; actions: Action[] }
    | { type: "option"; index: number; label: string; prompt: string; selected: boolean; actions: Action[] }
    | { type: "answer"; index: number; label: string; question: string; answered: boolean }
    | { type: "video"; resource: string; autoplay: boolean; loop: boolean; muted: boolean; caption?: string }
    | { type: "audio"; resource: string; caption?: string }
    | { type: "gallery"; resources: string[]; caption?: string }
    | { type: "image"; resource: string; caption?: string }
    | { type: "toc"; target: string; label: string }
  );

// ---------------------------------------------------------------------------
// Painting
// ---------------------------------------------------------------------------

/** Decoded images by resource name. Returns `null` until the image is ready. */
export interface ImageProvider {
  get(resource: string): CanvasImageSource | null;
  /** Intrinsic size from the IR, available before the bitmap is. */
  size(resource: string): { width: number; height: number } | null;
}

export type Ctx2D = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;

export interface PaintEnv {
  palette: ResolvedPalette;
  fonts: FontSet;
  images: ImageProvider;
  /**
   * Milliseconds since the page became visible, for entrance animations.
   * `undefined` paints the final state (used for textures of turning pages
   * and whenever reduced motion is on).
   */
  time?: number;
}

export interface PaintResult {
  /** True while an entrance animation is still running at `env.time`. */
  animating: boolean;
  /** Resources that were needed but not decoded yet (the caller repaints when they arrive). */
  missing: string[];
}

/** Which blank or generated surface to paint. */
export type Surface =
  /** A blank paper page (the back of a single-sided leaf, padding at the end of the book). */
  | "blank"
  /** Inside of the front or back board. */
  | "endpaper"
  | "front-cover"
  | "back-cover";

/** The functions the rest of the reader imports from `layout/` and `paint/`. */
export interface LayoutApi {
  layoutBook(input: LayoutInput): BookLayout;
}

export interface PaintApi {
  /**
   * Paint a page. The context is already scaled so that 1 unit = 1 CSS px
   * and its origin is the page's top-left corner; `size` is the page size in
   * CSS px. Must fill the whole page (paper colour or `page.background`).
   */
  paintPage(ctx: Ctx2D, page: PageLayout, size: { width: number; height: number }, env: PaintEnv): PaintResult;
  /** Paint a cover, endpaper or blank page of the same size as a page. */
  paintSurface(ctx: Ctx2D, surface: Surface, book: Book, size: { width: number; height: number }, env: PaintEnv): PaintResult;
}

export type { Covers, State };
