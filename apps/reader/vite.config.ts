import { defineConfig } from "vitest/config";
import type { Plugin } from "vite";

/**
 * Content-Security-Policy for production builds. A book can never make the
 * reader run script or talk to the network: scripts come only from the app
 * itself (plus WebAssembly), there is no `connect-src` beyond the app's own
 * origin, and the only cross-origin loads allowed at all are https media
 * (remote video/audio that a book declared and the user approved).
 * It is injected at build time so that the dev server's hot reload keeps working.
 */
const CSP = [
  "default-src 'self'",
  "script-src 'self' 'wasm-unsafe-eval'",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' blob: data:",
  "media-src 'self' blob: https:",
  "font-src 'self' blob: data:",
  "connect-src 'self' ipc: http://ipc.localhost",
  "worker-src 'self' blob:",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'none'",
].join("; ");

function csp(): Plugin {
  return {
    name: "darm-csp",
    apply: "build",
    transformIndexHtml: (html) => html.replace("<head>", `<head>\n    <meta http-equiv="Content-Security-Policy" content="${CSP}" />`),
  };
}

/**
 * Lists every file of the build in `precache.json`, so the service worker can
 * cache the whole application (WebAssembly core, fonts, hyphenation patterns
 * included) on first visit and the reader keeps working with no network.
 */
function precache(): Plugin {
  return {
    name: "darm-precache",
    apply: "build",
    generateBundle(_options, bundle) {
      const files = Object.keys(bundle)
        .filter((f) => !f.endsWith(".map"))
        .sort();
      const extra = ["./", "icon.svg", "icon-192.png", "icon-512.png", "icon-maskable.svg", "icon-maskable-192.png", "icon-maskable-512.png", "manifest.webmanifest"];
      this.emitFile({ type: "asset", fileName: "precache.json", source: JSON.stringify([...extra, ...files]) });
    },
  };
}

// The reader is a static single-page app. `base: "./"` keeps every URL
// relative so the same build runs from a web server, from the `darm open`
// dev server and from inside the Tauri shell.
export default defineConfig({
  base: "./",
  plugins: [csp(), precache()],
  build: {
    target: "es2022",
    sourcemap: true,
    assetsInlineLimit: 0,
    chunkSizeWarningLimit: 1200,
  },
  server: { port: 5173, strictPort: true },
  test: {
    include: ["tests/**/*.test.ts"],
    environment: "node",
  },
});
