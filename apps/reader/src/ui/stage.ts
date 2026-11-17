/**
 * The stage: the canvas on which the book is drawn, the renderer behind it,
 * the flip controller that animates it, and the gestures that drive it.
 *
 * The stage knows nothing about DARM content. It asks its host to paint
 * *surfaces* (pages, covers, endpapers) into a scratch canvas and uploads
 * them as textures; the host decides what a surface key means.
 *
 * Rendering is on demand: a frame is produced only while something moves or
 * after `requestRender()`, so a book that is simply being read costs no CPU
 * or battery.
 */
import { FlipController, MOTION, type ScenePose, type Sheet, type SurfaceKey, buildSheets, pagesAt, posForPage } from "../flip/controller";
import { BOARD_OVERHANG, GlRenderer, type SceneStyle, type SceneView } from "../gl/renderer";
import { Canvas2dRenderer } from "../fallback/renderer2d";

/** Height kept free for the floating controls above and below the book, CSS px. */
const RESERVED_TOP = 56;
const RESERVED_BOTTOM = 50;

export interface SceneRenderer {
  resize(view: SceneView): void;
  hasSurface(key: SurfaceKey): boolean;
  setSurface(key: SurfaceKey, source: HTMLCanvasElement, width: number, height: number): void;
  deleteSurfaces(match: (key: SurfaceKey) => boolean): void;
  render(pose: ScenePose, style: SceneStyle): void;
  dispose(): void;
}

export interface PageGeometry {
  /** Two pages side by side, or one. */
  spread: boolean;
  /** Size of one page in CSS px. */
  width: number;
  height: number;
  /** Device pixels per CSS pixel actually used for page textures. */
  textureScale: number;
}

export interface StageHost {
  /**
   * Paint a surface. The context is scaled so that one unit is one CSS px;
   * the surface is `width` × `height` CSS px. Return `true` if the result
   * is final, `false` if it should be repainted later (images still loading).
   */
  paint(key: SurfaceKey, ctx: CanvasRenderingContext2D, width: number, height: number): boolean;
  /** The page size changed: the host must re-layout and call `setPages`. */
  onGeometry(geometry: PageGeometry): void;
  /** `pos` changed (immediately when a turn starts) or the book came to rest. */
  onPosition?(pos: number, settled: boolean): void;
  /** A tap that was not a drag. `page` is the page side hit, coordinates are CSS px within that page. */
  onTap?(tap: { side: "left" | "right" | "outside"; x: number; y: number; clientX: number; clientY: number }): void;
  /** Called every rendered frame; return true to keep the loop running (content animations). */
  onFrame?(now: number): boolean;
  /** Per-frame timing sample, for the performance overlay and tests. */
  onFrameTime?(ms: number, animating: boolean): void;
}

export interface StageOptions {
  container: HTMLElement;
  host: StageHost;
  style: SceneStyle;
  /** Page width / height. Fixed-layout books pass their exact proportion. */
  aspect: { preferred: number; fixed: boolean };
  /** `auto` picks a spread when the container is wide enough. */
  mode: "auto" | "single" | "spread";
  reducedMotion: boolean;
  /** Force the Canvas 2D renderer (tests, diagnostics). */
  forceFallback?: boolean;
}

/** Rectangle of a page at rest, in CSS px relative to the container. */
export interface PageRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export class Stage {
  canvas: HTMLCanvasElement;
  readonly renderer: SceneRenderer;
  readonly webgl: boolean;
  readonly controller: FlipController;
  private host: StageHost;
  private container: HTMLElement;
  private style: SceneStyle;
  private aspect: { preferred: number; fixed: boolean };
  private mode: "auto" | "single" | "spread";
  reducedMotion: boolean;
  private view: SceneView = { width: 1, height: 1, pagePx: 2, aspect: 1.5, centerX: 0, centerY: 0, spread: true };
  geometry: PageGeometry = { spread: true, width: 1, height: 1, textureScale: 1 };
  private dpr = 1;
  private pageCount = 0;
  private scratch = document.createElement("canvas");
  private scratchCtx: CanvasRenderingContext2D;
  /** Surfaces painted before their images were ready. */
  private provisional = new Set<SurfaceKey>();
  private raf = 0;
  private lastTime = 0;
  private wasAnimating = false;
  private lastPos = -1;
  private resizeObserver: ResizeObserver;
  private pointer: { id: number; startX: number; startY: number; lastX: number; lastY: number; dragging: boolean; worldX: number; v: number; grabX: number; gain: number } | null = null;
  private wheelLock = 0;
  private disposed = false;
  private started = false;
  /** Frame times of the most recent animation, for diagnostics. */
  readonly frameTimes: number[] = [];

  constructor(options: StageOptions) {
    this.container = options.container;
    this.host = options.host;
    this.style = options.style;
    this.aspect = options.aspect;
    this.mode = options.mode;
    this.reducedMotion = options.reducedMotion;
    this.canvas = document.createElement("canvas");
    this.canvas.className = "stage-canvas";
    this.canvas.setAttribute("aria-hidden", "true");
    // First in the container, so overlays added by the host come after it in tab order.
    this.container.prepend(this.canvas);
    const gl = options.forceFallback ? null : GlRenderer.create(this.canvas);
    if (gl) {
      gl.onContextRestored = () => {
        this.provisional.clear();
        this.requestRender();
      };
      this.renderer = gl;
      this.webgl = true;
    } else {
      // A canvas that was asked for a WebGL context can no longer give a 2D one.
      const fresh = document.createElement("canvas");
      fresh.className = this.canvas.className;
      fresh.setAttribute("aria-hidden", "true");
      this.canvas.replaceWith(fresh);
      this.canvas = fresh;
      this.renderer = new Canvas2dRenderer(this.canvas);
      this.webgl = false;
    }
    this.scratchCtx = this.scratch.getContext("2d", { alpha: false })!;
    this.controller = new FlipController({ sheets: buildSheets(0, true), spread: true, ghost: "ghost" });
    this.resizeObserver = new ResizeObserver(() => {
      if (this.started) this.measure();
    });
    this.resizeObserver.observe(this.container);
    this.bindInput();
  }

  /** Measure the container and report the first geometry. Call once the host holds this stage. */
  start(): void {
    this.started = true;
    this.measure(true);
  }

  // --- geometry ---------------------------------------------------------------

  /** Recompute the page size from the container. Notifies the host when it changes. */
  measure(force = false): void {
    const cw = this.container.clientWidth;
    const ch = this.container.clientHeight;
    if (cw < 2 || ch < 2) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 3);
    const phone = Math.min(cw, ch) < 520;
    // Two pages whenever the window is clearly wider than tall (desktop, tablets and phones in landscape).
    const spread = this.mode === "spread" || (this.mode === "auto" && cw >= 600 && cw / ch >= 1.2);
    const n = spread ? 2 : 1;
    const pad = phone ? 6 : Math.max(14, Math.min(44, Math.min(cw, ch) * 0.045));
    // The controls float over the top and bottom of the stage: keep the book clear of them.
    const top = phone ? 0 : RESERVED_TOP;
    const bottom = phone ? 0 : RESERVED_BOTTOM;
    // Leave room for the board overhang and the fanned edges of the page stack.
    const extra = 1 + 2 * BOARD_OVERHANG + 0.012;
    const availW = (cw - 2 * pad) / (n * extra);
    const availH = (ch - top - bottom - 2 * pad) / (1 + 2 * BOARD_OVERHANG * (this.aspect.preferred || 0.7));
    let a = this.aspect.preferred;
    if (!this.aspect.fixed) {
      // Reflowable books take the proportion that best fills the window, within bookish bounds.
      const fit = availW / availH;
      const [lo, hi] = spread ? [0.6, 0.8] : phone ? [0.46, 0.86] : [0.6, 0.82];
      a = Math.min(hi, Math.max(lo, fit));
    }
    let h = Math.min(availH, availW / a);
    let w = h * a;
    // Even device-pixel sizes keep resting pages exactly on the pixel grid.
    const even = (v: number) => Math.max(2, 2 * Math.round(v / 2));
    const pagePx = even(w * dpr);
    const pageHpx = even(h * dpr);
    w = pagePx / dpr;
    h = pageHpx / dpr;
    const width = Math.round(cw * dpr);
    const height = Math.round(ch * dpr);
    const view: SceneView = {
      width,
      height,
      pagePx,
      aspect: pageHpx / pagePx,
      centerX: 2 * Math.round(width / 4),
      centerY: 2 * Math.round(((top + (ch - top - bottom) / 2) * dpr) / 2),
      spread,
    };
    // Texture resolution: full device resolution, bounded by GPU limits and a memory budget.
    const maxSide = this.renderer instanceof GlRenderer ? Math.min(this.renderer.maxTextureSize, 3072) : 3072;
    const textureScale = Math.min(dpr, maxSide / Math.max(w, h), Math.sqrt(5_000_000 / (w * h)));
    const changed =
      force || spread !== this.geometry.spread || Math.abs(w - this.geometry.width) > 0.01 || Math.abs(h - this.geometry.height) > 0.01 || dpr !== this.dpr;
    this.view = view;
    this.dpr = dpr;
    this.canvas.style.width = `${cw}px`;
    this.canvas.style.height = `${ch}px`;
    this.renderer.resize(view);
    if (changed) {
      this.geometry = { spread, width: w, height: h, textureScale };
      this.invalidate();
      // The host lays the book out again and calls setPages() with the page
      // that now holds the reader's place (page numbers change with page size).
      this.host.onGeometry(this.geometry);
    }
    this.requestRender();
  }

  setMode(mode: "auto" | "single" | "spread"): void {
    this.mode = mode;
    this.measure(true);
  }

  setAspect(aspect: { preferred: number; fixed: boolean }): void {
    this.aspect = aspect;
    this.measure(true);
  }

  setStyle(style: SceneStyle): void {
    this.style = style;
    this.invalidate();
  }

  /** First page currently visible, or null when the book is closed. */
  currentPage(): number | null {
    const pos = this.controller.pos;
    if (pos <= 0 || pos >= this.controller.count) return null;
    const { left, right } = pagesAt(pos, this.controller.spread);
    const first = left !== null && left >= 0 ? left : right;
    return first !== null && first < this.pageCount ? first : this.pageCount - 1;
  }

  /** Pages visible at rest: indices or null. */
  visiblePages(): { left: number | null; right: number | null } {
    const pos = this.controller.pos;
    if (pos <= 0 || pos >= this.controller.count) return { left: null, right: null };
    const { left, right } = pagesAt(pos, this.controller.spread);
    const ok = (i: number | null) => (i !== null && i >= 0 && i < this.pageCount ? i : null);
    return { left: ok(left), right: ok(right) };
  }

  /** Where the resting pages are on screen, in CSS px relative to the container. */
  pageRects(): { left: PageRect | null; right: PageRect | null } {
    const v = this.view;
    const pose = this.controller.pose();
    const scale = (v.pagePx * pose.zoom) / this.dpr;
    const cx = v.centerX / this.dpr;
    const cy = v.centerY / this.dpr;
    const h = scale * v.aspect;
    const rect = (x0: number): PageRect => ({ x: cx + (x0 - pose.camX) * scale, y: cy - h / 2, width: scale, height: h });
    return { left: this.geometry.spread ? rect(-1) : null, right: rect(0) };
  }

  /** Tell the stage how many pages the laid-out book has, keeping the reading position. */
  setPages(count: number, page: number | null, closed: "front" | "back" | null = null): void {
    this.pageCount = count;
    const spread = this.geometry.spread;
    const sheets: Sheet[] = buildSheets(count, spread);
    let pos = 0;
    if (closed === "back") pos = sheets.length;
    else if (closed === null && page !== null) pos = posForPage(Math.max(0, Math.min(count - 1, page)), spread);
    this.controller.reset({ sheets, spread, ghost: "ghost" }, pos);
    this.lastPos = -1;
    this.invalidate();
  }

  // --- surfaces ---------------------------------------------------------------

  /** Forget painted surfaces (all, or those matching) so they are painted again. */
  invalidate(match?: (key: SurfaceKey) => boolean): void {
    this.renderer.deleteSurfaces(match ?? (() => true));
    if (match) {
      for (const k of [...this.provisional]) if (match(k)) this.provisional.delete(k);
    } else {
      this.provisional.clear();
    }
    this.requestRender();
  }

  private ensure(key: SurfaceKey | null): void {
    if (key === null) return;
    if (this.renderer.hasSurface(key) && !this.provisional.has(key)) return;
    const g = this.geometry;
    const scale = g.textureScale;
    const w = Math.max(2, Math.round(g.width * scale));
    const h = Math.max(2, Math.round(g.height * scale));
    if (this.scratch.width !== w || this.scratch.height !== h) {
      this.scratch.width = w;
      this.scratch.height = h;
    }
    const ctx = this.scratchCtx;
    ctx.setTransform(w / g.width, 0, 0, h / g.height, 0, 0);
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = "source-over";
    let final = true;
    try {
      final = this.host.paint(key, ctx, g.width, g.height);
    } catch (e) {
      console.error(`painting ${key} failed`, e);
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.fillStyle = "#888";
      ctx.fillRect(0, 0, w, h);
    }
    this.renderer.setSurface(key, this.scratch, w, h);
    if (final) this.provisional.delete(key);
    else this.provisional.add(key);
  }

  /** Paint the surfaces a turn in either direction would need, while idle. */
  private prewarm(): void {
    const c = this.controller;
    if (c.animating) return;
    const wanted: SurfaceKey[] = [];
    for (const i of [c.pos, c.pos - 1, c.pos + 1, c.pos - 2]) {
      const s = c.sheets[i];
      if (s) wanted.push(s.front, s.back);
    }
    wanted.push("ghost");
    const next = wanted.find((k) => !this.renderer.hasSurface(k));
    if (next === undefined) return;
    const run = () => {
      if (this.disposed || this.controller.animating) return;
      this.ensure(next);
      this.prewarm();
    };
    if ("requestIdleCallback" in window) window.requestIdleCallback(run, { timeout: 400 });
    else setTimeout(run, 60);
  }

  // --- navigation -------------------------------------------------------------

  private instant(change: () => void): void {
    // Reduced motion: no flight, a brief dip in opacity marks the change.
    change();
    this.canvas.animate([{ opacity: 0.55 }, { opacity: 1 }], { duration: 140, easing: "ease-out" });
    this.requestRender();
  }

  next(): boolean {
    const c = this.controller;
    if (c.pos >= c.count) return false;
    if (this.reducedMotion) this.instant(() => c.setPosition(c.pos + 1));
    else c.next();
    this.requestRender();
    return true;
  }

  prev(): boolean {
    const c = this.controller;
    if (c.pos <= 0) return false;
    if (this.reducedMotion) this.instant(() => c.setPosition(c.pos - 1));
    else c.prev();
    this.requestRender();
    return true;
  }

  /** Go to a stack position. `flip` riffles, `fade` cross-fades, `none` cuts. */
  goToPos(pos: number, transition: "flip" | "fade" | "none" = "flip"): void {
    const c = this.controller;
    pos = Math.max(0, Math.min(c.count, pos));
    if (pos === c.pos && !c.animating) return;
    if (transition === "flip" && !this.reducedMotion) {
      c.jumpTo(pos);
    } else if (transition === "none") {
      c.setPosition(pos);
    } else {
      c.setPosition(pos);
      this.canvas.animate([{ opacity: 0 }, { opacity: 1 }], { duration: this.reducedMotion ? 140 : 420, easing: "ease-out" });
    }
    this.requestRender();
  }

  goToPage(page: number, transition: "flip" | "fade" | "none" = "flip"): void {
    this.goToPos(posForPage(Math.max(0, Math.min(this.pageCount - 1, page)), this.geometry.spread), transition);
  }

  // --- input ------------------------------------------------------------------

  private toWorld(clientX: number, clientY: number): { x: number; v: number; inside: boolean } {
    const r = this.canvas.getBoundingClientRect();
    const pose = this.controller.pose();
    const v = this.view;
    const scale = v.pagePx * pose.zoom;
    const x = pose.camX + ((clientX - r.left) * this.dpr - v.centerX) / scale;
    const y = -((clientY - r.top) * this.dpr - v.centerY) / scale;
    const vv = y / v.aspect + 0.5;
    return { x, v: vv, inside: vv >= -0.04 && vv <= 1.04 && Math.abs(x) <= 1.06 };
  }

  private camera(): { x: number; distance: number } {
    return { x: this.controller.pose().camX, distance: MOTION.cameraDistance };
  }

  private bindInput(): void {
    const el = this.canvas;
    el.style.touchAction = "pan-y pinch-zoom";
    el.addEventListener("pointerdown", (e) => {
      if (e.button !== 0 || this.pointer) return;
      const w = this.toWorld(e.clientX, e.clientY);
      this.pointer = { id: e.pointerId, startX: e.clientX, startY: e.clientY, lastX: e.clientX, lastY: e.clientY, dragging: false, worldX: w.x, v: w.v, grabX: w.x, gain: 1 };
    });
    el.addEventListener("pointermove", (e) => {
      const p = this.pointer;
      if (!p || p.id !== e.pointerId) return;
      p.lastX = e.clientX;
      p.lastY = e.clientY;
      const dx = e.clientX - p.startX;
      const dy = e.clientY - p.startY;
      if (!p.dragging) {
        // A drag starts once the pointer has clearly moved sideways.
        if (Math.abs(dx) < 7 || Math.abs(dx) < Math.abs(dy) * 0.8) return;
        if (this.reducedMotion) return;
        const start = this.toWorld(p.startX, p.startY);
        // With one page on screen the previous leaf lies out of view on the
        // left, so a rightward swipe anywhere on the page pulls it back in:
        // the leaf is driven by how far the finger has travelled rather than
        // by where the finger is.
        const pullBack = !this.geometry.spread && dx > 0;
        p.grabX = pullBack ? -0.9 : p.worldX;
        p.gain = pullBack ? 2.3 : 1;
        if (!start.inside || !this.controller.grab(p.grabX, Math.min(1, Math.max(0, p.v)), this.camera())) {
          this.pointer = null;
          return;
        }
        p.dragging = true;
        try {
          el.setPointerCapture(e.pointerId);
        } catch {
          /* the pointer may already be gone */
        }
      }
      const w = this.toWorld(e.clientX, e.clientY);
      this.controller.dragTo(p.grabX + (w.x - p.worldX) * p.gain, w.v - p.v, this.camera());
      this.requestRender();
      e.preventDefault();
    });
    const finish = (e: PointerEvent, cancelled: boolean) => {
      const p = this.pointer;
      if (!p || p.id !== e.pointerId) return;
      this.pointer = null;
      if (p.dragging) {
        if (cancelled) this.controller.cancel();
        else this.controller.release();
        this.requestRender();
        return;
      }
      if (cancelled) return;
      const dx = e.clientX - p.startX;
      if (this.reducedMotion && Math.abs(dx) > 40) {
        // With reduced motion a swipe still turns the page, just without the flight.
        if (dx < 0) this.next();
        else this.prev();
        return;
      }
      if (Math.hypot(dx, e.clientY - p.startY) > 8) return;
      const rects = this.pageRects();
      const cr = this.container.getBoundingClientRect();
      const px = e.clientX - cr.left;
      const py = e.clientY - cr.top;
      const within = (r: PageRect | null) => r !== null && px >= r.x && px <= r.x + r.width && py >= r.y && py <= r.y + r.height;
      const side = within(rects.right) ? "right" : within(rects.left) ? "left" : "outside";
      const r = side === "left" ? rects.left! : rects.right!;
      this.host.onTap?.({ side, x: px - r.x, y: py - r.y, clientX: e.clientX, clientY: e.clientY });
    };
    el.addEventListener("pointerup", (e) => finish(e, false));
    el.addEventListener("pointercancel", (e) => finish(e, true));
    el.addEventListener(
      "wheel",
      (e) => {
        // Two-finger horizontal swipes on a trackpad turn pages.
        if (Math.abs(e.deltaX) <= Math.abs(e.deltaY) * 1.2 || Math.abs(e.deltaX) < 24) return;
        e.preventDefault();
        const now = performance.now();
        if (now < this.wheelLock) return;
        this.wheelLock = now + 420;
        if (e.deltaX > 0) this.next();
        else this.prev();
      },
      { passive: false },
    );
  }

  // --- frame loop -------------------------------------------------------------

  requestRender(): void {
    if (this.raf || this.disposed) return;
    this.raf = requestAnimationFrame((t) => this.frame(t));
  }

  /**
   * Advance the simulation by `seconds` in fixed steps and draw the result,
   * independently of the display's frame clock (tests, screenshots, and
   * environments that throttle animation frames).
   */
  advance(seconds: number): void {
    const step = 1 / 120;
    for (let t = 0; t < seconds; t += step) this.controller.tick(step);
    this.lastTime = 0;
    this.wasAnimating = false;
    this.draw(performance.now());
    const moving = this.controller.animating;
    this.lastPos = this.controller.pos;
    this.host.onPosition?.(this.controller.pos, !moving);
    if (moving) this.requestRender();
  }

  /** Render one frame synchronously (tests, screenshots). */
  renderNow(): void {
    this.draw(performance.now());
  }

  private draw(now: number): boolean {
    const pose = this.controller.pose();
    this.ensure(pose.left);
    this.ensure(pose.right);
    for (const f of pose.flyers) {
      this.ensure(f.front);
      if (!f.mirrorBack) this.ensure(f.back);
    }
    this.renderer.render(pose, this.style);
    return this.host.onFrame?.(now) ?? false;
  }

  private frame(now: number): void {
    this.raf = 0;
    if (this.disposed) return;
    const dt = this.lastTime ? Math.min(0.1, (now - this.lastTime) / 1000) : 1 / 60;
    const moving = this.controller.tick(this.wasAnimating ? dt : 1 / 120);
    const t0 = performance.now();
    const contentAnimating = this.draw(now);
    const cost = performance.now() - t0;
    if (moving && this.wasAnimating) {
      this.frameTimes.push(dt * 1000);
      if (this.frameTimes.length > 600) this.frameTimes.shift();
    }
    this.host.onFrameTime?.(cost, moving);
    const pos = this.controller.pos;
    if (pos !== this.lastPos || (this.wasAnimating && !moving)) {
      this.lastPos = pos;
      try {
        this.host.onPosition?.(pos, !moving);
      } catch (e) {
        // A failure in the host must not stop the frame loop with a half-turned page on screen.
        console.error("position update failed", e);
      }
    }
    this.wasAnimating = moving;
    this.lastTime = moving || contentAnimating ? now : 0;
    if (moving || contentAnimating || this.provisional.size > 0) {
      if (moving || contentAnimating) this.requestRender();
    } else {
      this.prewarm();
    }
  }

  /** True while pages are turning. */
  get animating(): boolean {
    return this.controller.animating;
  }

  dispose(): void {
    this.disposed = true;
    cancelAnimationFrame(this.raf);
    this.resizeObserver.disconnect();
    this.renderer.dispose();
    this.canvas.remove();
  }
}
