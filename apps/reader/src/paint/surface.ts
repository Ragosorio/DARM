/**
 * Surfaces that are not pages of the book: the two boards, the endpapers
 * pasted inside them, and blank leaves.
 */
import type { Book } from "../ir";
import type { Ctx2D, PaintEnv, PaintResult, Surface } from "../layout/types";
import { alpha, darken, luminance, mix } from "./color";
import { paintBackCover, paintFrontCover } from "./cover";

/**
 * Endpaper: a pale tint of the cloth with a small diaper pattern, the kind
 * of quiet repeat printed on the pastedowns of trade editions. Laid lines
 * give the sheet a paper grain. Low contrast on purpose: it is the first
 * thing seen when the cover opens and must not shout. In dark mode the
 * same pattern is printed on a deep tint instead of a pale one.
 */
function paintEndpaper(ctx: Ctx2D, book: Book, w: number, h: number, env: PaintEnv): void {
  const cloth = /^#[0-9a-f]{6}$/i.test(String(book.cover?.front?.color)) ? book.cover.front.color : "#6b4a3a";
  const accent = /^#[0-9a-f]{6}$/i.test(String(book.cover?.front?.accent)) ? book.cover.front.accent : "#c9a45c";
  const dark = env.palette.dark;
  const ground = dark ? mix(darken(cloth, 0.72), "#101010", 0.35) : mix(cloth, "#f4eee0", luminance(cloth) > 0.5 ? 0.6 : 0.84);
  const motif = dark ? mix(ground, cloth, 0.5) : mix(ground, cloth, 0.2);
  const spark = dark ? mix(ground, accent, 0.4) : mix(ground, accent, 0.36);

  ctx.save();
  ctx.globalAlpha = 1;
  ctx.globalCompositeOperation = "source-over";
  ctx.fillStyle = ground;
  ctx.fillRect(0, 0, w, h);

  // Laid lines.
  ctx.strokeStyle = alpha(dark ? "#ffffff" : "#000000", dark ? 0.02 : 0.022);
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let y = 1.5; y < h; y += 3) {
    ctx.moveTo(0, y);
    ctx.lineTo(w, y);
  }
  ctx.stroke();

  // Diaper: florets on a half-drop grid, joined by a fine diagonal lattice, with a dot in every cell.
  const cell = Math.max(18, Math.min(30, w / 20));
  const half = cell / 2;
  ctx.strokeStyle = alpha(motif, 0.55);
  ctx.lineWidth = 0.75;
  ctx.beginPath();
  for (let k = -Math.ceil(h / cell); k * cell < w + h; k++) {
    const x = k * cell;
    ctx.moveTo(x, 0);
    ctx.lineTo(x + h, h);
    ctx.moveTo(x + h, 0);
    ctx.lineTo(x, h);
  }
  ctx.stroke();

  const petal = cell * 0.2;
  for (let row = 0, y = 0; y <= h + cell; row++, y += half) {
    for (let x = row % 2 === 0 ? 0 : half; x <= w + cell; x += cell) {
      // Floret at the lattice crossings.
      ctx.fillStyle = motif;
      ctx.beginPath();
      for (let p = 0; p < 4; p++) {
        const a = (p * Math.PI) / 2;
        const tx = x + Math.cos(a) * petal;
        const ty = y + Math.sin(a) * petal;
        const nx = -Math.sin(a) * petal * 0.42;
        const ny = Math.cos(a) * petal * 0.42;
        ctx.moveTo(x, y);
        ctx.quadraticCurveTo(x + (tx - x) * 0.5 + nx, y + (ty - y) * 0.5 + ny, tx, ty);
        ctx.quadraticCurveTo(x + (tx - x) * 0.5 - nx, y + (ty - y) * 0.5 - ny, x, y);
      }
      ctx.fill();
      // A spark of the foil colour in the centre of each lozenge.
      ctx.fillStyle = spark;
      ctx.beginPath();
      ctx.arc(x + half, y, Math.max(0.7, cell * 0.035), 0, Math.PI * 2);
      ctx.fill();
    }
  }

  // The sheet darkens slightly towards its edges, where it is pasted down.
  const g = ctx.createRadialGradient(w / 2, h / 2, Math.min(w, h) * 0.35, w / 2, h / 2, Math.max(w, h) * 0.75);
  g.addColorStop(0, alpha("#000000", 0));
  g.addColorStop(1, alpha("#000000", dark ? 0.2 : 0.08));
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, w, h);
  ctx.restore();
}

export function paintSurface(ctx: Ctx2D, surface: Surface, book: Book, size: { width: number; height: number }, env: PaintEnv): PaintResult {
  const w = size.width;
  const h = size.height;
  let missing: string[] = [];
  if (surface === "front-cover") missing = paintFrontCover(ctx, book, size, env);
  else if (surface === "back-cover") missing = paintBackCover(ctx, book, size, env);
  else if (surface === "endpaper") paintEndpaper(ctx, book, w, h, env);
  else {
    ctx.save();
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = "source-over";
    ctx.fillStyle = env.palette.paper;
    ctx.fillRect(0, 0, w, h);
    ctx.restore();
  }
  return { animating: false, missing: Array.from(new Set(missing)) };
}
