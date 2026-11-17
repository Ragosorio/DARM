/**
 * The boards: front and back cover of a cloth-bound book.
 *
 * Everything is procedural and deterministic. The cloth is the cover
 * colour with a woven grain seeded from the book id; rules and ornaments
 * are "foil" (the accent colour with a metallic gradient and a pressed-in
 * shadow); lettering is the cover ink, also slightly debossed. Covers keep
 * their own colours in dark mode: a book does not change its binding when
 * the lights go down.
 */
import type { BackCover, Book, FrontCover, Span } from "../ir";
import { balanceLines, breakLines, placeLine, tokenize, type Piece, type RunStyle } from "../layout/text";
import type { Ctx2D, PaintEnv } from "../layout/types";
import { alpha, darken, hashString, lighten, luminance, mix, prng } from "./color";
import { drawOrnament } from "./ornaments";
import { drawImageFit, fillSpaced, roundRectPath, spacedWidth } from "./util";

interface Board {
  ctx: Ctx2D;
  w: number;
  h: number;
  env: PaintEnv;
  missing: string[];
  /** Cover colours. */
  cloth: string;
  ink: string;
  accent: string;
  /** Foil paint for rules and ornaments. */
  foil: CanvasGradient | string;
}

// ---------------------------------------------------------------------------
// Cloth
// ---------------------------------------------------------------------------

/** Woven ground: base colour, soft mottling, warp and weft threads, a few slubs. */
function clothGround(ctx: Ctx2D, w: number, h: number, color: string, rand: () => number): void {
  ctx.fillStyle = color;
  ctx.fillRect(0, 0, w, h);

  // Uneven dye: large, very soft patches.
  for (let i = 0; i < 12; i++) {
    const cx = rand() * w;
    const cy = rand() * h;
    const r = (0.2 + rand() * 0.45) * Math.max(w, h);
    const g = ctx.createRadialGradient(cx, cy, 0, cx, cy, r);
    const tone = rand() < 0.5 ? "#000000" : "#ffffff";
    g.addColorStop(0, alpha(tone, 0.045));
    g.addColorStop(1, alpha(tone, 0));
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, w, h);
  }

  // Threads. The pitch follows the board size so the grain looks the same on a phone and on a desktop.
  const pitch = Math.max(1.5, Math.min(2.6, w / 240));
  ctx.lineWidth = pitch * 0.55;
  ctx.lineCap = "butt";
  for (let y = pitch / 2; y < h; y += pitch) {
    const a = 0.03 + rand() * 0.03;
    ctx.strokeStyle = rand() < 0.5 ? alpha("#ffffff", a) : alpha("#000000", a * 1.25);
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(w, y);
    ctx.stroke();
  }
  for (let x = pitch / 2; x < w; x += pitch) {
    const a = 0.025 + rand() * 0.028;
    ctx.strokeStyle = rand() < 0.5 ? alpha("#ffffff", a) : alpha("#000000", a * 1.25);
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, h);
    ctx.stroke();
  }
  // Slubs: short thicker stretches of thread, the small irregularities of real book cloth.
  const slubs = Math.round((w * h) / 6500);
  for (let i = 0; i < slubs; i++) {
    const horizontal = rand() < 0.6;
    const len = 6 + rand() * 26;
    const x = rand() * w;
    const y = rand() * h;
    ctx.lineWidth = pitch * (0.7 + rand() * 0.5);
    ctx.strokeStyle = rand() < 0.55 ? alpha("#ffffff", 0.05 + rand() * 0.05) : alpha("#000000", 0.06 + rand() * 0.05);
    ctx.beginPath();
    ctx.moveTo(x, y);
    ctx.lineTo(horizontal ? x + len : x, horizontal ? y : y + len);
    ctx.stroke();
  }
}

/** Light on the finished board: rolled edges, a soft diagonal sheen and the hinge groove beside the spine. */
function boardFinish(ctx: Ctx2D, w: number, h: number, spine: "left" | "right"): void {
  const edge = Math.max(4, w * 0.03);
  const shade = (x0: number, y0: number, x1: number, y1: number, a: number): CanvasGradient => {
    const g = ctx.createLinearGradient(x0, y0, x1, y1);
    g.addColorStop(0, alpha("#000000", a));
    g.addColorStop(1, alpha("#000000", 0));
    return g;
  };
  ctx.fillStyle = shade(0, 0, edge, 0, spine === "left" ? 0.1 : 0.2);
  ctx.fillRect(0, 0, edge, h);
  ctx.fillStyle = shade(w, 0, w - edge, 0, spine === "right" ? 0.1 : 0.2);
  ctx.fillRect(w - edge, 0, edge, h);
  ctx.fillStyle = shade(0, 0, 0, edge, 0.14);
  ctx.fillRect(0, 0, w, edge);
  ctx.fillStyle = shade(0, h, 0, h - edge, 0.22);
  ctx.fillRect(0, h - edge, w, edge);

  const sheen = ctx.createLinearGradient(0, 0, w, h);
  sheen.addColorStop(0, alpha("#ffffff", 0.07));
  sheen.addColorStop(0.45, alpha("#ffffff", 0));
  sheen.addColorStop(1, alpha("#000000", 0.1));
  ctx.fillStyle = sheen;
  ctx.fillRect(0, 0, w, h);

  // Hinge: the groove where the board meets the spine.
  const gw = Math.max(3, w * 0.02);
  const gx = spine === "left" ? w * 0.048 : w - w * 0.048 - gw;
  const groove = ctx.createLinearGradient(gx, 0, gx + gw, 0);
  const stops: [number, string][] =
    spine === "left"
      ? [
          [0, alpha("#000000", 0)],
          [0.45, alpha("#000000", 0.26)],
          [0.75, alpha("#000000", 0.05)],
          [1, alpha("#ffffff", 0.07)],
        ]
      : [
          [0, alpha("#ffffff", 0.07)],
          [0.25, alpha("#000000", 0.05)],
          [0.55, alpha("#000000", 0.26)],
          [1, alpha("#000000", 0)],
        ];
  for (const [at, c] of stops) groove.addColorStop(at, c);
  ctx.fillStyle = groove;
  ctx.fillRect(gx, 0, gw, h);
}

/** Metallic paint: the accent colour with a slow light-dark-light sweep across the board. */
function foilPaint(ctx: Ctx2D, w: number, h: number, accent: string): CanvasGradient {
  const g = ctx.createLinearGradient(0, 0, w, h);
  g.addColorStop(0, lighten(accent, 0.12));
  g.addColorStop(0.22, lighten(accent, 0.38));
  g.addColorStop(0.42, accent);
  g.addColorStop(0.62, darken(accent, 0.14));
  g.addColorStop(0.8, lighten(accent, 0.3));
  g.addColorStop(1, darken(accent, 0.08));
  return g;
}

// ---------------------------------------------------------------------------
// Stamped elements
// ---------------------------------------------------------------------------

const PRESS = "rgba(0,0,0,0.3)";

/** A foil rule with the shadow of the impression under it. */
function rule(b: Board, x1: number, y1: number, x2: number, y2: number, width: number): void {
  const { ctx } = b;
  const off = Math.max(0.5, width * 0.5);
  ctx.lineWidth = width;
  ctx.strokeStyle = PRESS;
  ctx.beginPath();
  ctx.moveTo(x1, y1 + off);
  ctx.lineTo(x2, y2 + off);
  ctx.stroke();
  ctx.strokeStyle = b.foil;
  ctx.beginPath();
  ctx.moveTo(x1, y1);
  ctx.lineTo(x2, y2);
  ctx.stroke();
}

function frame(b: Board, inset: number, width: number, radius = 0): void {
  const { ctx, w, h } = b;
  const off = Math.max(0.5, width * 0.5);
  ctx.lineWidth = width;
  ctx.strokeStyle = PRESS;
  roundRectPath(ctx, inset, inset + off, w - 2 * inset, h - 2 * inset, radius);
  ctx.stroke();
  ctx.strokeStyle = b.foil;
  roundRectPath(ctx, inset, inset, w - 2 * inset, h - 2 * inset, radius);
  ctx.stroke();
}

function ornament(b: Board, style: "fleuron" | "asterism" | "chapter", cx: number, cy: number, ow: number, oh: number): void {
  drawOrnament(b.ctx, style, cx - ow / 2, cy - oh / 2 + Math.max(0.5, oh * 0.05), ow, oh, PRESS);
  drawOrnament(b.ctx, style, cx - ow / 2, cy - oh / 2, ow, oh, b.foil);
}

interface Lettering {
  font: string;
  size: number;
  /** Letter spacing, px. */
  spacing: number;
  color: string;
  align: "left" | "center";
}

/** One line of stamped lettering. Squeezed horizontally as a last resort so it can never leave the board. */
function letter(b: Board, text: string, x: number, y: number, maxW: number, l: Lettering): void {
  const { ctx } = b;
  if (text === "") return;
  ctx.font = l.font;
  ctx.textBaseline = "alphabetic";
  const tw = spacedWidth(ctx, text, l.spacing);
  const k = tw > maxW && tw > 0 ? maxW / tw : 1;
  const left = l.align === "center" ? x - (tw * k) / 2 : x;
  ctx.save();
  ctx.translate(left, y);
  if (k !== 1) ctx.scale(k, 1);
  ctx.fillStyle = "rgba(0,0,0,0.28)";
  fillSpaced(ctx, text, 0, Math.max(0.5, l.size * 0.03), l.spacing);
  ctx.fillStyle = l.color;
  fillSpaced(ctx, text, 0, 0, l.spacing);
  ctx.restore();
}

interface Fit {
  size: number;
  lines: string[];
  lineH: number;
  height: number;
}

/**
 * Largest size at which `text`, broken at word boundaries into balanced
 * lines, fits the box. Long titles get smaller and take more lines; a
 * single very long word shrinks the whole title.
 */
function fitLines(
  ctx: Ctx2D,
  text: string,
  fontOf: (px: number) => string,
  o: { maxW: number; maxH: number; max: number; min: number; lineHeight: number; maxLines: number; spacing?: number },
): Fit {
  let size = o.max;
  let lines: string[] = [];
  // The first size that fits is kept as a fallback; the search goes on a little further for a
  // setting without a stranded short word ("An" alone above a long line), which a slightly
  // smaller size usually resolves by letting it join its neighbour.
  let firstFit: { size: number; lines: string[] } | null = null;
  for (let guard = 0; guard < 80; guard++) {
    ctx.font = fontOf(size);
    const sp = (o.spacing ?? 0) * size;
    const widthOf = (s: string): number => spacedWidth(ctx, s, sp);
    lines = balanceLines(text, o.maxW, widthOf);
    const fits = lines.length <= o.maxLines && lines.every((l) => widthOf(l) <= o.maxW + 0.5) && lines.length * size * o.lineHeight <= o.maxH + 0.5;
    if (fits) {
      let widest = 0;
      let narrowest = Infinity;
      for (const l of lines) {
        const lw = widthOf(l);
        widest = Math.max(widest, lw);
        narrowest = Math.min(narrowest, lw);
      }
      const even = lines.length < 3 || narrowest >= widest * 0.34;
      if (even) {
        firstFit = null;
        break;
      }
      firstFit ??= { size, lines };
      if (size <= firstFit.size * 0.7) break;
    }
    if (size <= o.min) break;
    size = Math.max(o.min, size * 0.95);
  }
  if (firstFit) {
    size = firstFit.size;
    lines = firstFit.lines;
  }
  if (lines.length > o.maxLines) {
    lines = lines.slice(0, o.maxLines);
    lines[lines.length - 1] = lines[lines.length - 1]! + "…";
  }
  const lineH = size * o.lineHeight;
  return { size, lines, lineH, height: lines.length * lineH };
}

/** Draw fitted lines; `top` is the top of the block. Returns the bottom. */
function letterLines(b: Board, fit: Fit, fontOf: (px: number) => string, x: number, top: number, maxW: number, color: string, align: "left" | "center", spacingEm = 0): number {
  const font = fontOf(fit.size);
  let y = top + fit.lineH * 0.5 + fit.size * 0.34;
  for (const line of fit.lines) {
    letter(b, line, x, y, maxW, { font, size: fit.size, spacing: spacingEm * fit.size, color, align });
    y += fit.lineH;
  }
  return top + fit.height;
}

function upper(book: Book, s: string): string {
  try {
    return s.toLocaleUpperCase(book.meta.language);
  } catch {
    return s.toUpperCase();
  }
}

/** An inset picture with a hairline foil border; a darker patch of cloth while the image loads. */
function panel(b: Board, resource: string, x: number, y: number, pw: number, ph: number, border = true): void {
  const { ctx } = b;
  if (pw <= 2 || ph <= 2) return;
  ctx.fillStyle = "rgba(0,0,0,0.22)";
  ctx.fillRect(x, y + 1, pw, ph);
  if (!drawImageFit(ctx, b.env.images, resource, x, y, pw, ph, "cover")) {
    b.missing.push(resource);
    ctx.fillStyle = darken(b.cloth, 0.18);
    ctx.fillRect(x, y, pw, ph);
  }
  if (border) {
    const lw = Math.max(0.75, b.w * 0.0022);
    const gap = Math.max(3, b.w * 0.012);
    ctx.lineWidth = lw;
    ctx.strokeStyle = b.foil;
    ctx.strokeRect(x - gap, y - gap, pw + 2 * gap, ph + 2 * gap);
  }
}

// ---------------------------------------------------------------------------
// Front cover
// ---------------------------------------------------------------------------

function fontsOf(env: PaintEnv): { heading: (w: number, italic?: boolean) => (px: number) => string; body: (w: number, italic?: boolean) => (px: number) => string; sans: (w: number) => (px: number) => string } {
  const mk =
    (family: string) =>
    (weight: number, italic = false) =>
    (px: number): string =>
      `${italic ? "italic " : ""}${weight} ${Math.round(px * 100) / 100}px ${family}`;
  return { heading: mk(env.fonts.heading), body: mk(env.fonts.body), sans: mk(env.fonts.sans) };
}

function classic(b: Board, book: Book, c: FrontCover, hasPanel: boolean): void {
  const { ctx, w, h } = b;
  const f = fontsOf(b.env);
  const m = w * 0.066;
  frame(b, m, Math.max(1.5, w * 0.0058));
  frame(b, m + w * 0.02, Math.max(0.75, w * 0.0021));
  // Small lozenges at the corners of the inner frame.
  const ci = m + w * 0.02;
  const d = Math.max(2, w * 0.008);
  ctx.fillStyle = b.foil;
  for (const [cx, cy] of [
    [ci, ci],
    [w - ci, ci],
    [ci, h - ci],
    [w - ci, h - ci],
  ] as const) {
    ctx.beginPath();
    ctx.moveTo(cx - d, cy);
    ctx.lineTo(cx, cy - d);
    ctx.lineTo(cx + d, cy);
    ctx.lineTo(cx, cy + d);
    ctx.closePath();
    ctx.fill();
  }

  const cx = w / 2;
  const cw = w - 2 * (ci + w * 0.07);
  if (hasPanel && c.image !== undefined) {
    if (!c.show_text) {
      const pad = w * 0.06;
      panel(b, c.image, ci + pad, ci + pad, w - 2 * (ci + pad), h - 2 * (ci + pad));
      return;
    }
    const title = fitLines(ctx, c.title, f.heading(600), { maxW: cw, maxH: h * 0.17, max: w * 0.1, min: w * 0.04, lineHeight: 1.12, maxLines: 3 });
    const top = h * 0.215 - title.height / 2;
    letterLines(b, title, f.heading(600), cx, top, cw, b.ink, "center");
    const pw = cw * 0.84;
    const ph = h * 0.36;
    panel(b, c.image, cx - pw / 2, h * 0.35, pw, ph);
    const author = fitLines(ctx, upper(book, c.author), f.heading(500), { maxW: cw, maxH: h * 0.07, max: w * 0.034, min: w * 0.02, lineHeight: 1.5, maxLines: 2, spacing: 0.14 });
    letterLines(b, author, f.heading(500), cx, h * 0.835 - author.height / 2, cw, b.ink, "center", 0.14);
    return;
  }
  if (!c.show_text) {
    ornament(b, "fleuron", cx, h / 2, w * 0.34, w * 0.055);
    return;
  }

  const title = fitLines(ctx, c.title, f.heading(600), { maxW: cw, maxH: h * 0.3, max: w * 0.128, min: w * 0.045, lineHeight: 1.1, maxLines: 4 });
  const sub = c.subtitle ? fitLines(ctx, c.subtitle, f.body(400, true), { maxW: cw * 0.86, maxH: h * 0.12, max: w * 0.043, min: w * 0.026, lineHeight: 1.36, maxLines: 3 }) : null;
  const ornH = w * 0.05;
  const gap = h * 0.034;
  const blockH = title.height + gap + ornH + (sub ? gap + sub.height : 0);
  // The block hangs from a point above the middle: the optical centre of a cover is higher than its geometric one.
  let y = Math.max(ci + h * 0.07, h * 0.4 - blockH / 2);
  y = letterLines(b, title, f.heading(600), cx, y, cw, b.ink, "center");
  y += gap;
  ornament(b, "fleuron", cx, y + ornH / 2, w * 0.3, ornH);
  y += ornH;
  if (sub) letterLines(b, sub, f.body(400, true), cx, y + gap, cw, mix(b.ink, b.cloth, 0.12), "center");

  const author = fitLines(ctx, upper(book, c.author), f.heading(500), { maxW: cw, maxH: h * 0.08, max: w * 0.035, min: w * 0.02, lineHeight: 1.55, maxLines: 2, spacing: 0.14 });
  const ay = h * 0.84 - author.height / 2;
  ornament(b, "chapter", cx, ay - h * 0.028, w * 0.17, w * 0.017);
  letterLines(b, author, f.heading(500), cx, ay, cw, b.ink, "center", 0.14);
}

function band(b: Board, book: Book, c: FrontCover, hasPanel: boolean): void {
  const { ctx, w, h } = b;
  const f = fontsOf(b.env);
  const cx = w / 2;
  const y0 = h * (hasPanel ? 0.47 : 0.28);
  const y1 = y0 + h * 0.27;
  // The band is the same cloth, a shade deeper, as if a strip had been dyed twice.
  ctx.fillStyle = alpha(darken(b.cloth, 0.55), 0.42);
  ctx.fillRect(0, y0, w, y1 - y0);
  const thick = Math.max(1.5, w * 0.006);
  const thin = Math.max(0.75, w * 0.002);
  const off = w * 0.014;
  rule(b, 0, y0, w, y0, thick);
  rule(b, 0, y0 + off, w, y0 + off, thin);
  rule(b, 0, y1, w, y1, thick);
  rule(b, 0, y1 - off, w, y1 - off, thin);

  if (hasPanel && c.image !== undefined) {
    const px = w * 0.12;
    const top = h * 0.09;
    panel(b, c.image, px, top, w - 2 * px, y0 - top - h * 0.05);
  } else {
    ornament(b, "asterism", cx, h * 0.15, w * 0.2, w * 0.035);
  }
  if (!c.show_text) return;

  const cw = w * 0.78;
  const inner = y1 - y0 - 2 * off - h * 0.03;
  const sub = c.subtitle ? fitLines(ctx, c.subtitle, f.body(400, true), { maxW: cw * 0.9, maxH: inner * 0.3, max: w * 0.04, min: w * 0.025, lineHeight: 1.34, maxLines: 2 }) : null;
  const gap = sub ? h * 0.014 : 0;
  const title = fitLines(ctx, c.title, f.heading(600), { maxW: cw, maxH: inner - (sub ? sub.height + gap : 0), max: w * 0.118, min: w * 0.04, lineHeight: 1.1, maxLines: 3 });
  const blockH = title.height + (sub ? gap + sub.height : 0);
  let y = (y0 + y1) / 2 - blockH / 2;
  y = letterLines(b, title, f.heading(600), cx, y, cw, b.ink, "center");
  if (sub) letterLines(b, sub, f.body(400, true), cx, y + gap, cw, mix(b.ink, b.cloth, 0.1), "center");

  const author = fitLines(ctx, c.author, f.heading(500), { maxW: cw, maxH: h * 0.09, max: w * 0.05, min: w * 0.026, lineHeight: 1.3, maxLines: 2 });
  const ay = (hasPanel ? h * 0.865 : h * 0.8) - author.height / 2;
  if (!hasPanel) ornament(b, "fleuron", cx, (y1 + ay) / 2, w * 0.26, w * 0.044);
  letterLines(b, author, f.heading(500), cx, ay, cw, b.ink, "center");
}

function minimal(b: Board, book: Book, c: FrontCover, hasPanel: boolean): void {
  const { ctx, w, h } = b;
  const f = fontsOf(b.env);
  const cx = w / 2;
  const cw = w * 0.68;
  if (hasPanel && c.image !== undefined) {
    const pw = w * 0.4;
    const size = b.env.images.size(c.image);
    const aspect = size ? Math.max(0.6, Math.min(1.6, size.width / size.height)) : 1;
    const ph = Math.min(h * 0.36, pw / aspect);
    panel(b, c.image, cx - pw / 2, h * 0.52 - ph / 2, pw, ph);
  }
  if (!c.show_text) return;
  const title = fitLines(ctx, upper(book, c.title), f.heading(500), { maxW: cw, maxH: h * 0.14, max: w * 0.044, min: w * 0.024, lineHeight: 1.6, maxLines: 3, spacing: 0.22 });
  let y = h * 0.2 - title.height / 2;
  y = letterLines(b, title, f.heading(500), cx, y, cw, b.ink, "center", 0.22);
  y += h * 0.022;
  rule(b, cx - w * 0.05, y, cx + w * 0.05, y, Math.max(1, w * 0.0026));
  if (c.subtitle) {
    const sub = fitLines(ctx, c.subtitle, f.body(400, true), { maxW: cw, maxH: h * 0.1, max: w * 0.034, min: w * 0.022, lineHeight: 1.4, maxLines: 3 });
    letterLines(b, sub, f.body(400, true), cx, y + h * 0.022, cw, mix(b.ink, b.cloth, 0.18), "center");
  }
  const author = fitLines(ctx, upper(book, c.author), f.heading(500), { maxW: cw, maxH: h * 0.07, max: w * 0.03, min: w * 0.018, lineHeight: 1.6, maxLines: 2, spacing: 0.2 });
  letterLines(b, author, f.heading(500), cx, h * 0.895 - author.height / 2, cw, b.ink, "center", 0.2);
}

function modern(b: Board, book: Book, c: FrontCover, hasPanel: boolean): void {
  const { ctx, w, h } = b;
  const f = fontsOf(b.env);
  const left = w * 0.105;
  const cw = w - 2 * left;
  // A short bar of foil above the title, the one flourish of this style.
  ctx.fillStyle = PRESS;
  ctx.fillRect(left, h * 0.088 + 1, w * 0.15, Math.max(3, w * 0.012));
  ctx.fillStyle = b.foil;
  ctx.fillRect(left, h * 0.088, w * 0.15, Math.max(3, w * 0.012));

  if (hasPanel && c.image !== undefined) {
    panel(b, c.image, left, h * 0.5, cw, h * 0.32, false);
  }
  if (c.show_text) {
    const title = fitLines(ctx, c.title, f.sans(700), { maxW: cw, maxH: h * (hasPanel ? 0.27 : 0.46), max: w * 0.19, min: w * 0.055, lineHeight: 1.04, maxLines: 6, spacing: -0.02 });
    let y = h * 0.13;
    y = letterLines(b, title, f.sans(700), left, y, cw, b.ink, "left", -0.02);
    if (c.subtitle) {
      const sub = fitLines(ctx, c.subtitle, f.sans(400), { maxW: cw * 0.86, maxH: h * 0.1, max: w * 0.042, min: w * 0.026, lineHeight: 1.36, maxLines: 3 });
      letterLines(b, sub, f.sans(400), left, y + h * 0.022, cw, mix(b.ink, b.cloth, 0.15), "left");
    }
    rule(b, left, h * 0.872, w - left, h * 0.872, Math.max(1, w * 0.0026));
    const author = fitLines(ctx, upper(book, c.author), f.sans(600), { maxW: cw, maxH: h * 0.06, max: w * 0.031, min: w * 0.019, lineHeight: 1.5, maxLines: 2, spacing: 0.12 });
    letterLines(b, author, f.sans(600), left, h * 0.892, cw, b.ink, "left", 0.12);
  }
}

function makeBoard(ctx: Ctx2D, book: Book, size: { width: number; height: number }, env: PaintEnv, colors: { color: string; ink: string; accent: string }, salt: string, missing: string[]): Board {
  const w = size.width;
  const h = size.height;
  const cloth = safe(colors.color, "#5b3a2e");
  const accent = safe(colors.accent, "#d9b46a");
  let ink = safe(colors.ink, "#f3e9d2");
  // Lettering that would vanish into the cloth is nudged towards legibility.
  if (Math.abs(luminance(ink) - luminance(cloth)) < 0.08) ink = luminance(cloth) > 0.4 ? darken(cloth, 0.75) : lighten(cloth, 0.8);
  ctx.save();
  ctx.globalAlpha = 1;
  ctx.globalCompositeOperation = "source-over";
  clothGround(ctx, w, h, cloth, prng(hashString(`${String(book.meta.id)}|${salt}`)));
  return { ctx, w, h, env, missing, cloth, ink, accent, foil: foilPaint(ctx, w, h, accent) };
}

function safe(c: unknown, fallback: string): string {
  return typeof c === "string" && /^#(?:[0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(c) ? c : fallback;
}

export function paintFrontCover(ctx: Ctx2D, book: Book, size: { width: number; height: number }, env: PaintEnv): string[] {
  const missing: string[] = [];
  const c = book.cover.front;
  const b = makeBoard(ctx, book, size, env, c, "front", missing);
  const { w, h } = b;
  const hasImage = typeof c.image === "string" && c.image !== "";
  const bleed = hasImage && c.full_bleed === true;
  if (bleed && c.image !== undefined) {
    if (!drawImageFit(ctx, env.images, c.image, 0, 0, w, h, "cover")) missing.push(c.image);
    else if (c.show_text) {
      // A scrim in the cloth colour keeps the lettering readable over any picture.
      const g = ctx.createLinearGradient(0, 0, 0, h);
      const tone = darken(b.cloth, 0.6);
      g.addColorStop(0, alpha(tone, 0.62));
      g.addColorStop(0.5, alpha(tone, 0.3));
      g.addColorStop(1, alpha(tone, 0.66));
      ctx.fillStyle = g;
      ctx.fillRect(0, 0, w, h);
    }
  }
  // An image that carries its own lettering and covers the board needs nothing stamped over it.
  if (!(bleed && !c.show_text)) {
    const hasPanel = hasImage && !bleed;
    if (c.style === "band") band(b, book, c, hasPanel);
    else if (c.style === "minimal") minimal(b, book, c, hasPanel);
    else if (c.style === "modern") modern(b, book, c, hasPanel);
    else classic(b, book, c, hasPanel);
  }
  boardFinish(ctx, w, h, "left");
  ctx.restore();
  return missing;
}

// ---------------------------------------------------------------------------
// Back cover
// ---------------------------------------------------------------------------

function blurbPieces(spans: Span[], base: (weight: number, italic: boolean) => string, size: number, color: string): Piece[] {
  const styles = new Map<string, RunStyle>();
  const out: Piece[] = [];
  for (const sp of spans) {
    if (!sp || typeof sp !== "object") continue;
    if (typeof sp.text !== "string" || sp.text === "") {
      if (sp.break) out.push("br");
      continue;
    }
    const key = `${sp.bold ? 1 : 0}${sp.italic ? 1 : 0}`;
    let st = styles.get(key);
    if (!st) {
      st = { font: base(sp.bold ? 700 : 400, sp.italic === true), size, color };
      styles.set(key, st);
    }
    out.push({ text: sp.text, st });
    // `break` is a hard line break after the span's text.
    if (sp.break) out.push("br");
  }
  return out;
}

export function paintBackCover(ctx: Ctx2D, book: Book, size: { width: number; height: number }, env: PaintEnv): string[] {
  const missing: string[] = [];
  const c: BackCover = book.cover.back;
  const b = makeBoard(ctx, book, size, env, c, "back", missing);
  const { w, h } = b;
  const f = fontsOf(env);
  const cx = w / 2;

  // Blind-stamped frame: pressed into the cloth without foil.
  const inset = w * 0.066;
  ctx.lineWidth = Math.max(1, w * 0.004);
  ctx.strokeStyle = alpha("#000000", 0.2);
  ctx.strokeRect(inset, inset, w - 2 * inset, h - 2 * inset);
  ctx.lineWidth = Math.max(0.5, w * 0.0015);
  ctx.strokeStyle = alpha("#ffffff", 0.08);
  ctx.strokeRect(inset + ctx.lineWidth * 2, inset + ctx.lineWidth * 2, w - 2 * inset, h - 2 * inset);

  const mx = w * 0.16;
  const cw = w - 2 * mx;
  let y = h * 0.135;

  const tagline = typeof c.tagline === "string" ? c.tagline.trim() : "";
  if (tagline) {
    const fit = fitLines(ctx, tagline, f.heading(500, true), { maxW: cw, maxH: h * 0.16, max: w * 0.064, min: w * 0.034, lineHeight: 1.22, maxLines: 4 });
    y = letterLines(b, fit, f.heading(500, true), cx, y, cw, b.ink, "center");
    y += h * 0.03;
    ornament(b, "chapter", cx, y, w * 0.17, w * 0.017);
    y += h * 0.04;
  }

  // Imprint at the foot: a short rule and the title in small spaced capitals.
  const imprint = fitLines(ctx, upper(book, String(book.meta.title ?? "")), f.heading(500), { maxW: cw, maxH: h * 0.05, max: w * 0.026, min: w * 0.016, lineHeight: 1.5, maxLines: 1, spacing: 0.22 });
  const imprintTop = h * 0.9 - imprint.height / 2;
  rule(b, cx - w * 0.045, imprintTop - h * 0.018, cx + w * 0.045, imprintTop - h * 0.018, Math.max(1, w * 0.0026));
  letterLines(b, imprint, f.heading(500), cx, imprintTop, cw, mix(b.ink, b.cloth, 0.15), "center", 0.22);
  let floor = imprintTop - h * 0.05;

  if (typeof c.image === "string" && c.image !== "") {
    const sz = env.images.size(c.image);
    const aspect = sz ? Math.max(0.6, Math.min(1.8, sz.width / sz.height)) : 1;
    const pw = w * 0.3;
    const ph = Math.min(h * 0.2, pw / aspect);
    panel(b, c.image, cx - pw / 2, floor - ph, pw, ph);
    floor -= ph + h * 0.045;
  }

  // Blurb: a comfortable ragged measure. The type shrinks until the paragraphs fit above the foot.
  let paragraphs: Span[][] = Array.isArray(c.blurb) ? c.blurb.filter((p) => Array.isArray(p) && p.length > 0) : [];
  if (paragraphs.length === 0 && typeof book.meta.description === "string" && book.meta.description.trim() !== "") {
    paragraphs = [[{ text: book.meta.description }]];
  }
  if (paragraphs.length > 0) {
    const measure = (font: string, text: string): number => {
      if (ctx.font !== font) ctx.font = font;
      return ctx.measureText(text).width;
    };
    const color = mix(b.ink, b.cloth, 0.06);
    const avail = floor - y;
    for (let s = Math.max(10.5, Math.min(18, w * 0.0365)), guard = 0; guard < 24; guard++, s *= 0.94) {
      const lh = Math.round(s * 1.5);
      const base = (weight: number, italic: boolean): string => `${italic ? "italic " : ""}${weight} ${Math.round(s * 100) / 100}px ${env.fonts.body}`;
      const set = paragraphs.map((p) => breakLines(tokenize(blurbPieces(p, base, s, color), measure), () => cw, { measure, justify: false }));
      let total = 0;
      for (const lines of set) total += lines.length * lh;
      total += (set.length - 1) * lh * 0.6;
      if (total > avail && s > 8 && guard < 23) continue;
      let ty = y;
      ctx.textBaseline = "alphabetic";
      for (const lines of set) {
        for (const line of lines) {
          if (ty + lh > floor + 0.5) break;
          const placed = placeLine(line, mx, ty + lh * 0.5 + s * 0.33, cw, "left");
          for (const it of placed.items) {
            if (it.kind !== "text") continue;
            ctx.font = it.font;
            ctx.fillStyle = "rgba(0,0,0,0.22)";
            ctx.fillText(it.text, it.x, it.y + 0.6);
            ctx.fillStyle = it.color;
            ctx.fillText(it.text, it.x, it.y);
          }
          ty += lh;
        }
        ty += lh * 0.6;
      }
      break;
    }
  }

  boardFinish(ctx, w, h, "right");
  ctx.restore();
  return missing;
}
