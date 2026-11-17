import { describe, expect, it } from "vitest";
import { FlipController, MOTION, buildSheets, pagesAt, posForPage } from "../src/flip/controller";
import { type LeafPose, MESH_COLS, MESH_ROWS, VERTEX_SIZE, apparentX, buildLeaf, crossSection, leafPoint, solveProgress, tangent } from "../src/flip/geometry";

const families = [MOTION.lambdaDrag, MOTION.lambdaThrow, 0, 6, -2];

describe("leaf geometry", () => {
  it("lies flat at both ends of a turn, in both directions", () => {
    for (const lambda of families) {
      for (const dir of [1, -1] as const) {
        for (const u of [0, 0.3, 1]) {
          expect(tangent(0, u, lambda, dir)).toBeCloseTo(0, 9);
          expect(tangent(1, u, lambda, dir)).toBeCloseTo(Math.PI, 9);
        }
      }
    }
  });

  it("never stretches the sheet and never dips below the stack", () => {
    const n = 400;
    const xs = new Array<number>(n + 1);
    const zs = new Array<number>(n + 1);
    const ts = new Array<number>(n + 1);
    for (const lambda of families) {
      for (const dir of [1, -1] as const) {
        for (let p = 0; p <= 1.0001; p += 0.05) {
          crossSection(p, lambda, dir, n, xs, zs, ts);
          let length = 0;
          for (let i = 1; i <= n; i++) {
            length += Math.hypot(xs[i]! - xs[i - 1]!, zs[i]! - zs[i - 1]!);
            expect(zs[i]!).toBeGreaterThanOrEqual(0);
            expect(ts[i]!).toBeGreaterThanOrEqual(-1e-9);
            expect(ts[i]!).toBeLessThanOrEqual(Math.PI + 1e-9);
          }
          // The polyline of a smooth curve is slightly shorter than its arc; never longer.
          expect(length).toBeLessThanOrEqual(1 + 1e-6);
          expect(length).toBeGreaterThan(0.995);
          expect(xs[0]).toBe(0);
          expect(zs[0]).toBe(0);
        }
      }
    }
  });

  it("drag family: the free edge leads; throw family: the spine leads", () => {
    const mid = 0.4;
    expect(tangent(mid, 1, MOTION.lambdaDrag, 1)).toBeGreaterThan(tangent(mid, 0, MOTION.lambdaDrag, 1));
    expect(tangent(mid, 0, MOTION.lambdaThrow, 1)).toBeGreaterThan(tangent(mid, 1, MOTION.lambdaThrow, 1));
    // Backward travel mirrors the family: the same part still leads.
    expect(tangent(0.6, 1, MOTION.lambdaDrag, -1)).toBeLessThan(tangent(0.6, 0, MOTION.lambdaDrag, -1));
  });

  it("moves continuously: a small step in p moves every vertex a small distance", () => {
    const a = new Float32Array((MESH_COLS + 1) * (MESH_ROWS + 1) * VERTEX_SIZE);
    const b = new Float32Array(a.length);
    for (const lambda of [MOTION.lambdaDrag, MOTION.lambdaThrow]) {
      for (let p = 0; p < 1; p += 0.01) {
        const pose: LeafPose = { p, dir: 1, lambda, kappa: 0.3, vGrab: 0.1 };
        buildLeaf(pose, 1.4, 0, a);
        buildLeaf({ ...pose, p: p + 0.01 }, 1.4, 0, b);
        let worst = 0;
        for (let i = 0; i < a.length; i += VERTEX_SIZE) {
          worst = Math.max(worst, Math.hypot(a[i]! - b[i]!, a[i + 2]! - b[i + 2]!));
        }
        expect(worst).toBeLessThan(0.12);
      }
    }
  });

  it("the grabbed point tracks the pointer", () => {
    const cam = { x: 0, distance: MOTION.cameraDistance };
    for (const dir of [1, -1] as const) {
      const base = { dir, lambda: MOTION.lambdaDrag, kappa: 0, vGrab: 0.5 };
      let last = -1;
      for (let x = 0.85; x >= -0.85; x -= 0.05) {
        const p = solveProgress(base, 0.9, x, cam);
        const [px, pz] = leafPoint({ ...base, p }, 0.9, 0.5);
        expect(apparentX(px, pz, cam)).toBeCloseTo(x, 3);
        expect(p).toBeGreaterThanOrEqual(last); // dragging left only ever advances the turn
        last = p;
      }
      expect(solveProgress(base, 0.9, 5, cam)).toBe(0);
      expect(solveProgress(base, 0.9, -5, cam)).toBe(1);
    }
  });
});

function settle(c: FlipController, max = 6): number {
  let t = 0;
  while (c.tick(1 / 120) && t < max) t += 1 / 120;
  return t;
}

function make(pages: number, spread = true): FlipController {
  return new FlipController({ sheets: buildSheets(pages, spread), spread, ghost: "ghost" });
}

describe("sheet model", () => {
  it("maps pages to sheets", () => {
    const s = buildSheets(5, true);
    expect(s.map((x) => `${x.front}|${x.back}`)).toEqual([
      "front-cover|endpaper-front",
      "page:0|page:1",
      "page:2|page:3",
      "page:4|blank",
      "endpaper-back|back-cover",
    ]);
    expect(buildSheets(3, false).length).toBe(5);
    expect(posForPage(0, true)).toBe(1);
    expect(posForPage(1, true)).toBe(2);
    expect(posForPage(2, true)).toBe(2);
    expect(posForPage(3, true)).toBe(3);
    // Every page is visible at the position computed for it.
    for (const spread of [true, false]) {
      for (let i = 0; i < 9; i++) {
        const at = pagesAt(posForPage(i, spread), spread);
        expect(at.left === i || at.right === i).toBe(true);
      }
    }
    expect(posForPage(3, false)).toBe(4);
    expect(pagesAt(2, true)).toEqual({ left: 1, right: 2 });
    expect(pagesAt(1, true)).toEqual({ left: null, right: 0 });
    expect(pagesAt(4, false)).toEqual({ left: null, right: 3 });
  });
});

describe("flip controller", () => {
  it("starts closed on the front cover and ends closed on the back cover", () => {
    const c = make(4);
    let pose = c.pose();
    expect(pose.right).toBe("front-cover");
    expect(pose.rightIsBoard).toBe(true);
    expect(pose.left).toBeNull();
    expect(pose.camX).toBeCloseTo(0.5);
    expect(pose.openness).toBe(0);

    expect(c.next()).toBe(true);
    expect(c.flyers[0]!.board).toBe(true);
    settle(c);
    pose = c.pose();
    expect(pose.left).toBe("endpaper-front");
    expect(pose.right).toBe("page:0");
    expect(pose.camX).toBeCloseTo(0);
    expect(pose.openness).toBeCloseTo(1);
    expect(pose.leftBase && pose.rightBase).toBe(true);

    c.next();
    settle(c);
    expect(c.pose().left).toBe("page:1");
    expect(c.pose().right).toBe("page:2");
    c.next();
    settle(c);
    expect(c.pose().right).toBe("endpaper-back");
    c.next();
    settle(c);
    pose = c.pose();
    expect(c.pos).toBe(c.count);
    expect(pose.left).toBe("back-cover");
    expect(pose.leftIsBoard).toBe(true);
    expect(pose.right).toBeNull();
    expect(pose.camX).toBeCloseTo(-0.5);
    expect(c.next()).toBe(false);

    // And all the way back.
    while (c.prev()) settle(c);
    expect(c.pos).toBe(0);
    expect(c.pose().right).toBe("front-cover");
  });

  it("single-page mode keeps the view on the right-hand page", () => {
    const c = make(3, false);
    c.next();
    settle(c);
    expect(c.pose().camX).toBeCloseTo(0.5);
    expect(c.pose().right).toBe("page:0");
    c.next();
    settle(c);
    expect(c.pose().right).toBe("page:1");
  });

  it("a turn takes a consistent, sub-second time and progresses monotonically", () => {
    const c = make(20);
    c.setPosition(3);
    c.next();
    let last = 0;
    let t = 0;
    while (c.tick(1 / 60)) {
      const f = c.flyers[0];
      if (f) {
        expect(f.p).toBeGreaterThanOrEqual(last - 1e-9);
        last = f.p;
      }
      t += 1 / 60;
      expect(t).toBeLessThan(3);
    }
    expect(t).toBeGreaterThan(0.4);
    expect(t).toBeLessThan(1.3);
    const first = t;
    c.next();
    expect(Math.abs(settle(c) - first)).toBeLessThan(0.05);
  });

  it("is frame-rate independent", () => {
    const run = (hz: number) => {
      const c = make(20);
      c.setPosition(3);
      c.next();
      let t = 0;
      const samples: number[] = [];
      while (t < 0.3 - 1e-9) {
        c.tick(1 / hz);
        t += 1 / hz;
      }
      samples.push(c.flyers[0]?.p ?? 1);
      return samples[0]!;
    };
    expect(run(60)).toBeCloseTo(run(120), 2);
    expect(run(30)).toBeCloseTo(run(120), 2);
  });

  it("rapid taps put several leaves in the air, in stacking order", () => {
    const c = make(40);
    c.setPosition(2);
    for (let i = 0; i < 5; i++) {
      c.next();
      for (let k = 0; k < 8; k++) c.tick(1 / 120);
    }
    expect(c.pos).toBe(7);
    expect(c.flyers.length).toBeGreaterThanOrEqual(3);
    for (let k = 0; k < 400; k++) {
      c.tick(1 / 120);
      for (let i = 1; i < c.flyers.length; i++) {
        expect(c.flyers[i]!.p).toBeLessThanOrEqual(c.flyers[i - 1]!.p + 1e-9);
      }
    }
    expect(c.flyers.length).toBe(0);
    expect(c.pose().left).toBe("page:11");
    expect(c.pose().right).toBe("page:12");
  });

  it("can be reversed in mid-air", () => {
    const c = make(20);
    c.setPosition(3);
    c.next();
    for (let k = 0; k < 20; k++) c.tick(1 / 120);
    const mid = c.flyers[0]!.p;
    expect(mid).toBeGreaterThan(0);
    expect(mid).toBeLessThan(1);
    c.prev();
    expect(c.pos).toBe(3);
    expect(c.flyers.length).toBe(1); // the same leaf turns round; no new one is spawned
    settle(c);
    expect(c.pose().right).toBe("page:4");
  });

  it("drag: follows the pointer, completes past the middle, cancels before it", () => {
    const cam = { x: 0, distance: MOTION.cameraDistance };
    const c = make(20);
    c.setPosition(3);
    expect(c.grab(0.92, 0.1, cam)).toBe(true);
    expect(c.pos).toBe(3);
    for (let x = 0.9; x > -0.6; x -= 0.03) {
      c.dragTo(x, 0, cam);
      c.tick(1 / 60);
    }
    const f = c.flyers[0]!;
    expect(f.held).toBe(true);
    expect(f.p).toBeGreaterThan(0.6);
    expect(Math.abs(f.kappa)).toBeGreaterThan(0.05); // grabbed near the bottom corner: it lifts first
    c.release();
    expect(c.pos).toBe(4);
    settle(c);
    expect(c.pose().left).toBe("page:5");

    // A short drag that is let go falls back.
    c.grab(0.9, 0.5, cam);
    c.dragTo(0.6, 0, cam);
    for (let k = 0; k < 30; k++) c.tick(1 / 60);
    c.release();
    expect(c.pos).toBe(4);
    settle(c);
    expect(c.pose().right).toBe("page:6");

    // Dragging the left page back.
    expect(c.grab(-0.9, 0.5, cam)).toBe(true);
    for (let x = -0.9; x < 0.7; x += 0.04) {
      c.dragTo(x, 0, cam);
      c.tick(1 / 60);
    }
    c.release();
    settle(c);
    expect(c.pos).toBe(3);

    // A fast flick completes even though the page is short of the middle.
    c.grab(0.9, 0.5, cam);
    for (const x of [0.8, 0.62, 0.44]) {
      c.dragTo(x, 0, cam);
      c.tick(1 / 60);
    }
    expect(c.flyers[0]!.p).toBeLessThan(0.5);
    c.release();
    settle(c);
    expect(c.pos).toBe(4);

    // Cancel returns the page.
    c.grab(0.9, 0.5, cam);
    c.dragTo(-0.5, 0, cam);
    for (let k = 0; k < 30; k++) c.tick(1 / 60);
    c.cancel();
    settle(c);
    expect(c.pos).toBe(4);
  });

  it("nothing to grab beyond the covers", () => {
    const cam = { x: 0.5, distance: MOTION.cameraDistance };
    const c = make(4);
    expect(c.grab(-0.5, 0.5, cam)).toBe(false); // closed: nothing on the left
    expect(c.grab(0.9, 0.5, cam)).toBe(true); // the cover itself can be dragged open
    expect(c.flyers[0]!.board).toBe(true);
    expect(c.flyers[0]!.lambda).toBe(0);
    c.cancel();
    settle(c);
    expect(c.pos).toBe(0);
  });

  it("long jumps riffle a bounded number of leaves and arrive quickly", () => {
    const c = make(400);
    c.setPosition(1);
    c.jumpTo(150);
    let peak = 0;
    let t = 0;
    while (c.tick(1 / 120) && t < 5) {
      peak = Math.max(peak, c.flyers.length);
      t += 1 / 120;
    }
    expect(c.pos).toBe(150);
    expect(peak).toBeLessThanOrEqual(MOTION.maxRiffle);
    expect(peak).toBeGreaterThanOrEqual(4);
    expect(t).toBeLessThan(1.8);
    expect(c.pose().right).toBe(`page:${2 * 149}`);
    expect(c.pose().left).toBe(`page:${2 * 148 + 1}`);
    // Backwards too.
    c.jumpTo(10);
    settle(c);
    expect(c.pos).toBe(10);
    expect(c.pose().right).toBe("page:18");
  });

  it("a jump from the closed book opens the cover first, and can end on the back cover", () => {
    const c = make(60);
    c.jumpTo(12);
    expect(c.flyers.length).toBe(1);
    expect(c.flyers[0]!.board).toBe(true);
    settle(c);
    expect(c.pos).toBe(12);
    expect(c.pose().openness).toBeCloseTo(1);
    c.jumpTo(c.count);
    settle(c);
    expect(c.pos).toBe(c.count);
    expect(c.pose().left).toBe("back-cover");
    c.jumpTo(0);
    settle(c);
    expect(c.pos).toBe(0);
    expect(c.pose().right).toBe("front-cover");
  });

  it("never leaves the valid range under random input", () => {
    let seed = 12345;
    const rnd = () => {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      return seed / 0x7fffffff;
    };
    const cam = { x: 0, distance: MOTION.cameraDistance };
    const c = make(9);
    for (let i = 0; i < 4000; i++) {
      const r = rnd();
      if (r < 0.2) c.next();
      else if (r < 0.4) c.prev();
      else if (r < 0.5) c.grab(rnd() * 2 - 1, rnd(), cam);
      else if (r < 0.65) c.dragTo(rnd() * 2.4 - 1.2, rnd() - 0.5, cam);
      else if (r < 0.75) c.release();
      else if (r < 0.78) c.cancel();
      else if (r < 0.82) c.jumpTo(Math.floor(rnd() * (c.count + 1)));
      c.tick(rnd() * 0.05);
      expect(c.pos).toBeGreaterThanOrEqual(0);
      expect(c.pos).toBeLessThanOrEqual(c.count);
      for (const f of c.flyers) {
        expect(Number.isFinite(f.p)).toBe(true);
        expect(f.p).toBeGreaterThanOrEqual(0);
        expect(f.p).toBeLessThanOrEqual(1);
      }
      const pose = c.pose();
      expect(Number.isFinite(pose.camX)).toBe(true);
    }
    c.release();
    settle(c, 20);
    expect(c.flyers.length).toBe(0);
    const pose = c.pose();
    // At rest the two visible surfaces are always consecutive faces of the stack.
    if (c.pos > 0) expect(pose.left).toBe(c.sheets[c.pos - 1]!.back);
    if (c.pos < c.count) expect(pose.right).toBe(c.sheets[c.pos]!.front);
  });
});
