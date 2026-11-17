/** Reader preferences, kept in localStorage (they are small and device-specific). */
import type { ReaderSettings } from "../layout/types";

export interface Prefs extends ReaderSettings {
  /**
   * Colour of the pages. A book is paper unless the reader asks for night
   * pages: it does not turn dark because the system is in dark mode.
   */
  theme: "paper" | "dark";
  pageMode: "auto" | "single" | "spread";
  /** `system` follows prefers-reduced-motion. */
  motion: "system" | "full" | "reduced";
}

export const DEFAULT_PREFS: Prefs = {
  fontScale: 1,
  lineHeight: "normal",
  margins: "normal",
  justify: null,
  hyphenate: null,
  theme: "paper",
  pageMode: "auto",
  motion: "system",
};

const KEY = "darm.prefs.v1";
const oneOf = <T extends string>(v: unknown, allowed: readonly T[], fallback: T): T => (allowed.includes(v as T) ? (v as T) : fallback);

export function loadPrefs(): Prefs {
  let raw: Record<string, unknown> = {};
  try {
    raw = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Record<string, unknown>;
  } catch {
    /* corrupted or unavailable storage: defaults */
  }
  const scale = Number(raw.fontScale);
  return {
    fontScale: Number.isFinite(scale) ? Math.min(1.6, Math.max(0.8, scale)) : 1,
    lineHeight: oneOf(raw.lineHeight, ["compact", "normal", "relaxed"] as const, "normal"),
    margins: oneOf(raw.margins, ["narrow", "normal", "wide"] as const, "normal"),
    justify: typeof raw.justify === "boolean" ? raw.justify : null,
    hyphenate: typeof raw.hyphenate === "boolean" ? raw.hyphenate : null,
    theme: oneOf(raw.theme, ["paper", "dark"] as const, "paper"),
    pageMode: oneOf(raw.pageMode, ["auto", "single", "spread"] as const, "auto"),
    motion: oneOf(raw.motion, ["system", "full", "reduced"] as const, "system"),
  };
}

export function savePrefs(p: Prefs): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(p));
  } catch {
    /* storage may be disabled; preferences then last for the session only */
  }
}

export function systemDark(): boolean {
  return matchMedia("(prefers-color-scheme: dark)").matches;
}

export function systemReducedMotion(): boolean {
  return matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function resolveDark(p: Prefs): boolean {
  return p.theme === "dark";
}

export function resolveReducedMotion(p: Prefs): boolean {
  return p.motion === "reduced" || (p.motion === "system" && systemReducedMotion());
}
