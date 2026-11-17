/**
 * The painter against a recording stub of the 2D context: every item is
 * drawn, missing images are reported, entrance animations run and end.
 */
import { describe, expect, it } from "vitest";
import { initialState } from "../src/core/state";
import { layoutBook } from "../src/layout";
import type { Ctx2D, ImageProvider, LayoutApi, PaintApi, PaintEnv, Surface } from "../src/layout/types";
import { audioWidgetLayout, galleryWidgetLayout, motionAt, paintPage, paintSurface, rowWidgetLayout, videoWidgetLayout } from "../src/paint";
import { bookOf, fonts, loadKitchenSink, makeInput, palette } from "./layout.helpers";

interface Call {
  name: string;
  args: unknown[];
  alpha: number;
  fill: unknown;
  font: unknown;
}

/** A 2D context that records calls and tracks the properties the painter sets (with save/restore). */
function recorder(): { ctx: Ctx2D; calls: Call[]; named(name: string): Call[] } {
  const calls: Call[] = [];
  let state: Record<string, unknown> = { globalAlpha: 1, fillStyle: "#000000", strokeStyle: "#000000", font: "10px sans-serif", lineWidth: 1, letterSpacing: "0px" };
  const stack: Record<string, unknown>[] = [];
  const gradient = { addColorStop(): void {} };
  const target = {};
  const ctx = new Proxy(target, {
    get(_t, prop: string) {
      if (prop in state) return state[prop];
      return (...args: unknown[]): unknown => {
        calls.push({ name: prop, args, alpha: state.globalAlpha as number, fill: state.fillStyle, font: state.font });
        if (prop === "save") stack.push({ ...state });
        else if (prop === "restore") state = stack.pop() ?? state;
        else if (prop === "measureText") return { width: String(args[0]).length * 6, actualBoundingBoxAscent: 7, fontBoundingBoxAscent: 9, fontBoundingBoxDescent: 3 };
        else if (prop === "createLinearGradient" || prop === "createRadialGradient") return gradient;
        return undefined;
      };
    },
    set(_t, prop: string, value: unknown) {
      state[prop] = value;
      return true;
    },
    has(_t, prop: string) {
      return prop in state;
    },
  }) as unknown as Ctx2D;
  return { ctx, calls, named: (name) => calls.filter((c) => c.name === name) };
}

// The modules implement the contract exactly as typed in layout/types.ts.
const layoutApi: LayoutApi = { layoutBook };
const paintApi: PaintApi = { paintPage, paintSurface };
void layoutApi;
void paintApi;

const book = loadKitchenSink();
const size = { width: 560, height: 820 };

function images(ready: boolean): ImageProvider & { asked: string[] } {
  const asked: string[] = [];
  return {
    asked,
    get: (name) => {
      asked.push(name);
      return ready ? ({ width: 10, height: 10 } as unknown as CanvasImageSource) : null;
    },
    size: (name) => {
      const r = book.resources.find((x) => x.name === name);
      return r && r.width && r.height ? { width: r.width, height: r.height } : null;
    },
  };
}

function env(over: Partial<PaintEnv> = {}): PaintEnv {
  return { palette, fonts, images: images(true), ...over };
}

const layout = layoutBook(makeInput(book, { state: { ...initialState(book), route: "cliff" }, ui: { quiz: { star_quiz: { picked: 1, correct: false } }, choice: { fork: 0 } } }));

describe("paintPage", () => {
  it("fills the page and draws every item of every page", () => {
    for (const page of layout.pages) {
      const r = recorder();
      const result = paintPage(r.ctx, page, size, env());
      expect(result).toEqual({ animating: false, missing: [] });

      // Paper first, over the whole page.
      const first = r.named("fillRect")[0]!;
      expect(first.args).toEqual([0, 0, 560, 820]);
      expect(first.fill).toBe(page.background ?? palette.paper);

      const drawnText = r.named("fillText");
      for (const it of page.items) {
        if (it.kind === "text") {
          const hit = drawnText.find((c) => c.args[0] === it.text && c.args[1] === it.x && c.args[2] === it.y && c.font === it.font && c.fill === it.color);
          expect(hit, `page ${page.index}: text ${JSON.stringify(it.text)}`).toBeDefined();
          if (it.opacity !== undefined) expect(hit!.alpha).toBeCloseTo(it.opacity, 6);
        }
      }
      const count = (kind: string): number => page.items.filter((it) => it.kind === kind).length;
      const widgets = page.items.flatMap((it) => (it.kind === "widget" ? [it.widget] : []));
      // Images: plain image items, video posters and the first picture of a gallery.
      const expectedImages = count("image") + widgets.filter((w) => (w.type === "video" && w.poster !== undefined) || w.type === "gallery").length;
      expect(r.named("drawImage").length, `page ${page.index}: images`).toBe(expectedImages);
      expect(r.named("moveTo").length).toBeGreaterThanOrEqual(count("line") + count("ornament"));
      expect(r.named("fill").length + r.named("stroke").length).toBeGreaterThanOrEqual(count("rect") + count("line") + count("ornament") + widgets.length);
      // Balanced save/restore: the caller's context state is left as it was.
      expect(r.named("save").length).toBe(r.named("restore").length);
    }
  });

  it("draws a placeholder and reports resources that are not decoded yet", () => {
    const withImages = layout.pages.filter((p) => p.items.some((it) => it.kind === "image" || (it.kind === "widget" && (it.widget.type === "gallery" || it.widget.type === "video"))));
    expect(withImages.length).toBeGreaterThanOrEqual(4);
    const all = new Set<string>();
    for (const page of withImages) {
      const r = recorder();
      const result = paintPage(r.ctx, page, size, env({ images: images(false) }));
      expect(result.missing.length).toBeGreaterThan(0);
      expect(new Set(result.missing).size).toBe(result.missing.length);
      expect(r.named("drawImage").length).toBe(0);
      for (const m of result.missing) all.add(m);
    }
    expect(Array.from(all).sort()).toEqual(["clip__poster", "map", "pic"]);
  });

  it("runs entrance animations from `time` and reports when they are over", () => {
    const page = layout.pages.find((p) => p.animated)!;
    expect(page.chapterId).toBe("cliff");
    const animatedText = page.items.filter((it) => it.kind === "text" && it.anim !== undefined);
    expect(animatedText.length).toBeGreaterThan(0);
    const anim = animatedText[0]!.anim!;
    expect(anim).toEqual({ kind: "rise", delay_ms: 200, duration_ms: 800 });

    const at = (time: number | undefined) => {
      const r = recorder();
      const result = paintPage(r.ctx, page, size, env({ time }));
      const first = animatedText[0]!;
      const call = r.named("fillText").find((c) => first.kind === "text" && c.args[0] === first.text);
      const shifts = r.named("translate").map((c) => c.args[1] as number);
      return { result, alpha: call?.alpha, shift: shifts.length ? Math.max(...shifts) : 0 };
    };
    // Before the delay has passed the block is invisible (and the animation pending).
    expect(at(0).result.animating).toBe(true);
    expect(at(0).alpha).toBeUndefined();
    expect(at(199).result.animating).toBe(true);
    // Half-way: partly opaque and still below its resting place; cubic ease-out is past the midpoint.
    const mid = at(600);
    expect(mid.result.animating).toBe(true);
    expect(mid.alpha).toBeCloseTo(0.875, 6);
    expect(mid.shift).toBeCloseTo(14 * 0.125, 6);
    // Finished, and the same as painting without a clock.
    for (const t of [1000, 5000, undefined]) {
      const done = at(t);
      expect(done.result.animating).toBe(false);
      expect(done.alpha).toBe(1);
      expect(done.shift).toBe(0);
    }
    // Pages without animated items never report animating.
    const still = layout.pages.find((p) => !p.animated && p.chapterKind === "chapter")!;
    expect(paintPage(recorder().ctx, still, size, env({ time: 0 })).animating).toBe(false);
  });

  it("maps every animation kind to its motion", () => {
    const m = (kind: Parameters<typeof motionAt>[0]["kind"], t: number) => motionAt({ kind, delay_ms: 0, duration_ms: 1000 }, t);
    expect(m("fade", 0)).toMatchObject({ alpha: 0, dx: 0, dy: 0, scale: 1, running: true });
    expect(m("rise", 0).dy).toBe(14);
    expect(m("drop", 0).dy).toBe(-14);
    expect(m("slide_left", 0).dx).toBe(24);
    expect(m("slide_right", 0).dx).toBe(-24);
    expect(m("zoom", 0).scale).toBeCloseTo(0.94, 6);
    for (const kind of ["fade", "rise", "drop", "slide_left", "slide_right", "zoom"] as const) {
      expect(m(kind, 1000)).toEqual({ alpha: 1, dx: 0, dy: 0, scale: 1, running: false });
      // Ease-out: more than half-way there at half time.
      expect(m(kind, 500).alpha).toBeGreaterThan(0.5);
    }
    // A zoom pivots on the middle of its block.
    const zoomBook = bookOf(book, [{ id: "z", type: "image", resource: "pic", width: 0.5, fit: "contain", anim: { kind: "zoom", delay_ms: 0, duration_ms: 400 } }]);
    const zl = layoutBook(makeInput(zoomBook));
    const page = zl.pages[zl.blockPages.get("z")!]!;
    const img = page.items.find((it) => it.kind === "image")!;
    const r = recorder();
    expect(paintPage(r.ctx, page, size, env({ time: 100 })).animating).toBe(true);
    const pivot = r.named("translate")[0]!.args as number[];
    expect(img.kind === "image" && pivot[0]).toBeCloseTo(img.kind === "image" ? img.x + img.w / 2 : 0, 6);
    expect(r.named("scale").length).toBe(1);
  });

  it("draws widgets in every state without touching anything outside their box", () => {
    const widgetPages = layout.pages.filter((p) => p.items.some((it) => it.kind === "widget"));
    const seen = new Set<string>();
    for (const page of widgetPages) {
      for (const it of page.items) {
        if (it.kind !== "widget") continue;
        seen.add(it.widget.type === "answer" ? `answer:${it.widget.state}` : it.widget.type === "option" ? `option:${it.widget.selected}` : it.widget.type);
        const r = recorder();
        paintPage(r.ctx, { ...page, items: [it] }, size, env());
        for (const c of r.calls) {
          if (c.name !== "arc" && c.name !== "moveTo" && c.name !== "lineTo") continue;
          const [x, y] = c.args as number[];
          const pad = c.name === "arc" ? (c.args[2] as number) : 0;
          expect(x! + pad).toBeLessThanOrEqual(it.x + it.w + 1);
          expect(x! - pad).toBeGreaterThanOrEqual(it.x - 1);
          expect(y! + pad).toBeLessThanOrEqual(it.y + it.h + 1);
          expect(y! - pad).toBeGreaterThanOrEqual(it.y - 1);
        }
      }
    }
    expect(Array.from(seen).sort()).toEqual(["answer:dimmed", "answer:missed", "answer:wrong", "audio", "button", "gallery", "option:false", "option:true", "video"]);
  });
});

describe("widget part rectangles", () => {
  it("stay inside the widget and do not overlap", () => {
    for (const [w, h] of [
      [390, 56],
      [220, 56],
      [431, 80],
      [600, 120],
    ] as const) {
      const a = audioWidgetLayout(w, h);
      const parts = [a.play, a.title, a.track, a.time];
      for (const r of parts) {
        expect(r.x).toBeGreaterThanOrEqual(0);
        expect(r.y).toBeGreaterThanOrEqual(0);
        expect(r.x + r.w).toBeLessThanOrEqual(w + 0.01);
        expect(r.y + r.h).toBeLessThanOrEqual(h + 0.01);
      }
      expect(a.title.x).toBeGreaterThanOrEqual(a.play.x + a.play.w);
      expect(a.track.y).toBeGreaterThanOrEqual(a.title.y + a.title.h);
      expect(a.time.x).toBeGreaterThanOrEqual(a.track.x + a.track.w);
    }
    const v = videoWidgetLayout(431, 242);
    expect(v.frame).toEqual({ x: 0, y: 0, w: 431, h: 242 });
    expect(v.play.x + v.play.w / 2).toBeCloseTo(431 / 2, 6);
    expect(v.play.y + v.play.h / 2).toBeCloseTo(121, 6);
    const g = galleryWidgetLayout(431, 300);
    expect(g.image.h + g.indicator.h).toBeCloseTo(300, 6);
    const row = rowWidgetLayout(431, 44);
    expect(row.textX).toBeGreaterThan(row.marker.x + row.marker.w);
    expect(row.textX + row.textW).toBeLessThanOrEqual(431);
  });
});

describe("paintSurface", () => {
  const surfaces: Surface[] = ["blank", "endpaper", "front-cover", "back-cover"];

  it("fills the whole surface and never animates", () => {
    for (const s of surfaces) {
      const r = recorder();
      const result = paintSurface(r.ctx, s, book, size, env());
      expect(result).toEqual({ animating: false, missing: [] });
      expect(r.named("fillRect")[0]!.args).toEqual([0, 0, 560, 820]);
      expect(r.named("save").length).toBe(r.named("restore").length);
    }
    const blank = recorder();
    paintSurface(blank.ctx, "blank", book, size, env());
    expect(blank.named("fillRect")[0]!.fill).toBe(palette.paper);
  });

  it("keeps the cloth colour of the covers in dark mode, and follows the palette for blank pages", () => {
    const dark = { ...palette, dark: true, paper: "#181614", ink: "#e8e1d3" };
    const front = recorder();
    paintSurface(front.ctx, "front-cover", book, size, env({ palette: dark }));
    expect(front.named("fillRect")[0]!.fill).toBe(book.cover.front.color);
    const blank = recorder();
    paintSurface(blank.ctx, "blank", book, size, env({ palette: dark }));
    expect(blank.named("fillRect")[0]!.fill).toBe("#181614");
  });

  it("letters the front cover in every style and omits lettering when asked", () => {
    for (const style of ["classic", "band", "minimal", "modern"] as const) {
      const b = { ...book, cover: { ...book.cover, front: { ...book.cover.front, style } } };
      const r = recorder();
      paintSurface(r.ctx, "front-cover", b, size, env());
      const text = r.named("fillText").map((c) => String(c.args[0])).join(" ").toLowerCase();
      for (const word of ["kitchen", "sink", "reyes", "okafor", "everything"]) expect(text, `${style}: ${word}`).toContain(word);

      const silent = { ...b, cover: { ...b.cover, front: { ...b.cover.front, show_text: false, image: "pic" } } };
      const q = recorder();
      expect(paintSurface(q.ctx, "front-cover", silent, size, env()).missing).toEqual([]);
      expect(q.named("fillText").length).toBe(0);
      expect(q.named("drawImage").length).toBe(1);
    }
  });

  it("is deterministic: the cloth is seeded from the book id", () => {
    const strokes = (id: string): string => {
      const r = recorder();
      paintSurface(r.ctx, "front-cover", { ...book, meta: { ...book.meta, id } }, size, env());
      // Thread positions are regular; the slubs (short thick threads) are where the seed shows.
      return JSON.stringify(r.named("lineTo").map((c) => c.args));
    };
    expect(strokes("a")).toBe(strokes("a"));
    expect(strokes("a")).not.toBe(strokes("b"));
  });

  it("fits a very long title inside the board and reports a cover image that is not ready", () => {
    const long = "An Unreasonably Long Title for a Short Book About Harbours, Tides, Lighthouses and the Incomprehensibly-Hyphenated-Compound-Word";
    for (const style of ["classic", "band", "minimal", "modern"] as const) {
      const b = { ...book, cover: { ...book.cover, front: { ...book.cover.front, style, title: long, image: "pic" } } };
      const r = recorder();
      const result = paintSurface(r.ctx, "front-cover", b, size, env({ images: images(false) }));
      expect(result.missing).toEqual(["pic"]);
      // Each line is either narrow enough or drawn under a horizontal squeeze that makes it so.
      let scale = 1;
      let origin = 0;
      for (const c of r.calls) {
        if (c.name === "save") {
          scale = 1;
          origin = 0;
        } else if (c.name === "translate") origin = c.args[0] as number;
        else if (c.name === "scale") scale = c.args[0] as number;
        else if (c.name === "fillText") {
          const w = String(c.args[0]).length * 6 * scale;
          expect(origin + (c.args[1] as number) + w, `${style}: ${String(c.args[0])}`).toBeLessThanOrEqual(560 + 0.5);
          expect(origin + (c.args[1] as number)).toBeGreaterThanOrEqual(-0.5);
        }
      }
    }
  });

  it("sets the back-cover blurb with its bold and italic runs, and the imprint", () => {
    const r = recorder();
    paintSurface(r.ctx, "back-cover", book, size, env());
    const drawn = r.named("fillText");
    const text = drawn.map((c) => String(c.args[0])).join(" ");
    expect(text).toContain("One of everything.");
    expect(text).toContain("contraportada");
    expect(text).toContain("THE KITCHEN SINK");
    expect(drawn.find((c) => String(c.args[0]).includes("contraportada"))!.font).toMatch(/^italic /);
  });
});
