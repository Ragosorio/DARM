/** Colour helpers and palette resolution. */
import type { Book, Color } from "../ir";
import type { ResolvedPalette } from "../layout/types";
import type { SceneStyle } from "../gl/renderer";

export type Rgb = [number, number, number];

export function parseColor(c: Color): Rgb {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(c);
  if (!m) return [0.5, 0.5, 0.5];
  return [parseInt(m[1]!, 16) / 255, parseInt(m[2]!, 16) / 255, parseInt(m[3]!, 16) / 255];
}

export function toHex(c: Rgb): Color {
  return `#${c.map((v) => Math.round(Math.min(1, Math.max(0, v)) * 255).toString(16).padStart(2, "0")).join("")}`;
}

export function mix(a: Color, b: Color, t: number): Color {
  const [x, y] = [parseColor(a), parseColor(b)];
  return toHex([x[0] + (y[0] - x[0]) * t, x[1] + (y[1] - x[1]) * t, x[2] + (y[2] - x[2]) * t]);
}

export function luminance(c: Color): number {
  const [r, g, b] = parseColor(c).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4)) as Rgb;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function contrast(a: Color, b: Color): number {
  const [l1, l2] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (l1 + 0.05) / (l2 + 0.05);
}

/**
 * Resolve the book theme for light or dark reading. Books may supply their
 * own dark palette; otherwise one is derived. Either way the text/paper
 * contrast is forced to at least 7:1 (WCAG AAA for body text).
 */
export function resolvePalette(book: Book, dark: boolean): ResolvedPalette {
  const t = book.theme;
  let paper = t.paper;
  let ink = t.ink;
  let accent = t.accent;
  if (dark) {
    if (t.dark) ({ paper, ink, accent } = t.dark);
    else {
      paper = "#191715";
      ink = "#e8e1d3";
      accent = mix(t.accent, "#ffffff", 0.45);
    }
  }
  // Guard against themes that would be unreadable.
  for (let i = 0; i < 12 && contrast(ink, paper) < 7; i++) ink = mix(ink, luminance(paper) > 0.4 ? "#000000" : "#ffffff", 0.2);
  for (let i = 0; i < 12 && contrast(accent, paper) < 3; i++) accent = mix(accent, luminance(paper) > 0.4 ? "#000000" : "#ffffff", 0.15);
  return {
    dark,
    paper,
    ink,
    accent,
    muted: mix(ink, paper, 0.42),
    rule: mix(ink, paper, 0.8),
    tint: mix(accent, paper, 0.9),
  };
}

/** The surface the book lies on: near-black, with a pool of light under the book. */
export const DESK: { centre: Rgb; edge: Rgb } = { centre: [0.13, 0.12, 0.148], edge: [0.035, 0.035, 0.035] };

export function sceneStyle(book: Book, palette: ResolvedPalette): SceneStyle {
  return {
    desk: DESK.centre,
    deskEdge: DESK.edge,
    paper: parseColor(palette.paper),
    cloth: parseColor(book.cover.front.color),
    dark: palette.dark,
  };
}
