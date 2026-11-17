/**
 * Development harness for the page-turn engine, independent of DARM content:
 * synthetic pages, real renderer, real controller, real gestures.
 *
 * URL parameters:
 *   pages=40          number of pages
 *   mode=auto|single|spread
 *   dark=1            dark style
 *   2d=1              force the Canvas 2D fallback
 *   pos=3             starting position (0 = closed)
 *   freeze=0.45       hold one leaf at this progress (for screenshots)
 *   lambda=2.4        shape family of the frozen leaf (default: throw)
 *   kappa=0.3         corner skew of the frozen leaf
 *   dir=-1            direction of the frozen leaf
 *   fan=4             freeze this many leaves, fanned out
 */
import { MOTION } from "../src/flip/controller";
import { Stage } from "../src/ui/stage";

const q = new URLSearchParams(location.search);
const num = (k: string, d: number) => (q.has(k) ? Number(q.get(k)) : d);
const pages = num("pages", 40);
const dark = q.get("dark") === "1";
const paper = dark ? "#1b1917" : "#f7f3ea";
const ink = dark ? "#e6dfd2" : "#211d18";

const words = "the tide kept its accounts in the old way a line of wrack for every hour it stayed and what it took it entered as a loan from the rail the town looked like a sentence somebody had started with confidence".split(" ");

function paint(key: string, ctx: CanvasRenderingContext2D, w: number, h: number): boolean {
  const cover = key === "front-cover" || key === "back-cover";
  ctx.fillStyle = cover ? "#7a2e22" : key.startsWith("endpaper") ? (dark ? "#3a2622" : "#c9b79a") : paper;
  ctx.fillRect(0, 0, w, h);
  if (cover) {
    ctx.strokeStyle = "#d9b46a";
    ctx.lineWidth = Math.max(1, w * 0.004);
    ctx.strokeRect(w * 0.08, w * 0.08, w * 0.84, h - w * 0.16);
    ctx.strokeRect(w * 0.095, w * 0.095, w * 0.81, h - w * 0.19);
    ctx.fillStyle = "#f4ead5";
    ctx.textAlign = "center";
    ctx.font = `600 ${w * 0.085}px Georgia, serif`;
    ctx.fillText(key === "front-cover" ? "The Flip" : "Back board", w / 2, h * 0.36);
    ctx.font = `italic ${w * 0.04}px Georgia, serif`;
    ctx.fillText(key === "front-cover" ? "a harness for turning pages" : "contraportada", w / 2, h * 0.43);
    ctx.textAlign = "left";
    return true;
  }
  if (key === "blank" || key.startsWith("endpaper")) return true;
  const ghost = key === "ghost";
  const n = ghost ? 0 : Number(key.split(":")[1]);
  const m = w * 0.12;
  const size = w / 30;
  ctx.fillStyle = ink;
  ctx.globalAlpha = ghost ? 0.35 : 1;
  ctx.font = `${size}px Georgia, serif`;
  let y = m * 1.5;
  let seed = n * 7919 + 13;
  const rnd = () => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff), seed / 0x7fffffff);
  if (!ghost) {
    ctx.font = `600 ${size * 1.6}px Georgia, serif`;
    ctx.fillText(`Page ${n + 1}`, m, y);
    y += size * 2.4;
    ctx.font = `${size}px Georgia, serif`;
  }
  while (y < h - m * 1.2) {
    let x = m;
    const end = rnd() < 0.12 ? m + (w - 2 * m) * (0.3 + rnd() * 0.5) : w - m;
    while (x < end - size * 2) {
      const word = words[Math.floor(rnd() * words.length)]!;
      const ww = ctx.measureText(word).width;
      if (x + ww > end) break;
      if (ghost) ctx.fillRect(x, y - size * 0.55, ww, size * 0.5);
      else ctx.fillText(word, x, y);
      x += ww + size * 0.3;
    }
    y += size * 1.5;
  }
  ctx.globalAlpha = 1;
  if (!ghost) {
    ctx.font = `${size * 0.8}px Georgia, serif`;
    ctx.textAlign = "center";
    ctx.fillText(String(n + 1), w / 2, h - m * 0.6);
    ctx.textAlign = "left";
  }
  return true;
}

const hud = document.getElementById("hud")!;
const stage = new Stage({
  container: document.getElementById("stage")!,
  style: dark
    ? { desk: [0.11, 0.105, 0.1], deskEdge: [0.045, 0.043, 0.04], paper: [0.106, 0.098, 0.09], cloth: [0.478, 0.18, 0.133], dark: true }
    : { desk: [0.87, 0.85, 0.81], deskEdge: [0.66, 0.635, 0.6], paper: [0.969, 0.953, 0.918], cloth: [0.478, 0.18, 0.133], dark: false },
  aspect: { preferred: 0.68, fixed: false },
  mode: (q.get("mode") as "auto" | "single" | "spread") ?? "auto",
  reducedMotion: q.get("reduced") === "1",
  forceFallback: q.get("2d") === "1",
  host: {
    paint,
    onGeometry: () => (stage.setPages(pages, num("pos", 0) > 0 ? (stage.geometry.spread ? Math.max(0, (num("pos", 0) - 1) * 2 - 1) : num("pos", 0) - 1) : null)),
    onPosition: (pos, settled) => {
      hud.textContent = `pos ${pos}/${stage.controller.count}  ${settled ? "rest" : "turning"}  ${stage.webgl ? "webgl2" : "canvas2d"}  ${stage.geometry.spread ? "spread" : "single"}`;
    },
    onTap: (t) => {
      if (stage.controller.pos === 0 || t.side === "right" ? t.x > stage.geometry.width * 0.4 || stage.controller.pos === 0 : false) stage.next();
      else stage.prev();
    },
  },
});

window.addEventListener("keydown", (e: KeyboardEvent) => {
  if (e.key === "ArrowRight" || e.key === " ") stage.next();
  else if (e.key === "ArrowLeft") stage.prev();
  else if (e.key === "End") stage.goToPos(stage.controller.count);
  else if (e.key === "Home") stage.goToPos(0);
  else if (e.key === "j") stage.goToPage(Math.floor(pages * 0.7));
});

stage.start();

// Frozen poses for screenshots.
if (q.has("freeze")) {
  queueMicrotask(() => {
    const c = stage.controller;
    const p = num("freeze", 0.5);
    const fan = num("fan", 1);
    const dir = num("dir", 1) as 1 | -1;
    for (let i = 0; i < fan; i++) {
      if (dir > 0) c.next();
      else c.prev();
    }
    c.flyers.forEach((f, i) => {
      const k = dir > 0 ? i : c.flyers.length - 1 - i;
      f.p = Math.max(0, Math.min(1, p - k * 0.17 * dir * (dir > 0 ? 1 : -1)));
      f.lambda = f.lambdaTarget = f.board ? 0 : num("lambda", MOTION.lambdaThrow);
      f.kappa = f.kappaTarget = num("kappa", 0);
      f.vGrab = num("vgrab", 0.5);
      f.held = true;
      f.heldP = f.p;
    });
    stage.renderNow();
  });
}
Object.assign(window, { stage });
