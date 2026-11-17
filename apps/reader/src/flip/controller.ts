/**
 * The book as a stack of sheets, and the physics of turning them.
 *
 * A book is a list of *sheets* in stacking order: the front board, the paper
 * leaves, the back board. `pos` is how many sheets lie on the left:
 *
 *   pos = 0            closed, front cover up
 *   pos = 1 … S − 1    open; left shows the back of sheet pos − 1, right the
 *                      front of sheet pos
 *   pos = S            closed, back cover up
 *
 * Sheets in the air are *flyers*. Each flyer is a damped spring on its turn
 * progress p ∈ [0, 1]; springs (rather than fixed-duration tweens) mean a
 * turn can be reversed, grabbed or released mid-flight without a
 * discontinuity, and every turn uses the same few constants, so the motion
 * stays consistent. Several flyers may be airborne at once — quick taps or a
 * long jump fan the leaves like the reference animation — and they are kept
 * in stacking order so leaves never pass through each other.
 *
 * This module is pure state + arithmetic (no DOM, no clock): the app feeds
 * it time steps and pointer positions and reads back a `ScenePose` to draw.
 */
import { type Camera, type LeafPose, ease, solveProgress } from "./geometry";

export type SurfaceKey = string;

export interface Sheet {
  front: SurfaceKey;
  back: SurfaceKey;
  /** Rigid cover board (no bending, thicker, slower). */
  board: boolean;
  /** Single-page mode: the back shows the front's ink faintly, mirrored. */
  mirrorBack?: boolean;
}

/** Tuning constants. One place, so every turn feels like the same book. */
export const MOTION = {
  /** Shape family while dragging: the free edge leads (fold width = 1/λ). */
  lambdaDrag: 2.4,
  /** Shape family for taps, keys and riffles: the spine leads, the edge trails. */
  lambdaThrow: -0.42,
  /** Natural frequencies (rad/s) of the turn springs; all critically damped. */
  omegaThrow: 13.5,
  omegaRelease: 16,
  omegaBoard: 8.6,
  omegaHeld: 46,
  /** Rate (1/s) at which shape parameters (λ, κ) relax to their targets. */
  shapeRate: 14,
  /** Minimum separation in p between adjacent airborne leaves. */
  gap: 0.035,
  /** Delay between leaves of a riffle, seconds. */
  stagger: 0.05,
  /** Most leaves animated for one jump. */
  maxRiffle: 6,
  /** Most leaves in the air at once before the oldest is settled early. */
  maxFlyers: 9,
  /** Corner-lift skew when a page is grabbed away from its middle. */
  cornerSkew: 0.34,
  /** Camera height above the page plane, in page widths. */
  cameraDistance: 5.2,
} as const;

export interface Flyer {
  sheet: number;
  /** Surfaces shown on this flyer (ghost pages during a riffle). */
  front: SurfaceKey;
  back: SurfaceKey;
  board: boolean;
  mirrorBack: boolean;
  p: number;
  v: number;
  target: 0 | 1;
  dir: 1 | -1;
  lambda: number;
  lambdaTarget: number;
  kappa: number;
  kappaTarget: number;
  vGrab: number;
  uGrab: number;
  held: boolean;
  /** Progress the pointer asks for while held. */
  heldP: number;
  /** Seconds before the flyer starts moving (riffle stagger). */
  delay: number;
  omega: number;
}

export interface FlyerPose extends LeafPose {
  front: SurfaceKey;
  back: SurfaceKey;
  board: boolean;
  mirrorBack: boolean;
}

export interface ScenePose {
  /** Surface lying face-up on the left / right, or null when that side is empty. */
  left: SurfaceKey | null;
  right: SurfaceKey | null;
  /** The resting surface is a cover board (drawn board-sized, not page-sized). */
  leftIsBoard: boolean;
  rightIsBoard: boolean;
  /** The opened boards lying under the page block. */
  leftBase: boolean;
  rightBase: boolean;
  flyers: FlyerPose[];
  /** Fraction of the paper leaves resting on each side (drives the visible stack thickness). */
  stackLeft: number;
  stackRight: number;
  /** Camera pan (world x of the view centre) and zoom (1 = reading size). */
  camX: number;
  zoom: number;
  /** 0 when closed, 1 when fully open: used to fade reader chrome and overlays. */
  openness: number;
}

export interface ControllerOptions {
  sheets: Sheet[];
  /** Two-page spread (view centred on the spine) or single page (centred on the right page). */
  spread: boolean;
  /** Surface used for the leaves in the middle of a riffle. */
  ghost: SurfaceKey;
}

export class FlipController {
  sheets: Sheet[];
  spread: boolean;
  ghost: SurfaceKey;
  /** Number of sheets that are (or are heading) on the left. */
  pos = 0;
  flyers: Flyer[] = [];
  /** A jump that still has stages to run (opening the board, then riffling, …). */
  private pending: number | null = null;

  constructor(options: ControllerOptions) {
    this.sheets = options.sheets;
    this.spread = options.spread;
    this.ghost = options.ghost;
  }

  get count(): number {
    return this.sheets.length;
  }

  get animating(): boolean {
    return this.flyers.length > 0 || this.pending !== null;
  }

  /** Replace the book model (re-layout, mode change) keeping a position. */
  reset(options: ControllerOptions, pos: number): void {
    this.sheets = options.sheets;
    this.spread = options.spread;
    this.ghost = options.ghost;
    this.flyers = [];
    this.pending = null;
    this.pos = Math.max(0, Math.min(this.count, Math.round(pos)));
  }

  /** Move without animation. */
  setPosition(pos: number): void {
    this.flyers = [];
    this.pending = null;
    this.pos = Math.max(0, Math.min(this.count, Math.round(pos)));
  }

  private makeFlyer(sheet: number, dir: 1 | -1): Flyer {
    const s = this.sheets[sheet]!;
    return {
      sheet,
      front: s.front,
      back: s.back,
      board: s.board,
      mirrorBack: s.mirrorBack ?? false,
      p: dir > 0 ? 0 : 1,
      v: 0,
      target: dir > 0 ? 1 : 0,
      dir,
      lambda: s.board ? 0 : MOTION.lambdaThrow,
      lambdaTarget: s.board ? 0 : MOTION.lambdaThrow,
      kappa: 0,
      kappaTarget: 0,
      vGrab: 0.5,
      uGrab: 1,
      held: false,
      heldP: dir > 0 ? 0 : 1,
      delay: 0,
      omega: s.board ? MOTION.omegaBoard : MOTION.omegaThrow,
    };
  }

  private insert(f: Flyer): void {
    this.flyers.push(f);
    this.flyers.sort((a, b) => a.sheet - b.sheet || 0);
    // Too many leaves in the air: land the one that is furthest along.
    while (this.flyers.filter((x) => !x.held).length > MOTION.maxFlyers) {
      let best = -1;
      let bestDist = Infinity;
      this.flyers.forEach((x, i) => {
        const d = Math.abs(x.target - x.p);
        if (!x.held && d < bestDist) {
          bestDist = d;
          best = i;
        }
      });
      if (best < 0) break;
      this.flyers.splice(best, 1);
    }
  }

  private syncPos(): void {
    // Sheets below every flyer rest on the left; pos counts those plus flyers heading left.
    if (this.flyers.length === 0) return;
    const lo = this.flyers[0]!.sheet;
    const firstRight = this.flyers.find((f) => f.target === 0);
    this.pos = firstRight ? firstRight.sheet : this.flyers[this.flyers.length - 1]!.sheet + 1;
    if (this.pos < lo) this.pos = lo;
  }

  /** Turn one sheet forward. Returns false at the end of the book. */
  next(): boolean {
    this.pending = null;
    if (this.pos >= this.count) return false;
    const sheet = this.pos;
    const existing = this.flyers.find((f) => f.sheet === sheet);
    if (existing) {
      if (existing.held) return false;
      existing.target = 1;
      existing.delay = 0;
    } else {
      this.insert(this.makeFlyer(sheet, 1));
    }
    this.pos = sheet + 1;
    return true;
  }

  /** Turn one sheet back. Returns false at the front cover. */
  prev(): boolean {
    this.pending = null;
    if (this.pos <= 0) return false;
    const sheet = this.pos - 1;
    const existing = this.flyers.find((f) => f.sheet === sheet);
    if (existing) {
      if (existing.held) return false;
      existing.target = 0;
      existing.delay = 0;
    } else {
      this.insert(this.makeFlyer(sheet, -1));
    }
    this.pos = sheet;
    return true;
  }

  /**
   * Animate to `target`. Short hops turn the sheets one after another; long
   * jumps riffle a handful of leaves (the ones in the middle show ghost
   * pages) so the book always arrives in under a second. Boards never join a
   * riffle: the cover opens first, and closes last.
   */
  jumpTo(target: number): void {
    target = Math.max(0, Math.min(this.count, Math.round(target)));
    if (target === this.pos && this.flyers.length === 0) return;
    this.pending = target;
    this.advancePending();
  }

  private advancePending(): void {
    const target = this.pending;
    if (target === null) return;
    if (target === this.pos) {
      this.pending = null;
      return;
    }
    const last = this.count - 1;
    const forward = target > this.pos;
    // Stage 1: get the nearer board out of the way and wait for it.
    const boardFlyer = this.flyers.find((f) => f.board);
    if (boardFlyer) {
      const passed = boardFlyer.target === 1 ? boardFlyer.p > 0.6 : boardFlyer.p < 0.4;
      if (!passed) return;
    }
    if (forward && this.pos === 0) {
      this.next();
      this.pending = target;
      return;
    }
    if (!forward && this.pos === this.count) {
      this.prev();
      this.pending = target;
      return;
    }
    // Stage 2: riffle the paper leaves up to (not including) the far board.
    const paperTarget = forward ? Math.min(target, last) : Math.max(target, 1);
    if (paperTarget !== this.pos) {
      // Leaves already in the air would collide with the riffle: land them.
      if (this.flyers.some((f) => !f.board)) {
        if (this.flyers.some((f) => !f.board && Math.abs(f.target - f.p) > 0.35)) return;
        this.flyers = this.flyers.filter((f) => f.board);
      }
      this.riffle(this.pos, paperTarget);
      this.pending = target === paperTarget ? null : target;
      return;
    }
    // Stage 3: the far board, once the leaves have mostly landed.
    if (this.flyers.some((f) => !f.board && Math.abs(f.target - f.p) > 0.5)) return;
    if (forward) this.next();
    else this.prev();
    this.pending = null;
  }

  private riffle(from: number, to: number): void {
    const forward = to > from;
    const n = Math.abs(to - from);
    const k = Math.min(n, MOTION.maxRiffle);
    // Sheets that travel: in a forward riffle sheet `from` leaves first and
    // sheet `to − 1` lands last; intermediate ones are stand-ins.
    for (let i = 0; i < k; i++) {
      const t = k === 1 ? 0 : i / (k - 1);
      const sheet = forward ? Math.round(from + t * (n - 1)) : Math.round(from - 1 - t * (n - 1));
      const f = this.makeFlyer(sheet, forward ? 1 : -1);
      const first = i === 0;
      const lastOne = i === k - 1;
      if (n > k) {
        // Only the outer faces of the fan are ever seen at rest.
        if (forward) {
          if (!first) f.front = this.ghost;
          if (!lastOne) f.back = this.ghost;
        } else {
          if (!first) f.back = this.ghost;
          if (!lastOne) f.front = this.ghost;
        }
        f.mirrorBack = f.mirrorBack && f.back !== this.ghost;
      }
      f.delay = i * MOTION.stagger;
      // A touch of variety so the fan does not look mechanical.
      f.kappaTarget = f.kappa = (i % 2 === 0 ? 1 : -1) * 0.05 * (1 + (i % 3));
      this.flyers.push(f);
    }
    this.flyers.sort((a, b) => a.sheet - b.sheet);
    this.pos = to;
  }

  // --- direct manipulation ---------------------------------------------------

  /**
   * Take hold of a page. `x` is the pointer on the page plane in page widths
   * from the spine (positive = right page), `v` its height on the page
   * (0 = bottom, 1 = top). Returns false when there is nothing to grab there.
   */
  grab(x: number, v: number, cam: Camera): boolean {
    this.pending = null;
    if (this.flyers.some((f) => f.held)) return false;
    const forward = x >= 0;
    const sheet = forward ? this.pos : this.pos - 1;
    if (sheet < 0 || sheet >= this.count) return false;
    let f = this.flyers.find((q) => q.sheet === sheet);
    if (!f) {
      // A sheet can only be picked up from the top of its pile.
      const blocked = this.flyers.some((q) => (forward ? q.sheet > sheet : q.sheet < sheet));
      if (blocked) return false;
      f = this.makeFlyer(sheet, forward ? 1 : -1);
      f.target = forward ? 0 : 1; // stays put unless the gesture completes
      this.insert(f);
    }
    f.held = true;
    f.delay = 0;
    f.v = 0;
    f.uGrab = Math.min(1, Math.max(0.2, Math.abs(x)));
    f.vGrab = Math.min(1, Math.max(0, v));
    f.lambdaTarget = f.board ? 0 : MOTION.lambdaDrag;
    if (!f.board && (f.p <= 0 || f.p >= 1)) f.lambda = MOTION.lambdaDrag;
    f.kappaTarget = f.board ? 0 : MOTION.cornerSkew * (2 * f.vGrab - 1) * (f.dir > 0 ? 1 : -1);
    f.heldP = f.p;
    this.dragTo(x, 0, cam);
    this.syncPos();
    return true;
  }

  /** Move the held page. `dy` is the vertical travel since the grab, in page heights (up = positive). */
  dragTo(x: number, dy: number, cam: Camera): void {
    const f = this.flyers.find((q) => q.held);
    if (!f) return;
    f.heldP = solveProgress({ dir: f.dir, lambda: f.lambda, kappa: 0, vGrab: f.vGrab }, f.uGrab, x, cam);
    if (!f.board) {
      const corner = MOTION.cornerSkew * (2 * f.vGrab - 1);
      const k = (corner - dy * 0.9) * (f.dir > 0 ? 1 : -1);
      f.kappaTarget = Math.max(-0.6, Math.min(0.6, k));
    }
  }

  /**
   * Let go. The turn completes if the page is past the middle or is moving
   * fast enough towards the other side; otherwise it falls back.
   */
  release(): void {
    const f = this.flyers.find((q) => q.held);
    if (!f) return;
    f.held = false;
    const projected = f.p + f.v * 0.16;
    f.target = projected > 0.5 ? 1 : 0;
    f.omega = f.board ? MOTION.omegaBoard : MOTION.omegaRelease;
    f.kappaTarget = 0;
    this.syncPosAfterRelease(f);
  }

  private syncPosAfterRelease(f: Flyer): void {
    // Flyers above a sheet that goes left must already be going left, and vice versa.
    for (const q of this.flyers) {
      if (q === f || q.held) continue;
      if (f.target === 1 && q.sheet < f.sheet) q.target = 1;
      if (f.target === 0 && q.sheet > f.sheet) q.target = 0;
    }
    this.syncPos();
    if (this.flyers.length === 1) this.pos = f.target === 1 ? f.sheet + 1 : f.sheet;
  }

  /** Abort a gesture (pointer cancelled): the page returns where it came from. */
  cancel(): void {
    const f = this.flyers.find((q) => q.held);
    if (!f) return;
    f.held = false;
    f.target = f.dir > 0 ? 0 : 1;
    f.omega = MOTION.omegaRelease;
    f.kappaTarget = 0;
    this.syncPosAfterRelease(f);
  }

  // --- integration -----------------------------------------------------------

  /** Advance the simulation by `dt` seconds. Returns true while anything is moving. */
  tick(dt: number): boolean {
    dt = Math.min(dt, 0.1);
    const steps = Math.max(1, Math.ceil(dt / (1 / 240)));
    const h = dt / steps;
    for (let s = 0; s < steps; s++) this.step(h);
    this.advancePending();
    return this.animating;
  }

  private step(h: number): void {
    const relax = 1 - Math.exp(-MOTION.shapeRate * h);
    for (const f of this.flyers) {
      f.lambda += (f.lambdaTarget - f.lambda) * relax;
      f.kappa += (f.kappaTarget - f.kappa) * relax;
      if (f.delay > 0) {
        f.delay -= h;
        continue;
      }
      const goal = f.held ? f.heldP : f.target;
      const w = f.held ? MOTION.omegaHeld : f.omega;
      // Critically damped spring, semi-implicit Euler.
      const a = w * w * (goal - f.p) - 2 * w * f.v;
      f.v += a * h;
      f.p += f.v * h;
      if (f.p <= 0) {
        f.p = 0;
        if (f.v < 0) f.v = 0;
      } else if (f.p >= 1) {
        f.p = 1;
        if (f.v > 0) f.v = 0;
      }
    }
    // Keep stacking order: a sheet lower in the right pile can never be ahead
    // of the one above it.
    for (let i = 1; i < this.flyers.length; i++) {
      const above = this.flyers[i - 1]!;
      const below = this.flyers[i]!;
      // The separation vanishes at both ends so that landing leaves do not queue up.
      const limit = Math.max(0, above.p - MOTION.gap * Math.sin(Math.PI * above.p));
      if (below.p > limit) {
        below.p = limit;
        if (below.v > above.v) below.v = above.v;
      }
    }
    // Retire flyers that have landed.
    this.flyers = this.flyers.filter((f) => f.held || f.delay > 0 || Math.abs(f.p - f.target) > 0.0015 || Math.abs(f.v) > 0.03);
  }

  // --- what to draw ----------------------------------------------------------

  /** World x of the view centre while reading. */
  private get readingPan(): number {
    return this.spread ? 0 : 0.5;
  }

  pose(): ScenePose {
    const S = this.count;
    const lo = this.flyers.length ? this.flyers[0]!.sheet : this.pos;
    const hi = this.flyers.length ? this.flyers[this.flyers.length - 1]!.sheet : this.pos - 1;
    const leftSheet = lo - 1;
    const rightSheet = hi + 1;
    const left = leftSheet >= 0 ? this.sheets[leftSheet]! : null;
    const right = rightSheet < S ? this.sheets[rightSheet]! : null;

    const front = this.flyers.find((f) => f.sheet === 0);
    const back = this.flyers.find((f) => f.sheet === S - 1);
    // Progress of each board: 0 = on the right, 1 = on the left.
    const frontP = front ? front.p : lo > 0 || (this.flyers.length === 0 && this.pos > 0) ? 1 : 0;
    const backP = back ? back.p : (this.flyers.length ? lo : this.pos) >= S ? 1 : 0;
    const pan = this.readingPan;
    const camX = 0.5 + (pan - 0.5) * ease(frontP) + (-0.5 - pan) * ease(backP);
    const openness = Math.min(ease(frontP), 1 - ease(backP));

    const leaves = Math.max(1, S - 2);
    const restingLeftLeaves = Math.max(0, Math.min(leaves, lo - 1));
    const restingRightLeaves = Math.max(0, Math.min(leaves, S - 2 - hi));

    return {
      left: left ? left.back : null,
      right: right ? right.front : null,
      leftIsBoard: left?.board === true && leftSheet === S - 1,
      rightIsBoard: right?.board === true && rightSheet === 0,
      leftBase: !front && leftSheet >= 0 && !(leftSheet === S - 1),
      rightBase: !back && rightSheet <= S - 1 && rightSheet > 0,
      flyers: this.flyers.map((f) => ({
        p: f.p,
        dir: f.dir,
        lambda: f.lambda,
        kappa: f.kappa,
        vGrab: f.vGrab,
        front: f.front,
        back: f.back,
        board: f.board,
        mirrorBack: f.mirrorBack,
      })),
      stackLeft: restingLeftLeaves / leaves,
      stackRight: restingRightLeaves / leaves,
      camX,
      zoom: 0.9 + 0.1 * openness,
      openness,
    };
  }
}

/** Build the sheet list for a book of `pages` pages. */
export function buildSheets(pages: number, spread: boolean): Sheet[] {
  const sheets: Sheet[] = [{ front: "front-cover", back: "endpaper-front", board: true }];
  if (spread) {
    for (let i = 0; i < pages; i += 2) {
      sheets.push({ front: `page:${i}`, back: i + 1 < pages ? `page:${i + 1}` : "blank", board: false });
    }
  } else {
    for (let i = 0; i < pages; i++) {
      sheets.push({ front: `page:${i}`, back: `page:${i}`, board: false, mirrorBack: true });
    }
  }
  sheets.push({ front: "endpaper-back", back: "back-cover", board: true });
  return sheets;
}

/** Position at which page `index` is visible. */
export function posForPage(index: number, spread: boolean): number {
  // In a spread, page 0 is alone on the right at pos 1; then pages (1, 2) at pos 2, (3, 4) at pos 3, …
  return spread ? Math.floor((index + 1) / 2) + 1 : index + 1;
}

/** Pages visible at `pos` (indices may be out of range at the ends). */
export function pagesAt(pos: number, spread: boolean): { left: number | null; right: number | null } {
  if (spread) return { left: pos >= 2 ? 2 * (pos - 2) + 1 : null, right: pos >= 1 ? 2 * (pos - 1) : null };
  return { left: null, right: pos >= 1 ? pos - 1 : null };
}
