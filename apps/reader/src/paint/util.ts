/** Canvas helpers shared by the page, widget and cover painters. */
import type { Ctx2D, ImageProvider } from "../layout/types";

export function roundRectPath(ctx: Ctx2D, x: number, y: number, w: number, h: number, r: number): void {
  const rr = Math.max(0, Math.min(r, Math.abs(w) / 2, Math.abs(h) / 2));
  ctx.beginPath();
  if (rr <= 0) {
    ctx.rect(x, y, w, h);
    return;
  }
  ctx.moveTo(x + rr, y);
  ctx.lineTo(x + w - rr, y);
  ctx.arcTo(x + w, y, x + w, y + rr, rr);
  ctx.lineTo(x + w, y + h - rr);
  ctx.arcTo(x + w, y + h, x + w - rr, y + h, rr);
  ctx.lineTo(x + rr, y + h);
  ctx.arcTo(x, y + h, x, y + h - rr, rr);
  ctx.lineTo(x, y + rr);
  ctx.arcTo(x, y, x + rr, y, rr);
  ctx.closePath();
}

/** Pixel size out of a CSS font shorthand. */
export function fontPx(font: string): number {
  const m = /(\d+(?:\.\d+)?)px/.exec(font);
  return m ? Number(m[1]) : 16;
}

function intrinsic(img: CanvasImageSource): { width: number; height: number } | null {
  const any = img as { naturalWidth?: number; naturalHeight?: number; videoWidth?: number; videoHeight?: number; displayWidth?: number; displayHeight?: number; width?: unknown; height?: unknown };
  const w = any.naturalWidth || any.videoWidth || any.displayWidth || (typeof any.width === "number" ? any.width : 0);
  const h = any.naturalHeight || any.videoHeight || any.displayHeight || (typeof any.height === "number" ? any.height : 0);
  return w > 0 && h > 0 ? { width: w, height: h } : null;
}

/**
 * Draw a resource into a box with `contain` or `cover` fitting, clipped to
 * the (optionally rounded) box. Returns false when the bitmap is not
 * decoded yet; the caller paints a placeholder and reports it as missing.
 */
export function drawImageFit(
  ctx: Ctx2D,
  images: ImageProvider,
  resource: string,
  x: number,
  y: number,
  w: number,
  h: number,
  fit: "contain" | "cover",
  radius = 0,
): boolean {
  if (w <= 0 || h <= 0) return true;
  let img: CanvasImageSource | null = null;
  try {
    img = images.get(resource);
  } catch {
    img = null;
  }
  if (!img) return false;
  // The IR's intrinsic size wins: SVG images may report none of their own.
  const size = images.size(resource) ?? intrinsic(img) ?? { width: w, height: h };
  const scale = fit === "cover" ? Math.max(w / size.width, h / size.height) : Math.min(w / size.width, h / size.height);
  const dw = size.width * scale;
  const dh = size.height * scale;
  const dx = x + (w - dw) / 2;
  const dy = y + (h - dh) / 2;
  ctx.save();
  if (fit === "cover" || radius > 0) {
    if (fit === "cover") roundRectPath(ctx, x, y, w, h, radius);
    else roundRectPath(ctx, dx, dy, dw, dh, radius);
    ctx.clip();
  }
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  try {
    ctx.drawImage(img, dx, dy, dw, dh);
  } catch {
    // A broken bitmap must not take the page down with it.
    ctx.restore();
    return false;
  }
  ctx.restore();
  return true;
}

/** Fill text with letter spacing; falls back to placing glyphs one by one where the canvas lacks `letterSpacing`. */
export function fillSpaced(ctx: Ctx2D, text: string, x: number, y: number, spacing: number): void {
  if (!spacing) {
    ctx.fillText(text, x, y);
    return;
  }
  const c = ctx as Ctx2D & { letterSpacing?: string };
  if (typeof c.letterSpacing === "string") {
    c.letterSpacing = `${spacing}px`;
    ctx.fillText(text, x, y);
    c.letterSpacing = "0px";
    return;
  }
  let cx = x;
  for (const ch of text) {
    ctx.fillText(ch, cx, y);
    cx += ctx.measureText(ch).width + spacing;
  }
}

/** Width of text as `fillSpaced` draws it, without the trailing spacing. */
export function spacedWidth(ctx: Ctx2D, text: string, spacing: number): number {
  const n = Array.from(text).length;
  return ctx.measureText(text).width + (n > 1 ? spacing * (n - 1) : 0);
}
