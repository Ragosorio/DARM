/**
 * Fonts for the canvas.
 *
 * The three built-in families are variable fonts bundled with the app (no
 * network at run time). A book may also embed its own body / heading face
 * as a resource; it is registered through the FontFace API under a family
 * name unique to that book and resource.
 *
 * `loadFonts` resolves only when every face the layout can ask for is
 * usable, because a canvas measures with a fallback face, without any
 * error, while the real one is still loading.
 */
import "@fontsource-variable/literata/wght.css";
import "@fontsource-variable/literata/wght-italic.css";
import "@fontsource-variable/inter/wght.css";
import "@fontsource-variable/jetbrains-mono/wght.css";

import type { Book, BuiltinFont, FontRef } from "../ir";
import type { FontSet } from "../layout/types";
import { hashString } from "./color";

const BUILTIN: Record<BuiltinFont, string> = {
  serif: '"Literata Variable", "Iowan Old Style", "Palatino Linotype", Palatino, "Book Antiqua", Georgia, serif',
  sans: '"Inter Variable", system-ui, -apple-system, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif',
  mono: '"JetBrains Mono Variable", ui-monospace, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
};

/** Weights and styles the layout and the covers set type in, per role. */
const BODY_FACES = ["400", "italic 400", "700", "italic 700", "500", "italic 500", "600", "800"];
const SANS_FACES = ["400", "500", "600", "700"];
const MONO_FACES = ["400", "700"];

const registered = new Map<string, Promise<string | null>>();

/** Register an embedded font once; resolves to its CSS family name, or `null` if it cannot be used. */
function registerEmbedded(book: Book, name: string, loadResource: (name: string) => Promise<ArrayBuffer>): Promise<string | null> {
  const res = book.resources.find((r) => r.name === name && r.kind === "font");
  if (!res) return Promise.resolve(null);
  // The digest makes the name unique per file, so two books with a font called "display" never collide.
  const family = `darm-${hashString(`${book.meta.id}|${name}|${res.sha256}`).toString(36)}`;
  let p = registered.get(family);
  if (!p) {
    p = (async (): Promise<string | null> => {
      try {
        const data = await loadResource(name);
        const face = new FontFace(family, data);
        await face.load();
        document.fonts.add(face);
        return family;
      } catch {
        return null;
      }
    })();
    registered.set(family, p);
  }
  return p;
}

async function resolve(book: Book, ref: FontRef, loadResource: (name: string) => Promise<ArrayBuffer>): Promise<string> {
  if (ref && ref.source === "resource") {
    const family = await registerEmbedded(book, ref.name, loadResource);
    // The embedded face first; the built-in serif behind it covers glyphs it lacks.
    if (family) return `"${family}", ${BUILTIN.serif}`;
    return BUILTIN.serif;
  }
  const builtin = ref && ref.source === "builtin" ? BUILTIN[ref.family] : undefined;
  return builtin ?? BUILTIN.serif;
}

/**
 * Characters the book actually uses. Built-in fonts are split into
 * unicode-range subsets; asking the browser to load a face "for this
 * text" fetches exactly the subsets needed (Latin, Latin Extended, ...).
 */
function sampleText(book: Book): string {
  const seen = new Set<string>("AaBbHhXx0123456789 .,;:!?-()'\"");
  const add = (s: unknown): void => {
    if (typeof s !== "string") return;
    for (const ch of s) {
      if (seen.size > 2000) return;
      seen.add(ch);
    }
  };
  const walk = (v: unknown, depth: number): void => {
    if (depth > 40 || seen.size > 2000) return;
    if (typeof v === "string") add(v);
    else if (Array.isArray(v)) for (const x of v) walk(x, depth + 1);
    else if (v && typeof v === "object") for (const x of Object.values(v as Record<string, unknown>)) walk(x, depth + 1);
  };
  walk(book.meta, 0);
  walk(book.cover, 0);
  walk(book.toc, 0);
  walk(book.chapters, 0);
  // Characters the reader itself prints: dashes, quotes, bullets, ellipsis, accented words of the built-in labels.
  add("—– •…“”‘’");
  add("Íóèéáã");
  return Array.from(seen).join("");
}

export async function loadFonts(book: Book, loadResource: (name: string) => Promise<ArrayBuffer>): Promise<FontSet> {
  const [body, heading] = await Promise.all([resolve(book, book.theme.body_font, loadResource), resolve(book, book.theme.heading_font, loadResource)]);
  const fonts: FontSet = { body, heading, mono: BUILTIN.mono, sans: BUILTIN.sans };

  const text = sampleText(book);
  const wanted = new Set<string>();
  for (const face of BODY_FACES) {
    wanted.add(`${face} 16px ${fonts.body}`);
    wanted.add(`${face} 16px ${fonts.heading}`);
  }
  for (const face of SANS_FACES) wanted.add(`${face} 16px ${fonts.sans}`);
  for (const face of MONO_FACES) wanted.add(`${face} 16px ${fonts.mono}`);
  // A face that fails to load must not block the book: the fallbacks in the family list take over.
  await Promise.all(Array.from(wanted, (font) => document.fonts.load(font, text).catch(() => [])));
  return fonts;
}
