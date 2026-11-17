/**
 * Layout engine invariants on the kitchen-sink fixture and on synthetic
 * books, with a deterministic fake measurer (no canvas, no DOM).
 */
import { describe, expect, it } from "vitest";
import { initialState } from "../src/core/state";
import type { Block } from "../src/ir";
import { computeGeometry, layoutBook } from "../src/layout";
import type { BookLayout, DrawItem, LayoutInput, PageLayout } from "../src/layout/types";
import { bookOf, fakeMeasurer, linesOfBlock, loadKitchenSink, makeInput, novel, settings, sizeOfFont, textWidth, visibleBlocks, type TextItem } from "./layout.helpers";

const book = loadKitchenSink();
const EPS = 0.75;

function bounds(it: DrawItem): { x0: number; y0: number; x1: number; y1: number } {
  if (it.kind === "line") return { x0: Math.min(it.x1, it.x2), y0: Math.min(it.y1, it.y2), x1: Math.max(it.x1, it.x2), y1: Math.max(it.y1, it.y2) };
  if (it.kind === "text") {
    const size = sizeOfFont(it.font);
    return { x0: it.x, y0: it.y - size * 0.72, x1: it.x + textWidth(it), y1: it.y + size * 0.12 };
  }
  return { x0: it.x, y0: it.y, x1: it.x + it.w, y1: it.y + it.h };
}

function expectInsidePage(layout: BookLayout, input: LayoutInput): void {
  const { width, height } = input.page;
  for (const p of layout.pages) {
    if (p.fixed) continue; // fixed pages may bleed
    for (const it of p.items) {
      const b = bounds(it);
      const where = `page ${p.index} ${it.kind} ${it.kind === "text" ? JSON.stringify(it.text) : ""}`;
      expect(b.x0, where).toBeGreaterThanOrEqual(-EPS);
      expect(b.y0, where).toBeGreaterThanOrEqual(-EPS);
      expect(b.x1, where).toBeLessThanOrEqual(width + EPS);
      expect(b.y1, where).toBeLessThanOrEqual(height + EPS);
    }
    for (const r of p.regions) {
      expect(r.x).toBeGreaterThanOrEqual(-EPS);
      expect(r.x + r.w).toBeLessThanOrEqual(width + EPS);
      expect(r.y + r.h).toBeLessThanOrEqual(height + EPS);
    }
  }
}

function expectNoTextCollisions(page: PageLayout): void {
  const texts = page.items.filter((it): it is TextItem => it.kind === "text" && it.text.trim() !== "").map((it) => ({ it, b: bounds(it) }));
  for (let i = 0; i < texts.length; i++) {
    for (let j = i + 1; j < texts.length; j++) {
      const a = texts[i]!;
      const c = texts[j]!;
      const overlapX = Math.min(a.b.x1, c.b.x1) - Math.max(a.b.x0, c.b.x0);
      const overlapY = Math.min(a.b.y1, c.b.y1) - Math.max(a.b.y0, c.b.y0);
      if (overlapX > 0.5 && overlapY > 0.5) {
        throw new Error(`page ${page.index}: ${JSON.stringify(a.it.text)} overlaps ${JSON.stringify(c.it.text)}`);
      }
    }
  }
}

describe.each([
  ["desktop spread", { page: { width: 560, height: 820 }, spread: true }],
  ["phone single", { page: { width: 360, height: 640 }, spread: false }],
  ["large relaxed type", { page: { width: 560, height: 820 }, spread: true, settings: { ...settings, fontScale: 1.5, lineHeight: "relaxed" as const, margins: "wide" as const } }],
  ["compact narrow", { page: { width: 700, height: 900 }, spread: false, settings: { ...settings, fontScale: 0.8, lineHeight: "compact" as const, margins: "narrow" as const } }],
])("kitchen sink, %s", (_name, over) => {
  const input = makeInput(book, over);
  const layout = layoutBook(input);

  it("numbers pages without gaps", () => {
    expect(layout.pages.length).toBeGreaterThan(5);
    layout.pages.forEach((p, i) => expect(p.index).toBe(i));
  });

  it("keeps every draw item and region inside the page box", () => {
    expectInsidePage(layout, input);
  });

  it("never sets one piece of text over another", () => {
    for (const p of layout.pages) expectNoTextCollisions(p);
  });

  it("keeps regions apart", () => {
    for (const p of layout.pages) {
      for (let i = 0; i < p.regions.length; i++) {
        for (let j = i + 1; j < p.regions.length; j++) {
          const a = p.regions[i]!;
          const b = p.regions[j]!;
          const overlap = a.x < b.x + b.w - 0.01 && b.x < a.x + a.w - 0.01 && a.y < b.y + b.h - 0.01 && b.y < a.y + a.h - 0.01;
          expect(overlap, `page ${p.index}: ${a.type} over ${b.type}`).toBe(false);
        }
      }
    }
  });

  it("gives every visible block exactly one starting page", () => {
    const blocks = visibleBlocks(book, input.state);
    expect(blocks.length).toBeGreaterThan(40);
    for (const { block } of blocks) {
      const start = layout.blockPages.get(block.id);
      expect(start, `block ${block.id} (${block.type})`).toBeDefined();
      expect(layout.pages[start!]!.blocks).toContain(block.id);
      // It is the first page that lists the block.
      for (let i = 0; i < start!; i++) expect(layout.pages[i]!.blocks).not.toContain(block.id);
    }
    // Blocks hidden by a false condition are not in the book.
    for (const hidden of ["verdict_high", "verdict_lit", "plate"]) expect(layout.blockPages.has(hidden)).toBe(false);
  });

  it("resolves every visible anchor and every chapter id", () => {
    const visible = new Set(visibleBlocks(book, input.state).map((v) => v.block.id));
    for (const [name, a] of Object.entries(book.anchors)) {
      const chapter = layout.chapters.find((c) => c.id === a.chapter);
      const shown = chapter !== undefined && (a.block === undefined || visible.has(a.block));
      expect(layout.anchors.has(name), name).toBe(shown);
      if (shown) {
        const p = layout.anchors.get(name)!;
        expect(p).toBeGreaterThanOrEqual(chapter!.firstPage);
        expect(p).toBeLessThan(chapter!.firstPage + chapter!.pageCount);
      }
    }
    for (const c of layout.chapters) expect(layout.anchors.get(c.id)).toBe(c.firstPage);
  });

  it("reports chapters, start page and contents consistently", () => {
    let next = 0;
    for (const c of layout.chapters) {
      expect(c.firstPage).toBe(next);
      expect(c.pageCount).toBeGreaterThan(0);
      for (let i = 0; i < c.pageCount; i++) expect(layout.pages[c.firstPage + i]!.chapterId).toBe(c.id);
      next += c.pageCount;
    }
    expect(next).toBe(layout.pages.length);
    expect(layout.startPage).toBe(layout.chapters.find((c) => c.kind === "chapter")!.firstPage);
    expect(layout.contentsPage).toBe(layout.chapters.find((c) => c.kind === "contents")!.firstPage);
    // Every contents line is a region that points at its entry, and prints the folio of its page.
    const contents = layout.pages[layout.contentsPage!]!;
    const regions = contents.regions.filter((r) => r.type === "toc");
    expect(regions.map((r) => (r.type === "toc" ? r.target : ""))).toEqual(layout.toc.map((t) => t.target));
    for (const t of layout.toc) {
      expect(t.page).toBe(layout.anchors.get(t.target));
      const folio = String(t.page - layout.startPage + 1);
      expect(contents.items.some((it) => it.kind === "text" && it.text === folio)).toBe(true);
    }
  });

  it("puts no folio or running head on front matter, and folios from 1 on body pages", () => {
    for (const p of layout.pages) {
      const folio = String(p.index - layout.startPage + 1);
      const furniture = p.items.filter((it) => it.kind === "text" && it.block === undefined);
      if (p.chapterKind !== "chapter" || p.fixed) {
        expect(p.items.some((it) => it.kind === "text" && it.y > computeGeometry(input).bottom && it.block === undefined && /^\d+$/.test(it.text))).toBe(false);
      } else {
        expect(furniture.some((it) => it.kind === "text" && it.text === folio), `page ${p.index}`).toBe(true);
      }
    }
  });

  it("starts each chapter on a new page and leaves no page empty", () => {
    for (const p of layout.pages) expect(p.items.length, `page ${p.index}`).toBeGreaterThan(0);
  });
});

describe("state and reader records", () => {
  it("adds the conditional chapter and its fixed page when the route changes", () => {
    const base = layoutBook(makeInput(book));
    const cliff = layoutBook(makeInput(book, { state: { ...initialState(book), route: "cliff" } }));
    expect(base.chapters.some((c) => c.id === "cliff")).toBe(false);
    const span = cliff.chapters.find((c) => c.id === "cliff")!;
    expect(span).toBeDefined();
    expect(cliff.pages.length).toBe(base.pages.length + span.pageCount);

    const fixed = cliff.pages.filter((p) => p.fixed);
    expect(fixed.length).toBe(1);
    const plate = fixed[0]!;
    expect(plate.chapterId).toBe("cliff");
    expect(plate.background).toBe("#102030");
    expect(cliff.blockPages.get("plate")).toBe(plate.index);
    expect(cliff.anchors.get("plate")).toBe(plate.index);
    // Frames are percentages of the whole page: the art covers it, the button sits where it was placed.
    const art = plate.items.find((it) => it.kind === "image")!;
    expect(art).toMatchObject({ x: 0, y: 0, w: 560, h: 820, fit: "cover" });
    const button = plate.regions.find((r) => r.type === "button")!;
    expect(button).toMatchObject({ x: 560 * 0.3, y: 820 * 0.82, w: 560 * 0.4, h: 820 * 0.09 });
    // No folio, no running head.
    expect(plate.items.every((it) => it.block !== undefined)).toBe(true);
    // The pages around the plate flow normally.
    expect(cliff.pages[plate.index - 1]!.blocks).toContain("_b44");
    expect(cliff.pages[plate.index + 1]!.blocks).toContain("_b50");
    // The entrance animation is carried by the items of its block only.
    const animated = cliff.pages[span.firstPage]!;
    expect(animated.animated).toBe(true);
    for (const it of animated.items) expect(it.anim !== undefined).toBe(it.block === "_b44");
  });

  it("filters the contents by condition", () => {
    const base = layoutBook(makeInput(book));
    expect(base.toc.map((t) => t.title)).toEqual(["Arrival", "The quay", "What the tide left", "Afterword"]);
    const cliff = layoutBook(makeInput(book, { state: { ...initialState(book), route: "cliff" } }));
    expect(cliff.toc.map((t) => t.title)).toEqual(["Arrival", "The quay", "What the tide left", "The cliff", "Afterword"]);
    expect(cliff.toc.find((t) => t.target === "cliff")!.page).toBe(cliff.anchors.get("cliff"));
  });

  it("shows conditional blocks and {var} values for the current state", () => {
    const text = (l: BookLayout, id: string): string =>
      l.pages
        .flatMap((p) => p.semantics)
        .filter((s) => "block" in s && s.block === id && "text" in s)
        .map((s) => ("text" in s ? s.text : ""))
        .join(" ");
    const base = layoutBook(makeInput(book));
    expect(text(base, "verdict_low")).toBe("You stay on the shingle with the gulls.");
    const lit = layoutBook(makeInput(book, { state: { ...initialState(book), lit: true, courage: 4 } }));
    expect(lit.blockPages.has("verdict_low")).toBe(false);
    expect(text(lit, "verdict_lit")).toBe("The lantern hisses. Courage: 4.");
  });

  it("draws choices from the reader's records and hides options whose condition is false", () => {
    const options = (l: BookLayout) => l.pages.flatMap((p) => p.regions).filter((r) => r.type === "option");
    const widgets = (l: BookLayout) => l.pages.flatMap((p) => p.items).flatMap((it) => (it.kind === "widget" && it.widget.type === "option" ? [it.widget] : []));

    const base = layoutBook(makeInput(book));
    // Indices are positions in the IR array, also when an option in between is hidden.
    expect(options(base).map((r) => (r.type === "option" ? [r.index, r.label, r.selected] : []))).toEqual([
      [0, "Climb to the lighthouse", false],
      [2, "Stay on the shingle", false],
    ]);
    const picked = layoutBook(makeInput(book, { state: { ...initialState(book), lit: true }, ui: { quiz: {}, choice: { fork: 2 } } }));
    expect(widgets(picked).map((w) => [w.label, w.selected])).toEqual([
      ["Climb to the lighthouse", false],
      ["Wade into the sea caves", false],
      ["Stay on the shingle", true],
    ]);
    expect(options(picked).map((r) => (r.type === "option" ? r.selected : null))).toEqual([false, false, true]);
  });

  it("marks quiz answers once answered and then shows the explanation", () => {
    const answers = (l: BookLayout) => l.pages.flatMap((p) => p.items).flatMap((it) => (it.kind === "widget" && it.widget.type === "answer" ? [it.widget.state] : []));
    const notes = (l: BookLayout) => l.pages.flatMap((p) => p.semantics).filter((s) => s.role === "note" && s.block === "star_quiz");

    const open = layoutBook(makeInput(book));
    expect(answers(open)).toEqual(["idle", "idle", "idle"]);
    expect(notes(open)).toEqual([]);
    expect(open.pages.flatMap((p) => p.regions).filter((r) => r.type === "answer").every((r) => r.type === "answer" && !r.answered)).toBe(true);

    const wrong = layoutBook(makeInput(book, { ui: { quiz: { star_quiz: { picked: 1, correct: false } }, choice: {} } }));
    expect(answers(wrong)).toEqual(["missed", "wrong", "dimmed"]);
    expect(notes(wrong)).toEqual([{ role: "note", text: "Polaris sits within a degree of the celestial pole.", block: "star_quiz" }]);

    const right = layoutBook(makeInput(book, { ui: { quiz: { star_quiz: { picked: 0, correct: true } }, choice: {} } }));
    expect(answers(right)).toEqual(["correct", "dimmed", "dimmed"]);
  });
});

describe("outline for assistive technology", () => {
  const layout = layoutBook(makeInput(book));
  const all = layout.pages.flatMap((p) => p.semantics);

  it("follows reading order with headings, images, captions and lists", () => {
    const opener = layout.pages[layout.anchors.get("arrival")!]!;
    expect(opener.semantics[0]).toEqual({ role: "heading", level: 1, text: "Arrival" });
    expect(opener.anchor).toEqual({ chapterId: "arrival", block: null, line: 0 });
    expect(all).toContainEqual({ role: "heading", level: 2, text: "The quay", block: "quay" });
    expect(all).toContainEqual({ role: "heading", level: 3, text: "A minor heading", block: "_b24" });
    expect(all).toContainEqual({ role: "image", alt: "A low sun over a dark sea", block: "_b6" });
    expect(all).toContainEqual({ role: "caption", text: "Fig. 1. The harbour at dusk, looking west.", block: "_b6" });
    expect(all).toContainEqual({ role: "list", ordered: true, items: ["Rope, tarred, about four fathoms.", "A brass sounding lead with the tallow still in it.", "One boot. Nobody asks about the other."], block: "_b16" });
    expect(all).toContainEqual({ role: "list", ordered: false, items: ["Limpets", "Bladderwrack"], block: "_b17" });
    expect(all.filter((s) => s.role === "separator").length).toBe(2);
  });

  it("rejoins words hyphenated at line ends", () => {
    const first = all.find((s) => "block" in s && s.block === "_b1");
    expect(first && "text" in first ? first.text : "").toMatch(/^The ferry came in sideways, as it always did, shouldering the tide\./);
    const whole = all
      .filter((s) => "block" in s && s.block === "_b2" && "text" in s)
      .map((s) => ("text" in s ? s.text : ""))
      .join(" ");
    expect(whole).toContain("intends to coil it again tomorrow, and the harbour clock");
  });
});

describe("pagination", () => {
  const long = novel(book, 7000);
  const input = makeInput(long);
  const layout = layoutBook(input);

  it("keeps at least two lines of a paragraph on either side of a page break", () => {
    let split = 0;
    for (const ch of long.chapters) {
      for (const b of ch.blocks ?? []) {
        const perPage = layout.pages.map((p) => linesOfBlock(p, b.id).length).filter((n) => n > 0);
        if (perPage.length < 2) continue;
        split++;
        const total = perPage.reduce((a, n) => a + n, 0);
        expect(total).toBeGreaterThanOrEqual(4);
        for (const n of perPage) expect(n, `paragraph ${b.id} split ${perPage.join("+")}`).toBeGreaterThanOrEqual(2);
      }
    }
    expect(split).toBeGreaterThan(8);
  });

  it("keeps body lines of facing pages on one baseline grid", () => {
    const g = computeGeometry(input);
    let ref: number | null = null;
    let checked = 0;
    for (const p of layout.pages) {
      if (p.chapterKind !== "chapter") continue;
      for (const it of p.items) {
        // Body-size text of the book's blocks (not folios, openers or drop capitals).
        if (it.kind !== "text" || it.block === undefined || sizeOfFont(it.font) !== g.body) continue;
        ref ??= it.y;
        const k = (it.y - ref) / g.L;
        expect(Math.abs(k - Math.round(k)), `page ${p.index} ${JSON.stringify(it.text)}`).toBeLessThan(1e-6);
        checked++;
      }
    }
    expect(checked).toBeGreaterThan(1000);
    // The first baseline sits at the same height on every full page.
    const firsts = new Set<number>();
    for (const p of layout.pages) {
      if (p.chapterKind !== "chapter" || p.anchor.block === null) continue;
      firsts.add(Math.round(linesOfBlock(p, p.anchor.block)[0]!.y * 100));
    }
    expect(firsts.size).toBe(1);
  });

  it("fills pages: no page of running text ends more than three lines short", () => {
    const g = computeGeometry(input);
    for (const c of layout.chapters) {
      if (c.kind !== "chapter") continue;
      // The last page of a chapter ends where the chapter ends.
      for (let i = c.firstPage; i < c.firstPage + c.pageCount - 1; i++) {
        const p = layout.pages[i]!;
        let bottom = 0;
        for (const it of p.items) if (it.kind === "text" && it.block !== undefined) bottom = Math.max(bottom, it.y);
        expect(g.bottom - bottom, `page ${i}`).toBeLessThan(3 * g.L);
      }
    }
  });

  it("never leaves a heading at the foot of a page", () => {
    const blocks: Block[] = [];
    for (let i = 0; i < 40; i++) {
      blocks.push({ id: `h${i}`, type: "heading", level: 2, spans: [{ text: `Section ${i}` }] });
      blocks.push({ id: `t${i}`, type: "paragraph", style: "body", spans: [{ text: "Nobody met the boat. The gulls conducted their usual inquiry. A man in an oilskin was coiling rope with the patience of someone who intends to coil it again tomorrow. ".repeat(1 + (i % 3)) }] });
    }
    // Sweep the page height so that headings land on every possible position near the foot.
    for (let height = 700; height <= 820; height += 6) {
      const l = layoutBook(makeInput(bookOf(book, blocks), { page: { width: 560, height } }));
      for (let i = 0; i < 40; i++) {
        const page = l.pages[l.blockPages.get(`h${i}`)!]!;
        expect(l.blockPages.get(`t${i}`), `height ${height}, heading h${i}`).toBe(page.index);
        expect(linesOfBlock(page, `t${i}`).length, `height ${height}, heading h${i}`).toBeGreaterThanOrEqual(2);
      }
    }
  });

  it("moves a figure that does not fit to the next page and scales one that is taller than a page", () => {
    const filler = (id: string): Block => ({ id, type: "paragraph", style: "body", spans: [{ text: "The tide kept its accounts in the old way. ".repeat(30) }] });
    const tall = { ...book, resources: [...book.resources, { ...book.resources[0]!, name: "tall", width: 200, height: 2000 }] };
    const l = layoutBook(
      makeInput(
        bookOf(tall, [
          filler("a"),
          { id: "fig", type: "image", resource: "pic", width: 1, fit: "contain", caption: [{ text: "A caption." }] },
          filler("b"),
          { id: "tower", type: "image", resource: "tall", width: 1, fit: "contain" },
          { id: "ghost", type: "image", resource: "no-such-resource", width: 1, fit: "contain", caption: [{ text: "Caption of a missing picture." }] },
        ]),
      ),
    );
    const g = computeGeometry(makeInput(book));
    const figPage = l.pages[l.blockPages.get("fig")!]!;
    const img = figPage.items.find((it) => it.kind === "image" && it.block === "fig")!;
    expect(img.kind === "image" && img.w / img.h).toBeCloseTo(360 / 240, 3);
    // The caption stays on the figure's page.
    expect(figPage.semantics).toContainEqual({ role: "caption", text: "A caption.", block: "fig" });
    const tower = l.pages[l.blockPages.get("tower")!]!.items.find((it) => it.kind === "image" && it.block === "tower")!;
    expect(tower.kind === "image" && tower.h).toBeLessThanOrEqual(g.bottom - g.top + 0.01);
    expect(tower.kind === "image" && tower.w / tower.h).toBeCloseTo(0.1, 3);
    // Unknown resource: nothing drawn, caption kept.
    expect(l.pages.flatMap((p) => p.items).some((it) => it.kind === "image" && it.block === "ghost")).toBe(false);
    expect(l.pages.flatMap((p) => p.semantics)).toContainEqual({ role: "caption", text: "Caption of a missing picture.", block: "ghost" });
    expectInsidePage(l, makeInput(book));
  });

  it("breaks a box across pages and draws its decoration on each fragment", () => {
    const l = layoutBook(
      makeInput(
        bookOf(book, [
          {
            id: "box",
            type: "box",
            style: "tint",
            title: "Long",
            blocks: Array.from({ length: 12 }, (_, i): Block => ({ id: `in${i}`, type: "paragraph", style: "body", spans: [{ text: "A coast is a sentence the sea keeps rewriting. ".repeat(8) }] })),
          },
        ]),
      ),
    );
    const withBox = l.pages.filter((p) => p.blocks.includes("box"));
    expect(withBox.length).toBeGreaterThan(1);
    for (const p of withBox) {
      const deco = p.items.filter((it) => it.kind === "rect" && it.block === "box");
      expect(deco.length, `page ${p.index}`).toBe(1);
      const d = deco[0]!;
      // The tint encloses all of the box's text on that page.
      for (const it of p.items) {
        if (it.kind !== "text" || !it.block?.startsWith("in")) continue;
        expect(d.kind === "rect" && it.y > d.y && it.y < d.y + d.h).toBe(true);
      }
    }
    expectInsidePage(l, makeInput(book));
  });

  it("honours page breaks without leaving blank pages, except where two breaks ask for one", () => {
    const para = (id: string): Block => ({ id, type: "paragraph", style: "body", spans: [{ text: "Short." }] });
    const brk = (id: string): Block => ({ id, type: "page_break" });
    const l = layoutBook(makeInput(bookOf(book, [brk("b0"), para("p1"), brk("b1"), para("p2"), brk("b2"), brk("b3"), para("p3"), brk("b4")])));
    const span = l.chapters.find((c) => c.id === "only")!;
    // p1 | p2 | (forced blank) | p3 ; the leading and trailing breaks add nothing.
    expect(span.pageCount).toBe(4);
    const pages = l.pages.slice(span.firstPage);
    expect(pages.map((p) => p.blocks.filter((b) => b.startsWith("p")).join(","))).toEqual(["p1", "p2", "", "p3"]);
    for (const id of ["b0", "b1", "b2", "b3", "b4"]) expect(l.blockPages.has(id)).toBe(true);
  });
});

describe("robustness", () => {
  it("terminates on tiny pages with huge type", () => {
    for (const page of [
      { width: 200, height: 300 },
      { width: 120, height: 160 },
      { width: 60, height: 80 },
    ]) {
      const t0 = performance.now();
      const input = makeInput(book, { page, spread: false, settings: { ...settings, fontScale: 1.6, lineHeight: "relaxed", margins: "wide" }, state: { ...initialState(book), route: "cliff" } });
      const l = layoutBook(input);
      expect(performance.now() - t0).toBeLessThan(3000);
      expect(l.pages.length).toBeGreaterThan(10);
      l.pages.forEach((p, i) => expect(p.index).toBe(i));
      for (const { block } of visibleBlocks(book, input.state)) expect(l.blockPages.has(block.id), block.id).toBe(true);
    }
  });

  it("keeps text inside the column on a small page", () => {
    const input = makeInput(book, { page: { width: 200, height: 300 }, spread: false, settings: { ...settings, fontScale: 1.6 } });
    const l = layoutBook(input);
    const g = computeGeometry(input);
    for (const p of l.pages) {
      if (p.chapterKind !== "chapter") continue;
      for (const it of p.items) {
        if (it.kind !== "text" || it.block === undefined) continue;
        expect(it.x + textWidth(it), `page ${p.index} ${JSON.stringify(it.text)}`).toBeLessThanOrEqual(g.inner + g.colW + EPS);
      }
    }
  });

  it("splits words that are wider than the measure", () => {
    const word = "pneumonoultramicroscopicsilicovolcanoconiosis".repeat(6);
    const url = "https://example.org/" + "a/".repeat(120);
    const input = makeInput(
      bookOf(book, [
        { id: "w", type: "paragraph", style: "body", spans: [{ text: `Before ${word} after.` }] },
        { id: "u", type: "paragraph", style: "mono", spans: [{ text: url }] },
        { id: "h", type: "heading", level: 2, spans: [{ text: word }] },
      ]),
      { hyphenate: undefined },
    );
    const l = layoutBook(input);
    const g = computeGeometry(input);
    for (const id of ["w", "u", "h"]) {
      const lines = l.pages.flatMap((p) => linesOfBlock(p, id));
      expect(lines.length, id).toBeGreaterThan(3);
      for (const line of lines) expect(line.x1, `${id}: ${line.text}`).toBeLessThanOrEqual(Math.max(g.inner, g.outer) + g.colW + EPS);
      // Nothing is lost.
      const joined = lines.map((x) => x.text).join("").replace(/\s/g, "");
      expect(joined).toBe((id === "w" ? `Before${word}after.` : id === "u" ? url : word).replace(/\s/g, ""));
    }
  });

  it("survives a measurer that returns nonsense and a hyphenator that lies or throws", () => {
    const bad = { width: (_f: string, t: string) => (t.length % 3 === 0 ? Number.NaN : t.length % 3 === 1 ? -5 : 0), metrics: () => ({ ascent: Number.NaN, descent: -1, xHeight: 0 }) };
    const t0 = performance.now();
    const a = layoutBook(makeInput(book, { measurer: bad }));
    expect(a.pages.length).toBeGreaterThan(3);
    const b = layoutBook(makeInput(book, { hyphenate: (w) => [w.slice(0, 2), "zz", w.slice(2)] }));
    const c = layoutBook(makeInput(book, { hyphenate: () => { throw new Error("no patterns"); } }));
    expect(performance.now() - t0).toBeLessThan(3000);
    // A hyphenator whose pieces do not add up to the word is ignored: no invented letters.
    const plain = layoutBook(makeInput(book, { hyphenate: undefined }));
    const words = (l: BookLayout): string => l.pages.flatMap((p) => p.semantics).map((s) => ("text" in s ? s.text : "")).join(" ");
    expect(words(b)).toBe(words(plain));
    expect(words(c)).toBe(words(plain));
  });

  it("does not throw on malformed but type-shaped input", () => {
    const odd = bookOf(book, [
      { id: "e1", type: "paragraph", style: "dropcap", spans: [] },
      { id: "e2", type: "paragraph", style: "dropcap", spans: [{ text: "1984 was a year." }] },
      { id: "e3", type: "list", ordered: true, items: [] },
      { id: "e4", type: "quote", paragraphs: [] },
      { id: "e5", type: "gallery", resources: ["nope", "clip"] },
      { id: "e6", type: "video", resource: "pic" },
      { id: "e7", type: "audio", resource: "nope" },
      { id: "e8", type: "space", lines: 400 },
      { id: "e9", type: "box", style: "rule", blocks: [] },
      { id: "e10", type: "choice", prompt: [], options: [] },
      { id: "e11", type: "quiz", question: [], answers: [] },
      { id: "e12", type: "group", blocks: [] },
      { id: "e13", type: "page", blocks: [{ id: "e14", type: "paragraph", style: "body", spans: [{ text: "word ".repeat(400) }], frame: { x: 10, y: 10, w: 30, h: 5 } }] },
      { id: "e15", type: "button", label: "A label far too long to fit on any button that a reasonable person would ever design ".repeat(3), actions: [] },
      { id: "e16", type: "paragraph", style: "body", spans: [{ var: "nope" }, { var: "__proto__" }, { text: " ok" }], scale: 100, color: "javascript:alert(1)" as never },
    ]);
    const input = makeInput(odd);
    const l = layoutBook(input);
    for (let i = 1; i <= 16; i++) expect(l.blockPages.has(`e${i}`), `e${i}`).toBe(true);
    expectInsidePage(l, input);
    // An invalid colour never reaches the canvas.
    for (const p of l.pages) for (const it of p.items) if (it.kind === "text") expect(it.color).toMatch(/^#[0-9a-f]{6}$/i);
  });

  it("is deterministic", () => {
    const state = { ...initialState(book), route: "cliff" };
    expect(layoutBook(makeInput(book, { state }))).toEqual(layoutBook(makeInput(book, { state })));
  });

  it("lays out a 7000-word book quickly", () => {
    const long = novel(book, 7000);
    layoutBook(makeInput(long));
    const t0 = performance.now();
    const l = layoutBook(makeInput(long));
    const ms = performance.now() - t0;
    expect(l.pages.length).toBeGreaterThan(20);
    // Generous for a loaded CI machine; the browser figure is reported by the harness.
    expect(ms).toBeLessThan(1000);
  });
});

describe("page geometry", () => {
  it("uses equal side margins on a single page and a wider inner margin on a spread", () => {
    const single = computeGeometry(makeInput(book, { spread: false }));
    expect(single.inner).toBe(single.outer);
    const spread = computeGeometry(makeInput(book, { spread: true }));
    expect(spread.inner).toBeGreaterThan(spread.outer);
    const l = layoutBook(makeInput(novel(book, 1500), { spread: true }));
    // Recto pages (even index) have the inner margin on the left.
    const lefts = l.pages.filter((p) => p.chapterKind === "chapter" && p.anchor.block !== null).map((p) => [p.index % 2, Math.min(...linesOfBlock(p, p.anchor.block!).map((x) => x.x0))]);
    for (const [parity, x] of lefts) expect(x).toBeGreaterThanOrEqual((parity === 0 ? spread.inner : spread.outer) - 0.01);
    expect(new Set(lefts.filter(([par]) => par === 0).map(([, x]) => Math.floor(x!))).size).toBeLessThanOrEqual(2);
  });

  it("holds a whole number of body lines and scales with the reader's settings", () => {
    for (const lineHeight of ["compact", "normal", "relaxed"] as const) {
      for (const margins of ["narrow", "normal", "wide"] as const) {
        const g = computeGeometry(makeInput(book, { settings: { ...settings, lineHeight, margins } }));
        expect((g.bottom - g.top) / g.L).toBeCloseTo(Math.round((g.bottom - g.top) / g.L), 6);
        expect(g.bottom).toBeLessThan(820);
      }
    }
    const small = computeGeometry(makeInput(book, { settings: { ...settings, fontScale: 0.8 } }));
    const big = computeGeometry(makeInput(book, { settings: { ...settings, fontScale: 1.6 } }));
    expect(big.body / small.body).toBeCloseTo(2, 1);
    const narrow = computeGeometry(makeInput(book, { settings: { ...settings, margins: "narrow" } }));
    const wide = computeGeometry(makeInput(book, { settings: { ...settings, margins: "wide" } }));
    expect(narrow.colW).toBeGreaterThan(wide.colW);
    void fakeMeasurer;
  });
});
