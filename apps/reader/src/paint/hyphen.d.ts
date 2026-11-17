/** The `hyphen` package ships no types; this is the part of its per-language modules we use. */
declare module "hyphen/*" {
  export function hyphenateSync(text: string, options?: { hyphenChar?: string; minWordLength?: number }): string;
  const _default: { hyphenateSync: typeof hyphenateSync } | undefined;
  export default _default;
}
