/**
 * Text measurement on a canvas, for the layout engine.
 *
 * Fonts must be loaded before measuring (see fonts.ts): the canvas
 * silently measures with a fallback face otherwise, and every line would
 * break in the wrong place once the real font arrives.
 */
import type { FontMetrics, Measurer } from "../layout/types";

type MeasureCtx = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;

function makeContext(): MeasureCtx {
  if (typeof OffscreenCanvas !== "undefined") {
    const ctx = new OffscreenCanvas(8, 8).getContext("2d");
    if (ctx) return ctx;
  }
  const ctx = document.createElement("canvas").getContext("2d");
  if (!ctx) throw new Error("2D canvas is not available");
  return ctx;
}

function sizeOf(font: string): number {
  const m = /(\d+(?:\.\d+)?)px/.exec(font);
  return m ? Number(m[1]) : 16;
}

const MAX_ENTRIES = 60000;

export function createCanvasMeasurer(): Measurer {
  const ctx = makeContext();
  let current = "";
  // Widths survive across layouts (a state change re-lays the same words); the cache is dropped when it grows large.
  let widths = new Map<string, Map<string, number>>();
  let entries = 0;
  const metrics = new Map<string, FontMetrics>();

  const use = (font: string): void => {
    if (font !== current) {
      ctx.font = font;
      current = font;
    }
  };

  return {
    width(font: string, text: string): number {
      let m = widths.get(font);
      if (!m) {
        m = new Map();
        widths.set(font, m);
      }
      let w = m.get(text);
      if (w === undefined) {
        use(font);
        w = ctx.measureText(text).width;
        if (++entries > MAX_ENTRIES) {
          widths = new Map();
          entries = 0;
        } else m.set(text, w);
      }
      return w;
    },
    metrics(font: string): FontMetrics {
      let fm = metrics.get(font);
      if (!fm) {
        use(font);
        const size = sizeOf(font);
        const x = ctx.measureText("x");
        const cap = ctx.measureText("H");
        const pos = (v: number | undefined, fallback: number): number => (typeof v === "number" && Number.isFinite(v) && v > 0 ? v : fallback);
        fm = {
          // Font-wide metrics where the browser has them; otherwise typical proportions of a text face.
          ascent: pos(x.fontBoundingBoxAscent, size * 0.93),
          descent: pos(x.fontBoundingBoxDescent, size * 0.26),
          xHeight: pos(x.actualBoundingBoxAscent, size * 0.48),
          capHeight: pos(cap.actualBoundingBoxAscent, size * 0.68),
        };
        metrics.set(font, fm);
      }
      return fm;
    },
  };
}
