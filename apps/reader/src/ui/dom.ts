/** Tiny DOM helpers. Book content is never inserted as HTML: text goes through `textContent`. */

type Attrs = Record<string, string | number | boolean | null | undefined | ((e: Event) => void)>;

export function h<K extends keyof HTMLElementTagNameMap>(tag: K, attrs: Attrs = {}, ...children: (Node | string | null | undefined | false)[]): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === null || v === undefined || v === false) continue;
    if (typeof v === "function") el.addEventListener(k.replace(/^on/, "").toLowerCase(), v);
    else if (k === "class") el.className = String(v);
    else if (k === "text") el.textContent = String(v);
    else if (v === true) el.setAttribute(k, "");
    else el.setAttribute(k, String(v));
  }
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    el.append(c);
  }
  return el;
}

/** Icons are drawn as SVG paths (24×24, stroke); no icon fonts, no emoji. */
const ICONS = {
  back: "M15 5l-7 7 7 7",
  contents: "M5 6h14M5 12h14M5 18h9",
  search: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM16.5 16.5L21 21",
  bookmark: "M7 4h10v16l-5-4-5 4z",
  settings: "M4 7h10M18 7h2M4 17h2M10 17h10M14 4.5v5M6 14.5v5",
  close: "M6 6l12 12M18 6L6 18",
  plus: "M12 5v14M5 12h14",
  play: "M8 5.5v13l11-6.5z",
  pause: "M8 5v14M16 5v14",
  prev: "M14 6l-6 6 6 6",
  next: "M10 6l6 6-6 6",
  lock: "M7 11V8a5 5 0 0 1 10 0v3M6 11h12v9H6z",
  trash: "M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13",
  book: "M5 4h9a4 4 0 0 1 4 4v12H9a4 4 0 0 1-4-4zM5 16a4 4 0 0 1 4-4h9",
  key: "M14 10a4 4 0 1 0-3.5 3.97L12 15.5V18h2.5v2H17v-3.2l-3.2-3.2A4 4 0 0 0 14 10z",
  minus: "M5 12h14",
} as const;

export type IconName = keyof typeof ICONS;

export function icon(name: IconName, filled = false): SVGSVGElement {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", "20");
  svg.setAttribute("height", "20");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("focusable", "false");
  const path = document.createElementNS(ns, "path");
  path.setAttribute("d", ICONS[name]);
  path.setAttribute("fill", filled ? "currentColor" : "none");
  path.setAttribute("stroke", "currentColor");
  path.setAttribute("stroke-width", "1.6");
  path.setAttribute("stroke-linecap", "round");
  path.setAttribute("stroke-linejoin", "round");
  svg.append(path);
  return svg;
}

export function iconButton(name: IconName, label: string, onClick: (e: Event) => void, extraClass = ""): HTMLButtonElement {
  const b = h("button", { class: `icon-button ${extraClass}`.trim(), type: "button", "aria-label": label, title: label, onClick });
  b.append(icon(name));
  return b;
}

/** Keep keyboard focus inside `container` while it is open; returns a function that releases the trap. */
export function trapFocus(container: HTMLElement, onEscape: () => void): () => void {
  const previous = document.activeElement as HTMLElement | null;
  const focusables = () =>
    [...container.querySelectorAll<HTMLElement>('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])')].filter(
      (el) => !el.hasAttribute("disabled") && el.offsetParent !== null,
    );
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      onEscape();
      return;
    }
    if (e.key !== "Tab") return;
    const f = focusables();
    if (f.length === 0) return;
    const first = f[0]!;
    const last = f[f.length - 1]!;
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  };
  container.addEventListener("keydown", onKey);
  queueMicrotask(() => (focusables()[0] ?? container).focus());
  return () => {
    container.removeEventListener("keydown", onKey);
    previous?.focus?.();
  };
}

export function formatTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
  const s = Math.floor(seconds);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}
