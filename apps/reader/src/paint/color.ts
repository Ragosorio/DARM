/** Small colour helpers for the painter. Colours in the IR are `#rrggbb`; anything else falls back to black. */

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

const cache = new Map<string, Rgb>();

export function parseColor(c: string): Rgb {
  let v = cache.get(c);
  if (v) return v;
  v = { r: 0, g: 0, b: 0 };
  if (typeof c === "string") {
    let m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})(?:[0-9a-f]{2})?$/i.exec(c);
    if (m) v = { r: parseInt(m[1]!, 16), g: parseInt(m[2]!, 16), b: parseInt(m[3]!, 16) };
    else {
      m = /^#([0-9a-f])([0-9a-f])([0-9a-f])[0-9a-f]?$/i.exec(c);
      if (m) v = { r: parseInt(m[1]! + m[1]!, 16), g: parseInt(m[2]! + m[2]!, 16), b: parseInt(m[3]! + m[3]!, 16) };
    }
  }
  if (cache.size > 512) cache.clear();
  cache.set(c, v);
  return v;
}

function hex2(n: number): string {
  return Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, "0");
}

export function toHex(c: Rgb): string {
  return `#${hex2(c.r)}${hex2(c.g)}${hex2(c.b)}`;
}

/** `a` moved towards `b` by `t` (0 = a, 1 = b). */
export function mix(a: string, b: string, t: number): string {
  const x = parseColor(a);
  const y = parseColor(b);
  return toHex({ r: x.r + (y.r - x.r) * t, g: x.g + (y.g - x.g) * t, b: x.b + (y.b - x.b) * t });
}

export function alpha(c: string, a: number): string {
  const v = parseColor(c);
  return `rgba(${v.r},${v.g},${v.b},${Math.max(0, Math.min(1, a))})`;
}

export function darken(c: string, t: number): string {
  return mix(c, "#000000", t);
}

export function lighten(c: string, t: number): string {
  return mix(c, "#ffffff", t);
}

/** Relative luminance, 0 (black) … 1 (white). */
export function luminance(c: string): number {
  const v = parseColor(c);
  const f = (n: number): number => {
    const s = n / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * f(v.r) + 0.7152 * f(v.g) + 0.0722 * f(v.b);
}

/** FNV-1a hash of a string, for seeding procedural textures from the book id. */
export function hashString(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

/** Deterministic PRNG (mulberry32): the same book always gets the same cloth. */
export function prng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
