/**
 * Fixed `page` compositions: exactly one page, no margins, every child
 * placed by its `frame` (percent of the page box).
 *
 * Type in a fixed page scales with the page, not with the reader's font
 * setting: a composition is a picture and has to keep its proportions on
 * every screen. Text that still overflows its frame is shrunk in steps,
 * down to 50%, as the language reference promises.
 */
import { holds } from "../core/state";
import type { Block, BlockOf, Frame } from "../ir";
import type { Lx } from "./context";
import { addBlock, buttonItems, newFlow, type FlowEnv } from "./blocks";
import { Stack } from "./paginate";
import { mergeSemantics, type FixedPayload, type Row, type RowSem } from "./rows";
import type { DrawItem, Region, Widget } from "./types";

const DEFAULT_FRAME: Frame = { x: 8, y: 8, w: 84, h: 84 };
const MIN_SHRINK = 0.5;

function num(v: unknown, fallback: number, lo: number, hi: number): number {
  return typeof v === "number" && Number.isFinite(v) ? Math.max(lo, Math.min(hi, v)) : fallback;
}

function stackedHeight(rows: Row[]): number {
  let h = 0;
  for (let i = 0; i < rows.length; i++) {
    const r = rows[i]!;
    if (r.kind !== "content") continue;
    h += r.h;
    const prev = rows[i - 1];
    if (prev) h += Math.max(prev.gapAfter, r.gapBefore);
  }
  return h;
}

export function buildFixed(lx: Lx, page: BlockOf<"page">): FixedPayload {
  const g = lx.geo;
  const W = g.pageW;
  const H = g.pageH;
  const items: DrawItem[] = [];
  const regions: Region[] = [];
  const sems: RowSem[] = [];
  const ids: string[] = [];
  const B0 = W / 29;
  const L0 = B0 * g.leading;

  const place = (b: Block, inherited: FlowEnv["anim"]): void => {
    const f = b.frame && typeof b.frame === "object" ? b.frame : DEFAULT_FRAME;
    // Frames may bleed off the page a little (full-bleed art), not wander away from it.
    const fx = (num(f.x, 0, -50, 150) / 100) * W;
    const fy = (num(f.y, 0, -50, 150) / 100) * H;
    const fw = Math.max(1, (num(f.w, 100, 0, 200) / 100) * W);
    const fh = Math.max(1, (num(f.h, 100, 0, 200) / 100) * H);
    const anim = b.anim && typeof b.anim === "object" ? { anim: b.anim, block: b.id } : inherited;
    const from = items.length;

    if (b.type === "image") {
      const res = lx.resources.get(b.resource);
      if (res && res.kind === "image") {
        items.push({ kind: "image", x: fx, y: fy, w: fw, h: fh, resource: b.resource, fit: b.fit === "cover" ? "cover" : "contain", block: b.id });
        sems.push({ key: b.id, role: "image", text: res.alt ?? "", block: b.id });
      }
    } else if (b.type === "button") {
      const label = typeof b.label === "string" ? b.label : "";
      for (const it of buttonItems(lx, b.id, label, fx, fy, fw, fh)) items.push(it);
      regions.push({ type: "button", x: fx, y: fy, w: fw, h: fh, block: b.id, label, actions: Array.isArray(b.actions) ? b.actions : [] });
    } else if (b.type === "video" || b.type === "audio" || b.type === "gallery") {
      placeMedia(lx, b, fx, fy, fw, fh, items, regions, sems);
    } else {
      // Everything else is flowed inside the frame, from its top, and shrunk until it fits.
      let rows: Row[] = [];
      for (let s = 1; ; s *= 0.92) {
        const scale = Math.max(MIN_SHRINK, s);
        const env: FlowEnv = { x: 0, w: fw, B: B0 * scale, L: L0 * scale, snap: false, ids: [], boxes: [], maxH: fh, fixed: true, depth: 1 };
        const flow = newFlow();
        addBlock(lx, flow, env, { ...b, anim: undefined });
        rows = flow.rows;
        if (scale <= MIN_SHRINK || stackedHeight(rows) <= fh + 0.5) break;
      }
      const st = new Stack(lx, fx, fy, 0);
      let first = true;
      for (const r of rows) {
        if (r.kind !== "content") continue;
        if (first) r.gapBefore = 0;
        first = false;
        st.add(r);
      }
      st.finishDeco();
      for (const it of st.deco) items.push(it);
      for (const it of st.items) items.push(it);
      for (const r of st.regions) regions.push(r);
      for (const s of st.sems) sems.push(s);
    }
    if (anim) {
      for (let i = from; i < items.length; i++) {
        const it = items[i]!;
        it.anim = anim.anim;
        it.block = anim.block;
      }
    }
  };

  const walk = (blocks: Block[] | undefined, inherited: FlowEnv["anim"], depth: number): void => {
    if (!Array.isArray(blocks) || depth > 24) return;
    for (const b of blocks) {
      if (!b || typeof b !== "object" || typeof b.id !== "string") continue;
      if (b.type === "group") {
        if (!holds(b.when, lx.input.state)) continue;
        ids.push(b.id);
        walk(b.blocks, b.anim && typeof b.anim === "object" ? { anim: b.anim, block: b.id } : inherited, depth + 1);
        continue;
      }
      ids.push(b.id);
      collectIds(b, ids, lx);
      if (b.type === "page" || b.type === "page_break" || b.type === "space") continue;
      place(b, inherited);
    }
  };
  walk(page.blocks, page.anim && typeof page.anim === "object" ? { anim: page.anim, block: page.id } : undefined, 0);

  const out: FixedPayload = { items, regions: dropCovered(regions), semantics: mergeSemantics(sems), ids };
  if (typeof page.background === "string" && /^#[0-9a-f]{3,8}$/i.test(page.background)) out.background = page.background;
  return out;
}

/** Ids of visible blocks nested in a framed child (a box inside a frame). */
function collectIds(b: Block, ids: string[], lx: Lx): void {
  if (b.type !== "box" && b.type !== "group") return;
  if (!Array.isArray(b.blocks)) return;
  for (const c of b.blocks) {
    if (!c || typeof c !== "object" || typeof c.id !== "string") continue;
    if (c.type === "group" && !holds(c.when, lx.input.state)) continue;
    ids.push(c.id);
    collectIds(c, ids, lx);
  }
}

function placeMedia(
  lx: Lx,
  b: BlockOf<"video"> | BlockOf<"audio"> | BlockOf<"gallery">,
  x: number,
  y: number,
  w: number,
  h: number,
  items: DrawItem[],
  regions: Region[],
  sems: RowSem[],
): void {
  if (b.type === "gallery") {
    const resources = (Array.isArray(b.resources) ? b.resources : []).filter((r) => lx.resources.get(r)?.kind === "image");
    if (resources.length === 0) return;
    const widget: Widget = { type: "gallery", resources, count: resources.length };
    items.push({ kind: "widget", widget, x, y, w, h, block: b.id });
    regions.push({ type: "gallery", x, y, w, h, block: b.id, resources });
    resources.forEach((r, i) => sems.push({ key: `${b.id}/${i}`, role: "image", text: lx.resources.get(r)?.alt ?? "", block: b.id }));
    return;
  }
  const res = lx.resources.get(b.resource);
  if (!res || res.kind !== b.type) return;
  if (b.type === "video") {
    const widget: Widget = { type: "video", resource: b.resource };
    if (res.poster && lx.resources.get(res.poster)?.kind === "image") widget.poster = res.poster;
    items.push({ kind: "widget", widget, x, y, w, h, block: b.id });
    regions.push({ type: "video", x, y, w, h, block: b.id, resource: b.resource, autoplay: b.autoplay === true, loop: b.loop === true, muted: b.muted === true });
    sems.push({ key: b.id, role: "image", text: res.alt ?? res.title ?? "", block: b.id });
  } else {
    const title = res.title ?? "";
    items.push({ kind: "widget", widget: { type: "audio", resource: b.resource, title }, x, y, w, h, block: b.id });
    regions.push({ type: "audio", x, y, w, h, block: b.id, resource: b.resource });
    if (title) sems.push({ key: b.id, role: "note", text: title, block: b.id });
  }
}

/**
 * Regions must not overlap. Frames are placed by hand and may be stacked
 * (a button over full-page art); the one painted last is on top and wins.
 */
function dropCovered(regions: Region[]): Region[] {
  const keep: Region[] = [];
  for (let i = 0; i < regions.length; i++) {
    const a = regions[i]!;
    let covered = false;
    for (let j = i + 1; j < regions.length && !covered; j++) {
      const b = regions[j]!;
      covered = a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
    }
    if (!covered) keep.push(a);
  }
  return keep;
}
