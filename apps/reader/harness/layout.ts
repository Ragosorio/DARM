/**
 * Visual harness for the layout engine and the painter.
 *
 *   npx vite            (from apps/reader)
 *   http://localhost:5173/harness/layout.html?w=560&h=820&spread=1
 *
 * URL parameters:
 *   w, h        page size in CSS px (default 560 x 820)
 *   spread      1 = two-page spread margins and facing pages side by side
 *   theme       light | dark
 *   font        type size multiplier (0.8 ... 1.6)
 *   lh          compact | normal | relaxed
 *   margins     narrow | normal | wide
 *   justify, hyphenate   0 | 1 (default: follow the book)
 *   state       name:value,...     e.g. route:cliff,lit:true,courage:3
 *   quiz        block:picked,...   e.g. star_quiz:1
 *   choice      block:index,...    e.g. fork:0
 *   cover       classic | band | minimal | modern (override the front style)
 *   title       override the cover title (to test auto-fit)
 *   time        ms into the entrance animations (default: final state)
 *   ir          URL of another IR JSON (default: the kitchen-sink fixture)
 *   perf        number of words of a synthetic book to time (e.g. 7000)
 */
import { initialState } from "../src/core/state";
import type { Book, Color, State, Value } from "../src/ir";
import { COMFORT_STRETCH, computeGeometry, layoutBook, LOOSE_SPACE_EM, MAX_SPACE_EM, resetTypesetStats, typesetStats } from "../src/layout/index";
import type { BookLayout, ImageProvider, LayoutInput, PageLayout, ReaderSettings, ResolvedPalette, Surface, UiRecords } from "../src/layout/types";
import { loadFonts } from "../src/paint/fonts";
import { loadHyphenator } from "../src/paint/hyphenate";
import { paintPage, paintSurface } from "../src/paint/index";
import { createCanvasMeasurer } from "../src/paint/measure";

const FIXTURE = "/tests/fixtures/kitchen-sink.ir.json";
const FIXTURE_FILES: Record<string, string> = {
  pic: "/tests/fixtures/kitchen-sink/art/pic.png",
  map: "/tests/fixtures/kitchen-sink/art/map.svg",
  clip__poster: "/tests/fixtures/kitchen-sink/art/pic.png",
};

const q = new URLSearchParams(location.search);
const num = (key: string, fallback: number): number => {
  const v = Number(q.get(key));
  return q.has(key) && Number.isFinite(v) ? v : fallback;
};
const flag = (key: string): boolean | null => (q.has(key) ? q.get(key) === "1" || q.get(key) === "true" : null);

function mix(a: Color, b: Color, t: number): Color {
  const p = (c: Color): number[] => [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16));
  const [x, y] = [p(a), p(b)];
  return `#${x.map((v, i) => Math.round(v + (y[i]! - v) * t).toString(16).padStart(2, "0")).join("")}`;
}

/** Same derivation as the app's resolvePalette (src/ui/color.ts), without its contrast guard. */
function palette(book: Book, dark: boolean): ResolvedPalette {
  const t = book.theme;
  const src = dark ? (t.dark ?? { paper: "#191715", ink: "#e8e1d3", accent: mix(t.accent, "#ffffff", 0.45) }) : t;
  return { dark, paper: src.paper, ink: src.ink, accent: src.accent, muted: mix(src.ink, src.paper, 0.42), rule: mix(src.ink, src.paper, 0.8), tint: mix(src.accent, src.paper, 0.9) };
}

function parseState(book: Book, spec: string | null): State {
  const state = initialState(book);
  if (!spec) return state;
  for (const part of spec.split(",")) {
    const [name, raw] = part.split(":");
    const decl = book.state.find((v) => v.name === name);
    if (!decl || raw === undefined || name === undefined) continue;
    let v: Value = raw;
    if (decl.type === "int") v = Number(raw);
    else if (decl.type === "bool") v = raw === "true" || raw === "1";
    state[name] = v;
  }
  return state;
}

function parseUi(): UiRecords {
  const ui: UiRecords = { quiz: {}, choice: {} };
  for (const part of (q.get("quiz") ?? "").split(",")) {
    const [id, picked] = part.split(":");
    if (id && picked !== undefined) ui.quiz[id] = { picked: Number(picked), correct: false };
  }
  for (const part of (q.get("choice") ?? "").split(",")) {
    const [id, index] = part.split(":");
    if (id && index !== undefined) ui.choice[id] = Number(index);
  }
  return ui;
}

async function loadImages(book: Book): Promise<ImageProvider> {
  const bitmaps = new Map<string, HTMLImageElement>();
  await Promise.all(
    book.resources
      .filter((r) => r.kind === "image" && FIXTURE_FILES[r.name] !== undefined)
      .map(async (r) => {
        const img = new Image();
        img.src = FIXTURE_FILES[r.name]!;
        try {
          await img.decode();
          bitmaps.set(r.name, img);
        } catch {
          /* stays missing: the painter shows its placeholder */
        }
      }),
  );
  return {
    get: (name) => bitmaps.get(name) ?? null,
    size: (name) => {
      const r = book.resources.find((x) => x.name === name);
      return r && r.width && r.height ? { width: r.width, height: r.height } : null;
    },
  };
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

function canvasFor(w: number, h: number, id: string): { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D } {
  const dpr = window.devicePixelRatio || 1;
  const canvas = el("canvas");
  canvas.id = id;
  canvas.width = Math.round(w * dpr);
  canvas.height = Math.round(h * dpr);
  canvas.style.width = `${w}px`;
  canvas.style.height = `${h}px`;
  const ctx = canvas.getContext("2d")!;
  ctx.scale(dpr, dpr);
  return { canvas, ctx };
}

/** A synthetic novel of about `words` words, built from the fixture's prose, for timing. */
function syntheticBook(base: Book, words: number): Book {
  const prose: string[] = [];
  const collect = (v: unknown): void => {
    if (Array.isArray(v)) v.forEach(collect);
    else if (v && typeof v === "object") {
      const o = v as Record<string, unknown>;
      if (o.type === "paragraph" && Array.isArray(o.spans)) prose.push((o.spans as { text?: string }[]).map((s) => s.text ?? "").join(""));
      Object.values(o).forEach(collect);
    }
  };
  collect(base.chapters);
  const long = prose.filter((p) => p.split(" ").length > 30);
  const chapters = base.chapters.filter((c) => c.kind !== "chapter");
  let count = 0;
  let n = 0;
  for (let c = 0; count < words; c++) {
    const blocks = [];
    for (let p = 0; p < 24 && count < words; p++) {
      const text = long[(n++ * 7) % long.length]!;
      count += text.split(" ").length;
      blocks.push({ id: `p${c}_${p}`, type: "paragraph" as const, style: p === 0 ? ("dropcap" as const) : ("body" as const), spans: [{ text }] });
    }
    chapters.push({ id: `c${c}`, kind: "chapter", title: `Chapter ${c + 1}`, number: c + 1, opener: true, blocks });
  }
  return { ...base, chapters, toc: chapters.filter((c) => c.kind === "chapter").map((c) => ({ title: c.title, target: c.id, level: 1 as const })), anchors: {} };
}

function stretchStats(layout: BookLayout): string {
  let items = 0;
  for (const p of layout.pages) items += p.items.length;
  const t = typesetStats;
  const share = (n: number): string => `${((100 * n) / Math.max(1, t.justified + t.ragged)).toFixed(1)}%`;
  return (
    `${layout.pages.length} pages, ${items} draw items\n` +
    `lines: ${t.lines} set, ${t.justified} justified (mean ${((100 * t.stretchSum) / Math.max(1, t.justified)).toFixed(0)}% of the hard limit, max ${(100 * t.stretchMax).toFixed(0)}%), ` +
    `${t.ragged} left ragged (${share(t.ragged)}), ${t.hyphens} hyphenated; spaces: aim ${COMFORT_STRETCH}x, tolerance ${MAX_SPACE_EM}em, hard limit ${LOOSE_SPACE_EM}em`
  );
}

async function main(): Promise<void> {
  const info = document.getElementById("info")!;
  const grid = document.getElementById("grid")!;
  const book = (await (await fetch(q.get("ir") ?? FIXTURE)).json()) as Book;
  const style = q.get("cover");
  if (style === "classic" || style === "band" || style === "minimal" || style === "modern") book.cover.front.style = style;
  if (q.has("title")) book.cover.front.title = q.get("title")!;
  if (q.has("coverimage")) {
    book.cover.front.image = q.get("coverimage")!;
    book.cover.front.full_bleed = q.get("bleed") === "1";
    book.cover.back.image = q.get("coverimage")!;
  }
  if (q.has("paragraph")) book.theme.paragraph = q.get("paragraph") === "spaced" ? "spaced" : "indent";

  const dark = q.get("theme") === "dark";
  const pal = palette(book, dark);
  document.body.style.background = dark ? "#0e0d0c" : "#2a2927";
  const [fonts, images, hyphenate] = await Promise.all([
    loadFonts(book, () => Promise.reject(new Error("no embedded fonts in the harness"))),
    loadImages(book),
    loadHyphenator(book.meta.language).catch(() => undefined),
  ]);
  const measurer = createCanvasMeasurer();
  const page = { width: num("w", 560), height: num("h", 820) };
  const spread = q.get("spread") === "1";
  const lh = q.get("lh");
  const margins = q.get("margins");
  const settings: ReaderSettings = {
    fontScale: num("font", 1),
    lineHeight: lh === "compact" || lh === "relaxed" ? lh : "normal",
    margins: margins === "narrow" || margins === "wide" ? margins : "normal",
    justify: flag("justify"),
    hyphenate: flag("hyphenate"),
  };
  const input: LayoutInput = { book, page, spread, settings, palette: pal, fonts, measurer, state: parseState(book, q.get("state")), ui: parseUi(), hyphenate };

  resetTypesetStats();
  const t0 = performance.now();
  const layout = layoutBook(input);
  const cold = performance.now() - t0;
  const stats = stretchStats(layout);
  const warm: number[] = [];
  for (let i = 0; i < 5; i++) {
    const t = performance.now();
    layoutBook(input);
    warm.push(performance.now() - t);
  }
  warm.sort((a, b) => a - b);

  const geo = computeGeometry(input);
  const sample = "The ferry came in sideways, as it always did, shouldering the tide. From the rail the town looked like a sentence somebody had started with confidence and then abandoned";
  const bodyFont = `400 ${geo.body}px ${fonts.body}`;
  const cpl = geo.colW / (measurer.width(bodyFont, sample) / sample.length);

  const lines: string[] = [
    `page ${page.width}x${page.height}  spread=${spread}  theme=${dark ? "dark" : "light"}  font=${settings.fontScale}  lh=${settings.lineHeight}  margins=${settings.margins}  dpr=${window.devicePixelRatio}`,
    `layout: ${cold.toFixed(1)} ms cold, ${warm[2]!.toFixed(1)} ms warm (median of 5)   ${stats}`,
    `word space ${(measurer.width(bodyFont, " ") / geo.body).toFixed(3)}em; body ${geo.body}px / ${geo.L}px, measure ${geo.colW.toFixed(0)}px = ${cpl.toFixed(1)} characters per line; text block ${geo.top}..${geo.bottom} (${Math.round((geo.bottom - geo.top) / geo.L)} lines)`,
    `startPage=${layout.startPage} contentsPage=${layout.contentsPage} chapters=${layout.chapters.map((c) => `${c.id}@${c.firstPage}+${c.pageCount}`).join(" ")}`,
    `toc: ${layout.toc.map((t) => `${t.title}→${t.page}`).join(", ")}`,
  ];

  let perf: { words: number; cold: number; warm: number; pages: number } | null = null;
  if (q.has("perf")) {
    const big = syntheticBook(book, num("perf", 7000));
    const words = JSON.stringify(big.chapters).split(" ").length;
    // A fresh measurer: the cold number includes measuring every word once, as on first open.
    const coldInput = { ...input, book: big, measurer: createCanvasMeasurer() };
    const a = performance.now();
    const res = layoutBook(coldInput);
    const coldMs = performance.now() - a;
    const runs: number[] = [];
    for (let i = 0; i < 7; i++) {
      const t = performance.now();
      layoutBook(coldInput);
      runs.push(performance.now() - t);
    }
    runs.sort((x, y) => x - y);
    perf = { words, cold: coldMs, warm: runs[3]!, pages: res.pages.length };
    lines.push(`perf: ~${words} words → ${res.pages.length} pages: ${coldMs.toFixed(1)} ms cold (fresh measurer), ${runs[3]!.toFixed(1)} ms warm (median of 7)`);
  }

  const time = q.has("time") ? num("time", 0) : undefined;
  const env = { palette: pal, fonts, images, time };
  const missing = new Set<string>();

  const cell = (id: string, label: string, paint: (ctx: CanvasRenderingContext2D) => void): HTMLElement => {
    const c = el("div", "cell");
    const { canvas, ctx } = canvasFor(page.width, page.height, id);
    paint(ctx);
    c.append(canvas, el("div", "label", label));
    return c;
  };
  const pageCell = (p: PageLayout): HTMLElement =>
    cell(`page-${p.index}`, `${p.index} ${p.chapterId}${p.fixed ? " [fixed]" : ""}${p.animated ? " [anim]" : ""} · ${p.blocks.join(" ")}`, (ctx) => {
      for (const m of paintPage(ctx, p, page, env).missing) missing.add(m);
    });

  const surfaces = el("div", "row single");
  for (const s of ["front-cover", "endpaper", "blank", "back-cover"] as Surface[]) {
    surfaces.append(
      cell(`surface-${s}`, s, (ctx) => {
        for (const m of paintSurface(ctx, s, book, page, env).missing) missing.add(m);
      }),
    );
  }
  grid.append(surfaces);

  if (spread) {
    // Facing pages side by side: page 0 is a right-hand page.
    for (let i = -1; i < layout.pages.length; i += 2) {
      const row = el("div", "row");
      const left = layout.pages[i];
      const right = layout.pages[i + 1];
      if (left) row.append(pageCell(left));
      else {
        const pad = el("div", "cell");
        pad.style.width = `${page.width}px`;
        row.append(pad);
      }
      if (right) row.append(pageCell(right));
      grid.append(row);
    }
  } else {
    const row = el("div", "row single");
    for (const p of layout.pages) row.append(pageCell(p));
    grid.append(row);
  }

  lines.push(`missing images: ${missing.size ? Array.from(missing).join(", ") : "none"}`);
  for (const p of layout.pages) {
    lines.push(`  p${p.index} ${p.chapterId}${p.fixed ? " fixed" : ""} items=${p.items.length} regions=[${p.regions.map((r) => r.type).join(",")}] anchor=${p.anchor.block ?? "-"}:${p.anchor.line} blocks=${p.blocks.join(" ")}`);
  }
  info.textContent = lines.join("\n");
  (window as unknown as { __harness: unknown }).__harness = { ready: true, cold, warm: warm[2], pages: layout.pages.length, cpl, perf, info: lines };
}

main().catch((err: unknown) => {
  document.getElementById("info")!.textContent = `harness failed: ${err instanceof Error ? (err.stack ?? err.message) : String(err)}`;
  (window as unknown as { __harness: unknown }).__harness = { ready: true, error: String(err) };
});
