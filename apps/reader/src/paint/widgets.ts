/**
 * Controls drawn on the page.
 *
 * `button`, `option` and `answer`: only the control itself is drawn here;
 * their labels are text items laid out by the layout engine and painted
 * right after the widget (see layout/widgetMetrics.ts). `audio`, `video` and
 * `gallery` are drawn whole. The rectangles of the audio and video parts
 * come from `audioWidgetLayout` / `videoWidgetLayout`, which the app uses to
 * align the live DOM players over the painted ones.
 */
import type { Ctx2D, DrawItem, PaintEnv, Widget } from "../layout/types";
import { audioWidgetLayout, buttonRadius, galleryWidgetLayout, rowWidgetLayout, videoWidgetLayout } from "../layout/widgetMetrics";
import { alpha, darken, lighten, mix } from "./color";
import { drawImageFit, roundRectPath } from "./util";

export { audioWidgetLayout, galleryWidgetLayout, rowWidgetLayout, videoWidgetLayout } from "../layout/widgetMetrics";

type WidgetItem = Extract<DrawItem, { kind: "widget" }>;

/** Semantic colours for quiz feedback; always paired with a tick or a cross. */
function feedback(env: PaintEnv): { good: string; bad: string } {
  return env.palette.dark ? { good: "#7ecf9f", bad: "#f0918a" } : { good: "#2c7a4b", bad: "#b23a2e" };
}

function tick(ctx: Ctx2D, cx: number, cy: number, r: number): void {
  ctx.beginPath();
  ctx.moveTo(cx - r * 0.5, cy + r * 0.02);
  ctx.lineTo(cx - r * 0.12, cy + r * 0.4);
  ctx.lineTo(cx + r * 0.52, cy - r * 0.36);
}

function cross(ctx: Ctx2D, cx: number, cy: number, r: number): void {
  const k = r * 0.38;
  ctx.beginPath();
  ctx.moveTo(cx - k, cy - k);
  ctx.lineTo(cx + k, cy + k);
  ctx.moveTo(cx + k, cy - k);
  ctx.lineTo(cx - k, cy + k);
}

function playGlyph(ctx: Ctx2D, cx: number, cy: number, r: number): void {
  // Optically centred: a triangle's centroid sits left of its bounding-box centre.
  const w = r * 0.78;
  const h = r * 0.9;
  const x = cx - w * 0.38;
  ctx.beginPath();
  ctx.moveTo(x, cy - h / 2);
  ctx.lineTo(x + w, cy);
  ctx.lineTo(x, cy + h / 2);
  ctx.closePath();
}

function button(ctx: Ctx2D, it: WidgetItem, env: PaintEnv): void {
  const { x, y, w, h } = it;
  const r = buttonRadius(h);
  const accent = env.palette.accent;
  const lip = Math.max(1, Math.min(2, h * 0.035));
  // The darker lip under the face is what makes it read as a pressed-in, inked shape rather than a flat tag.
  roundRectPath(ctx, x, y, w, h, r);
  ctx.fillStyle = darken(accent, 0.3);
  ctx.fill();
  roundRectPath(ctx, x, y, w, h - lip, r);
  ctx.fillStyle = accent;
  ctx.fill();
  // A whisper of light along the top edge.
  ctx.save();
  roundRectPath(ctx, x, y, w, h - lip, r);
  ctx.clip();
  const g = ctx.createLinearGradient(0, y, 0, y + h);
  g.addColorStop(0, alpha(lighten(accent, 0.5), 0.22));
  g.addColorStop(0.35, alpha(lighten(accent, 0.5), 0));
  g.addColorStop(1, alpha("#000000", 0.06));
  ctx.fillStyle = g;
  ctx.fillRect(x, y, w, h);
  ctx.restore();
}

function option(ctx: Ctx2D, it: WidgetItem, selected: boolean, env: PaintEnv): void {
  const { x, y, w, h } = it;
  const pal = env.palette;
  const lay = rowWidgetLayout(w, h);
  roundRectPath(ctx, x + 0.5, y + 0.5, w - 1, h - 1, lay.radius);
  if (selected) {
    ctx.fillStyle = alpha(pal.accent, pal.dark ? 0.16 : 0.08);
    ctx.fill();
  }
  ctx.lineWidth = selected ? 1.5 : 1;
  ctx.strokeStyle = selected ? pal.accent : mix(pal.rule, pal.muted, 0.35);
  ctx.stroke();

  const m = lay.marker;
  const cx = x + m.x + m.w / 2;
  const cy = y + m.y + m.h / 2;
  const r = m.w / 2;
  ctx.beginPath();
  ctx.arc(cx, cy, r - 1, 0, Math.PI * 2);
  ctx.lineWidth = 1.5;
  ctx.strokeStyle = selected ? pal.accent : pal.muted;
  ctx.stroke();
  if (selected) {
    ctx.beginPath();
    ctx.arc(cx, cy, r * 0.48, 0, Math.PI * 2);
    ctx.fillStyle = pal.accent;
    ctx.fill();
  }
}

function answer(ctx: Ctx2D, it: WidgetItem, state: "idle" | "correct" | "wrong" | "missed" | "dimmed", env: PaintEnv): void {
  const { x, y, w, h } = it;
  const pal = env.palette;
  const fb = feedback(env);
  const lay = rowWidgetLayout(w, h);
  const m = lay.marker;
  const cx = x + m.x + m.w / 2;
  const cy = y + m.y + m.h / 2;
  const r = m.w / 2;

  ctx.save();
  if (state === "dimmed") ctx.globalAlpha *= 0.5;
  const tone = state === "correct" || state === "missed" ? fb.good : state === "wrong" ? fb.bad : null;

  roundRectPath(ctx, x + 0.5, y + 0.5, w - 1, h - 1, lay.radius);
  if (tone && state !== "missed") {
    ctx.fillStyle = alpha(tone, pal.dark ? 0.16 : 0.09);
    ctx.fill();
  }
  ctx.lineWidth = tone ? 1.5 : 1;
  ctx.strokeStyle = tone ?? mix(pal.rule, pal.muted, 0.35);
  // The right answer that was not picked: a dashed outline, distinct in shape from "picked and right".
  if (state === "missed") ctx.setLineDash([5, 4]);
  ctx.stroke();
  ctx.setLineDash([]);

  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  if (state === "correct" || state === "wrong") {
    ctx.beginPath();
    ctx.arc(cx, cy, r, 0, Math.PI * 2);
    ctx.fillStyle = tone!;
    ctx.fill();
    ctx.strokeStyle = pal.paper;
    ctx.lineWidth = Math.max(1.75, r * 0.22);
    if (state === "correct") tick(ctx, cx, cy, r);
    else cross(ctx, cx, cy, r);
    ctx.stroke();
  } else if (state === "missed") {
    ctx.beginPath();
    ctx.arc(cx, cy, r - 1, 0, Math.PI * 2);
    ctx.lineWidth = 1.5;
    ctx.strokeStyle = fb.good;
    ctx.stroke();
    ctx.lineWidth = Math.max(1.5, r * 0.2);
    tick(ctx, cx, cy, r * 0.9);
    ctx.stroke();
  } else {
    ctx.beginPath();
    ctx.arc(cx, cy, r - 1, 0, Math.PI * 2);
    ctx.lineWidth = 1.5;
    ctx.strokeStyle = pal.muted;
    ctx.stroke();
  }
  ctx.restore();
}

function video(ctx: Ctx2D, it: WidgetItem, w: Extract<Widget, { type: "video" }>, env: PaintEnv, missing: string[]): void {
  const lay = videoWidgetLayout(it.w, it.h);
  const { x, y } = it;
  // Dark panel first: it is the letterbox behind a poster of another shape and the stand-in while it loads.
  roundRectPath(ctx, x, y, it.w, it.h, lay.radius);
  const bg = ctx.createLinearGradient(0, y, 0, y + it.h);
  bg.addColorStop(0, "#26262a");
  bg.addColorStop(1, "#141416");
  ctx.fillStyle = bg;
  ctx.fill();
  if (w.poster !== undefined) {
    if (!drawImageFit(ctx, env.images, w.poster, x, y, it.w, it.h, "cover", lay.radius)) missing.push(w.poster);
  }
  const p = lay.play;
  const cx = x + p.x + p.w / 2;
  const cy = y + p.y + p.h / 2;
  const r = p.w / 2;
  ctx.beginPath();
  ctx.arc(cx, cy, r, 0, Math.PI * 2);
  ctx.fillStyle = "rgba(12,12,14,0.5)";
  ctx.fill();
  ctx.lineWidth = Math.max(1.25, r * 0.06);
  ctx.strokeStyle = "rgba(255,255,255,0.85)";
  ctx.stroke();
  playGlyph(ctx, cx, cy, r);
  ctx.fillStyle = "rgba(255,255,255,0.95)";
  ctx.fill();
}

function audio(ctx: Ctx2D, it: WidgetItem, w: Extract<Widget, { type: "audio" }>, env: PaintEnv): void {
  const pal = env.palette;
  const lay = audioWidgetLayout(it.w, it.h);
  const { x, y } = it;
  roundRectPath(ctx, x + 0.5, y + 0.5, it.w - 1, it.h - 1, lay.radius);
  ctx.fillStyle = pal.tint;
  ctx.fill();
  ctx.lineWidth = 1;
  ctx.strokeStyle = pal.rule;
  ctx.stroke();

  const p = lay.play;
  const cx = x + p.x + p.w / 2;
  const cy = y + p.y + p.h / 2;
  ctx.beginPath();
  ctx.arc(cx, cy, p.w / 2, 0, Math.PI * 2);
  ctx.fillStyle = pal.accent;
  ctx.fill();
  playGlyph(ctx, cx, cy, p.w / 2);
  ctx.fillStyle = pal.paper;
  ctx.fill();

  const t = lay.title;
  const size = Math.max(10, Math.min(16, t.h * 0.78));
  ctx.font = `500 ${size}px ${env.fonts.sans}`;
  ctx.fillStyle = pal.ink;
  ctx.textBaseline = "alphabetic";
  let title = w.title;
  if (title && ctx.measureText(title).width > t.w) {
    const cps = Array.from(title);
    while (cps.length > 1 && ctx.measureText(cps.join("") + "\u2026").width > t.w) cps.pop();
    title = cps.join("").trimEnd() + "\u2026";
  }
  if (title) ctx.fillText(title, x + t.x, y + t.y + t.h - size * 0.3);

  // Track: a hairline with the playhead resting at the start.
  const tr = lay.track;
  const ty = Math.round(y + tr.y + tr.h / 2) + 0.5;
  ctx.beginPath();
  ctx.moveTo(x + tr.x + 2, ty);
  ctx.lineTo(x + tr.x + tr.w, ty);
  ctx.lineWidth = 2;
  ctx.lineCap = "round";
  ctx.strokeStyle = mix(pal.rule, pal.muted, 0.4);
  ctx.stroke();
  ctx.lineCap = "butt";
  ctx.beginPath();
  ctx.arc(x + tr.x + 4, ty, 4, 0, Math.PI * 2);
  ctx.fillStyle = pal.accent;
  ctx.fill();

  // Time read-out at rest. The live player prints the running time over the same rectangle.
  const tm = lay.time;
  const ts = Math.max(9, Math.min(12, tm.h * 0.8));
  ctx.font = `500 ${ts}px ${env.fonts.sans}`;
  ctx.fillStyle = pal.muted;
  const label = "0:00";
  ctx.fillText(label, x + tm.x + tm.w - ctx.measureText(label).width, ty + ts * 0.36);
}

function gallery(ctx: Ctx2D, it: WidgetItem, w: Extract<Widget, { type: "gallery" }>, env: PaintEnv, missing: string[]): void {
  const pal = env.palette;
  const lay = galleryWidgetLayout(it.w, it.h);
  const { x, y } = it;
  const im = lay.image;
  const first = w.resources[0];
  roundRectPath(ctx, x + im.x, y + im.y, im.w, im.h, lay.radius);
  ctx.fillStyle = pal.tint;
  ctx.fill();
  if (first !== undefined && !drawImageFit(ctx, env.images, first, x + im.x, y + im.y, im.w, im.h, "cover", lay.radius)) missing.push(first);

  const n = Math.max(1, w.count);
  // Count pill on the picture: says at a glance that there is more than one.
  if (n > 1 && im.w > 80) {
    const label = `1 / ${n}`;
    const size = 11;
    ctx.font = `600 ${size}px ${env.fonts.sans}`;
    const tw = ctx.measureText(label).width;
    const pw = tw + 16;
    const ph = 20;
    const px = x + im.x + im.w - pw - 8;
    const py = y + im.y + 8;
    roundRectPath(ctx, px, py, pw, ph, ph / 2);
    ctx.fillStyle = "rgba(16,16,18,0.6)";
    ctx.fill();
    ctx.fillStyle = "rgba(255,255,255,0.95)";
    ctx.textBaseline = "alphabetic";
    ctx.fillText(label, px + 8, py + ph / 2 + size * 0.36);
  }
  // Position dots in the strip below.
  const ind = lay.indicator;
  const dots = Math.min(n, 12);
  const gap = 12;
  const r = 2.5;
  const cy = y + ind.y + ind.h * 0.62;
  let cx = x + ind.x + ind.w / 2 - ((dots - 1) * gap) / 2;
  for (let i = 0; i < dots; i++) {
    ctx.beginPath();
    ctx.arc(cx, cy, i === 0 ? r + 0.5 : r, 0, Math.PI * 2);
    ctx.fillStyle = i === 0 ? pal.accent : mix(pal.rule, pal.muted, 0.35);
    ctx.fill();
    cx += gap;
  }
}

export function drawWidget(ctx: Ctx2D, it: WidgetItem, env: PaintEnv, missing: string[]): void {
  const w = it.widget;
  ctx.save();
  switch (w.type) {
    case "button":
      button(ctx, it, env);
      break;
    case "option":
      option(ctx, it, w.selected, env);
      break;
    case "answer":
      answer(ctx, it, w.state, env);
      break;
    case "video":
      video(ctx, it, w, env, missing);
      break;
    case "audio":
      audio(ctx, it, w, env);
      break;
    case "gallery":
      gallery(ctx, it, w, env, missing);
      break;
  }
  ctx.restore();
}
