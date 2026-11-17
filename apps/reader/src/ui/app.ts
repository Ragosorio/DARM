/**
 * Application shell: the personal library, importing and opening books,
 * unlocking protected ones, and switching between the shelf and the reader.
 */
import { type Credentials, DarmError, OpenBook, type ProbeInfo, probeSource, readKeyfile, verifySource } from "../core/book";
import { type BookRecord, Library, MemoryLibrary } from "../core/library";
import { type Prefs, loadPrefs, savePrefs } from "../core/prefs";
import { type ByteSource, blobSource, urlSource } from "../core/source";
import { h, icon, iconButton, trapFocus } from "./dom";
import { ReaderView } from "./reader";

/** Plain-language explanations for the failures a reader can meet. */
function explain(e: unknown): { title: string; detail: string; code: string } {
  const err = e instanceof DarmError ? e : new DarmError("E_READER", e instanceof Error ? e.message : String(e));
  const c = err.code;
  const title =
    c === "E_PKG_BAD_MAGIC"
      ? "This is not a DARM book"
      : c === "E_PKG_TRUNCATED"
        ? "This file is incomplete"
        : c === "E_PKG_UNSUPPORTED_VERSION" || c === "E_IR_VERSION"
          ? "This book needs a newer reader"
          : c === "E_PKG_INTEGRITY" || c.startsWith("E_PKG_CORRUPT") || c === "E_PKG_HEADER"
            ? "This book is damaged or was modified"
            : c.startsWith("E_IR")
              ? "This book's contents are not valid"
              : c === "E_READER_KDF" || c === "E_READER_LIMIT" || c === "E_PKG_LIMIT"
                ? "This book exceeds what this reader allows"
                : "The book could not be opened";
  return { title, detail: err.message, code: c };
}

interface AppOptions {
  root: HTMLElement;
}

export class App {
  private root: HTMLElement;
  private library: Library;
  private prefs: Prefs;
  reader: ReaderView | null = null;
  private shelf: HTMLElement | null = null;
  private dialog: { el: HTMLElement; release: () => void } | null = null;
  private thumbUrls: string[] = [];
  private dev = false;
  private forceFallback = false;
  private opening = false;

  constructor(options: AppOptions) {
    this.root = options.root;
    this.library = Library.available ? new Library() : new MemoryLibrary();
    this.prefs = loadPrefs();
  }

  async start(): Promise<void> {
    const q = new URLSearchParams(location.search);
    this.forceFallback = q.get("renderer") === "2d";
    this.dev = q.get("dev") === "1";
    this.bindGlobalDrop();
    this.bindLaunchQueue();
    const auto = q.get("book");
    if (auto) {
      // Used by `darm open` / `darm dev`: a same-origin preview that is not added to the library.
      const name = q.get("name") ?? "Preview";
      await this.openPreview(auto, name);
      if (this.dev) void this.watchDevServer(auto, name);
      return;
    }
    try {
      await this.library.list();
    } catch (e) {
      console.warn("persistent library unavailable, using memory", e);
      this.library = new MemoryLibrary();
    }
    await this.showLibrary();
  }

  // --- library ------------------------------------------------------------------

  async showLibrary(): Promise<void> {
    this.reader?.dispose();
    this.reader = null;
    this.shelf?.remove();
    for (const u of this.thumbUrls) URL.revokeObjectURL(u);
    this.thumbUrls = [];
    document.title = "DARM — Library";
    const books = await this.library.list().catch(() => [] as BookRecord[]);
    const input = h("input", { type: "file", accept: ".darm,application/vnd.darm", multiple: true, class: "sr-only", id: "import-input", "aria-label": "Choose DARM books to add" });
    input.addEventListener("change", () => {
      const files = [...(input.files ?? [])];
      input.value = "";
      void this.importFiles(files);
    });
    const add = h("button", { type: "button", class: "pill", onClick: () => input.click() }, icon("plus"), h("span", { text: "Add a book" }));
    const grid = h("ul", { class: "shelf-grid" });
    for (const b of books) grid.append(this.card(b));
    const empty = h(
      "div",
      { class: "shelf-empty" },
      h("h2", { text: "Your library is empty" }),
      h("p", { text: "Add a .darm book from this device, or drop one anywhere on this window. Books stay on this device." }),
    );
    this.shelf = h(
      "main",
      { class: "shelf" },
      h("header", { class: "shelf-head" }, h("div", { class: "wordmark" }, h("span", { class: "wordmark-name", text: "DARM" }), h("span", { class: "wordmark-sub", text: "Library" })), add, input),
      books.length ? grid : empty,
      h("div", { class: "drop-veil", "aria-hidden": "true" }, h("p", { text: "Drop to add to your library" })),
    );
    this.root.append(this.shelf);
  }

  private card(b: BookRecord): HTMLElement {
    let cover: HTMLElement;
    if (b.cover) {
      const url = URL.createObjectURL(b.cover);
      this.thumbUrls.push(url);
      cover = h("img", { class: "card-cover", src: url, alt: "" });
    } else {
      // No stored cover (never opened, or protected): a plain cloth placeholder.
      cover = h(
        "div",
        { class: "card-cover placeholder" },
        b.encrypted ? h("span", { class: "card-lock" }, icon("lock")) : null,
        h("span", { class: "placeholder-title", text: b.hasCard ? b.title : "Protected book" }),
        b.hasCard && b.authors.length ? h("span", { class: "placeholder-author", text: b.authors.join(", ") }) : null,
      );
      if (b.coverColor && /^#[0-9a-f]{6}$/i.test(b.coverColor)) cover.style.backgroundColor = b.coverColor;
    }
    const title = b.hasCard ? b.title : "Protected book";
    const status = b.finished ? "Finished" : b.progress > 0 ? `${Math.round(b.progress * 100)}% read` : "Not started";
    const open = h(
      "button",
      { type: "button", class: "card-open", "aria-label": `Open ${title}${b.authors.length ? ` by ${b.authors.join(", ")}` : ""}. ${status}.`, onClick: () => void this.openRecord(b.id) },
      cover,
      h("span", { class: "card-title", text: title }),
      h("span", { class: "card-author", text: b.hasCard ? b.authors.join(", ") : b.fileName }),
      h("span", { class: "card-status" }, h("span", { class: "card-progress" }, h("span", { style: `width:${Math.round(b.progress * 100)}%` })), h("span", { text: status })),
    );
    const remove = iconButton("trash", `Remove ${title} from the library`, () => this.confirmRemove(b), "card-remove");
    return h("li", { class: "card", "data-id": b.id }, open, remove);
  }

  private confirmRemove(b: BookRecord): void {
    const title = b.hasCard ? b.title : "this protected book";
    this.modal("Remove from the library?", [`“${title}” and its reading position, bookmarks and answers will be deleted from this device. The original file you imported is not affected.`], [
      { label: "Keep", ghost: true },
      {
        label: "Remove",
        run: async () => {
          await this.library.remove(b.id);
          await this.showLibrary();
        },
      },
    ]);
  }

  // --- importing ------------------------------------------------------------------

  /** Add files to the library. A single file is opened straight away. */
  async importFiles(files: File[]): Promise<void> {
    const failures: string[] = [];
    let last: string | null = null;
    for (const file of files) {
      try {
        const source = blobSource(file, file.name);
        const info = await probeSource(source);
        // Nothing damaged or altered goes on the shelf.
        if (!info.encrypted) {
          if (file.size > 32 * 1024 * 1024) this.busy(`Checking ${file.name}…`);
          await verifySource(source);
        }
        const existing = await this.library.get(info.uuid);
        const record: BookRecord = {
          id: info.uuid,
          title: info.card?.title ?? file.name.replace(/\.darm$/i, ""),
          authors: info.card?.authors ?? [],
          language: info.card?.language,
          coverColor: info.card?.cover_color,
          encrypted: info.encrypted,
          hasCard: info.card !== null,
          fileName: file.name,
          size: file.size,
          addedAt: existing?.addedAt ?? Date.now(),
          openedAt: existing?.openedAt ?? 0,
          progress: existing?.progress ?? 0,
          finished: existing?.finished ?? false,
          cover: existing?.cover,
        };
        await this.library.add(record, file);
        last = info.uuid;
      } catch (e) {
        const x = explain(e);
        failures.push(`${file.name}: ${x.title.toLowerCase()} (${x.detail})`);
      }
    }
    this.closeDialog();
    if (failures.length) {
      this.modal(failures.length === 1 ? "A file could not be added" : "Some files could not be added", failures, [{ label: "Close" }]);
      if (!this.reader) await this.showLibrary();
      return;
    }
    if (files.length === 1 && last) await this.openRecord(last);
    else if (!this.reader) await this.showLibrary();
  }

  private bindGlobalDrop(): void {
    let depth = 0;
    const hasFiles = (e: DragEvent) => [...(e.dataTransfer?.types ?? [])].includes("Files");
    window.addEventListener("dragenter", (e) => {
      if (!hasFiles(e)) return;
      depth++;
      document.body.classList.add("dragging");
    });
    window.addEventListener("dragleave", () => {
      depth = Math.max(0, depth - 1);
      if (depth === 0) document.body.classList.remove("dragging");
    });
    window.addEventListener("dragover", (e) => {
      if (hasFiles(e)) e.preventDefault();
    });
    window.addEventListener("drop", (e) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      depth = 0;
      document.body.classList.remove("dragging");
      const files = [...(e.dataTransfer?.files ?? [])];
      if (files.length) void this.importFiles(files);
    });
  }

  /** Files handed over by the operating system to an installed PWA. */
  private bindLaunchQueue(): void {
    const queue = (window as unknown as { launchQueue?: { setConsumer(cb: (p: { files?: { getFile(): Promise<File> }[] }) => void): void } }).launchQueue;
    queue?.setConsumer((params) => {
      void Promise.all((params.files ?? []).map((f) => f.getFile())).then((files) => {
        if (files.length) void this.importFiles(files);
      });
    });
  }

  // --- opening --------------------------------------------------------------------

  async openRecord(id: string): Promise<void> {
    if (this.opening) return;
    const [record, file] = await Promise.all([this.library.get(id), this.library.file(id)]);
    if (!record || !file) {
      this.modal("This book is no longer in the library", ["Its file could not be found on this device. Add it again."], [{ label: "Close" }]);
      await this.showLibrary();
      return;
    }
    await this.openSource(blobSource(file, record.fileName), record, this.library);
  }

  private async openPreview(url: string, name: string): Promise<void> {
    const memory = new MemoryLibrary();
    const record: BookRecord = { id: "preview", title: name, authors: [], encrypted: false, hasCard: true, fileName: name, size: 0, addedAt: Date.now(), openedAt: 0, progress: 0, finished: false };
    await memory.add(record, new Blob());
    this.previewLibrary = memory;
    try {
      const source = await urlSource(url, name);
      await this.openSource(source, record, memory);
    } catch (e) {
      this.fail(e, false);
    }
  }
  private previewLibrary: MemoryLibrary | null = null;

  private async openSource(source: ByteSource, record: BookRecord, library: Library): Promise<void> {
    this.opening = true;
    try {
      const info = await probeSource(source);
      let credentials: Credentials = {};
      for (;;) {
        try {
          if (info.encrypted) {
            const given = await this.askCredentials(info, record, credentials.password !== undefined || credentials.key !== undefined);
            if (!given) return; // cancelled
            credentials = given;
            this.busy("Unlocking…");
          }
          const book = await OpenBook.open(source, credentials);
          await this.enterReader(book, record, library);
          return;
        } catch (e) {
          if (e instanceof DarmError && e.code === "E_PKG_WRONG_CREDENTIALS") continue; // ask again, with a message
          throw e;
        } finally {
          this.closeDialog();
        }
      }
    } catch (e) {
      this.fail(e, library === this.library);
    } finally {
      this.opening = false;
    }
  }

  private async enterReader(book: OpenBook, record: BookRecord, library: Library): Promise<void> {
    this.shelf?.remove();
    this.shelf = null;
    this.reader?.dispose();
    document.title = `${book.ir.meta.title} — DARM`;
    const reader = new ReaderView({
      root: this.root,
      book,
      library,
      record,
      prefs: this.prefs,
      forceFallback: this.forceFallback,
      onPrefs: (p) => {
        this.prefs = p;
        savePrefs(p);
      },
      onExit: () => void (library === this.library ? this.showLibrary() : this.showLibrary()),
    });
    this.reader = reader;
    try {
      await reader.start();
    } catch (e) {
      reader.dispose();
      this.reader = null;
      throw e;
    }
    if (library === this.library) {
      const patch: Partial<BookRecord> = { openedAt: Date.now() };
      // Covers of protected books are not written to storage in the clear.
      if (!book.encrypted) {
        patch.title = book.ir.meta.title;
        patch.authors = book.ir.meta.authors;
        patch.coverColor = book.ir.cover.front.color;
        if (!record.cover) {
          const cover = await reader.coverThumbnail().catch(() => null);
          if (cover) patch.cover = cover;
        }
      }
      await this.library.update(record.id, patch).catch(() => undefined);
    }
  }

  private fail(e: unknown, backToShelf: boolean): void {
    const x = explain(e);
    console.error(e);
    this.modal(x.title, [x.detail, `Code: ${x.code}`], [{ label: "Close", run: () => (backToShelf && !this.reader ? this.showLibrary() : undefined) }]);
    if (backToShelf && !this.reader && !this.shelf) void this.showLibrary();
  }

  // --- dialogs --------------------------------------------------------------------

  private closeDialog(): void {
    if (!this.dialog) return;
    this.dialog.release();
    this.dialog.el.remove();
    this.dialog = null;
  }

  private busy(text: string): void {
    this.closeDialog();
    const el = h("div", { class: "modal-backdrop" }, h("div", { class: "modal busy", role: "status", tabindex: "-1" }, h("p", { text })));
    this.root.append(el);
    this.dialog = { el, release: () => undefined };
  }

  private modal(title: string, lines: string[], actions: { label: string; ghost?: boolean; run?: () => unknown }[]): void {
    this.closeDialog();
    const box = h("div", { class: "modal", role: "alertdialog", "aria-modal": "true", "aria-label": title, tabindex: "-1" }, h("h2", { text: title }));
    for (const l of lines) box.append(h("p", { text: l }));
    const row = h("div", { class: "modal-actions" });
    for (const a of actions) {
      row.append(
        h("button", {
          type: "button",
          class: a.ghost ? "pill ghost" : "pill",
          text: a.label,
          onClick: () => {
            this.closeDialog();
            void a.run?.();
          },
        }),
      );
    }
    box.append(row);
    const el = h("div", { class: "modal-backdrop" }, box);
    this.root.append(el);
    this.dialog = { el, release: trapFocus(el, () => this.closeDialog()) };
  }

  /** Ask for the password or key file of a protected book. Resolves to null when cancelled. */
  private askCredentials(info: ProbeInfo, record: BookRecord, retry: boolean): Promise<Credentials | null> {
    this.closeDialog();
    return new Promise((resolve) => {
      const kinds = new Set(info.slots.map((s) => s.kind));
      const title = info.card?.title ?? "Protected book";
      const password = h("input", { type: "password", class: "text-input", id: "unlock-password", autocomplete: "off", autocapitalize: "off", spellcheck: "false", "aria-describedby": "unlock-error" });
      const error = h("p", { class: "form-error", id: "unlock-error", role: "alert", text: retry ? "That password or key does not open this book. Try again." : "" });
      const keyInput = h("input", { type: "file", accept: ".darmkey,application/json,text/plain", class: "sr-only", "aria-label": "Choose a key file" });
      const finish = (c: Credentials | null) => {
        this.closeDialog();
        resolve(c);
      };
      keyInput.addEventListener("change", async () => {
        const file = keyInput.files?.[0];
        if (!file) return;
        try {
          if (file.size > 16 * 1024) throw new Error("too large");
          finish({ key: await readKeyfile(await file.text()) });
        } catch {
          error.textContent = "That file is not a DARM key file.";
        }
      });
      const form = h(
        "form",
        { class: "modal", role: "dialog", "aria-modal": "true", "aria-label": `Unlock ${title}`, tabindex: "-1" },
        h("span", { class: "modal-icon" }, icon("lock")),
        h("h2", { text: title }),
        info.card?.authors.length ? h("p", { class: "modal-sub", text: info.card.authors.join(", ") }) : null,
        h("p", { text: "This book is protected. It is decrypted on this device only, and the password is not stored." }),
        kinds.has("password") || kinds.size === 0 ? h("label", { for: "unlock-password", text: "Password" }) : null,
        kinds.has("password") || kinds.size === 0 ? password : null,
        error,
        h(
          "div",
          { class: "modal-actions" },
          h("button", { type: "button", class: "pill ghost", text: "Cancel", onClick: () => finish(null) }),
          kinds.has("keyfile") ? h("button", { type: "button", class: "pill ghost", onClick: () => keyInput.click() }, icon("key"), h("span", { text: "Use a key file" })) : null,
          kinds.has("password") || kinds.size === 0 ? h("button", { type: "submit", class: "pill", text: "Unlock" }) : null,
        ),
        keyInput,
      );
      form.addEventListener("submit", (e) => {
        e.preventDefault();
        if (!password.value) {
          error.textContent = "Enter the password.";
          return;
        }
        const value = password.value;
        password.value = "";
        finish({ password: value });
      });
      const el = h("div", { class: "modal-backdrop" }, form);
      this.root.append(el);
      this.dialog = { el, release: trapFocus(el, () => finish(null)) };
      void record;
    });
  }

  // --- live preview (`darm dev`) ----------------------------------------------------

  private async watchDevServer(url: string, name: string): Promise<void> {
    let version = 0;
    const banner = h("pre", { class: "dev-diagnostics", role: "status" });
    for (;;) {
      try {
        const r = await fetch(`/__darm/wait?v=${version}`, { cache: "no-store" });
        const info = (await r.json()) as { version: number; build?: { ok?: boolean; text?: string } };
        if (info.version !== version) {
          const first = version === 0;
          version = info.version;
          banner.remove();
          if (info.build && info.build.ok === false) {
            banner.textContent = info.build.text ?? "The build failed.";
            this.root.append(banner);
          } else if (!first) {
            // Same preview record, so the reading position carries over.
            const record = await this.previewLibrary?.get("preview");
            if (record && this.previewLibrary) {
              const source = await urlSource(url, name);
              await this.openSource(source, record, this.previewLibrary);
            }
          }
        }
      } catch {
        await new Promise((r) => setTimeout(r, 1500));
      }
    }
  }

  /** Entry point for native shells (Tauri) that receive a file from the operating system. */
  async openBytes(bytes: Uint8Array<ArrayBuffer>, name: string): Promise<void> {
    await this.importFiles([new File([bytes], name, { type: "application/vnd.darm" })]);
  }
}
