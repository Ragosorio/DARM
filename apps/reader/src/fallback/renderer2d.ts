/**
 * Canvas 2D fallback for devices without WebGL2.
 *
 * It draws the same scene from the same leaf model, with an orthographic
 * view: a leaf is cut into vertical slices, each slice of the page image is
 * drawn at the projected position of its piece of the curve (so the page
 * visibly bends and shows its reverse), and shaded from the slice's normal.
 * There is no perspective, corner skew or per-pixel shadow map; shadows are
 * soft rectangles. The quality is deliberately "reasonable", not equal.
 */
import type { FlyerPose, ScenePose, SurfaceKey } from "../flip/controller";
import { crossSection, ease } from "../flip/geometry";
import { BOARD_OVERHANG, type SceneStyle, type SceneView } from "../gl/renderer";
import type { SceneRenderer } from "../ui/stage";

const SLICES = 44;
const LIGHT: [number, number, number] = [-0.396, 0.479, 0.834];

const css = (c: [number, number, number], k = 1) =>
  `rgb(${Math.round(Math.min(1, c[0] * k) * 255)}, ${Math.round(Math.min(1, c[1] * k) * 255)}, ${Math.round(Math.min(1, c[2] * k) * 255)})`;

export class Canvas2dRenderer implements SceneRenderer {
  private ctx: CanvasRenderingContext2D;
  private surfaces = new Map<SurfaceKey, { canvas: HTMLCanvasElement; used: number }>();
  private view: SceneView = { width: 1, height: 1, pagePx: 2, aspect: 1.5, centerX: 0, centerY: 0, spread: true };
  private frame = 0;
  private xs = new Float32Array(SLICES + 1);
  private zs = new Float32Array(SLICES + 1);
  private ts = new Float32Array(SLICES + 1);
  /** Most page images kept; each is a full canvas. */
  maxSurfaces = 12;

  constructor(readonly canvas: HTMLCanvasElement) {
    const ctx = canvas.getContext("2d", { alpha: false });
    if (!ctx) throw new Error("this browser cannot draw: no 2D canvas");
    this.ctx = ctx;
  }

  resize(view: SceneView): void {
    this.view = view;
    if (this.canvas.width !== view.width) this.canvas.width = view.width;
    if (this.canvas.height !== view.height) this.canvas.height = view.height;
  }

  hasSurface(key: SurfaceKey): boolean {
    return this.surfaces.has(key);
  }

  setSurface(key: SurfaceKey, source: HTMLCanvasElement, width: number, height: number): void {
    let s = this.surfaces.get(key);
    if (!s) {
      s = { canvas: document.createElement("canvas"), used: this.frame };
      this.surfaces.set(key, s);
    }
    s.canvas.width = width;
    s.canvas.height = height;
    s.canvas.getContext("2d", { alpha: false })!.drawImage(source, 0, 0);
    s.used = this.frame;
    if (this.surfaces.size > this.maxSurfaces) {
      const oldest = [...this.surfaces.entries()].sort((a, b) => a[1].used - b[1].used)[0];
      if (oldest && oldest[1].used < this.frame - 1) this.surfaces.delete(oldest[0]);
    }
  }

  deleteSurfaces(match: (key: SurfaceKey) => boolean): void {
    for (const key of [...this.surfaces.keys()]) if (match(key)) this.surfaces.delete(key);
  }

  private image(key: SurfaceKey | null): HTMLCanvasElement | null {
    if (!key) return null;
    const s = this.surfaces.get(key);
    if (!s) return null;
    s.used = this.frame;
    return s.canvas;
  }

  render(pose: ScenePose, style: SceneStyle): void {
    const ctx = this.ctx;
    const v = this.view;
    this.frame++;
    const scale = v.pagePx * pose.zoom;
    const X = (x: number) => v.centerX + (x - pose.camX) * scale;
    const Y = (y: number) => v.centerY - y * scale;
    const h2 = v.aspect / 2;
    const ov = BOARD_OVERHANG;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalAlpha = 1;

    // Desk.
    const g = ctx.createRadialGradient(v.width * 0.4, v.height * 0.4, 0, v.width * 0.4, v.height * 0.4, Math.max(v.width, v.height) * 0.8);
    g.addColorStop(0, css(style.desk));
    g.addColorStop(1, css(style.deskEdge));
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, v.width, v.height);

    const leftPresent = pose.left !== null || pose.leftBase;
    const rightPresent = pose.right !== null || pose.rightBase;
    const x0 = leftPresent ? -1 - ov : 0;
    const x1 = rightPresent ? 1 + ov : 0;

    // Shadow under the book.
    if (x1 > x0) {
      ctx.save();
      ctx.shadowColor = `rgba(0,0,0,${style.dark ? 0.7 : 0.38})`;
      ctx.shadowBlur = scale * 0.05;
      ctx.shadowOffsetX = scale * 0.012;
      ctx.shadowOffsetY = scale * 0.018;
      ctx.fillStyle = css(style.cloth, 0.8);
      ctx.fillRect(X(x0), Y(h2 + ov), (x1 - x0) * scale, (v.aspect + 2 * ov) * scale);
      ctx.restore();
    }
    // Opened boards.
    ctx.fillStyle = css(style.cloth, 0.86);
    if (pose.leftBase) ctx.fillRect(X(-1 - ov), Y(h2 + ov), (1 + ov) * scale, (v.aspect + 2 * ov) * scale);
    if (pose.rightBase) ctx.fillRect(X(0), Y(h2 + ov), (1 + ov) * scale, (v.aspect + 2 * ov) * scale);
    // Page stack edges.
    const edge = style.dark ? css(style.paper, 1.6) : css(style.paper, 0.88);
    ctx.fillStyle = edge;
    if (pose.rightBase && pose.right && !pose.rightIsBoard) ctx.fillRect(X(1), Y(h2 - 0.002), (0.004 + 0.016 * pose.stackRight) * scale, (v.aspect - 0.004) * scale);
    if (pose.leftBase && pose.left && !pose.leftIsBoard) {
      const w = (0.004 + 0.016 * pose.stackLeft) * scale;
      ctx.fillRect(X(-1) - w, Y(h2 - 0.002), w, (v.aspect - 0.004) * scale);
    }

    // Resting surfaces.
    const resting = (key: SurfaceKey | null, side: -1 | 1, board: boolean) => {
      if (key === null) return;
      const img = this.image(key);
      const o = board ? ov : 0;
      const left = side > 0 ? X(0) : X(-1 - o);
      const top = Y(h2 + o);
      const w = (1 + o) * scale;
      const h = (v.aspect + 2 * o) * scale;
      if (img) ctx.drawImage(img, left, top, w, h);
      else {
        ctx.fillStyle = css(style.paper);
        ctx.fillRect(left, top, w, h);
      }
      if (!board && pose.openness > 0) {
        // Gutter shading.
        const gx = side > 0 ? X(0) : X(0) - scale * 0.16;
        const gg = ctx.createLinearGradient(side > 0 ? gx : gx + scale * 0.16, 0, side > 0 ? gx + scale * 0.16 : gx, 0);
        gg.addColorStop(0, `rgba(0,0,0,${0.3 * pose.openness})`);
        gg.addColorStop(0.25, `rgba(0,0,0,${0.08 * pose.openness})`);
        gg.addColorStop(1, "rgba(0,0,0,0)");
        ctx.fillStyle = gg;
        ctx.fillRect(gx, top, scale * 0.16, h);
      }
    };
    resting(pose.left, -1, pose.leftIsBoard);
    resting(pose.right, 1, pose.rightIsBoard);

    for (const f of pose.flyers) this.flyer(f, style, X, Y, scale);
  }

  private flyer(f: FlyerPose, style: SceneStyle, X: (x: number) => number, Y: (y: number) => number, scale: number): void {
    const ctx = this.ctx;
    const v = this.view;
    const h2 = v.aspect / 2;
    const o = f.board ? BOARD_OVERHANG : 0;
    const width = 1 + o;
    crossSection(f.board ? ease(f.p) : f.p, f.board ? 0 : f.lambda, f.board ? 1 : f.dir, SLICES, this.xs, this.zs, this.ts);
    if (f.board) {
      // Rigid: θ is constant, but crossSection eases p again; use a straight line.
      const th = Math.PI * ease(f.p);
      for (let i = 0; i <= SLICES; i++) {
        this.xs[i] = (i / SLICES) * Math.cos(th);
        this.zs[i] = (i / SLICES) * Math.sin(th);
        this.ts[i] = th;
      }
    }
    const top = Y(h2 + o);
    const height = (v.aspect + 2 * o) * scale;
    const front = this.image(f.front);
    const back = this.image(f.mirrorBack ? f.front : f.back);

    // Cast shadow: a soft band beside the lifted part of the sheet.
    let minX = Infinity;
    let maxX = -Infinity;
    let maxZ = 0;
    for (let i = 0; i <= SLICES; i++) {
      minX = Math.min(minX, this.xs[i]! * width);
      maxX = Math.max(maxX, this.xs[i]! * width);
      maxZ = Math.max(maxZ, this.zs[i]!);
    }
    if (maxZ > 0.01) {
      ctx.save();
      ctx.beginPath();
      ctx.rect(X(-1 - BOARD_OVERHANG), top, 2 * (1 + BOARD_OVERHANG) * scale, height);
      ctx.clip();
      ctx.shadowColor = `rgba(0,0,0,${0.28 * Math.min(1, maxZ * 3)})`;
      ctx.shadowBlur = scale * (0.02 + 0.05 * maxZ);
      ctx.shadowOffsetX = scale * 0.25 * maxZ + 20000;
      ctx.fillStyle = "#000";
      ctx.fillRect(X(minX) - 20000, top, (maxX - minX) * scale, height);
      ctx.restore();
    }

    // Slices, lowest first so nearer parts of the sheet cover farther ones.
    const order = Array.from({ length: SLICES }, (_, i) => i).sort((a, b) => this.zs[a]! + this.zs[a + 1]! - (this.zs[b]! + this.zs[b + 1]!));
    for (const i of order) {
      const xa = X(this.xs[i]! * width);
      const xb = X(this.xs[i + 1]! * width);
      const th = 0.5 * (this.ts[i]! + this.ts[i + 1]!);
      const facingUp = Math.cos(th) >= 0;
      const left = Math.min(xa, xb);
      const w = Math.abs(xb - xa);
      if (w < 0.05) continue;
      const img = facingUp ? front : back;
      const u0 = i / SLICES;
      const du = 1 / SLICES;
      if (img) {
        // The reverse of a sheet is the next page: its spine edge is on the other side.
        const su = facingUp || f.mirrorBack ? u0 : 1 - u0 - du;
        if (!facingUp && f.mirrorBack) {
          ctx.fillStyle = css(style.paper);
          ctx.fillRect(left, top, w + 0.6, height);
          ctx.globalAlpha = 0.085;
        }
        ctx.drawImage(img, su * img.width, 0, du * img.width, img.height, left, top, w + 0.6, height);
        ctx.globalAlpha = 1;
      } else {
        ctx.fillStyle = f.board ? css(style.cloth) : css(style.paper);
        ctx.fillRect(left, top, w + 0.6, height);
      }
      // Lighting from the slice normal (flipped when we see the reverse).
      const s = facingUp ? 1 : -1;
      const ndl = s * (-Math.sin(th) * LIGHT[0] + Math.cos(th) * LIGHT[2]);
      const lit = 0.58 + (0.42 * Math.max(ndl, 0)) / LIGHT[2];
      if (lit < 1) {
        ctx.fillStyle = `rgba(0,0,0,${Math.min(0.6, 1 - lit)})`;
        ctx.fillRect(left, top, w + 0.6, height);
      } else if (lit > 1.005) {
        ctx.fillStyle = `rgba(255,255,255,${Math.min(0.12, lit - 1)})`;
        ctx.fillRect(left, top, w + 0.6, height);
      }
    }
  }

  dispose(): void {
    this.surfaces.clear();
  }
}
