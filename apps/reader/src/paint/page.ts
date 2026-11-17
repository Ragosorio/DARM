/**
 * `paintPage`: draw the items of a laid-out page on a 2D canvas.
 *
 * The context arrives scaled so that one unit is one CSS pixel. Items are
 * painted in order. Entrance animations are evaluated per block: all items
 * that share a `block` and carry an `anim` move together.
 */
import type { Anim } from "../ir";
import type { Ctx2D, DrawItem, PageLayout, PaintEnv, PaintResult } from "../layout/types";
import { drawOrnament } from "./ornaments";
import { drawImageFit, fillSpaced, fontPx, roundRectPath, spacedWidth } from "./util";
import { drawWidget } from "./widgets";

interface Motion {
  alpha: number;
  dx: number;
  dy: number;
  scale: number;
  running: boolean;
}

const REST: Motion = { alpha: 1, dx: 0, dy: 0, scale: 1, running: false };

/** State of an entrance animation at `time` ms. Cubic ease-out: fast in, gentle landing. */
export function motionAt(anim: Anim, time: number): Motion {
  const delay = Number.isFinite(anim.delay_ms) ? Math.max(0, anim.delay_ms) : 0;
  const duration = Number.isFinite(anim.duration_ms) ? Math.max(1, anim.duration_ms) : 600;
  const t = Math.max(0, Math.min(1, (time - delay) / duration));
  if (t >= 1) return REST;
  const e = 1 - Math.pow(1 - t, 3);
  const rest = 1 - e;
  const m: Motion = { alpha: e, dx: 0, dy: 0, scale: 1, running: true };
  switch (anim.kind) {
    case "rise":
      m.dy = 14 * rest;
      break;
    case "drop":
      m.dy = -14 * rest;
      break;
    case "slide_left":
      m.dx = 24 * rest;
      break;
    case "slide_right":
      m.dx = -24 * rest;
      break;
    case "zoom":
      m.scale = 0.94 + 0.06 * e;
      break;
    case "fade":
    default:
      break;
  }
  return m;
}

interface Box {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

function itemBox(ctx: Ctx2D, it: DrawItem): Box {
  if (it.kind === "line") return { x0: Math.min(it.x1, it.x2), y0: Math.min(it.y1, it.y2), x1: Math.max(it.x1, it.x2), y1: Math.max(it.y1, it.y2) };
  if (it.kind === "text") {
    const size = fontPx(it.font);
    ctx.font = it.font;
    const w = spacedWidth(ctx, it.text, it.letterSpacing ?? 0);
    return { x0: it.x, y0: it.y - size * 0.8, x1: it.x + w, y1: it.y + size * 0.25 };
  }
  return { x0: it.x, y0: it.y, x1: it.x + it.w, y1: it.y + it.h };
}

function drawItem(ctx: Ctx2D, it: DrawItem, env: PaintEnv, missing: string[]): void {
  switch (it.kind) {
    case "text": {
      if (it.text === "") return;
      ctx.font = it.font;
      ctx.fillStyle = it.color;
      fillSpaced(ctx, it.text, it.x, it.y, it.letterSpacing ?? 0);
      if (it.underline) {
        const size = fontPx(it.font);
        const w = spacedWidth(ctx, it.text, it.letterSpacing ?? 0);
        const y = it.y + Math.max(1.5, size * 0.13);
        ctx.beginPath();
        ctx.moveTo(it.x, y);
        ctx.lineTo(it.x + w, y);
        ctx.lineWidth = Math.max(0.75, size * 0.05);
        ctx.strokeStyle = it.color;
        ctx.stroke();
      }
      return;
    }
    case "rect": {
      roundRectPath(ctx, it.x, it.y, it.w, it.h, it.radius ?? 0);
      if (it.fill !== undefined) {
        ctx.fillStyle = it.fill;
        ctx.fill();
      }
      if (it.stroke !== undefined) {
        ctx.lineWidth = it.lineWidth ?? 1;
        ctx.strokeStyle = it.stroke;
        ctx.stroke();
      }
      return;
    }
    case "line": {
      ctx.beginPath();
      ctx.moveTo(it.x1, it.y1);
      ctx.lineTo(it.x2, it.y2);
      ctx.lineWidth = it.width;
      ctx.strokeStyle = it.color;
      ctx.stroke();
      return;
    }
    case "image": {
      if (!drawImageFit(ctx, env.images, it.resource, it.x, it.y, it.w, it.h, it.fit, it.radius ?? 0)) {
        // Not decoded yet: a quiet tone holds the place until the caller repaints.
        roundRectPath(ctx, it.x, it.y, it.w, it.h, it.radius ?? 0);
        ctx.fillStyle = env.palette.tint;
        ctx.fill();
        missing.push(it.resource);
      }
      return;
    }
    case "ornament":
      drawOrnament(ctx, it.style, it.x, it.y, it.w, it.h, it.color);
      return;
    case "widget":
      drawWidget(ctx, it, env, missing);
      return;
  }
}

export function paintPage(ctx: Ctx2D, page: PageLayout, size: { width: number; height: number }, env: PaintEnv): PaintResult {
  const missing: string[] = [];
  ctx.save();
  ctx.globalAlpha = 1;
  ctx.globalCompositeOperation = "source-over";
  ctx.fillStyle = page.background ?? env.palette.paper;
  ctx.fillRect(0, 0, size.width, size.height);
  ctx.textBaseline = "alphabetic";
  ctx.textAlign = "left";
  ctx.lineCap = "butt";
  ctx.lineJoin = "miter";

  const time = env.time;
  const timed = time !== undefined && page.animated;
  let animating = false;

  // Scaling animations pivot on the centre of the whole block, so its items stay registered.
  let pivots: Map<string, Box> | null = null;
  if (timed) {
    for (const it of page.items) {
      if (!it.anim || it.anim.kind !== "zoom") continue;
      pivots ??= new Map();
      const key = it.block ?? "";
      const b = itemBox(ctx, it);
      const acc = pivots.get(key);
      if (!acc) pivots.set(key, b);
      else {
        acc.x0 = Math.min(acc.x0, b.x0);
        acc.y0 = Math.min(acc.y0, b.y0);
        acc.x1 = Math.max(acc.x1, b.x1);
        acc.y1 = Math.max(acc.y1, b.y1);
      }
    }
  }

  for (const it of page.items) {
    const opacity = it.opacity === undefined ? 1 : Math.max(0, Math.min(1, it.opacity));
    let m = REST;
    if (timed && it.anim) {
      m = motionAt(it.anim, time);
      if (m.running) animating = true;
    }
    const a = opacity * m.alpha;
    if (a <= 0) continue;
    const plain = a === 1 && m === REST;
    if (!plain) {
      ctx.save();
      ctx.globalAlpha = a;
      if (m.dx !== 0 || m.dy !== 0) ctx.translate(m.dx, m.dy);
      if (m.scale !== 1) {
        const p = pivots?.get(it.block ?? "");
        const cx = p ? (p.x0 + p.x1) / 2 : size.width / 2;
        const cy = p ? (p.y0 + p.y1) / 2 : size.height / 2;
        ctx.translate(cx, cy);
        ctx.scale(m.scale, m.scale);
        ctx.translate(-cx, -cy);
      }
    }
    drawItem(ctx, it, env, missing);
    if (!plain) ctx.restore();
  }
  ctx.restore();
  return { animating, missing: Array.from(new Set(missing)) };
}
