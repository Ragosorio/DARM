/**
 * Shared fixtures for the layout tests: a deterministic fake measurer (no
 * canvas), default inputs and the kitchen-sink book.
 */
import { readFileSync } from "node:fs";
import { holds, initialState } from "../src/core/state";
import type { Block, Book, Chapter, State } from "../src/ir";
import type { DrawItem, FontMetrics, LayoutInput, Measurer, PageLayout, ReaderSettings, ResolvedPalette, UiRecords } from "../src/layout/types";

export function loadKitchenSink(): Book {
  return JSON.parse(readFileSync(new URL("./fixtures/kitchen-sink.ir.json", import.meta.url), "utf8")) as Book;
}

function sizeOf(font: string): number {
  const m = /(\d+(?:\.\d+)?)px/.exec(font);
  return m ? Number(m[1]) : 16;
}

/** Half an em per character; bold is 8% wider, monospace 0.6em, spaces 0.28em. */
export const fakeMeasurer: Measurer = {
  width(font: string, text: string): number {
    const size = sizeOf(font);
    const mono = font.includes("mono");
    const bold = /\b(600|700|800)\b/.test(font);
    let w = 0;
    for (const ch of text) w += mono ? 0.6 : ch === " " ? 0.28 : ch === "." || ch === "," || ch === "i" || ch === "l" ? 0.3 : 0.5;
    return w * size * (bold ? 1.08 : 1);
  },
  metrics(font: string): FontMetrics {
    const size = sizeOf(font);
    return { ascent: size * 0.95, descent: size * 0.27, xHeight: size * 0.48, capHeight: size * 0.68 };
  },
};

export const settings: ReaderSettings = { fontScale: 1, lineHeight: "normal", margins: "normal", justify: null, hyphenate: null };

export const palette: ResolvedPalette = {
  dark: false,
  paper: "#f7f3ea",
  ink: "#1f1c18",
  accent: "#8c3b2a",
  muted: "#7a7368",
  rule: "#d8d0c0",
  tint: "#eee8da",
};

export const fonts = { body: "serif", heading: "serif", mono: "mono", sans: "sans" };

/** A crude syllable splitter: break after every vowel group. Enough to exercise hyphenation. */
export function fakeHyphenate(word: string): string[] {
  const parts = word.match(/[^aeiouy]*[aeiouy]+(?:[^aeiouy]*$)?/gi);
  return parts && parts.join("") === word ? parts : [word];
}

export function makeInput(book: Book, over: Partial<LayoutInput> & { state?: State; ui?: UiRecords } = {}): LayoutInput {
  return {
    book,
    page: { width: 560, height: 820 },
    spread: true,
    settings,
    palette,
    fonts,
    measurer: fakeMeasurer,
    state: initialState(book),
    ui: { quiz: {}, choice: {} },
    hyphenate: fakeHyphenate,
    ...over,
  };
}


export type TextItem = Extract<DrawItem, { kind: "text" }>;

export function sizeOfFont(font: string): number {
  return sizeOf(font);
}

export function textWidth(it: TextItem): number {
  return fakeMeasurer.width(it.font, it.text) + (it.letterSpacing ?? 0) * it.text.length;
}

export interface SetLine {
  y: number;
  items: TextItem[];
  x0: number;
  x1: number;
  text: string;
}

/** Text lines of one block on one page: items sharing a baseline, left to right. */
export function linesOfBlock(page: PageLayout, block: string): SetLine[] {
  const byY = new Map<number, TextItem[]>();
  for (const it of page.items) {
    if (it.kind !== "text" || it.block !== block) continue;
    const key = Math.round(it.y * 100) / 100;
    const list = byY.get(key) ?? [];
    list.push(it);
    byY.set(key, list);
  }
  return Array.from(byY.entries())
    .sort((a, b) => a[0] - b[0])
    .map(([y, items]) => {
      items.sort((a, b) => a.x - b.x);
      let text = "";
      let end = -Infinity;
      for (const it of items) {
        // Separate items on a line are separate words unless they touch (a style change inside a word).
        if (text !== "" && it.x - end > 0.5) text += " ";
        text += it.text;
        end = it.x + textWidth(it);
      }
      return { y, items, x0: items[0]!.x, x1: end, text };
    });
}

/** Every block of the book that is visible in `state`, in reading order (conditions evaluated). */
export function visibleBlocks(book: Book, state: State): { chapter: Chapter; block: Block }[] {
  const out: { chapter: Chapter; block: Block }[] = [];
  const walk = (chapter: Chapter, blocks: Block[] | undefined): void => {
    for (const b of blocks ?? []) {
      if (b.type === "group" && !holds(b.when, state)) continue;
      out.push({ chapter, block: b });
      if (b.type === "group" || b.type === "box" || b.type === "page") walk(chapter, b.blocks);
    }
  };
  for (const ch of book.chapters) if (holds(ch.when, state)) walk(ch, ch.blocks);
  return out;
}

const PROSE = [
  "The ferry came in sideways, as it always did, shouldering the tide. From the rail the town looked like a sentence somebody had started with confidence and then abandoned: a bright row of houses, a comma of breakwater, and after that only weather.",
  "Nobody met the boat. The gulls conducted their usual inquiry. A man in an oilskin was coiling rope with the patience of someone who intends to coil it again tomorrow, and the harbour clock, which had been wrong for so long that people navigated by its error, said a quarter past something.",
  "She had come for the archive, or that was what the letter said. The letter also said bring boots, which she had taken for a joke until she saw the quay. It was the kind of quay that keeps what it is given.",
  "There is a particular light on a coast in the last hour, when the water gives back more than the sky sends down. Painters have a word for it and so do pilots, and the two words do not agree.",
  "A short one.",
  "The painters mean that the edges of things go soft; the pilots mean that you can no longer trust a distance, and neither of them is wrong about what they have seen.",
];

/** A plain novel of about `words` words: chapters of ordinary paragraphs, for pagination and timing tests. */
export function novel(base: Book, words: number): Book {
  const chapters: Chapter[] = [];
  let count = 0;
  let n = 0;
  for (let c = 0; count < words; c++) {
    const blocks: Block[] = [];
    for (let p = 0; p < 30 && count < words; p++) {
      // Vary paragraph lengths so that page breaks fall in different places.
      let text = PROSE[n % PROSE.length]!;
      if (n % 3 === 1) text += " " + PROSE[(n + 2) % PROSE.length]!;
      n++;
      count += text.split(" ").length;
      blocks.push({ id: `c${c}p${p}`, type: "paragraph", style: p === 0 ? "dropcap" : "body", spans: [{ text }] });
    }
    chapters.push({ id: `c${c}`, kind: "chapter", title: `Chapter ${c + 1}`, number: c + 1, opener: true, blocks });
  }
  return {
    ...base,
    chapters: [...base.chapters.filter((ch) => ch.kind !== "chapter"), ...chapters],
    toc: chapters.map((ch) => ({ title: ch.title, target: ch.id, level: 1 as const, number: ch.number })),
    anchors: {},
  };
}

/** A one-chapter book around the given blocks. */
export function bookOf(base: Book, blocks: Block[], theme: Partial<Book["theme"]> = {}): Book {
  return {
    ...base,
    theme: { ...base.theme, ...theme },
    chapters: [{ id: "only", kind: "chapter", title: "Only", opener: false, blocks }],
    toc: [],
    anchors: {},
  };
}
