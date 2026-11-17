/**
 * The reading view: one opened book on the stage, with its chrome, panels,
 * overlays for interactive content, state, bookmarks and persistence.
 */
import type { OpenBook } from "../core/book";
import type { BookRecord, Bookmark, Library, ReadingState } from "../core/library";
import { type Prefs, resolveDark, resolveReducedMotion } from "../core/prefs";
import { applyActions, reconcileState } from "../core/state";
import { type SurfaceKey, posForPage } from "../flip/controller";
import type { Action, State, Transition } from "../ir";
import { layoutBook } from "../layout/index";
import type { BookLayout, FontSet, Measurer, PageAnchor, PageLayout, Region, ResolvedPalette, UiRecords } from "../layout/types";
import { loadFonts } from "../paint/fonts";
import { loadHyphenator } from "../paint/hyphenate";
import { paintPage, paintSurface } from "../paint/index";
import { createCanvasMeasurer } from "../paint/measure";
import { GlRenderer } from "../gl/renderer";
import { resolvePalette, sceneStyle } from "./color";
import { h, icon, iconButton } from "./dom";
import { type OverlayHost, Overlays } from "./overlays";
import { Panels } from "./panels";
import { type PageGeometry, Stage } from "./stage";

export interface ReaderOptions {
  root: HTMLElement;
  book: OpenBook;
  library: Library;
  /** Library record, or null for a book opened without importing it (preview). */
  record: BookRecord | null;
  prefs: Prefs;
  onPrefs(prefs: Prefs): void;
  onExit(): void;
  /** Force the Canvas 2D renderer (diagnostics, tests). */
  forceFallback?: boolean;
}

/** What the tests and the performance overlay can read. */
/** Entrance animations start this long after a turn begins, so they play as the leaf lands. */
const ENTRANCE_LEAD_MS = 180;

export interface ReaderDiagnostics {
  renderer: "webgl2" | "canvas2d";
  pages: number;
  page: number | null;
  pos: number;
  count: number;
  settled: boolean;
  spread: boolean;
  layoutMs: number;
  frameTimes: number[];
  state: State;
  /** Approximate GPU memory held by page textures, bytes (0 for the 2D renderer). */
  textureBytes: number;
}

export class ReaderView implements OverlayHost {
  readonly el: HTMLElement;
  readonly book: OpenBook;
  private library: Library;
  private record: BookRecord | null;
  prefs: Prefs;
  private onPrefs: (p: Prefs) => void;
  private onExit: () => void;
  private stageEl: HTMLElement;
  private stage!: Stage;
  private overlays!: Overlays;
  private panels!: Panels;
  private chromeTop: HTMLElement;
  private chromeBottom: HTMLElement;
  private hint: HTMLElement;
  private live: HTMLElement;
  private scrubber: HTMLInputElement;
  private locationLabel: HTMLElement;
  private bookmarkButton!: HTMLButtonElement;
  private fonts!: FontSet;
  private measurer!: Measurer;
  private hyphenate: ((word: string) => string[]) | undefined;
  palette!: ResolvedPalette;
  layout: BookLayout | null = null;
  private geometry: PageGeometry | null = null;
  vars: State = {};
  ui: UiRecords = { quiz: {}, choice: {} };
  private reading!: ReadingState;
  /** Reading position to return to after a re-layout. */
  private anchor: PageAnchor | null = null;
  /** Start time of entrance animations, per page index. */
  private animStart = new Map<number, number>();
  private animating = new Set<number>();
  /** Pages whose entrance has already played on this visit to them. */
  private seen = new Set<number>();
  private saveTimer = 0;
  private settled = true;
  private layoutMs = 0;
  private chromeVisible = true;
  private chromeTimer = 0;
  private disposed = false;
  private started = false;
  private pendingOpen: number | null = null;
  private mediaQueries: MediaQueryList[] = [];
  private forceFallback: boolean;

  constructor(options: ReaderOptions) {
    this.book = options.book;
    this.library = options.library;
    this.record = options.record;
    this.prefs = options.prefs;
    this.onPrefs = options.onPrefs;
    this.onExit = options.onExit;
    this.forceFallback = options.forceFallback ?? false;

    this.stageEl = h("div", { class: "stage" });
    this.hint = h("div", { class: "cover-hint", role: "status" });
    this.live = h("div", { class: "sr-only", role: "status", "aria-live": "polite" });
    this.scrubber = h("input", { type: "range", class: "scrubber", min: "0", max: "1", step: "1", value: "0", "aria-label": "Position in the book" });
    this.locationLabel = h("div", { class: "location" });
    this.chromeTop = h("header", { class: "chrome chrome-top" });
    this.chromeBottom = h("footer", { class: "chrome chrome-bottom" }, this.scrubber, this.locationLabel);
    this.el = h("section", { class: "reader", "aria-label": this.book.ir.meta.title }, this.stageEl, this.chromeTop, this.chromeBottom, this.hint, this.live);
    this.el.lang = this.book.ir.meta.language;
    options.root.append(this.el);
  }

  // --- start-up ---------------------------------------------------------------

  async start(): Promise<void> {
    const book = this.book;
    const [fonts, hyphenate, reading] = await Promise.all([
      loadFonts(book.ir, (name) => book.bytes(name)),
      loadHyphenator(book.ir.meta.language).catch(() => undefined),
      this.record ? this.library.state(this.record.id) : Promise.resolve<ReadingState | null>(null),
    ]);
    if (this.disposed) return;
    this.fonts = fonts;
    this.hyphenate = hyphenate;
    this.measurer = createCanvasMeasurer();
    this.reading = reading ?? { at: "front", anchor: null, vars: {}, ui: { quiz: {}, choice: {} }, bookmarks: [], allowedHosts: [], updatedAt: 0 };
    this.vars = reconcileState(book.ir, this.reading.vars);
    this.ui = this.sanitiseUi(this.reading.ui);
    this.anchor = this.reading.anchor;
    this.palette = resolvePalette(book.ir, resolveDark(this.prefs));
    this.applyChromeTheme();

    book.onImageReady = () => {
      // Repaint whatever was painted with a placeholder.
      this.stage?.requestRender();
    };

    this.buildChrome();
    this.overlays = new Overlays(this.stageEl, this.el, this);
    this.panels = new Panels(this.el, this);

    const meta = book.ir.meta;
    this.stage = new Stage({
      container: this.stageEl,
      style: sceneStyle(book.ir, this.palette),
      aspect: { preferred: meta.page_aspect, fixed: meta.layout === "fixed" },
      mode: this.prefs.pageMode,
      reducedMotion: resolveReducedMotion(this.prefs),
      forceFallback: this.forceFallback,
      host: {
        paint: (key, ctx, w, h) => this.paint(key, ctx, w, h),
        onGeometry: (g) => {
          this.geometry = g;
          this.relayout();
        },
        onPosition: (pos, settled) => this.onPosition(pos, settled),
        onTap: (tap) => this.onTap(tap),
        onFrame: (now) => this.onFrame(now),
      },
    });
    this.started = true;
    this.stage.start();

    window.addEventListener("keydown", this.onKey);
    for (const q of ["(prefers-color-scheme: dark)", "(prefers-reduced-motion: reduce)"]) {
      const mq = matchMedia(q);
      mq.addEventListener("change", this.onSystemChange);
      this.mediaQueries.push(mq);
    }
    document.addEventListener("visibilitychange", this.onVisibility);

    // A returning reader is taken back to their page: the cover opens by itself.
    if (this.reading.at !== "front" && this.layout) {
      const target = this.reading.at === "back" ? this.stage.controller.count : this.posForAnchor(this.anchor);
      this.pendingOpen = window.setTimeout(() => {
        this.pendingOpen = null;
        if (!this.disposed && this.stage.controller.pos === 0) this.stage.goToPos(target, "flip");
      }, 650);
    }
    this.updateChrome();
  }

  private sanitiseUi(ui: UiRecords): UiRecords {
    const out: UiRecords = { quiz: {}, choice: {} };
    for (const [k, v] of Object.entries(ui.quiz ?? {})) {
      if (v && Number.isInteger(v.picked) && typeof v.correct === "boolean") out.quiz[k] = { picked: v.picked, correct: v.correct };
    }
    for (const [k, v] of Object.entries(ui.choice ?? {})) {
      if (Number.isInteger(v) && v >= 0) out.choice[k] = v;
    }
    return out;
  }

  // --- layout -----------------------------------------------------------------

  /** Lay the book out for the current page size, settings and state. */
  private relayout(replay = true): void {
    const g = this.geometry;
    if (!g || !this.fonts) return;
    const keep = this.layout ? this.currentAnchor() : this.anchor;
    const wasClosedBack = this.layout !== null && this.stage.controller.pos >= this.stage.controller.count;
    const wasOpen = this.layout !== null && this.stage.controller.pos > 0 && !wasClosedBack;
    const t0 = performance.now();
    this.layout = layoutBook({
      book: this.book.ir,
      page: { width: g.width, height: g.height },
      spread: g.spread,
      settings: this.prefs,
      palette: this.palette,
      fonts: this.fonts,
      measurer: this.measurer,
      state: this.vars,
      ui: this.ui,
      hyphenate: this.hyphenate,
    });
    this.layoutMs = performance.now() - t0;
    this.anchor = keep;
    this.animStart.clear();
    this.animating.clear();
    this.seen.clear();
    const page = wasOpen ? this.pageForAnchor(keep) : null;
    this.stage.setPages(this.layout.pages.length, page, wasClosedBack ? "back" : wasOpen ? null : "front");
    // What the reader is looking at stays as it is: only pages that come
    // into view later play their entrance.
    if (replay === false) {
      const { left, right } = this.stage.visiblePages();
      for (const index of [left, right]) if (index !== null) this.seen.add(index);
    }
    this.scrubber.max = String(Math.max(0, this.layout.pages.length - 1));
    this.overlays?.clear();
    this.updateChrome();
    if (this.settled) queueMicrotask(() => !this.disposed && this.onPosition(this.stage.controller.pos, true));
  }

  /** Page that contains an anchor in the current layout. */
  pageForAnchor(anchor: PageAnchor | null): number {
    const layout = this.layout;
    if (!layout || !anchor) return layout?.startPage ?? 0;
    let best = -1;
    let chapterFirst = -1;
    for (const p of layout.pages) {
      if (p.chapterId !== anchor.chapterId) continue;
      if (chapterFirst < 0) chapterFirst = p.index;
      if (anchor.block === null) return p.index;
      if (p.anchor.block === anchor.block && p.anchor.line <= anchor.line) best = p.index;
      else if (best < 0 && p.blocks.includes(anchor.block)) best = p.index;
    }
    if (best >= 0) return best;
    if (chapterFirst >= 0) return chapterFirst;
    return Math.min(layout.startPage, layout.pages.length - 1);
  }

  private posForAnchor(anchor: PageAnchor | null): number {
    return posForPage(this.pageForAnchor(anchor), this.stage.geometry.spread);
  }

  private currentAnchor(): PageAnchor | null {
    const page = this.stage.currentPage();
    if (page === null || !this.layout) return this.anchor;
    // In a spread, the right-hand page is where reading continues after a relayout.
    const { left, right } = this.stage.visiblePages();
    const p = this.layout.pages[left ?? right ?? page];
    return p ? { ...p.anchor } : this.anchor;
  }

  // --- painting ---------------------------------------------------------------

  private paint(key: SurfaceKey, ctx: CanvasRenderingContext2D, width: number, height: number): boolean {
    const size = { width, height };
    const env = { palette: this.palette, fonts: this.fonts, images: this.book };
    if (key.startsWith("page:")) {
      const index = Number(key.slice(5));
      const page = this.layout?.pages[index];
      if (!page) {
        ctx.fillStyle = this.palette.paper;
        ctx.fillRect(0, 0, width, height);
        return true;
      }
      // Entrance animations run on a clock that starts when the page comes
      // into view (see `startEntrances`). A page painted before that, ahead
      // of a turn, is drawn at the start of its animation; a page whose clock
      // has been dropped again was seen to the end and is drawn finished.
      let time: number | undefined;
      const start = this.animStart.get(index);
      if (page.animated && !this.stage.reducedMotion && (start !== undefined || !this.seen.has(index))) {
        time = start === undefined ? 0 : Math.max(0, performance.now() - start);
      }
      const result = paintPage(ctx, page, size, { ...env, time });
      if (result.animating && start !== undefined) this.animating.add(index);
      else this.animating.delete(index);
      return result.missing.every((r) => !this.book.pending(r));
    }
    if (key === "ghost") {
      // Stand-in for the leaves in the middle of a riffle: paper with the grey of text.
      paintSurface(ctx, "blank", this.book.ir, size, env);
      ctx.fillStyle = this.palette.ink;
      ctx.globalAlpha = 0.16;
      const m = width * 0.13;
      const lh = Math.max(6, height / 34);
      for (let y = m * 1.2, i = 0; y < height - m; y += lh, i++) {
        const w = (width - 2 * m) * (i % 7 === 6 ? 0.55 : 1);
        ctx.fillRect(m, y, w, lh * 0.36);
      }
      ctx.globalAlpha = 1;
      return true;
    }
    const surface = key === "front-cover" ? "front-cover" : key === "back-cover" ? "back-cover" : key.startsWith("endpaper") ? "endpaper" : "blank";
    const result = paintSurface(ctx, surface, this.book.ir, size, env);
    return result.missing.every((r) => !this.book.pending(r));
  }

  private onFrame(_now: number): boolean {
    // Keep repainting pages whose entrance animation is running.
    if (this.animating.size === 0) return false;
    const visible = new Set<number>();
    const { left, right } = this.stage.visiblePages();
    if (left !== null) visible.add(left);
    if (right !== null) visible.add(right);
    for (const index of [...this.animating]) {
      if (!visible.has(index)) {
        this.animating.delete(index);
        continue;
      }
      this.stage.invalidate((k) => k === `page:${index}`);
    }
    return this.animating.size > 0;
  }

  /**
   * Start the entrance animations of the pages that have just come into view.
   * The clock starts a little ahead, so the motion plays as the leaf lands
   * rather than while it is still in the air.
   */
  private startEntrances(): void {
    const layout = this.layout;
    if (!layout) return;
    const { left, right } = this.stage.visiblePages();
    if (!this.stage.animating) {
      for (const index of [...this.seen]) {
        if (index === left || index === right) continue;
        // Left behind and out of sight: the entrance replays on the next visit.
        // (While leaves are in the air the page may still be showing under them.)
        this.seen.delete(index);
        this.animStart.delete(index);
        if (layout.pages[index]?.animated) this.stage.invalidate((k) => k === `page:${index}`);
      }
    }
    if (this.stage.reducedMotion) return;
    for (const index of [left, right]) {
      if (index === null || this.seen.has(index)) continue;
      this.seen.add(index);
      if (!layout.pages[index]?.animated) continue;
      this.animStart.set(index, performance.now() + (this.stage.animating ? ENTRANCE_LEAD_MS : 0));
      this.animating.add(index);
      this.stage.invalidate((k) => k === `page:${index}`);
    }
  }

  // --- position ---------------------------------------------------------------

  private onPosition(pos: number, settled: boolean): void {
    this.settled = settled;
    const c = this.stage.controller;
    this.startEntrances();
    if (!settled) {
      this.overlays.clear();
    } else {
      const { left, right } = this.stage.visiblePages();
      this.overlays.show(this.stage.pageRects(), { left: left !== null ? this.layout!.pages[left]! : null, right: right !== null ? this.layout!.pages[right]! : null });
      this.anchor = this.currentAnchor();
      this.announce(pos);
      this.scheduleSave();
    }
    const at = pos <= 0 ? "front" : pos >= c.count ? "back" : "page";
    if (at === "page" && this.el.dataset.at !== "page") this.showChrome(); // starts the idle timer
    if (at !== "page") {
      clearTimeout(this.chromeTimer);
      this.chromeVisible = true;
      this.el.classList.remove("chrome-hidden");
    }
    this.el.dataset.at = at;
    this.el.dataset.settled = String(settled);
    this.updateChrome();
  }

  private announce(pos: number): void {
    const c = this.stage.controller;
    if (pos <= 0) this.live.textContent = `Front cover. ${this.book.ir.meta.title}.`;
    else if (pos >= c.count) this.live.textContent = "Back cover. End of the book.";
    else {
      const page = this.stage.currentPage();
      if (page !== null && this.layout) this.live.textContent = `Page ${page + 1} of ${this.layout.pages.length}`;
    }
  }

  private onTap(tap: { side: "left" | "right" | "outside"; x: number; y: number }): void {
    if (this.panels.isOpen) {
      this.panels.close();
      return;
    }
    const c = this.stage.controller;
    if (c.pos <= 0) {
      if (tap.side !== "outside") this.open();
      return;
    }
    if (c.pos >= c.count) {
      if (tap.side !== "outside") this.stage.prev();
      return;
    }
    const w = this.stage.geometry.width;
    const spread = this.stage.geometry.spread;
    const forward = tap.side === "right" && tap.x > w * (spread ? 0.55 : 0.66);
    const backward = spread ? tap.side === "left" && tap.x < w * 0.45 : tap.side === "right" && tap.x < w * 0.28;
    if (forward) this.next();
    else if (backward) this.prev();
    else this.toggleChrome();
  }

  /** Open the book from its cover: to the saved position if there is one, else to the first page. */
  open(): void {
    if (this.pendingOpen !== null) {
      clearTimeout(this.pendingOpen);
      this.pendingOpen = null;
    }
    const c = this.stage.controller;
    if (c.pos > 0) return;
    if (this.reading.at === "page" && this.anchor) this.stage.goToPos(this.posForAnchor(this.anchor), "flip");
    else this.stage.next();
  }

  next(): void {
    if (this.stage.controller.pos <= 0) this.open();
    else this.stage.next();
  }

  prev(): void {
    this.stage.prev();
  }

  goToPage(page: number, transition: Transition = "flip"): void {
    this.stage.goToPage(page, transition);
  }

  /** Jump to a named target: an anchor, or cover | start | contents | end. */
  goTo(target: string, transition: Transition = "flip"): boolean {
    const layout = this.layout;
    if (!layout) return false;
    if (target === "cover") this.stage.goToPos(0, transition);
    else if (target === "end") this.stage.goToPos(this.stage.controller.count, transition);
    else if (target === "start") this.stage.goToPage(layout.startPage, transition);
    else if (target === "contents") this.stage.goToPage(layout.contentsPage ?? layout.startPage, transition);
    else {
      const page = layout.anchors.get(target);
      if (page === undefined) return false; // a destination that is not part of the book in this state
      this.stage.goToPage(page, transition);
    }
    return true;
  }

  // --- interaction (OverlayHost) ----------------------------------------------

  /** Run the actions of a button, option or quiz and apply their effects. */
  runActions(actions: Action[], record?: (ui: UiRecords) => void): void {
    const before = JSON.stringify([this.vars, this.ui]);
    const { state, effects } = applyActions(this.book.ir, actions, this.vars);
    record?.(this.ui);
    if (effects.resetAll) this.ui = { quiz: {}, choice: {} };
    this.vars = state;
    if (JSON.stringify([this.vars, this.ui]) !== before) {
      this.relayout(false);
      this.stage.invalidate();
    }
    if (effects.goto) {
      if (!this.goTo(effects.goto.target, effects.goto.transition ?? "flip")) {
        this.toast("That part of the book is not available on this path.");
      }
    }
    this.scheduleSave();
  }

  activate(region: Region): void {
    switch (region.type) {
      case "button":
        this.runActions(region.actions);
        break;
      case "option":
        this.runActions(region.actions, (ui) => {
          ui.choice[region.block] = region.index;
        });
        break;
      case "answer": {
        if (this.ui.quiz[region.block]) return; // the first answer stands
        const block = this.findQuiz(region.block);
        if (!block) return;
        const correct = block.answers[region.index]?.correct === true;
        this.runActions(correct ? (block.on_correct ?? []) : [], (ui) => {
          ui.quiz[region.block] = { picked: region.index, correct };
        });
        this.live.textContent = correct ? "Correct." : "Not quite.";
        break;
      }
      case "toc":
        this.goTo(region.target);
        break;
      case "link":
        if (region.link.kind === "anchor") {
          if (!this.goTo(region.link.target)) this.toast("That part of the book is not available on this path.");
        } else {
          this.panels.confirmExternal(region.link.href);
        }
        break;
      case "image":
        this.overlays.lightbox([region.resource], 0, region.caption);
        break;
      case "gallery":
        this.overlays.lightbox(region.resources, 0, region.caption);
        break;
      default:
        break;
    }
  }

  private findQuiz(id: string) {
    for (const c of this.book.ir.chapters) {
      const stack = [...(c.blocks ?? [])];
      while (stack.length) {
        const b = stack.pop()!;
        if (b.id === id && b.type === "quiz") return b;
        if ("blocks" in b) stack.push(...b.blocks);
      }
    }
    return null;
  }

  isHostAllowed(host: string): boolean {
    return this.reading.allowedHosts.includes(host);
  }

  allowHost(host: string): void {
    if (!this.reading.allowedHosts.includes(host)) this.reading.allowedHosts.push(host);
    this.scheduleSave();
  }

  toast(message: string): void {
    const t = h("div", { class: "toast", role: "status", text: message });
    this.el.append(t);
    setTimeout(() => t.classList.add("leaving"), 2600);
    setTimeout(() => t.remove(), 3100);
  }

  // --- bookmarks ---------------------------------------------------------------

  get bookmarks(): Bookmark[] {
    return this.reading.bookmarks;
  }

  private bookmarkAt(page: PageLayout | undefined): number {
    if (!page || !this.layout) return -1;
    return this.reading.bookmarks.findIndex((b) => this.pageForAnchor(b.anchor) === page.index);
  }

  /** Bookmarked pages among those visible. */
  private visibleBookmark(): number {
    const { left, right } = this.stage.visiblePages();
    for (const i of [left, right]) {
      if (i === null) continue;
      const at = this.bookmarkAt(this.layout?.pages[i]);
      if (at >= 0) return at;
    }
    return -1;
  }

  toggleBookmark(): void {
    const existing = this.visibleBookmark();
    if (existing >= 0) {
      this.reading.bookmarks.splice(existing, 1);
      this.live.textContent = "Bookmark removed.";
    } else {
      const page = this.stage.currentPage();
      const p = page !== null ? this.layout?.pages[page] : undefined;
      if (!p) return;
      const chapter = this.layout!.chapters.find((c) => c.id === p.chapterId);
      const firstText = p.semantics.find((s) => s.role === "paragraph" || s.role === "heading");
      const snippet = firstText && "text" in firstText ? firstText.text.slice(0, 70) : "";
      this.reading.bookmarks.push({ anchor: { ...p.anchor }, label: [chapter?.title, snippet].filter(Boolean).join(" — "), createdAt: Date.now() });
      this.live.textContent = "Bookmark added.";
    }
    this.updateChrome();
    this.scheduleSave();
  }

  removeBookmark(index: number): void {
    this.reading.bookmarks.splice(index, 1);
    this.updateChrome();
    this.scheduleSave();
  }

  // --- preferences -------------------------------------------------------------

  setPrefs(patch: Partial<Prefs>): void {
    this.prefs = { ...this.prefs, ...patch };
    this.onPrefs(this.prefs);
    const palette = resolvePalette(this.book.ir, resolveDark(this.prefs));
    const themeChanged = palette.dark !== this.palette.dark;
    this.palette = palette;
    this.applyChromeTheme();
    this.stage.reducedMotion = resolveReducedMotion(this.prefs);
    this.stage.setStyle(sceneStyle(this.book.ir, this.palette));
    if ("pageMode" in patch) this.stage.setMode(this.prefs.pageMode);
    else if (themeChanged || "fontScale" in patch || "lineHeight" in patch || "margins" in patch || "justify" in patch || "hyphenate" in patch) this.relayout();
    this.stage.invalidate();
  }

  private applyChromeTheme(): void {
    // The shell keeps its own colours; controls that sit on a page take the page's.
    const p = this.palette;
    this.el.dataset.pages = p.dark ? "dark" : "paper";
    for (const [name, value] of [["paper", p.paper], ["ink", p.ink], ["accent", p.accent], ["muted", p.muted], ["rule", p.rule], ["tint", p.tint]] as const) {
      this.el.style.setProperty(`--page-${name}`, value);
    }
  }

  private onSystemChange = (): void => {
    this.setPrefs({});
    this.relayout();
  };

  // --- chrome ------------------------------------------------------------------

  private buildChrome(): void {
    const meta = this.book.ir.meta;
    this.bookmarkButton = iconButton("bookmark", "Bookmark this page", () => this.toggleBookmark(), "bookmark-toggle");
    this.chromeTop.append(
      iconButton("back", "Back to the library", () => this.exit()),
      h("div", { class: "chrome-title" }, h("span", { class: "chrome-book", text: meta.title }), h("span", { class: "chrome-author", text: meta.authors.join(", ") })),
      h(
        "div",
        { class: "chrome-actions" },
        iconButton("contents", "Contents", () => this.panels.toggle("contents")),
        iconButton("search", "Search", () => this.panels.toggle("search")),
        this.bookmarkButton,
        iconButton("settings", "Reading settings", () => this.panels.toggle("settings")),
      ),
    );
    this.scrubber.addEventListener("change", () => this.goToPage(Number(this.scrubber.value)));
    this.scrubber.addEventListener("input", () => {
      const p = this.layout?.pages[Number(this.scrubber.value)];
      if (p) this.locationLabel.textContent = this.describe(p.index);
      this.fillScrubber();
    });
    this.el.addEventListener("pointermove", (e) => {
      // Any movement of the mouse brings the controls back, as in a film player.
      if (e.pointerType === "mouse" && (!this.chromeVisible || e.clientY < 72 || e.clientY > window.innerHeight - 72)) this.showChrome();
    });
    this.hint.addEventListener("click", () => {
      const c = this.stage.controller;
      if (c.pos <= 0) this.open();
      else if (c.pos >= c.count) this.goTo("start");
    });
  }

  /** Paint the read part of the progress line (WebKit and Chrome have no native fill). */
  private fillScrubber(): void {
    const max = Number(this.scrubber.max) || 1;
    this.scrubber.style.setProperty("--p", `${(100 * Number(this.scrubber.value)) / max}%`);
  }

  private describe(page: number): string {
    const layout = this.layout;
    if (!layout) return "";
    const chapter = layout.chapters.find((c) => page >= c.firstPage && page < c.firstPage + c.pageCount);
    const title = chapter && chapter.kind === "chapter" ? ` · ${chapter.title}` : "";
    return `Page ${page + 1} of ${layout.pages.length}${title}`;
  }

  private updateChrome(): void {
    if (!this.started) return;
    const c = this.stage.controller;
    const page = this.stage.currentPage();
    const closedFront = c.pos <= 0;
    const closedBack = c.pos >= c.count;
    this.el.classList.toggle("closed", closedFront || closedBack);
    if (page !== null && this.layout) {
      this.scrubber.value = String(page);
      this.locationLabel.textContent = this.describe(page);
      this.fillScrubber();
    }
    const marked = this.visibleBookmark() >= 0;
    this.bookmarkButton.replaceChildren(icon("bookmark", marked));
    this.bookmarkButton.setAttribute("aria-pressed", String(marked));
    this.bookmarkButton.setAttribute("aria-label", marked ? "Remove bookmark" : "Bookmark this page");
    this.bookmarkButton.title = marked ? "Remove bookmark" : "Bookmark this page";
    this.hint.replaceChildren();
    if (this.settled && closedFront) {
      const resume = this.reading.at === "page" && this.anchor && this.layout;
      const label = resume ? `Continue reading · page ${this.pageForAnchor(this.anchor) + 1}` : "Open the book";
      this.hint.append(h("button", { type: "button", class: "hint-button", text: label }));
    } else if (this.settled && closedBack) {
      this.hint.append(h("button", { type: "button", class: "hint-button", text: "Read again from the beginning" }));
    }
  }

  private showChrome(): void {
    this.chromeVisible = true;
    this.el.classList.remove("chrome-hidden");
    clearTimeout(this.chromeTimer);
    this.chromeTimer = window.setTimeout(() => {
      // Stay visible while a control inside it has focus or a panel is open.
      if (this.panels.isOpen || this.chromeTop.contains(document.activeElement) || this.chromeBottom.contains(document.activeElement)) return;
      this.hideChrome();
    }, 3800);
  }

  private hideChrome(): void {
    this.chromeVisible = false;
    this.el.classList.add("chrome-hidden");
  }

  private toggleChrome(): void {
    if (this.chromeVisible) this.hideChrome();
    else this.showChrome();
  }

  private onKey = (e: KeyboardEvent): void => {
    if (e.defaultPrevented || e.altKey || e.metaKey) return;
    const target = e.target as HTMLElement | null;
    const typing = target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.tagName === "SELECT" || target.isContentEditable);
    if (this.overlays.lightboxOpen || this.panels.modalOpen) return;
    if (typing && !(target instanceof HTMLInputElement && target.type === "range" && e.key.startsWith("Page"))) return;
    const onControl = target && (target.tagName === "BUTTON" || target.tagName === "VIDEO" || target.tagName === "A");
    switch (e.key) {
      case "ArrowRight":
      case "PageDown":
        this.next();
        break;
      case "ArrowLeft":
      case "PageUp":
        this.prev();
        break;
      case " ":
      case "Enter":
        // Space and Enter belong to a focused control.
        if (onControl) return;
        if (e.key === "Enter" && this.stage.controller.pos > 0) return;
        if (e.shiftKey) this.prev();
        else this.next();
        break;
      case "Home":
        this.goTo(e.ctrlKey ? "cover" : "start");
        break;
      case "End":
        this.goTo("end");
        break;
      case "Escape":
        if (this.panels.isOpen) this.panels.close();
        else this.showChrome();
        return;
      case "/":
        this.panels.open("search");
        break;
      case "f":
        if (!e.ctrlKey) return;
        this.panels.open("search");
        break;
      case "t":
        this.panels.toggle("contents");
        break;
      case "b":
        this.toggleBookmark();
        break;
      case ",":
        this.panels.toggle("settings");
        break;
      default:
        return;
    }
    e.preventDefault();
  };

  // --- persistence -------------------------------------------------------------

  private scheduleSave(): void {
    if (!this.record) return;
    clearTimeout(this.saveTimer);
    this.saveTimer = window.setTimeout(() => void this.save(), 350);
  }

  private onVisibility = (): void => {
    if (document.visibilityState === "hidden") void this.save();
  };

  private async save(): Promise<void> {
    if (!this.record || !this.started || !this.layout) return;
    clearTimeout(this.saveTimer);
    const c = this.stage.controller;
    const at = c.pos <= 0 ? "front" : c.pos >= c.count ? "back" : "page";
    // Closing the book on its front cover does not forget the page.
    const keepPage = c.pos <= 0 && this.reading.at === "page" && this.anchor !== null;
    this.reading = { ...this.reading, at: keepPage ? "page" : at, anchor: this.anchor, vars: this.vars, ui: this.ui };
    const page = this.stage.currentPage();
    const progress = at === "back" ? 1 : page !== null ? page / Math.max(1, this.layout.pages.length - 1) : this.record.progress;
    try {
      await this.library.saveState(this.record.id, this.reading);
      await this.library.update(this.record.id, { progress, finished: at === "back" || this.record.finished, openedAt: Date.now() });
    } catch (e) {
      console.warn("reading position could not be saved", e);
    }
  }

  /** Render the front cover to a small PNG for the library shelf. */
  async coverThumbnail(): Promise<Blob | null> {
    const g = this.geometry;
    if (!g) return null;
    const width = 360;
    const height = Math.round((width * g.height) / g.width);
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");
    if (!ctx) return null;
    const cover = this.book.ir.cover.front.image;
    if (cover) await this.book.ensureImage(cover).catch(() => undefined);
    ctx.scale(width / g.width, height / g.height);
    paintSurface(ctx, "front-cover", this.book.ir, { width: g.width, height: g.height }, { palette: this.palette, fonts: this.fonts, images: this.book });
    return new Promise((resolve) => canvas.toBlob((b) => resolve(b), "image/png"));
  }

  async exit(): Promise<void> {
    await this.save();
    this.onExit();
  }

  diagnostics(): ReaderDiagnostics {
    const c = this.stage.controller;
    return {
      renderer: this.stage.webgl ? "webgl2" : "canvas2d",
      pages: this.layout?.pages.length ?? 0,
      page: this.stage.currentPage(),
      pos: c.pos,
      count: c.count,
      settled: this.settled && !this.stage.animating && this.animating.size === 0,
      spread: this.stage.geometry.spread,
      layoutMs: this.layoutMs,
      frameTimes: [...this.stage.frameTimes],
      state: { ...this.vars },
      textureBytes: this.stage.renderer instanceof GlRenderer ? Math.round(this.stage.renderer.textureBytes) : 0,
    };
  }

  /** The stage, for tests that need to synthesise precise poses. */
  get stageForTests(): Stage {
    return this.stage;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    clearTimeout(this.saveTimer);
    clearTimeout(this.chromeTimer);
    if (this.pendingOpen !== null) clearTimeout(this.pendingOpen);
    window.removeEventListener("keydown", this.onKey);
    document.removeEventListener("visibilitychange", this.onVisibility);
    for (const mq of this.mediaQueries) mq.removeEventListener("change", this.onSystemChange);
    this.overlays?.dispose();
    this.panels?.dispose();
    this.stage?.dispose();
    this.book.onImageReady = null;
    this.book.close();
    this.el.remove();
  }
}
