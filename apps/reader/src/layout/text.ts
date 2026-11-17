/**
 * The typesetter: turns styled text into tokens, breaks tokens into lines
 * and positions the words of a line.
 *
 * It only needs a width function, so the same code sets body text during
 * layout (cached `Measurer`) and the blurb on the back cover during painting
 * (a canvas context).
 *
 * Two line breakers share the same rules:
 *
 *   - `breakGreedy` fills each line with as many words as fit. It is used for
 *     ragged text, where it is already optimal, and as the fallback for
 *     anything unusual (a word wider than the column).
 *   - `breakOptimal` chooses all the breaks of a justified paragraph together
 *     (the total-fit idea of Knuth and Plass): a greedy breaker discovers too
 *     late that a line is loose, while this one can take a word back two
 *     lines earlier to keep the colour of the column even.
 *
 * Rules: word spaces of a justified line may shrink to MIN_SPACE_SHRINK of
 * their natural width and stretch up to the limits described at
 * COMFORT_STRETCH below, never more: a line that cannot be set within the
 * hard limit is left ragged instead of opening rivers. Words are hyphenated
 * only when that visibly helps, never in the last word of a paragraph, and
 * at most twice on consecutive lines. Letters are never spaced out.
 */
import type { Align, Color, Link } from "../ir";
import type { DrawItem } from "./types";

/**
 * Word spaces of a justified line. Three thresholds, from aim to last resort:
 *
 *  - COMFORT_STRETCH: up to 1.6 times its natural width a space still looks
 *    evenly set. Badness is measured against this, and a hyphen is cheaper
 *    than going much beyond it.
 *  - MAX_SPACE_EM: the normal tolerance. A paragraph is first fitted so that
 *    no space exceeds 1.6 times its natural width or this fraction of the
 *    type size, whichever is larger. (The em bound exists because faces
 *    differ: Literata's word space is a tight 0.2 em, and 1.6 times that is
 *    narrower than the normal space of many book faces.)
 *  - LOOSE_SPACE_EM: when no set of breaks satisfies the normal tolerance
 *    (narrow columns, large type, the lines beside a drop capital), one loose
 *    line is better than a line that stops short of the margin, so a second
 *    pass allows spaces up to this. It is the hard limit: a line that would
 *    need more is left ragged.
 */
export const COMFORT_STRETCH = 1.6;
export const MAX_SPACE_EM = 0.46;
export const LOOSE_SPACE_EM = 0.72;
/** Kept for callers that only want one number: the comfortable ratio. */
export const MAX_SPACE_STRETCH = COMFORT_STRETCH;

/** Largest width a word space of natural width `w` at type size `size` may ever be stretched to. */
export function maxSpaceWidth(w: number, size: number): number {
  return Math.max(w * COMFORT_STRETCH, size * LOOSE_SPACE_EM);
}

function normalSpaceWidth(w: number, size: number): number {
  return Math.max(w * COMFORT_STRETCH, size * MAX_SPACE_EM);
}

/** Counters for the harness (how many lines were justified, hyphenated, left ragged). No effect on layout. */
export const typesetStats = { lines: 0, justified: 0, ragged: 0, hyphens: 0, stretchSum: 0, stretchMax: 0 };
export function resetTypesetStats(): void {
  typesetStats.lines = typesetStats.justified = typesetStats.ragged = typesetStats.hyphens = typesetStats.stretchSum = typesetStats.stretchMax = 0;
}
/** Word spaces of a justified line may be compressed down to this multiple. */
export const MIN_SPACE_SHRINK = 0.8;
/** Greedy breaker: a justified line that would use more than this share of its stretch tries to hyphenate the next word. */
const HYPHENATE_ABOVE = 0.35;
/** Hyphenation rules: minimum word length and letters kept on either side. */
const MIN_WORD = 6;
const MIN_BEFORE = 2;
const MIN_AFTER = 3;
const MAX_HYPHEN_RUN = 2;
const EPS = 0.01;

export interface RunStyle {
  font: string;
  size: number;
  color: Color;
  link?: Link;
  /** Whole text of the link this run belongs to (a link may wrap and change style inside). */
  linkLabel?: string;
  letterSpacing?: number;
  /** Width of a word space in this style, when it should not be the font's own (inline code). */
  spaceWidth?: number;
}

/** Input of the tokenizer: a run of text in one style, or a hard line break. */
export type Piece = { text: string; st: RunStyle } | "br";

const BOX = 0;
const GLUE = 1;
const OPT = 2;
const BR = 3;

interface BoxTok {
  k: typeof BOX;
  text: string;
  w: number;
  st: RunStyle;
}
interface GlueTok {
  k: typeof GLUE;
  w: number;
  st: RunStyle;
}
export type Token = BoxTok | GlueTok | { k: typeof OPT } | { k: typeof BR };

export type MeasureFn = (font: string, text: string) => number;

function boxWidth(measure: MeasureFn, st: RunStyle, text: string): number {
  return measure(st.font, text) + (st.letterSpacing ? st.letterSpacing * text.length : 0);
}

const SPACE_SPLIT = /([ \t\n\r\f\v\u2028\u2029]+)/;
// A break is allowed after a hyphen between two letters/digits and after a dash.
const OPT_SPLIT = /(?<=[\p{L}\p{N}]-)(?=[\p{L}\p{N}])|(?<=[\u2013\u2014])(?=.)/u;

export function tokenize(pieces: Piece[], measure: MeasureFn): Token[] {
  const out: Token[] = [];
  for (const p of pieces) {
    if (p === "br") {
      out.push({ k: BR });
      continue;
    }
    // Soft hyphens and control characters carry no ink; the typesetter decides hyphenation itself.
    const text = p.text.replace(/[\u00AD\u0000-\u0008\u000E-\u001F\u007F\u200B-\u200F\u202A-\u202E\u2060-\u2064\uFEFF]/g, "");
    if (text === "") continue;
    const parts = text.split(SPACE_SPLIT);
    for (let i = 0; i < parts.length; i++) {
      const part = parts[i]!;
      if (part === "") continue;
      if (i % 2 === 1) {
        const last = out[out.length - 1];
        if (!last || last.k !== GLUE) out.push({ k: GLUE, w: p.st.spaceWidth ?? boxWidth(measure, p.st, " "), st: p.st });
        continue;
      }
      const subs = part.length > 3 && /[-\u2013\u2014]/.test(part) ? part.split(OPT_SPLIT) : [part];
      for (let s = 0; s < subs.length; s++) {
        const sub = subs[s]!;
        if (sub === "") continue;
        if (s > 0) out.push({ k: OPT });
        out.push({ k: BOX, text: sub, w: boxWidth(measure, p.st, sub), st: p.st });
      }
    }
  }
  return out;
}

export interface Seg {
  text: string;
  st: RunStyle;
  w: number;
  space: boolean;
}

export interface Line {
  segs: Seg[];
  /** Natural width: boxes plus unstretched spaces. */
  width: number;
  spaces: number;
  spaceWidth: number;
  /** Total width the spaces of this line may gain when justified. */
  stretch: number;
  /** How the line ends: wrapped, wrapped inside a word, hard break, or end of paragraph. */
  end: "soft" | "hyphen" | "hard" | "para";
  /** Broken in the middle of an unbreakable word: never justified. */
  forced?: boolean;
}

export interface BreakOpts {
  measure: MeasureFn;
  /** Lines will be justified: spaces may shrink to fit a word. */
  justify: boolean;
  hyphenate?: (word: string) => string[];
  hyphenCache?: Map<string, number[]>;
}

const WORD_CORE = /^([^\p{L}]*)(\p{L}+)([^\p{L}]*)$/u;

/** Legal hyphenation offsets inside `text` (indices where the tail starts). */
function hyphenPoints(text: string, o: BreakOpts): number[] {
  if (!o.hyphenate) return [];
  const cached = o.hyphenCache?.get(text);
  if (cached) return cached;
  let points: number[] = [];
  const m = WORD_CORE.exec(text);
  if (m) {
    const lead = m[1]!;
    const core = m[2]!;
    if (core.length >= MIN_WORD) {
      let parts: string[] = [];
      try {
        parts = o.hyphenate(core);
      } catch {
        parts = [];
      }
      // The hyphenator is injected: accept its answer only if it is a real partition of the word.
      if (Array.isArray(parts) && parts.length > 1 && parts.every((s) => typeof s === "string") && parts.join("") === core) {
        let at = 0;
        for (let i = 0; i < parts.length - 1; i++) {
          at += parts[i]!.length;
          if (at >= MIN_BEFORE && core.length - at >= MIN_AFTER) points.push(lead.length + at);
        }
      }
    }
  }
  if (points.length === 0) points = NO_POINTS;
  o.hyphenCache?.set(text, points);
  return points;
}
const NO_POINTS: number[] = [];

export type WidthAt = (line: number, continuation: boolean) => number;

/**
 * Break tokens into lines. `widthAt(i, continuation)` is the width available
 * to line `i`; `continuation` tells whether the previous line was wrapped
 * (as opposed to ended by a hard break), which verse uses for its hanging
 * indent.
 */
export function breakLines(tokens: Token[], widthAt: WidthAt, o: BreakOpts): Line[] {
  if (o.justify) {
    const best = breakOptimal(tokens, widthAt, o);
    if (best) return best;
  }
  return breakGreedy(tokens, widthAt, o);
}

/**
 * First-fit breaking. Always terminates: every iteration either consumes a
 * token or emits a line, and a guard stops pathological inputs.
 */
export function breakGreedy(tokens: Token[], widthAt: WidthAt, o: BreakOpts): Line[] {
  const toks = tokens.slice();
  const lines: Line[] = [];
  const shrink = o.justify ? MIN_SPACE_SHRINK : 1;

  let lastBox = -1;
  for (let i = toks.length - 1; i >= 0; i--) {
    if (toks[i]!.k === BOX) {
      lastBox = i;
      break;
    }
  }
  if (lastBox < 0 && !toks.some((t) => t.k === BR)) return lines;

  let segs: Seg[] = [];
  let boxW = 0;
  let spW = 0;
  let nsp = 0;
  let room = 0;
  let pending: GlueTok | null = null;
  let hyphenRun = 0;
  let continuation = false;

  const finish = (end: Line["end"], forced = false): void => {
    lines.push({ segs, width: boxW + spW, spaces: nsp, spaceWidth: spW, stretch: room, end, forced: forced || undefined });
    hyphenRun = end === "hyphen" ? hyphenRun + 1 : 0;
    continuation = end === "soft" || end === "hyphen";
    segs = [];
    boxW = 0;
    spW = 0;
    nsp = 0;
    room = 0;
    pending = null;
  };
  const push = (boxes: BoxTok[]): void => {
    if (pending && segs.length > 0) {
      segs.push({ text: " ", st: pending.st, w: pending.w, space: true });
      spW += pending.w;
      room += maxSpaceWidth(pending.w, pending.st.size) - pending.w;
      nsp++;
    }
    pending = null;
    for (const b of boxes) {
      segs.push({ text: b.text, st: b.st, w: b.w, space: false });
      boxW += b.w;
    }
  };

  let chars = 0;
  for (const t of toks) if (t.k === BOX) chars += t.text.length;
  let budget = toks.length * 6 + chars * 2 + 64;

  let i = 0;
  while (i < toks.length) {
    if (--budget < 0) {
      // Cannot happen with a sane measurer; dump the rest on one line rather than loop.
      const rest: BoxTok[] = [];
      for (; i < toks.length; i++) {
        const t = toks[i]!;
        if (t.k === BOX) rest.push(t);
      }
      push(rest);
      break;
    }
    const t = toks[i]!;
    if (t.k === BR) {
      finish("hard");
      i++;
      continue;
    }
    if (t.k === GLUE) {
      if (segs.length > 0) pending = t;
      i++;
      continue;
    }
    if (t.k === OPT) {
      i++;
      continue;
    }

    // A word: consecutive boxes (style changes inside a word do not allow a break).
    let j = i;
    let wordW = 0;
    const word: BoxTok[] = [];
    while (j < toks.length) {
      const b = toks[j]!;
      if (b.k !== BOX) break;
      word.push(b);
      wordW += b.w;
      j++;
    }
    const avail = Math.max(1, widthAt(lines.length, continuation));
    const glue: GlueTok | null = pending;
    const glueW = glue !== null && segs.length > 0 ? (glue as GlueTok).w : 0;
    if (boxW + (spW + glueW) * shrink + wordW <= avail + EPS) {
      push(word);
      i = j;
      continue;
    }

    const lineEmpty = segs.length === 0;
    const isLastWord = j - 1 >= lastBox;
    let tryHyphen = o.hyphenate !== undefined && !isLastWord && hyphenRun < MAX_HYPHEN_RUN;
    if (tryHyphen && !lineEmpty) {
      const slack = avail - (boxW + spW);
      if (o.justify) tryHyphen = nsp === 0 || slack > room * HYPHENATE_ABOVE;
      else tryHyphen = slack > avail * 0.14;
    }
    if (tryHyphen) {
      const room = avail - boxW - (spW + glueW) * shrink;
      const split = splitWord(word, room, o);
      if (split) {
        push(split.head);
        finish("hyphen");
        toks.splice(i, j - i, ...split.tail);
        // `lastBox` may shift by the splice; recompute cheaply.
        lastBox += split.tail.length - (j - i);
        continue;
      }
    }
    if (!lineEmpty) {
      finish("soft");
      continue;
    }

    // The word alone is wider than the line: break it by characters.
    const first = word[0]!;
    const cut = charSplit(first, avail, o.measure);
    push([cut.head]);
    finish("soft", true);
    if (cut.tail) toks[i] = cut.tail;
    else i++;
  }

  if (segs.length > 0) finish("para");
  else if (lines.length > 0) lines[lines.length - 1]!.end = "para";
  return lines;
}

function splitWord(word: BoxTok[], room: number, o: BreakOpts): { head: BoxTok[]; tail: BoxTok[] } | null {
  const starts: number[] = [];
  let acc = 0;
  for (const b of word) {
    starts.push(acc);
    acc += b.w;
  }
  for (let k = word.length - 1; k >= 0; k--) {
    const start = starts[k]!;
    if (start >= room) continue;
    const b = word[k]!;
    const pts = hyphenPoints(b.text, o);
    for (let p = pts.length - 1; p >= 0; p--) {
      const at = pts[p]!;
      const headText = b.text.slice(0, at) + "-";
      const w = boxWidth(o.measure, b.st, headText);
      if (start + w <= room + EPS) {
        const tailText = b.text.slice(at);
        return {
          head: [...word.slice(0, k), { k: BOX, text: headText, w, st: b.st }],
          tail: [{ k: BOX, text: tailText, w: boxWidth(o.measure, b.st, tailText), st: b.st }, ...word.slice(k + 1)],
        };
      }
    }
  }
  return null;
}

/** Largest prefix (at least one character) of a box that fits `avail`. */
function charSplit(b: BoxTok, avail: number, measure: MeasureFn): { head: BoxTok; tail: BoxTok | null } {
  const cps = Array.from(b.text);
  if (cps.length <= 1) return { head: b, tail: null };
  let lo = 1;
  let hi = cps.length - 1;
  // Binary search: width grows with the prefix length.
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (boxWidth(measure, b.st, cps.slice(0, mid).join("")) <= avail) lo = mid;
    else hi = mid - 1;
  }
  const headText = cps.slice(0, lo).join("");
  const tailText = cps.slice(lo).join("");
  return {
    head: { k: BOX, text: headText, w: boxWidth(measure, b.st, headText), st: b.st },
    tail: { k: BOX, text: tailText, w: boxWidth(measure, b.st, tailText), st: b.st },
  };
}

// ---------------------------------------------------------------------------
// Total-fit breaking for justified paragraphs
// ---------------------------------------------------------------------------

/** A word, or a part of one between explicit break opportunities. */
interface Unit {
  boxes: BoxTok[];
  w: number;
  /** Space before the unit (null: first unit, or joined to the previous one after a hyphen or dash). */
  glue: GlueTok | null;
  /** Ways to break inside the unit, in reading order. */
  cuts: Cut[] | null;
}

interface Cut {
  head: BoxTok[];
  headW: number;
  tail: BoxTok[];
  tailW: number;
}

interface Node {
  /** The line ends after unit `unit` (cut < 0) or inside it at `cuts[cut]`. unit = -1 is the start. */
  unit: number;
  cut: number;
  line: number;
  demerits: number;
  prev: Node | null;
  /** Consecutive lines ending in a hyphen, including this one. */
  run: number;
  dead: boolean;
}

/** Cost of a hyphen, in demerits: about what a line with spaces at 1.45x costs. */
const HYPHEN_DEMERITS = 2600;
/** Extra cost of a second hyphen in a row. */
const HYPHEN_RUN_DEMERITS = 3200;
/** Cost of a last line that is a single short fragment. */
const RUNT_DEMERITS = 1800;
/** Cost of a line that cannot be justified within the limits and will be left ragged. */
const RAGGED_DEMERITS = 1e9;

function unitCuts(u: Unit, o: BreakOpts): Cut[] {
  if (u.cuts) return u.cuts;
  const cuts: Cut[] = [];
  let before = 0;
  for (let k = 0; k < u.boxes.length; k++) {
    const b = u.boxes[k]!;
    for (const at of hyphenPoints(b.text, o)) {
      const headText = b.text.slice(0, at) + "-";
      const tailText = b.text.slice(at);
      const hw = boxWidth(o.measure, b.st, headText);
      const tw = boxWidth(o.measure, b.st, tailText);
      let after = 0;
      for (let j = k + 1; j < u.boxes.length; j++) after += u.boxes[j]!.w;
      cuts.push({
        head: [...u.boxes.slice(0, k), { k: BOX, text: headText, w: hw, st: b.st }],
        headW: before + hw,
        tail: [{ k: BOX, text: tailText, w: tw, st: b.st }, ...u.boxes.slice(k + 1)],
        tailW: tw + after,
      });
    }
    before += b.w;
  }
  u.cuts = cuts;
  return cuts;
}

/**
 * Break one run of text (no hard breaks inside) so that the sum of the
 * lines' demerits is minimal. Returns `null` when a unit is wider than a
 * line; the caller then falls back to the greedy breaker, which can split
 * words by characters.
 */
function fitSegment(units: Unit[], firstLine: number, firstContinuation: boolean, lastOfParagraph: boolean, widthAt: WidthAt, o: BreakOpts): Line[] | null {
  const n = units.length;
  // Prefix sums over units: box widths, and the glue before each unit.
  const sw = new Float64Array(n + 1);
  const sg = new Float64Array(n + 1);
  const sn = new Int32Array(n + 1);
  const sx = new Float64Array(n + 1);
  const sl = new Float64Array(n + 1);
  for (let i = 0; i < n; i++) {
    const u = units[i]!;
    const g = i > 0 ? u.glue : null;
    sw[i + 1] = sw[i]! + u.w;
    sg[i + 1] = sg[i]! + (g ? g.w : 0);
    sn[i + 1] = sn[i]! + (g ? 1 : 0);
    sx[i + 1] = sx[i]! + (g ? normalSpaceWidth(g.w, g.st.size) - g.w : 0);
    sl[i + 1] = sl[i]! + (g ? maxSpaceWidth(g.w, g.st.size) - g.w : 0);
  }
  const shrinkRoom = 1 - MIN_SPACE_SHRINK;
  // Badness is measured against the comfortable stretch, feasibility against the hard limit.
  const comfort = COMFORT_STRETCH - 1;

  // Pass 0: normal tolerance. Pass 1: loose lines allowed. Pass 2: lines that cannot be justified at all are left ragged.
  for (let pass = 0; pass < 3; pass++) {
    const start: Node = { unit: -1, cut: -1, line: 0, demerits: 0, prev: null, run: 0, dead: false };
    let active: Node[] = [start];
    let final: Node | null = null;

    for (let e = 0; e < n && active.length > 0; e++) {
      const u = units[e]!;
      // Hyphen breaks inside the unit come first, then the break after it. The last word of a paragraph is never cut.
      const cuts = o.hyphenate && !(lastOfParagraph && e === n - 1) ? unitCuts(u, o) : NO_CUTS;
      for (let c = 0; c <= cuts.length; c++) {
        const whole = c === cuts.length;
        const cutIndex = whole ? -1 : c;
        const isFinal = whole && e === n - 1;
        const endW = whole ? u.w : cuts[c]!.headW;
        let best: Map<number, Node> | null = null;

        for (const a of active) {
          if (a.dead) continue;
          // The line starts with unit `s`: whole, or the tail left by a cut.
          const fromTail = a.cut >= 0;
          const s = fromTail ? a.unit : a.unit + 1;
          if (s > e) continue;
          // A line never consists of a fragment of a single word: within one word the only legal
          // lines are the whole word, or the tail left by an earlier cut.
          if (s === e && !whole) continue;
          let boxes: number;
          if (s === e) boxes = fromTail ? units[s]!.cuts![a.cut]!.tailW : endW;
          else boxes = (fromTail ? units[s]!.cuts![a.cut]!.tailW : units[s]!.w) + (sw[e]! - sw[s + 1]!) + endW;
          const spaces = sg[e + 1]! - sg[s + 1]!;
          const count = sn[e + 1]! - sn[s + 1]!;
          const avail = Math.max(1, widthAt(firstLine + a.line, a.line > 0 ? true : firstContinuation));

          if (boxes + spaces * MIN_SPACE_SHRINK > avail + EPS) {
            // Too long already, and every later break makes it longer: this node is done.
            if (whole || cuts.length === 0 || c === 0) a.dead = true;
            continue;
          }
          const natural = boxes + spaces;
          let cost: number;
          if (isFinal) {
            // The last line is set ragged: any length will do, but a lone stub is ugly.
            cost = 1;
            if (lastOfParagraph && a.line > 0 && count === 0 && natural < avail * 0.12) cost += RUNT_DEMERITS;
          } else if (natural > avail) {
            const r = spaces > 0 ? (natural - avail) / (spaces * shrinkRoom) : 1;
            cost = (1 + 100 * r * r * r) ** 2;
          } else {
            const slack = avail - natural;
            const r = spaces > 0 ? slack / (spaces * comfort) : Infinity;
            const room = pass === 0 ? sx[e + 1]! - sx[s + 1]! : sl[e + 1]! - sl[s + 1]!;
            if (slack <= room + EPS) cost = (1 + 100 * r * r * r) ** 2;
            // Ragged as a last resort, and then as full as possible.
            else if (pass === 2) cost = RAGGED_DEMERITS * (1 + 8 * (slack / avail) ** 2);
            else continue;
          }
          if (!whole) {
            if (a.run >= MAX_HYPHEN_RUN) continue;
            cost += HYPHEN_DEMERITS + (a.run > 0 ? HYPHEN_RUN_DEMERITS : 0);
          }
          const total = a.demerits + cost;
          const line = a.line + 1;
          const run = whole ? 0 : a.run + 1;
          // One survivor per line count and per hyphen run: what may follow a break depends on both
          // (line widths vary beside a drop capital; a third hyphen in a row is not allowed).
          const key = line * 4 + run;
          best ??= new Map();
          const cur = best.get(key);
          if (!cur || total < cur.demerits) best.set(key, { unit: e, cut: cutIndex, line, demerits: total, prev: a, run, dead: false });
        }

        if (best) {
          if (isFinal) {
            for (const node of best.values()) if (!final || node.demerits < final.demerits) final = node;
          } else {
            for (const node of best.values()) active.push(node);
          }
        }
      }
      if (active.length > 24) active = active.filter((a) => !a.dead);
    }

    if (!final) continue;
    // Walk back from the best final node and build the lines.
    const chain: Node[] = [];
    for (let node: Node | null = final; node && node.prev; node = node.prev) chain.push(node);
    chain.reverse();
    const lines: Line[] = [];
    for (const node of chain) {
      const a = node.prev!;
      const fromTail = a.cut >= 0;
      const s = fromTail ? a.unit : a.unit + 1;
      const segs: Seg[] = [];
      let boxW = 0;
      let spW = 0;
      let nsp = 0;
      let room = 0;
      for (let i = s; i <= node.unit; i++) {
        const u = units[i]!;
        if (i > s && u.glue) {
          segs.push({ text: " ", st: u.glue.st, w: u.glue.w, space: true });
          spW += u.glue.w;
          room += maxSpaceWidth(u.glue.w, u.glue.st.size) - u.glue.w;
          nsp++;
        }
        let boxes = u.boxes;
        if (i === s && fromTail) boxes = u.cuts![a.cut]!.tail;
        else if (i === node.unit && node.cut >= 0) boxes = u.cuts![node.cut]!.head;
        for (const b of boxes) {
          segs.push({ text: b.text, st: b.st, w: b.w, space: false });
          boxW += b.w;
        }
      }
      const last = node === final;
      lines.push({ segs, width: boxW + spW, spaces: nsp, spaceWidth: spW, stretch: room, end: node.cut >= 0 ? "hyphen" : last ? (lastOfParagraph ? "para" : "hard") : "soft" });
    }
    return lines;
  }
  return null;
}
const NO_CUTS: Cut[] = [];

/** Total-fit breaking of a whole paragraph; hard breaks split it into independently fitted runs. */
export function breakOptimal(tokens: Token[], widthAt: WidthAt, o: BreakOpts): Line[] | null {
  const out: Line[] = [];
  let units: Unit[] = [];
  let glue: GlueTok | null = null;
  let joined = false;
  let lastBox = -1;
  for (let i = tokens.length - 1; i >= 0; i--) {
    if (tokens[i]!.k === BOX) {
      lastBox = i;
      break;
    }
  }
  if (lastBox < 0) return null;

  const flush = (lastOfParagraph: boolean): boolean => {
    if (units.length === 0) {
      // An empty line (two hard breaks in a row).
      out.push({ segs: [], width: 0, spaces: 0, spaceWidth: 0, stretch: 0, end: lastOfParagraph ? "para" : "hard" });
      return true;
    }
    const prev = out[out.length - 1];
    const lines = fitSegment(units, out.length, prev !== undefined && prev.end !== "hard", lastOfParagraph, widthAt, o);
    units = [];
    glue = null;
    joined = false;
    if (!lines) return false;
    for (const l of lines) out.push(l);
    return true;
  };

  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i]!;
    if (t.k === BR) {
      if (i > lastBox) break;
      if (!flush(false)) return null;
    } else if (t.k === GLUE) {
      glue = t;
    } else if (t.k === OPT) {
      joined = true;
    } else {
      const prev = units[units.length - 1];
      const prevTok = tokens[i - 1];
      if (prev && prevTok && prevTok.k === BOX) {
        // Style change inside a word: same unit.
        prev.boxes.push(t);
        prev.w += t.w;
      } else {
        units.push({ boxes: [t], w: t.w, glue: units.length > 0 && !joined ? glue : null, cuts: null });
      }
      glue = null;
      joined = false;
    }
  }
  if (units.length > 0 && !flush(true)) return null;
  const last = out[out.length - 1];
  if (last) last.end = "para";
  return out;
}

export interface LinkFragment {
  x: number;
  w: number;
  link: Link;
  label: string;
}

export interface PlacedLine {
  items: DrawItem[];
  links: LinkFragment[];
  /** Plain text of the line, without the hyphen added by a word break. */
  text: string;
  /** The line ends inside a word: the next line continues it without a space. */
  glue: boolean;
  justified: boolean;
  /** Left and right ink edges. */
  x0: number;
  x1: number;
}

/**
 * Position the segments of a line. `x` is the left edge of the line box,
 * `baseline` its baseline, `width` the available width.
 */
export function placeLine(line: Line, x: number, baseline: number, width: number, align: Align, block?: string): PlacedLine {
  const natural = line.width;
  let perSpace = 0;
  let offset = 0;
  let justified = false;

  const wrapped = line.end === "soft" || line.end === "hyphen";
  if (line.spaces > 0 && natural > width + EPS) {
    // The line was accepted with compressed spaces.
    perSpace = (width - natural) / line.spaces;
    justified = align === "justify" && wrapped;
  } else if (align === "justify" && wrapped && !line.forced && line.spaces > 0 && line.spaceWidth > 0) {
    const extra = Math.max(0, width - natural);
    if (extra <= line.stretch + EPS) {
      // Each space takes a share of the slack proportional to its own room to stretch.
      perSpace = extra / line.spaces;
      justified = true;
    }
  }
  typesetStats.lines++;
  if (line.end === "hyphen") typesetStats.hyphens++;
  if (justified) {
    typesetStats.justified++;
    const k = line.stretch > 0 ? Math.max(0, width - natural) / line.stretch : 0;
    typesetStats.stretchSum += k;
    if (k > typesetStats.stretchMax) typesetStats.stretchMax = k;
  } else if (align === "justify" && wrapped) typesetStats.ragged++;
  if (!justified && perSpace === 0) {
    if (align === "center") offset = (width - natural) / 2;
    else if (align === "right") offset = width - natural;
  }

  const items: DrawItem[] = [];
  const links: LinkFragment[] = [];
  let cx = x + offset;
  const x0 = cx;
  let cur: { text: string; st: RunStyle; x: number } | null = null;
  let frag: { link: Link; st: RunStyle; x0: number; x1: number; label: string } | null = null;
  let text = "";

  const flush = (): void => {
    if (!cur) return;
    const it: DrawItem = { kind: "text", x: cur.x, y: baseline, text: cur.text, font: cur.st.font, color: cur.st.color };
    if (cur.st.letterSpacing) it.letterSpacing = cur.st.letterSpacing;
    if (block !== undefined) it.block = block;
    items.push(it);
    cur = null;
  };
  const closeFrag = (): void => {
    if (!frag) return;
    const f = frag;
    frag = null;
    // Every fragment of a wrapped link is labelled with the whole link text.
    const label = (f.st.linkLabel ?? f.label).trim();
    if (label === "" || f.x1 <= f.x0) return;
    links.push({ x: f.x0, w: f.x1 - f.x0, link: f.link, label });
    // The underline is one stroke per fragment so that it also runs under stretched spaces.
    const uy = baseline + Math.max(1.5, f.st.size * 0.13);
    const ul: DrawItem = { kind: "line", x1: f.x0, y1: uy, x2: f.x1, y2: uy, color: f.st.color, width: Math.max(0.75, f.st.size * 0.05) };
    if (block !== undefined) ul.block = block;
    items.push(ul);
  };

  const segs = line.segs;
  for (let s = 0; s < segs.length; s++) {
    const seg = segs[s]!;
    const start = cx;
    if (seg.space) {
      if (perSpace === 0 && cur && cur.st === seg.st && seg.st.spaceWidth === undefined) cur.text += " ";
      else flush();
      cx += seg.w + perSpace;
    } else {
      if (cur && cur.st === seg.st) cur.text += seg.text;
      else {
        flush();
        cur = { text: seg.text, st: seg.st, x: cx };
      }
      cx += seg.w;
    }
    text += seg.text;
    const link = seg.st.link;
    if (link) {
      if (frag && frag.link === link) {
        // A trailing space never extends the underline.
        if (!seg.space) frag.x1 = cx;
        frag.label += seg.text;
      } else if (!seg.space) {
        closeFrag();
        frag = { link, st: seg.st, x0: start, x1: cx, label: seg.text };
      }
    } else closeFrag();
  }
  flush();
  closeFrag();

  const glue = line.end === "hyphen";
  if (glue && text.endsWith("-")) text = text.slice(0, -1);
  return { items, links, text, glue, justified, x0, x1: cx };
}

/**
 * Break plain text into at most-`maxW` lines and even them out: titles and
 * captions look better as two lines of similar length than as a long line
 * and an orphaned word. Words wider than `maxW` stay whole (the caller
 * shrinks the type until they fit).
 */
export function balanceLines(text: string, maxW: number, widthOf: (s: string) => number): string[] {
  const words = text.split(/\s+/).filter((w) => w !== "");
  if (words.length === 0) return [];
  const greedy = (limit: number): string[] => {
    const lines: string[] = [];
    let cur = "";
    for (const w of words) {
      const next = cur === "" ? w : `${cur} ${w}`;
      if (cur !== "" && widthOf(next) > limit) {
        lines.push(cur);
        cur = w;
      } else cur = next;
    }
    lines.push(cur);
    return lines;
  };
  const base = greedy(maxW);
  if (base.length < 2) return base;
  let lo = 0;
  for (const w of words) lo = Math.max(lo, widthOf(w));
  lo = Math.min(lo, maxW);
  let hi = maxW;
  let best = base;
  for (let it = 0; it < 12 && hi - lo > 1; it++) {
    const mid = (lo + hi) / 2;
    const attempt = greedy(mid);
    if (attempt.length <= base.length) {
      best = attempt;
      hi = mid;
    } else lo = mid;
  }
  return best;
}

/** Cut `text` with an ellipsis so that it fits `maxW`. */
export function ellipsize(text: string, maxW: number, widthOf: (s: string) => number): string {
  if (widthOf(text) <= maxW) return text;
  const cps = Array.from(text);
  let lo = 0;
  let hi = cps.length;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (widthOf(cps.slice(0, mid).join("").trimEnd() + "\u2026") <= maxW) lo = mid;
    else hi = mid - 1;
  }
  return lo === 0 ? "" : cps.slice(0, lo).join("").trimEnd() + "\u2026";
}
