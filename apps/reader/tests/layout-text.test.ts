/**
 * The typesetter: line breaking, hyphenation, justification, hard breaks,
 * drop capitals. Uses the fake measurer; the phone regression also uses
 * the real English hyphenation patterns.
 */
import hyphenEn from "hyphen/en";
import { describe, expect, it } from "vitest";
import type { Block, Span } from "../src/ir";
import { computeGeometry, layoutBook, maxSpaceWidth, MIN_SPACE_SHRINK } from "../src/layout";
import { breakGreedy, breakLines, breakOptimal, placeLine, tokenize, type Line, type RunStyle } from "../src/layout/text";
import type { BookLayout, LayoutInput } from "../src/layout/types";
import { bookOf, fakeHyphenate, fakeMeasurer, linesOfBlock, loadKitchenSink, makeInput, settings, sizeOfFont, textWidth, type SetLine } from "./layout.helpers";

const book = loadKitchenSink();
const measure = (font: string, text: string): number => fakeMeasurer.width(font, text);
const st: RunStyle = { font: "400 16px serif", size: 16, color: "#000000" };
const SPACE = measure(st.font, " ");

const realHyphenate = (word: string): string[] => (hyphenEn as unknown as { hyphenateSync(t: string, o: object): string }).hyphenateSync(word, { hyphenChar: "|" }).split("|");

const PROSE =
  "Nobody met the boat. The gulls conducted their usual inquiry. A man in an oilskin was coiling rope with the patience of someone who intends to coil it again tomorrow, and the harbour clock, which had been wrong for so long that people navigated by its error, said a quarter past something. She had come for the archive, or that was what the letter said.";

function textOf(line: Line): string {
  return line.segs.map((s) => s.text).join("");
}

/** Width of every word space of the lines of a block, with the natural space width and type size. */
function spaceWidths(layout: BookLayout, block: string): { gap: number; natural: number; size: number; line: SetLine }[] {
  const out: { gap: number; natural: number; size: number; line: SetLine }[] = [];
  for (const p of layout.pages) {
    for (const line of linesOfBlock(p, block)) {
      for (let i = 1; i < line.items.length; i++) {
        const prev = line.items[i - 1]!;
        const cur = line.items[i]!;
        const gap = cur.x - (prev.x + textWidth(prev));
        if (gap < 0.5) continue; // a style change inside a word
        if (sizeOfFont(prev.font) !== sizeOfFont(cur.font)) continue; // the drop capital beside the lines
        if (prev.font.includes("mono")) continue; // inline code keeps the text's word space, not its own
        out.push({ gap, natural: fakeMeasurer.width(prev.font, " "), size: sizeOfFont(prev.font), line });
      }
    }
  }
  return out;
}

describe("line breaking", () => {
  it("breaks at word boundaries and keeps every word, in order", () => {
    for (const justify of [false, true]) {
      for (const width of [120, 200, 331, 480]) {
        const lines = breakLines(tokenize([{ text: PROSE, st }], measure), () => width, { measure, justify, hyphenate: fakeHyphenate });
        expect(lines.length).toBeGreaterThan(3);
        const rebuilt = lines
          .map((l) => textOf(l))
          .reduce((acc, t, i) => (i === 0 ? t : lines[i - 1]!.end === "hyphen" ? acc.slice(0, -1) + t : `${acc} ${t}`), "");
        expect(rebuilt).toBe(PROSE);
        for (const l of lines.slice(0, -1)) {
          // No line overflows, even with its spaces compressed as far as allowed.
          const boxes = l.width - l.spaceWidth;
          expect(boxes + l.spaceWidth * (justify ? MIN_SPACE_SHRINK : 1)).toBeLessThanOrEqual(width + 0.01);
        }
        expect(lines[lines.length - 1]!.end).toBe("para");
      }
    }
  });

  it("obeys the hyphenation rules", () => {
    const seen: string[] = [];
    for (const width of [90, 110, 130, 150, 170, 200, 240]) {
      for (const breaker of [breakGreedy, breakOptimal]) {
        const lines = breaker(tokenize([{ text: PROSE, st }], measure), () => width, { measure, justify: true, hyphenate: fakeHyphenate });
        if (!lines) continue;
        let run = 0;
        lines.forEach((l, i) => {
          if (l.end !== "hyphen") {
            run = 0;
            return;
          }
          run++;
          expect(run, "consecutive hyphenated lines").toBeLessThanOrEqual(2);
          const head = /(\p{L}+)-$/u.exec(textOf(l))![1]!;
          const tail = /^(\p{L}+)/u.exec(textOf(lines[i + 1]!))![1]!;
          seen.push(`${head}-${tail}`);
          expect(head.length).toBeGreaterThanOrEqual(2);
          expect(tail.length).toBeGreaterThanOrEqual(3);
          expect((head + tail).length).toBeGreaterThanOrEqual(6);
          // A line is never just a fragment of a word, unless the word alone is wider than the line.
          if (l.segs.length === 1) expect(measure(st.font, head + tail)).toBeGreaterThan(width);
        });
        // The last word of the paragraph stays whole.
        expect(textOf(lines[lines.length - 1]!)).toMatch(/(^| )said\.$/);
      }
    }
    expect(seen.length).toBeGreaterThan(5);
  });

  it("does not hyphenate when no hyphenator is given, nor short words when one is", () => {
    const plain = breakLines(tokenize([{ text: PROSE, st }], measure), () => 140, { measure, justify: true });
    expect(plain.every((l) => l.end !== "hyphen")).toBe(true);
    const short = breakLines(tokenize([{ text: "these words are short ones only every time again ".repeat(6), st }], measure), () => 100, { measure, justify: true, hyphenate: (w) => [w.slice(0, 2), w.slice(2)] });
    expect(short.every((l) => l.end !== "hyphen")).toBe(true);
  });

  it("total fit beats first fit: fewer loose and fewer ragged lines for the same text", () => {
    // Per line: how much of the comfortable stretch (spaces at 1.6x) it needs; ragged = beyond the hard limit.
    const judge = (lines: Line[], width: number): { ragged: number; loose: number } => {
      let ragged = 0;
      let loose = 0;
      for (const l of lines.slice(0, -1)) {
        const slack = width - l.width;
        if (slack > l.stretch + 0.01) ragged++;
        else if (slack > l.spaceWidth * 0.6 + 0.01) loose++;
      }
      return { ragged, loose };
    };
    let better = 0;
    for (const width of [180, 210, 240, 270, 300, 330, 360, 400]) {
      const greedy = judge(breakGreedy(tokenize([{ text: PROSE, st }], measure), () => width, { measure, justify: true, hyphenate: fakeHyphenate }), width);
      const optimal = judge(breakOptimal(tokenize([{ text: PROSE, st }], measure), () => width, { measure, justify: true, hyphenate: fakeHyphenate })!, width);
      // Never more lines that have to be left ragged, and none bought with a crowd of loose ones.
      expect(optimal.ragged, `width ${width}`).toBeLessThanOrEqual(greedy.ragged);
      expect(optimal.ragged + optimal.loose, `width ${width}`).toBeLessThanOrEqual(greedy.ragged + greedy.loose + 1);
      if (optimal.ragged < greedy.ragged) better++;
    }
    expect(better).toBeGreaterThanOrEqual(2);
  });

  it("allows a break after a hyphen or dash inside a compound, without adding one", () => {
    const lines = breakLines(tokenize([{ text: "a well-known mother-in-law—so they say—arrived", st }], measure), () => 60, { measure, justify: false });
    const texts = lines.map(textOf);
    expect(texts.join("").replace(/\s/g, "")).toBe("awell-knownmother-in-law—sotheysay—arrived");
    expect(lines.every((l) => l.end !== "hyphen")).toBe(true);
    // The compounds were broken at their own hyphens and dashes.
    expect(texts.filter((t) => /[-—]$/.test(t)).length).toBeGreaterThanOrEqual(2);
  });

  it("justifies by stretching word spaces only, within the limit, and leaves the last line ragged", () => {
    const width = 300;
    const lines = breakLines(tokenize([{ text: PROSE, st }], measure), () => width, { measure, justify: true, hyphenate: fakeHyphenate });
    lines.forEach((line, i) => {
      const placed = placeLine(line, 10, 20, width, "justify");
      const last = i === lines.length - 1;
      expect(placed.justified).toBe(!last);
      if (last) {
        expect(placed.x1).toBeLessThan(10 + width);
        return;
      }
      expect(placed.x0).toBeCloseTo(10, 6);
      expect(placed.x1).toBeCloseTo(10 + width, 6);
      const texts = placed.items.filter((it) => it.kind === "text");
      for (let k = 1; k < texts.length; k++) {
        const a = texts[k - 1]!;
        const b = texts[k]!;
        if (a.kind !== "text" || b.kind !== "text") continue;
        const gap = b.x - (a.x + measure(a.font, a.text));
        expect(gap).toBeLessThanOrEqual(maxSpaceWidth(SPACE, 16) + 1e-6);
        expect(gap).toBeGreaterThanOrEqual(SPACE * MIN_SPACE_SHRINK - 1e-6);
        // Letters are never spaced out.
        expect(a.letterSpacing).toBeUndefined();
      }
    });
  });

  it("leaves a line ragged rather than stretch its spaces beyond the hard limit", () => {
    // Long unbreakable words in a narrow column: some lines cannot be filled.
    const text = "extraordinarily incomprehensible a characteristically disproportionate b uncharacteristic";
    const width = 190;
    const lines = breakLines(tokenize([{ text, st }], measure), () => width, { measure, justify: true });
    let ragged = 0;
    for (const line of lines.slice(0, -1)) {
      const placed = placeLine(line, 0, 0, width, "justify");
      const slack = width - line.width;
      if (slack > line.stretch + 0.01) {
        ragged++;
        expect(placed.justified).toBe(false);
        expect(placed.x1).toBeCloseTo(line.width, 6);
      } else expect(placed.justified).toBe(line.spaces > 0);
    }
    expect(ragged).toBeGreaterThan(0);
  });

  it("aligns left, centre and right", () => {
    const [line] = breakLines(tokenize([{ text: "three short words", st }], measure), () => 400, { measure, justify: false });
    const w = line!.width;
    expect(placeLine(line!, 0, 0, 400, "left").x0).toBe(0);
    expect(placeLine(line!, 0, 0, 400, "center").x0).toBeCloseTo((400 - w) / 2, 6);
    expect(placeLine(line!, 0, 0, 400, "right").x1).toBeCloseTo(400, 6);
    // Ragged text is one item per style run: kerning across spaces is kept.
    expect(placeLine(line!, 0, 0, 400, "left").items.length).toBe(1);
  });
});

describe("hard line breaks", () => {
  const verse: Span[] = [
    { text: "The tide kept its accounts in the old way,", break: true },
    { text: "a line of wrack for every hour it stayed,", break: true },
    { text: "and what it took it entered as a loan." },
  ];
  const expected = ["The tide kept its accounts in the old way,", "a line of wrack for every hour it stayed,", "and what it took it entered as a loan."];
  const linesOf = (l: BookLayout, id: string): string[] => l.pages.flatMap((p) => linesOfBlock(p, id)).map((x) => x.text);

  it.each(["verse", "body", "small", "lead", "caption", "mono"] as const)("N break spans whose lines fit give N+1 lines (%s)", (style) => {
    for (const align of [undefined, "center", "justify"] as const) {
      const block: Block = { id: "v", type: "paragraph", style, spans: verse, ...(align ? { align } : {}) };
      const l = layoutBook(makeInput(bookOf(book, [block]), { page: { width: 900, height: 1000 }, spread: true }));
      expect(linesOf(l, "v")).toEqual(expected);
    }
  });

  it("breaks after the span that carries the flag, also across formatting", () => {
    const spans: Span[] = [{ text: "One " }, { text: "bold", bold: true, break: true }, { text: "two", italic: true }, { text: " three", break: true }, { break: true }, { text: "after a blank line" }];
    const l = layoutBook(makeInput(bookOf(book, [{ id: "v", type: "paragraph", style: "verse", spans }]), { page: { width: 900, height: 1000 } }));
    expect(linesOf(l, "v")).toEqual(["One bold", "two three", "after a blank line"]);
    // The empty line between them takes a line of space.
    const ys = l.pages.flatMap((p) => linesOfBlock(p, "v")).map((x) => x.y);
    const step = ys[1]! - ys[0]!;
    expect(ys[2]! - ys[1]!).toBeCloseTo(2 * step, 6);
  });

  it("wraps a verse line that is too long with a hanging indent, and keeps the breaks", () => {
    const input = makeInput(bookOf(book, [{ id: "v", type: "paragraph", style: "verse", spans: verse }]), { page: { width: 240, height: 640 }, spread: false });
    const l = layoutBook(input);
    const lines = l.pages.flatMap((p) => linesOfBlock(p, "v"));
    expect(lines.length).toBeGreaterThan(3);
    const starts = expected.map((t) => lines.findIndex((x) => t.startsWith(x.text)));
    expect(starts.every((i) => i >= 0)).toBe(true);
    const left = Math.min(...lines.map((x) => x.x0));
    lines.forEach((line, i) => {
      // Lines that begin a verse line are flush; continuations are indented.
      if (starts.includes(i)) expect(line.x0).toBeCloseTo(left, 6);
      else expect(line.x0).toBeGreaterThan(left + 10);
    });
    expect(lines.map((x) => x.text).join(" ")).toBe(expected.join(" "));
  });

  it("keeps breaks inside a frame of a fixed page", () => {
    const page: Block = { id: "pg", type: "page", blocks: [{ id: "v", type: "paragraph", style: "body", spans: verse, frame: { x: 5, y: 10, w: 90, h: 40 } }] };
    const l = layoutBook(makeInput(bookOf(book, [page]), { page: { width: 900, height: 1200 } }));
    expect(l.pages.filter((p) => p.fixed).length).toBe(1);
    expect(linesOf(l, "v")).toEqual(expected);
  });
});

describe("justified pages", () => {
  const configs: [string, Partial<LayoutInput>][] = [
    ["desktop spread", { page: { width: 560, height: 820 }, spread: true }],
    ["phone", { page: { width: 360, height: 640 }, spread: false }],
    ["phone 351x700", { page: { width: 351, height: 700 }, spread: false }],
    ["large type", { page: { width: 560, height: 820 }, spread: true, settings: { ...settings, fontScale: 1.4 } }],
  ];

  it.each(configs)("never stretches a word space beyond the limit (%s)", (_name, over) => {
    const l = layoutBook(makeInput(book, { ...over, hyphenate: realHyphenate }));
    let justifiedLines = 0;
    for (const id of ["_b1", "_b2", "_b3", "_b7", "_b25", "_b26", "_b51"]) {
      const lines = new Set<SetLine>();
      for (const s of spaceWidths(l, id)) {
        expect(s.gap, `${id}: ${s.line.text}`).toBeLessThanOrEqual(maxSpaceWidth(s.natural, s.size) + 0.01);
        expect(s.gap, `${id}: ${s.line.text}`).toBeGreaterThanOrEqual(s.natural * MIN_SPACE_SHRINK - 0.01);
        lines.add(s.line);
      }
      justifiedLines += lines.size;
    }
    expect(justifiedLines).toBeGreaterThan(10);
  });

  // Regression: at phone width the drop-cap paragraph once produced a line holding only "al-",
  // and justified paragraphs had lines that stopped well short of the margin.
  it.each(configs)("sets no fragment-only lines and no short lines inside a justified paragraph (%s)", (_name, over) => {
    for (const hyphenate of [realHyphenate, fakeHyphenate, undefined]) {
      const input = makeInput(book, { ...over, hyphenate });
      const l = layoutBook(input);
      const g = computeGeometry(input);
      const span = l.chapters.find((c) => c.id === "arrival")!;
      const pages = l.pages.slice(span.firstPage, span.firstPage + span.pageCount);
      // Justified paragraphs of the first chapter: the drop-cap opening and the body paragraphs.
      for (const id of ["_b1", "_b2", "_b3", "_b7", "_b25"]) {
        const lines = pages.flatMap((p) => linesOfBlock(p, id).map((line) => ({ line, right: (input.spread ? (p.index % 2 === 0 ? g.inner : g.outer) : g.inner) + g.colW })));
        expect(lines.length).toBeGreaterThan(2);
        lines.forEach(({ line, right }, i) => {
          const letters = line.text.replace(/[^\p{L}]/gu, "").length;
          if (line.text.endsWith("-")) expect(letters, `${id}: "${line.text}"`).toBeGreaterThanOrEqual(4);
          if (i === lines.length - 1) return; // the last line of a paragraph is ragged
          // Without hyphenation a narrow column cannot always be filled; the rule is for hyphenated text.
          if (!hyphenate) return;
          // The drop capital is part of line 1 here, so every line starts within the column and must reach
          // (at least 80% of the way to) the right-hand margin.
          expect(right - line.x1, `${id} line ${i}: "${line.text}"`).toBeLessThanOrEqual(g.colW * 0.2);
        });
      }
    }
  });

  it("with real patterns at 351x700, every mid-paragraph line of the first chapter is flush right", () => {
    const input = makeInput(book, { page: { width: 351, height: 700 }, spread: false, hyphenate: realHyphenate });
    const l = layoutBook(input);
    const g = computeGeometry(input);
    for (const id of ["_b1", "_b2", "_b3", "_b7"]) {
      const lines = l.pages.flatMap((p) => linesOfBlock(p, id));
      for (const line of lines.slice(0, -1)) expect(line.x1, `${id}: "${line.text}"`).toBeCloseTo(g.inner + g.colW, 1);
    }
  });
});

describe("paragraph styles", () => {
  const para = (id: string, text: string, style: "body" | "dropcap" = "body"): Block => ({ id, type: "paragraph", style, spans: [{ text }] });
  const long = PROSE;

  it("indents paragraphs that follow a paragraph, and nothing else", () => {
    const input = makeInput(bookOf(book, [para("a", long), para("b", long), { id: "h", type: "heading", level: 2, spans: [{ text: "Heading" }] }, para("c", long), para("d", long)]));
    const l = layoutBook(input);
    const first = (id: string): number => l.pages.flatMap((p) => linesOfBlock(p, id))[0]!.x0;
    const second = (id: string): number => l.pages.flatMap((p) => linesOfBlock(p, id))[1]!.x0;
    expect(first("a")).toBeCloseTo(second("a"), 6);
    expect(first("b")).toBeGreaterThan(second("b") + 10);
    expect(first("c")).toBeCloseTo(second("c"), 6);
    expect(first("d")).toBeGreaterThan(second("d") + 10);
  });

  it("separates paragraphs with space instead of an indent in a spaced theme", () => {
    const l = layoutBook(makeInput(bookOf(book, [para("a", long), para("b", long)], { paragraph: "spaced" })));
    const a = l.pages.flatMap((p) => linesOfBlock(p, "a"));
    const b = l.pages.flatMap((p) => linesOfBlock(p, "b"));
    const step = a[1]!.y - a[0]!.y;
    expect(b[0]!.x0).toBeCloseTo(b[1]!.x0, 6);
    expect(b[0]!.y - a[a.length - 1]!.y).toBeCloseTo(step * 1.5, 6);
  });

  it("sets a three-line drop capital and shortens the lines beside it", () => {
    const input = makeInput(bookOf(book, [para("d", long, "dropcap")]));
    const l = layoutBook(input);
    const g = computeGeometry(input);
    const page = l.pages[l.blockPages.get("d")!]!;
    const cap = page.items.find((it) => it.kind === "text" && it.block === "d" && sizeOfFont(it.font) > g.body * 2);
    expect(cap && cap.kind === "text" && cap.text).toBe("N");
    if (!cap || cap.kind !== "text") return;
    const body = page.items.filter((it) => it.kind === "text" && it.block === "d" && it !== cap);
    const ys = Array.from(new Set(body.map((it) => (it.kind === "text" ? it.y : 0)))).sort((x, y) => x - y);
    // Foot on the third baseline; top at the cap height of the first line.
    expect(cap.y).toBeCloseTo(ys[2]!, 6);
    const capHeight = (font: string): number => fakeMeasurer.metrics(font).capHeight!;
    expect(cap.y - capHeight(cap.font)).toBeCloseTo(ys[0]! - capHeight(`400 ${g.body}px serif`), 1);
    const capRight = cap.x + textWidth(cap);
    for (const it of body) {
      if (it.kind !== "text") continue;
      if (it.y <= ys[2]! + 0.01) expect(it.x).toBeGreaterThan(capRight);
    }
    const fourth = body.filter((it) => it.kind === "text" && Math.abs(it.y - ys[3]!) < 0.01);
    expect(Math.min(...fourth.map((it) => (it.kind === "text" ? it.x : 0)))).toBeCloseTo(cap.x, 6);
    // The capital is the paragraph's first letter and is not repeated.
    expect(page.semantics[0]).toMatchObject({ role: "paragraph", text: expect.stringMatching(/^Nobody met the boat\./) });
  });

  it("falls back to a plain paragraph when a drop capital makes no sense", () => {
    const cases: [string, string, boolean][] = [
      ["short", "Too short.", false],
      ["digit", "1984 was a long year for everyone concerned. " + long, false],
      ["quote", "“Nobody met the boat,” she said. " + long, true],
      ["spanish", "¿Quién vino a recibir el barco? " + long, true],
    ];
    for (const [id, text, expectCap] of cases) {
      const input = makeInput(bookOf(book, [para(id, text, "dropcap")]));
      const l = layoutBook(input);
      const g = computeGeometry(input);
      const big = l.pages.flatMap((p) => p.items).filter((it) => it.kind === "text" && it.block === id && sizeOfFont(it.font) > g.body * 2);
      expect(big.length, id).toBe(expectCap ? 1 : 0);
      if (id === "quote") expect(big[0]!.kind === "text" && big[0]!.text).toBe("N");
      if (id === "spanish") expect(big[0]!.kind === "text" && big[0]!.text).toBe("¿Q");
      const all = l.pages.flatMap((p) => p.semantics).map((s) => ("text" in s ? s.text : "")).join(" ");
      expect(all.replace(/\s+/g, " ")).toBe(text.replace(/\s+/g, " ").trim());
    }
  });

  it("underlines links, gives each line fragment a region, and shares one baseline between fonts", () => {
    const spans: Span[] = [
      { text: "Plain text with " },
      { text: "a link that is long enough to wrap onto a second line of the column for sure", link: { kind: "anchor", target: "fork" } },
      { text: " then " },
      { text: "code", code: true },
      { text: ", " },
      { text: "bold", bold: true },
      { text: " and " },
      { text: "italic", italic: true },
      { text: " in one paragraph that keeps going for a while so that it wraps again." },
    ];
    const input = makeInput(bookOf(book, [{ id: "m", type: "paragraph", style: "body", spans }]));
    const l = layoutBook(input);
    const page = l.pages[l.blockPages.get("m")!]!;
    const links = page.regions.filter((r) => r.type === "link");
    expect(links.length).toBe(2);
    // Each fragment carries the whole link text, so a control built over it reads sensibly on its own.
    for (const r of links) expect(r.type === "link" && r.label).toBe("a link that is long enough to wrap onto a second line of the column for sure");
    expect(links.every((r) => r.type === "link" && r.link.kind === "anchor" && r.link.target === "fork")).toBe(true);
    const underlines = page.items.filter((it) => it.kind === "line" && it.block === "m");
    expect(underlines.length).toBe(2);
    links.forEach((r, i) => {
      const u = underlines[i]!;
      expect(u.kind === "line" && u.x1).toBeCloseTo(r.x, 6);
      expect(u.kind === "line" && u.x2).toBeCloseTo(r.x + r.w, 6);
      expect(u.kind === "line" && u.color).toBe(input.palette.accent);
    });
    const lines = linesOfBlock(page, "m");
    for (const line of lines) expect(new Set(line.items.map((it) => it.y)).size).toBe(1);
    const fonts = new Set(lines.flatMap((x) => x.items.map((it) => it.font)));
    expect(Array.from(fonts).some((f) => f.includes("mono"))).toBe(true);
    expect(Array.from(fonts).some((f) => f.startsWith("italic"))).toBe(true);
    expect(Array.from(fonts).some((f) => f.startsWith("700"))).toBe(true);
  });
});
