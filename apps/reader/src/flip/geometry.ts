/**
 * Geometry of a turning leaf.
 *
 * A leaf is a sheet hinged along the spine (x = 0). Its cross-section,
 * perpendicular to the spine, is described by the tangent angle θ(u) at
 * arc-length u ∈ [0, 1] from the spine: 0 = lying flat on the right-hand
 * stack, π = lying flat on the left-hand stack. Positions are obtained by
 * integrating the tangent,
 *
 *     x(u) = ∫₀ᵘ cos θ,   z(u) = ∫₀ᵘ sin θ,
 *
 * so the sheet never stretches: every cross-section keeps length 1 exactly,
 * which is what makes the motion read as paper rather than rubber. Because
 * θ stays in [0, π], z ≥ 0: the leaf can never dip below the stacks.
 *
 * θ follows a travelling wave,
 *
 *     θ(u) = π · E( q·(1+|λ|) − |λ|·d(u) ),   E = smoothstep on [0, 1],
 *
 * where q is the progress of the turn and λ selects *which part leads*:
 *
 *   λ > 0  the free edge leads (d = 1 − u). The page peels up from its edge
 *          and rolls over a moving fold: the classic curl, used while a
 *          finger drags the page. 1/λ is the width of the fold.
 *   λ < 0  the spine leads (d = u). The leaf is thrown from its root and the
 *          free edge trails behind, bowing the sheet and whipping over at the
 *          end, like the leaves in the reference animation. Used for taps,
 *          keys and riffles.
 *   λ = 0  rigid rotation (cover boards).
 *
 * A leaf travelling backwards (left → right) uses the mirrored family
 * θ ↦ π − θ with q ↦ 1 − p, so the same part of the sheet leads in both
 * directions. κ skews the progress along the spine so a grabbed corner
 * lifts first; it tapers to zero at both ends of the turn.
 */

export interface LeafPose {
  /** Progress of the turn, 0 = flat on the right, 1 = flat on the left. */
  p: number;
  /** Direction the leaf is travelling: +1 right→left, −1 left→right. */
  dir: 1 | -1;
  /** Shape family, see module docs. */
  lambda: number;
  /** Corner skew. */
  kappa: number;
  /** Height (0 … 1 along the spine, 0 = bottom) of the row whose progress is exactly `p`. */
  vGrab: number;
}

/** Smoothstep with clamping. C¹, so angular velocity along the sheet is continuous. */
export function ease(x: number): number {
  const t = x <= 0 ? 0 : x >= 1 ? 1 : x;
  return t * t * (3 - 2 * t);
}

/** Progress of the row at height v. */
export function rowProgress(pose: LeafPose, v: number): number {
  const taper = Math.sin(Math.PI * pose.p);
  const p = pose.p + pose.kappa * (v - pose.vGrab) * taper;
  return p <= 0 ? 0 : p >= 1 ? 1 : p;
}

/** Tangent angle at arc-length u for a row with progress p. */
export function tangent(p: number, u: number, lambda: number, dir: 1 | -1): number {
  const q = dir > 0 ? p : 1 - p;
  const a = Math.abs(lambda);
  const d = lambda >= 0 ? 1 - u : u;
  const phi = Math.PI * ease(q * (1 + a) - a * d);
  return dir > 0 ? phi : Math.PI - phi;
}

/**
 * Integrate one cross-section. Writes `n + 1` samples of x, z and θ into the
 * given arrays (at u = i / n) using the trapezoid rule.
 */
export function crossSection(
  p: number,
  lambda: number,
  dir: 1 | -1,
  n: number,
  outX: Float32Array | number[],
  outZ: Float32Array | number[],
  outTheta: Float32Array | number[],
  offset = 0,
): void {
  const du = 1 / n;
  let x = 0;
  let z = 0;
  let prev = tangent(p, 0, lambda, dir);
  let pc = Math.cos(prev);
  let ps = Math.sin(prev);
  outX[offset] = 0;
  outZ[offset] = 0;
  outTheta[offset] = prev;
  for (let i = 1; i <= n; i++) {
    const th = tangent(p, i * du, lambda, dir);
    const c = Math.cos(th);
    const s = Math.sin(th);
    x += 0.5 * (pc + c) * du;
    z += 0.5 * (ps + s) * du;
    outX[offset + i] = x;
    outZ[offset + i] = z < 0 ? 0 : z;
    outTheta[offset + i] = th;
    prev = th;
    pc = c;
    ps = s;
  }
}

/** Number of columns (along the page width) and rows (along the spine) of a leaf mesh. */
export const MESH_COLS = 56;
export const MESH_ROWS = 20;
/** Floats per vertex: position (3), normal (3), uv (2). */
export const VERTEX_SIZE = 8;

/** Triangle indices shared by every leaf mesh. */
export function leafIndices(cols = MESH_COLS, rows = MESH_ROWS): Uint16Array {
  const idx = new Uint16Array(cols * rows * 6);
  let k = 0;
  const stride = cols + 1;
  for (let j = 0; j < rows; j++) {
    for (let i = 0; i < cols; i++) {
      const a = j * stride + i;
      const b = a + 1;
      const c = a + stride;
      const d = c + 1;
      // Counter-clockwise seen from +z when the leaf lies flat on the right.
      idx[k++] = a;
      idx[k++] = b;
      idx[k++] = d;
      idx[k++] = a;
      idx[k++] = d;
      idx[k++] = c;
    }
  }
  return idx;
}

const scratchX = new Float32Array(MESH_COLS + 1);
const scratchZ = new Float32Array(MESH_COLS + 1);
const scratchT = new Float32Array(MESH_COLS + 1);

/**
 * Fill `out` with the vertices of a leaf. World units: the page is 1 wide and
 * `height` tall, the spine is the line x = 0, y runs from −height/2 (bottom)
 * to +height/2, z points at the viewer. `lift` raises the whole leaf slightly
 * to keep it clear of the pages it rests on.
 */
export function buildLeaf(pose: LeafPose, height: number, lift: number, out: Float32Array, cols = MESH_COLS, rows = MESH_ROWS): void {
  let o = 0;
  for (let j = 0; j <= rows; j++) {
    const v = j / rows;
    const p = rowProgress(pose, v);
    crossSection(p, pose.lambda, pose.dir, cols, scratchX, scratchZ, scratchT);
    const y = (v - 0.5) * height;
    for (let i = 0; i <= cols; i++) {
      const th = scratchT[i]!;
      out[o++] = scratchX[i]!;
      out[o++] = y;
      out[o++] = scratchZ[i]! + lift;
      out[o++] = -Math.sin(th);
      out[o++] = 0;
      out[o++] = Math.cos(th);
      out[o++] = i / cols;
      out[o++] = v;
    }
  }
}

/** Position of the point (u, v) of a leaf: [x, z]. */
export function leafPoint(pose: LeafPose, u: number, v: number): [number, number] {
  const p = rowProgress(pose, v);
  const n = 48;
  const xs = new Array<number>(n + 1);
  const zs = new Array<number>(n + 1);
  const ts = new Array<number>(n + 1);
  crossSection(p, pose.lambda, pose.dir, n, xs, zs, ts);
  const f = Math.min(Math.max(u, 0), 1) * n;
  const i = Math.min(Math.floor(f), n - 1);
  const t = f - i;
  return [xs[i]! + (xs[i + 1]! - xs[i]!) * t, zs[i]! + (zs[i + 1]! - zs[i]!) * t];
}

/** A camera looking straight down at the book from height `distance`, centred on x = `x`. */
export interface Camera {
  x: number;
  distance: number;
}

/** Horizontal position on the z = 0 plane where a point at (x, z) appears. */
export function apparentX(x: number, z: number, cam: Camera): number {
  return cam.x + ((x - cam.x) * cam.distance) / (cam.distance - z);
}

/**
 * Find the progress p at which the grabbed point (u, vGrab) of the leaf
 * appears under the pointer (`targetX`, measured on the page plane).
 *
 * The apparent x of the grabbed point decreases monotonically with p for
 * both shape families, so a bisection is exact and cheap.
 */
export function solveProgress(pose: Omit<LeafPose, "p">, u: number, targetX: number, cam: Camera): number {
  let lo = 0;
  let hi = 1;
  const at = (p: number) => {
    const [x, z] = leafPoint({ ...pose, p, kappa: 0 }, u, pose.vGrab);
    return apparentX(x, z, cam);
  };
  if (targetX >= at(0)) return 0;
  if (targetX <= at(1)) return 1;
  for (let i = 0; i < 22; i++) {
    const mid = 0.5 * (lo + hi);
    if (at(mid) > targetX) lo = mid;
    else hi = mid;
  }
  return 0.5 * (lo + hi);
}

/**
 * Vertices of a rigid board (cover) as a thin slab: 8 corners × (pos, normal, uv)
 * laid out as 6 quads (outer face, inner face, fore-edge, head, tail, spine).
 * `p` is the turn progress, `width`/`height` the board size, `inset` shifts the
 * hinge so the board overhangs the pages on every side, `thick` its thickness.
 * Returns the number of vertices written (24).
 */
export function buildBoard(p: number, width: number, height: number, thick: number, lift: number, out: Float32Array): number {
  const th = Math.PI * ease(p);
  const c = Math.cos(th);
  const s = Math.sin(th);
  // Board normal (outer face, which looks up when the board rests on the right).
  const nx = -s;
  const nz = c;
  const h2 = height / 2;
  const corner = (d: number, y: number, side: number): [number, number, number] => [
    d * c + nx * thick * side,
    y,
    d * s + nz * thick * side + lift,
  ];
  let o = 0;
  const quad = (a: [number, number, number], b: [number, number, number], cc: [number, number, number], d: [number, number, number], n: [number, number, number], uv: [number, number, number, number]) => {
    const pts = [a, b, cc, d];
    const uvs: [number, number][] = [
      [uv[0], uv[1]],
      [uv[2], uv[1]],
      [uv[2], uv[3]],
      [uv[0], uv[3]],
    ];
    for (let i = 0; i < 4; i++) {
      out[o++] = pts[i]![0];
      out[o++] = pts[i]![1];
      out[o++] = pts[i]![2];
      out[o++] = n[0];
      out[o++] = n[1];
      out[o++] = n[2];
      out[o++] = uvs[i]![0];
      out[o++] = uvs[i]![1];
    }
  };
  const o00 = corner(0, -h2, 1);
  const o10 = corner(width, -h2, 1);
  const o11 = corner(width, h2, 1);
  const o01 = corner(0, h2, 1);
  const i00 = corner(0, -h2, 0);
  const i10 = corner(width, -h2, 0);
  const i11 = corner(width, h2, 0);
  const i01 = corner(0, h2, 0);
  // Outer face (cover art): u runs away from the spine.
  quad(o00, o10, o11, o01, [nx, 0, nz], [0, 0, 1, 1]);
  // Inner face: seen from the other side, so wind it the other way.
  quad(i10, i00, i01, i11, [-nx, 0, -nz], [1, 0, 0, 1]);
  // Fore-edge.
  quad(i10, i11, o11, o10, [c, 0, s], [0, 0, 1, 1]);
  // Head (top) and tail (bottom).
  quad(i01, o01, o11, i11, [0, 1, 0], [0, 0, 1, 1]);
  quad(i00, i10, o10, o00, [0, -1, 0], [0, 0, 1, 1]);
  // Spine side.
  quad(i00, o00, o01, i01, [-c, 0, -s], [0, 0, 1, 1]);
  return 24;
}

/**
 * A rigid, page-sized rectangle that turns with a board, displaced by
 * `offset` along the board's normal: the endpaper pasted inside a cover.
 * Writes 4 vertices (same layout as a flat quad: u runs away from the spine).
 */
export function buildPaste(p: number, height: number, offset: number, lift: number, out: Float32Array): void {
  const th = Math.PI * ease(p);
  const c = Math.cos(th);
  const s = Math.sin(th);
  const nx = -s;
  const nz = c;
  const h2 = height / 2;
  const pts: [number, number, number, number][] = [
    [0, -h2, 0, 0],
    [1, -h2, 1, 0],
    [1, h2, 1, 1],
    [0, h2, 0, 1],
  ];
  let o = 0;
  for (const [d, y, u, v] of pts) {
    out[o++] = d * c + nx * offset;
    out[o++] = y;
    out[o++] = d * s + nz * offset + lift;
    out[o++] = nx;
    out[o++] = 0;
    out[o++] = nz;
    out[o++] = u;
    out[o++] = v;
  }
}

/** Indices for the 6 quads written by `buildBoard`. */
export function boardIndices(): Uint16Array {
  const idx = new Uint16Array(36);
  for (let q = 0; q < 6; q++) {
    const b = q * 4;
    idx.set([b, b + 1, b + 2, b, b + 2, b + 3], q * 6);
  }
  return idx;
}
