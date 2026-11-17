/**
 * Overlays: real DOM controls laid over the pages while the book is at rest.
 *
 * Pages are painted on a canvas so they can bend, but a canvas is opaque to
 * keyboards and screen readers. So for every page at rest the reader adds:
 *
 *   - a visually hidden outline of the page (headings, paragraphs, image
 *     descriptions) for assistive technology;
 *   - a transparent, focusable control over each interactive region the
 *     layout reported (links, buttons, options, answers, contents lines);
 *   - real <video>/<audio> elements over the media widgets.
 *
 * Overlays are removed the moment a page starts to turn (media pauses) and
 * rebuilt when the book comes to rest.
 */
import type { OpenBook } from "../core/book";
import type { PageLayout, Region, SemanticNode } from "../layout/types";
import { formatTime, h, icon, iconButton, trapFocus } from "./dom";
import type { PageRect } from "./stage";

export interface OverlayHost {
  readonly book: OpenBook;
  activate(region: Region): void;
  isHostAllowed(host: string): boolean;
  allowHost(host: string): void;
  toast(message: string): void;
}

function semanticNode(n: SemanticNode): HTMLElement | null {
  switch (n.role) {
    case "heading":
      return h(`h${Math.min(6, n.level + 1)}` as "h2", { text: n.text });
    case "paragraph":
    case "caption":
    case "note":
      return h("p", { text: n.text });
    case "quote":
      return h("blockquote", { text: n.text });
    case "list": {
      const list = h(n.ordered ? "ol" : "ul");
      for (const item of n.items) list.append(h("li", { text: item }));
      return list;
    }
    case "image":
      return h("p", { role: "img", "aria-label": n.alt || "Image" });
    case "separator":
      return h("hr");
    default:
      return null;
  }
}

function regionLabel(r: Region): string {
  switch (r.type) {
    case "link":
      return r.link.kind === "url" ? `${r.label} (external link)` : r.label;
    case "button":
      return r.label;
    case "option":
      return `${r.label}${r.selected ? " (selected)" : ""}`;
    case "answer":
      return r.label;
    case "toc":
      return r.label;
    case "image":
      return "Enlarge image";
    case "gallery":
      return `Open gallery of ${r.resources.length} images`;
    default:
      return "";
  }
}

export class Overlays {
  private layer: HTMLElement;
  private media: HTMLMediaElement[] = [];
  private box: { el: HTMLElement; release: () => void } | null = null;

  /**
   * `stage` is the element the page rectangles are relative to; `root` hosts
   * full-window UI such as the lightbox.
   */
  constructor(
    stage: HTMLElement,
    private root: HTMLElement,
    private host: OverlayHost,
  ) {
    this.layer = h("div", { class: "overlay-layer" });
    stage.append(this.layer);
  }

  get lightboxOpen(): boolean {
    return this.box !== null;
  }

  /** Remove every overlay and stop media. */
  clear(): void {
    for (const m of this.media) {
      m.pause();
      m.removeAttribute("src");
      m.load();
    }
    this.media = [];
    this.layer.replaceChildren();
  }

  /** Build overlays for the pages now at rest. */
  show(rects: { left: PageRect | null; right: PageRect | null }, pages: { left: PageLayout | null; right: PageLayout | null }): void {
    this.clear();
    for (const side of ["left", "right"] as const) {
      const rect = rects[side];
      const page = pages[side];
      if (!rect || !page) continue;
      const el = h("div", { class: "page-overlay", "data-page": page.index, "data-side": side });
      el.style.left = `${rect.x}px`;
      el.style.top = `${rect.y}px`;
      el.style.width = `${rect.width}px`;
      el.style.height = `${rect.height}px`;
      const outline = h("article", { class: "sr-only", "aria-label": `Page ${page.index + 1}` });
      for (const n of page.semantics) {
        const node = semanticNode(n);
        if (node) outline.append(node);
      }
      el.append(outline);
      // Group radio-like controls for assistive technology.
      const groups = new Map<string, HTMLElement>();
      for (const region of page.regions) {
        const control = this.control(region);
        if (!control) continue;
        control.style.left = `${region.x}px`;
        control.style.top = `${region.y}px`;
        control.style.width = `${region.w}px`;
        control.style.height = `${region.h}px`;
        if (region.type === "option" || region.type === "answer") {
          let g = groups.get(region.block);
          if (!g) {
            g = h("div", { role: "group", "aria-label": region.type === "option" ? region.prompt : region.question, class: "region-group" });
            groups.set(region.block, g);
            el.append(g);
          }
          g.append(control);
        } else {
          el.append(control);
        }
      }
      this.layer.append(el);
    }
  }

  private control(region: Region): HTMLElement | null {
    if (region.type === "video") return this.video(region);
    if (region.type === "audio") return this.audio(region);
    const label = regionLabel(region);
    const b = h("button", { type: "button", class: `region region-${region.type}`, "aria-label": label, "data-block": region.block });
    if (region.type === "option") {
      b.setAttribute("aria-pressed", String(region.selected));
      b.dataset.index = String(region.index);
    }
    if (region.type === "answer") {
      b.dataset.index = String(region.index);
      if (region.answered) b.setAttribute("aria-disabled", "true");
    }
    if (region.type === "toc") b.dataset.target = region.target;
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      this.host.activate(region);
    });
    return b;
  }

  /** Resolve where a media resource plays from: the package, or an allowed remote host after consent. */
  private async mediaSource(resource: string): Promise<{ url: string } | { consent: string; url: string } | null> {
    const book = this.host.book;
    const r = book.resource(resource);
    if (!r) return null;
    if (r.source.location === "packaged") return { url: await book.url(resource) };
    const url = book.remoteUrl(resource);
    if (!url) return null;
    const hostName = new URL(url).hostname;
    return this.host.isHostAllowed(hostName) ? { url } : { consent: hostName, url };
  }

  /** Ask before the first network request a book makes to a host. */
  private consent(container: HTMLElement, hostName: string, load: () => void): void {
    const note = h(
      "div",
      { class: "media-consent" },
      h("p", { text: `This media is not inside the book. Playing it contacts ${hostName}.` }),
      h("button", {
        type: "button",
        class: "pill",
        text: `Allow ${hostName} for this book`,
        onClick: () => {
          this.host.allowHost(hostName);
          note.remove();
          load();
        },
      }),
    );
    container.append(note);
  }

  private video(region: Extract<Region, { type: "video" }>): HTMLElement {
    const book = this.host.book;
    const r = book.resource(region.resource);
    const wrap = h("div", { class: "region region-video", "data-block": region.block });
    const v = h("video", { controls: true, playsinline: true, preload: "metadata", "aria-label": r?.alt ?? r?.title ?? "Video" });
    v.loop = region.loop;
    v.muted = region.muted || region.autoplay;
    wrap.append(v);
    this.media.push(v);
    const attach = (url: string) => {
      v.src = url;
      // Tracks are attached in the order the book declares them (the first is
      // the default), whatever order their data becomes available in.
      const tracks = r?.captions ?? [];
      void Promise.all(tracks.map((t) => book.url(t.resource).catch(() => null))).then((sources) => {
        sources.forEach((src, i) => {
          if (!src) return;
          const t = tracks[i]!;
          const track = h("track", { kind: "captions", srclang: t.lang, label: t.label, src });
          if (i === 0) track.default = true;
          v.append(track);
        });
      });
      if (region.autoplay) void v.play().catch(() => undefined);
    };
    v.addEventListener("error", () => {
      if (!v.src) return;
      wrap.classList.add("media-failed");
      wrap.append(h("p", { class: "media-error", text: "This video cannot be played on this device." }));
    });
    void (async () => {
      try {
        if (r?.poster) v.poster = await book.url(r.poster);
        const source = await this.mediaSource(region.resource);
        if (!source) {
          wrap.append(h("p", { class: "media-error", text: "This video is not available." }));
        } else if ("consent" in source) {
          this.consent(wrap, source.consent, () => attach(source.url));
        } else {
          attach(source.url);
        }
      } catch {
        wrap.append(h("p", { class: "media-error", text: "This video could not be loaded from the book." }));
      }
    })();
    return wrap;
  }

  private audio(region: Extract<Region, { type: "audio" }>): HTMLElement {
    const book = this.host.book;
    const r = book.resource(region.resource);
    const title = r?.title ?? "Audio";
    // Kept in the document (without native controls) so assistive tools and tests can see its state.
    const a = h("audio", { preload: "metadata" });
    this.media.push(a);
    const play = h("button", { type: "button", class: "audio-play", "aria-label": `Play ${title}` });
    play.append(icon("play", true));
    const seek = h("input", { type: "range", class: "audio-seek", min: "0", max: "1000", value: "0", "aria-label": `Position in ${title}` });
    const time = h("span", { class: "audio-time", text: "0:00" });
    const name = h("span", { class: "audio-title", text: title });
    const wrap = h("div", { class: "region region-audio", role: "group", "aria-label": title, "data-block": region.block }, play, h("div", { class: "audio-main" }, name, seek), time);
    wrap.append(a);
    if (r?.transcript) {
      const details = h("details", { class: "audio-transcript" }, h("summary", { text: "Transcript" }), h("p", { text: r.transcript }));
      wrap.append(details);
    }
    const setIcon = () => {
      play.replaceChildren(icon(a.paused ? "play" : "pause", true));
      play.setAttribute("aria-label", `${a.paused ? "Play" : "Pause"} ${title}`);
    };
    let ready = false;
    const load = async (): Promise<boolean> => {
      if (ready) return true;
      const source = await this.mediaSource(region.resource);
      if (!source) {
        this.host.toast("This audio is not available.");
        return false;
      }
      if ("consent" in source) {
        this.consent(wrap, source.consent, () => {
          a.src = source.url;
          ready = true;
          void a.play();
        });
        return false;
      }
      a.src = source.url;
      ready = true;
      return true;
    };
    play.addEventListener("click", async (e) => {
      e.stopPropagation();
      if (!(await load().catch(() => false))) return;
      if (a.paused) await a.play().catch(() => this.host.toast("This audio cannot be played on this device."));
      else a.pause();
    });
    a.addEventListener("play", setIcon);
    a.addEventListener("pause", setIcon);
    a.addEventListener("ended", setIcon);
    const tick = () => {
      const d = a.duration || (r?.duration_ms ? r.duration_ms / 1000 : 0);
      if (d > 0 && document.activeElement !== seek) seek.value = String(Math.round((a.currentTime / d) * 1000));
      time.textContent = a.currentTime > 0 || !d ? formatTime(a.currentTime) : formatTime(d);
    };
    a.addEventListener("timeupdate", tick);
    a.addEventListener("loadedmetadata", tick);
    seek.addEventListener("input", async () => {
      if (!(await load().catch(() => false))) return;
      const d = a.duration || 0;
      if (d > 0) a.currentTime = (Number(seek.value) / 1000) * d;
    });
    tick();
    return wrap;
  }

  /** Full-window viewer for an image or a gallery. */
  lightbox(resources: string[], start: number, caption?: string): void {
    this.closeLightbox();
    const book = this.host.book;
    let index = start;
    const img = h("img", { class: "lightbox-image", alt: "" });
    const count = h("span", { class: "lightbox-count" });
    const text = h("p", { class: "lightbox-caption" });
    const go = (i: number) => {
      index = (i + resources.length) % resources.length;
      const name = resources[index]!;
      const r = book.resource(name);
      img.alt = r?.alt ?? "";
      text.textContent = [r?.alt, caption].filter(Boolean).join(" — ");
      count.textContent = resources.length > 1 ? `${index + 1} / ${resources.length}` : "";
      void book
        .url(name)
        .then((u) => {
          img.src = u;
        })
        .catch(() => this.host.toast("The image could not be loaded."));
    };
    const el = h(
      "div",
      { class: "lightbox", role: "dialog", "aria-modal": "true", "aria-label": resources.length > 1 ? "Gallery" : "Image", tabindex: "-1" },
      h("div", { class: "lightbox-bar" }, count, iconButton("close", "Close", () => this.closeLightbox())),
      h(
        "div",
        { class: "lightbox-stage" },
        resources.length > 1 ? iconButton("prev", "Previous image", () => go(index - 1), "lightbox-nav") : null,
        img,
        resources.length > 1 ? iconButton("next", "Next image", () => go(index + 1), "lightbox-nav") : null,
      ),
      text,
    );
    el.addEventListener("click", (e) => {
      if (e.target === el || (e.target as HTMLElement).classList.contains("lightbox-stage")) this.closeLightbox();
    });
    el.addEventListener("keydown", (e) => {
      if (e.key === "ArrowRight") go(index + 1);
      else if (e.key === "ArrowLeft") go(index - 1);
    });
    // Swipe between images on touch screens.
    let downX = 0;
    img.addEventListener("pointerdown", (e) => (downX = e.clientX));
    img.addEventListener("pointerup", (e) => {
      const dx = e.clientX - downX;
      if (Math.abs(dx) > 40 && resources.length > 1) go(index + (dx < 0 ? 1 : -1));
    });
    this.root.append(el);
    this.box = { el, release: trapFocus(el, () => this.closeLightbox()) };
    go(start);
  }

  closeLightbox(): void {
    if (!this.box) return;
    this.box.release();
    this.box.el.remove();
    this.box = null;
  }

  dispose(): void {
    this.clear();
    this.closeLightbox();
    this.layer.remove();
  }
}
