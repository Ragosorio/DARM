import "@fontsource-variable/inter/wght.css";
import "./styles.css";
import "@fontsource-variable/literata/wght.css";
import "@fontsource-variable/literata/wght-italic.css";
import "@fontsource-variable/jetbrains-mono/wght.css";
import { core } from "./core/book";
import { initNativeShell } from "./shell/tauri";
import { App } from "./ui/app";

const root = document.getElementById("app")!;
const app = new App({ root });

// Test and shell hook: diagnostics only, no book content.
Object.assign(window, {
  darm: {
    app,
    /** Hand a file received by a native shell to the reader. */
    openFile: (bytes: Uint8Array<ArrayBuffer>, name: string) => app.openBytes(bytes, name),
    diagnostics: () => app.reader?.diagnostics() ?? null,
  },
});

// Inside the desktop/mobile shell, books handed over by the operating system
// (double-click, "Open with", a second launch) arrive through the shell.
app
  .start()
  .then(() => initNativeShell((file) => app.importFiles([file])))
  .catch((e) => {
  console.error(e);
  root.textContent = "The reader could not start. This browser may be missing WebAssembly support.";
});

// Fetch the pieces every book needs right away (not on first open), so that a
// reader who goes offline after loading the app can still open their books.
void core().catch(() => undefined);
for (const font of ['400 16px "Literata Variable"', 'italic 400 16px "Literata Variable"', '400 16px "Inter Variable"', '400 16px "JetBrains Mono Variable"']) {
  void document.fonts.load(font).catch(() => undefined);
}

// Offline support: the service worker caches the whole application on first
// visit. Not used for `darm open` / `darm dev` previews (short-lived origins).
const preview = new URLSearchParams(location.search).has("book");
if ("serviceWorker" in navigator && import.meta.env.PROD && window.isSecureContext && !preview) {
  navigator.serviceWorker.register("./sw.js").catch(() => undefined);
}
