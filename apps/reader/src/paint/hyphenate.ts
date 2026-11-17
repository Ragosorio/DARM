/**
 * Hyphenation patterns (Liang's algorithm, from the `hyphen` package),
 * loaded on demand for the book's language so that only one set of
 * patterns is ever downloaded.
 */

type Module = { hyphenateSync?: (text: string, options?: { hyphenChar?: string; minWordLength?: number }) => string; default?: Module };

// Static import() calls, one per language, so the bundler can split each pattern set into its own chunk.
const LOADERS: Record<string, () => Promise<unknown>> = {
  en: () => import("hyphen/en"),
  es: () => import("hyphen/es"),
  fr: () => import("hyphen/fr"),
  de: () => import("hyphen/de"),
  pt: () => import("hyphen/pt"),
  it: () => import("hyphen/it"),
};

/** Soft hyphen: never occurs inside a word we pass in, so it is a safe separator. */
const SEP = String.fromCharCode(0xad);
const MAX_CACHE = 20000;

/**
 * Returns a function that splits a word at its legal hyphenation points
 * (`"hyphenate"` -> `["hy", "phen", "ate"]`), or `undefined` when there are
 * no patterns for the language. Results are cached per word.
 */
export async function loadHyphenator(lang: string): Promise<((word: string) => string[]) | undefined> {
  const key = String(lang ?? "")
    .toLowerCase()
    .split(/[-_]/)[0];
  const loader = key !== undefined ? LOADERS[key] : undefined;
  if (!loader) return undefined;
  let mod: Module;
  try {
    mod = (await loader()) as Module;
  } catch {
    return undefined;
  }
  // The package is CommonJS: depending on the bundler the functions are on the namespace or on `default`.
  const fn = mod.hyphenateSync ?? mod.default?.hyphenateSync;
  if (typeof fn !== "function") return undefined;
  const cache = new Map<string, string[]>();
  return (word: string): string[] => {
    let parts = cache.get(word);
    if (!parts) {
      try {
        parts = fn(word, { hyphenChar: SEP, minWordLength: 5 }).split(SEP);
      } catch {
        parts = [word];
      }
      if (parts.join("") !== word) parts = [word];
      if (cache.size >= MAX_CACHE) cache.clear();
      cache.set(word, parts);
    }
    return parts;
  };
}
