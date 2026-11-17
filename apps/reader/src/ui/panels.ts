/**
 * Side panels of the reading view: contents and bookmarks, search, settings,
 * and the confirmation shown before an external link leaves the reader.
 */
import type { Prefs } from "../core/prefs";
import { h, iconButton, trapFocus } from "./dom";
import type { ReaderView } from "./reader";

export type PanelKind = "contents" | "search" | "settings";

export class Panels {
  private sheet: HTMLElement | null = null;
  private kind: PanelKind | null = null;
  private release: (() => void) | null = null;
  private modal: { el: HTMLElement; release: () => void } | null = null;
  private searchTimer = 0;
  private lastQuery = "";

  constructor(
    private parent: HTMLElement,
    private reader: ReaderView,
  ) {}

  get isOpen(): boolean {
    return this.sheet !== null;
  }

  get modalOpen(): boolean {
    return this.modal !== null;
  }

  toggle(kind: PanelKind): void {
    if (this.kind === kind) this.close();
    else this.open(kind);
  }

  open(kind: PanelKind): void {
    this.close();
    const titles: Record<PanelKind, string> = { contents: "Contents", search: "Search", settings: "Reading settings" };
    const body = h("div", { class: "panel-body" });
    const sheet = h(
      "aside",
      { class: `panel panel-${kind}`, role: "dialog", "aria-label": titles[kind], tabindex: "-1" },
      h("div", { class: "panel-head" }, h("h2", { text: titles[kind] }), iconButton("close", "Close", () => this.close())),
      body,
    );
    if (kind === "contents") this.contents(body);
    else if (kind === "search") this.search(body);
    else this.settings(body);
    this.parent.append(sheet);
    this.sheet = sheet;
    this.kind = kind;
    this.release = trapFocus(sheet, () => this.close());
  }

  close(): void {
    clearTimeout(this.searchTimer);
    this.release?.();
    this.release = null;
    this.sheet?.remove();
    this.sheet = null;
    this.kind = null;
  }

  // --- contents and bookmarks --------------------------------------------------

  private contents(body: HTMLElement): void {
    const r = this.reader;
    const layout = r.layout;
    if (!layout) return;
    const current = r.diagnostics().page ?? -1;
    const nav = h("nav", { "aria-label": "Table of contents" });
    const list = h("ol", { class: "toc" });
    const fixed: [string, string][] = [["Cover", "cover"]];
    for (const [label, target] of fixed) {
      list.append(h("li", { class: "toc-1" }, h("button", { type: "button", class: "toc-line", onClick: () => this.jump(() => r.goTo(target)) }, h("span", { class: "toc-title", text: label }))));
    }
    layout.toc.forEach((entry, i) => {
      const nextPage = layout.toc[i + 1]?.page ?? Infinity;
      const here = current >= entry.page && current < nextPage;
      list.append(
        h(
          "li",
          { class: `toc-${entry.level}` },
          h(
            "button",
            { type: "button", class: "toc-line", "aria-current": here ? "location" : null, onClick: () => this.jump(() => r.goToPage(entry.page)) },
            entry.number !== undefined ? h("span", { class: "toc-number", text: String(entry.number) }) : null,
            h("span", { class: "toc-title", text: entry.title }),
            h("span", { class: "toc-page", text: String(entry.page + 1) }),
          ),
        ),
      );
    });
    list.append(h("li", { class: "toc-1" }, h("button", { type: "button", class: "toc-line", onClick: () => this.jump(() => r.goTo("end")) }, h("span", { class: "toc-title", text: "Back cover" }))));
    nav.append(list);
    body.append(nav);

    body.append(h("h3", { class: "panel-sub", text: "Bookmarks" }));
    if (r.bookmarks.length === 0) {
      body.append(h("p", { class: "panel-empty", text: "No bookmarks yet. Press B or use the ribbon button to mark a page." }));
      return;
    }
    const marks = h("ul", { class: "bookmarks" });
    r.bookmarks.forEach((b, i) => {
      const page = r.pageForAnchor(b.anchor);
      marks.append(
        h(
          "li",
          {},
          h(
            "button",
            { type: "button", class: "toc-line", onClick: () => this.jump(() => r.goToPage(page)) },
            h("span", { class: "toc-title", text: b.label || `Page ${page + 1}` }),
            h("span", { class: "toc-page", text: String(page + 1) }),
          ),
          iconButton("trash", "Remove bookmark", () => {
            r.removeBookmark(i);
            this.open("contents");
          }),
        ),
      );
    });
    body.append(marks);
  }

  private jump(go: () => unknown): void {
    this.close();
    go();
  }

  // --- search -------------------------------------------------------------------

  private search(body: HTMLElement): void {
    const r = this.reader;
    const input = h("input", { type: "search", class: "search-input", placeholder: "Search in this book", "aria-label": "Search in this book", autocomplete: "off", spellcheck: "false" });
    const status = h("p", { class: "panel-empty", role: "status", "aria-live": "polite" });
    const results = h("ol", { class: "search-results" });
    const run = () => {
      const q = input.value.trim();
      this.lastQuery = q;
      results.replaceChildren();
      if (q.length < 2) {
        status.textContent = q.length === 0 ? "" : "Type at least two characters.";
        return;
      }
      const layout = r.layout;
      if (!layout) return;
      const hits = r.book.search(q, 300);
      let shown = 0;
      for (const hit of hits) {
        // Text that is not part of the book on the reader's current path is not revealed.
        const page = layout.blockPages.get(hit.block);
        if (page === undefined) continue;
        if (++shown > 150) break;
        const line = h("button", { type: "button", class: "search-hit", onClick: () => this.jump(() => r.goToPage(page)) });
        const snippet = h("span", { class: "search-snippet" });
        snippet.append(hit.snippet.slice(0, hit.start), h("mark", { text: hit.snippet.slice(hit.start, hit.end) }), hit.snippet.slice(hit.end));
        line.append(snippet, h("span", { class: "toc-page", text: `p. ${page + 1}` }));
        results.append(h("li", {}, line));
      }
      status.textContent = shown === 0 ? "Nothing found." : shown > 150 ? "Showing the first 150 results." : `${shown} result${shown === 1 ? "" : "s"}`;
    };
    input.addEventListener("input", () => {
      clearTimeout(this.searchTimer);
      this.searchTimer = window.setTimeout(run, 140);
    });
    input.value = this.lastQuery;
    body.append(input, status, results);
    if (this.lastQuery) run();
  }

  // --- settings -----------------------------------------------------------------

  private settings(body: HTMLElement): void {
    const r = this.reader;
    const group = <T extends string>(label: string, key: keyof Prefs, options: [T, string][], current: T) => {
      const field = h("fieldset", { class: "setting" }, h("legend", { text: label }));
      const row = h("div", { class: "segmented", role: "radiogroup", "aria-label": label });
      for (const [value, text] of options) {
        const b = h("button", { type: "button", role: "radio", "aria-checked": String(value === current), text });
        b.addEventListener("click", () => {
          r.setPrefs({ [key]: value } as Partial<Prefs>);
          for (const sib of row.children) sib.setAttribute("aria-checked", String(sib === b));
        });
        row.append(b);
      }
      field.append(row);
      return field;
    };
    const size = h("output", { class: "size-value", text: `${Math.round(r.prefs.fontScale * 100)}%` });
    const step = (d: number) => {
      const next = Math.round(Math.min(1.6, Math.max(0.8, r.prefs.fontScale + d)) * 100) / 100;
      r.setPrefs({ fontScale: next });
      size.textContent = `${Math.round(next * 100)}%`;
    };
    const fixed = r.book.ir.meta.layout === "fixed";
    body.append(
      h(
        "fieldset",
        { class: "setting" },
        h("legend", { text: "Text size" }),
        h("div", { class: "stepper" }, iconButton("minus", "Smaller text", () => step(-0.1)), size, iconButton("plus", "Larger text", () => step(0.1))),
        fixed ? h("p", { class: "setting-note", text: "This book uses fixed pages; text size applies only to flowing passages." }) : null,
      ),
      group("Line spacing", "lineHeight", [["compact", "Compact"], ["normal", "Normal"], ["relaxed", "Relaxed"]], r.prefs.lineHeight),
      group("Margins", "margins", [["narrow", "Narrow"], ["normal", "Normal"], ["wide", "Wide"]], r.prefs.margins),
      group("Pages colour", "theme", [["paper", "Paper"], ["dark", "Night"]], r.prefs.theme),
      group("Pages", "pageMode", [["auto", "Auto"], ["single", "One"], ["spread", "Two"]], r.prefs.pageMode),
      group("Motion", "motion", [["system", "System"], ["full", "Full"], ["reduced", "Reduced"]], r.prefs.motion),
      h("p", { class: "setting-note", text: "Reduced motion replaces page turning with a quick fade. Auto and System follow this device's settings." }),
    );
  }

  // --- external links -----------------------------------------------------------

  /** Books cannot open web pages by themselves: the reader asks first and shows the real address. */
  confirmExternal(href: string): void {
    this.closeModal();
    let display = href;
    try {
      const u = new URL(href);
      display = u.protocol === "mailto:" ? href : `${u.hostname}${u.pathname === "/" ? "" : u.pathname}`;
    } catch {
      return;
    }
    const el = h(
      "div",
      { class: "modal-backdrop" },
      h(
        "div",
        { class: "modal", role: "alertdialog", "aria-modal": "true", "aria-label": "Open external link", tabindex: "-1" },
        h("h2", { text: "Leave the book?" }),
        h("p", { text: "This link opens outside the reader:" }),
        h("p", { class: "modal-url", text: display }),
        h(
          "div",
          { class: "modal-actions" },
          h("button", { type: "button", class: "pill ghost", text: "Stay", onClick: () => this.closeModal() }),
          h("button", {
            type: "button",
            class: "pill",
            text: "Open link",
            onClick: () => {
              this.closeModal();
              window.open(href, "_blank", "noopener,noreferrer");
            },
          }),
        ),
      ),
    );
    this.parent.append(el);
    this.modal = { el, release: trapFocus(el, () => this.closeModal()) };
  }

  private closeModal(): void {
    if (!this.modal) return;
    this.modal.release();
    this.modal.el.remove();
    this.modal = null;
  }

  dispose(): void {
    this.close();
    this.closeModal();
  }
}
