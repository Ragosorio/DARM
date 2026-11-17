/**
 * Typographic ornaments drawn as vector paths, so they stay crisp at any
 * size and take any colour. All three are designed in a unit system where
 * `u` is half the height of the ornament's box.
 */
import type { Ctx2D, OrnamentStyle } from "../layout/types";

/** A leaf: pointed at both ends, widest slightly before the middle. Drawn along +x from the origin. */
function leaf(ctx: Ctx2D, len: number, half: number): void {
  ctx.moveTo(0, 0);
  ctx.bezierCurveTo(len * 0.28, -half * 1.25, len * 0.72, -half * 0.9, len, 0);
  ctx.bezierCurveTo(len * 0.72, half * 0.9, len * 0.28, half * 1.25, 0, 0);
}

/** A swelled rule: a hairline that thickens towards `x0` and tapers to a point at `x1`. */
function swell(ctx: Ctx2D, x0: number, x1: number, y: number, thick: number): void {
  const t = thick / 2;
  ctx.moveTo(x0, y - t * 0.55);
  ctx.quadraticCurveTo((x0 + x1) / 2, y - t, x1, y);
  ctx.quadraticCurveTo((x0 + x1) / 2, y + t, x0, y + t * 0.55);
  ctx.closePath();
}

function diamond(ctx: Ctx2D, cx: number, cy: number, rx: number, ry: number): void {
  ctx.moveTo(cx - rx, cy);
  ctx.lineTo(cx, cy - ry);
  ctx.lineTo(cx + rx, cy);
  ctx.lineTo(cx, cy + ry);
  ctx.closePath();
}

/**
 * Fleuron: a four-leaved floret between two dots and two swelled rules.
 * Used for section breaks and on the title page.
 */
function fleuron(ctx: Ctx2D, cx: number, cy: number, w: number, h: number): void {
  const u = h / 2;
  // Floret: long horizontal leaves, shorter vertical ones.
  for (let k = 0; k < 4; k++) {
    ctx.save();
    ctx.translate(cx, cy);
    ctx.rotate((k * Math.PI) / 2);
    ctx.beginPath();
    leaf(ctx, k % 2 === 0 ? u * 1.25 : u * 0.92, u * 0.27);
    ctx.fill();
    ctx.restore();
  }
  // Small diagonal leaves give the floret a second, lighter rhythm.
  for (let k = 0; k < 4; k++) {
    ctx.save();
    ctx.translate(cx, cy);
    ctx.rotate(Math.PI / 4 + (k * Math.PI) / 2);
    ctx.beginPath();
    leaf(ctx, u * 0.52, u * 0.11);
    ctx.fill();
    ctx.restore();
  }
  const dot = Math.max(0.6, u * 0.13);
  const dotX = u * 1.72;
  const ruleFrom = u * 2.15;
  const ruleTo = Math.max(ruleFrom + u, w / 2);
  const thick = Math.max(1, u * 0.2);
  ctx.beginPath();
  ctx.arc(cx - dotX, cy, dot, 0, Math.PI * 2);
  ctx.arc(cx + dotX, cy, dot, 0, Math.PI * 2);
  ctx.fill();
  ctx.beginPath();
  swell(ctx, cx + ruleFrom, cx + ruleTo, cy, thick);
  swell(ctx, cx - ruleFrom, cx - ruleTo, cy, thick);
  ctx.fill();
}

/** Asterism: three six-armed asterisks in a row, each arm a small club that widens outwards. */
function asterism(ctx: Ctx2D, cx: number, cy: number, w: number, h: number): void {
  const r = Math.min(h * 0.46, w / 9);
  const step = Math.min(w / 3, r * 4.2);
  for (let i = -1; i <= 1; i++) {
    const x = cx + i * step;
    for (let k = 0; k < 6; k++) {
      ctx.save();
      ctx.translate(x, cy);
      ctx.rotate(-Math.PI / 2 + (k * Math.PI) / 3);
      ctx.beginPath();
      ctx.moveTo(0, -r * 0.05);
      ctx.quadraticCurveTo(r * 0.6, -r * 0.2, r, -r * 0.17);
      ctx.quadraticCurveTo(r * 1.08, 0, r, r * 0.17);
      ctx.quadraticCurveTo(r * 0.6, r * 0.2, 0, r * 0.05);
      ctx.closePath();
      ctx.fill();
      ctx.restore();
    }
  }
}

/** Chapter ornament: a lozenge on a swelled rule, the classic companion of a chapter numeral. */
function chapter(ctx: Ctx2D, cx: number, cy: number, w: number, h: number): void {
  const u = h / 2;
  const rx = u * 0.95;
  const ry = u * 0.95;
  // At larger sizes the lozenge is pierced (even-odd fill), so it reads as an outline.
  ctx.beginPath();
  diamond(ctx, cx, cy, rx, ry);
  if (u >= 5) diamond(ctx, cx, cy, rx * 0.4, ry * 0.4);
  ctx.fill("evenodd");
  const from = rx + u * 0.7;
  const to = Math.max(from + u, w / 2);
  const thick = Math.max(1, u * 0.34);
  ctx.beginPath();
  swell(ctx, cx + from, cx + to, cy, thick);
  swell(ctx, cx - from, cx - to, cy, thick);
  ctx.fill();
}

/** `color` may be a gradient: the covers stamp ornaments in foil. */
export function drawOrnament(ctx: Ctx2D, style: OrnamentStyle, x: number, y: number, w: number, h: number, color: string | CanvasGradient): void {
  if (w <= 0 || h <= 0) return;
  ctx.save();
  ctx.fillStyle = color;
  const cx = x + w / 2;
  const cy = y + h / 2;
  if (style === "asterism") asterism(ctx, cx, cy, w, h);
  else if (style === "chapter") chapter(ctx, cx, cy, w, h);
  else fleuron(ctx, cx, cy, w, h);
  ctx.restore();
}
