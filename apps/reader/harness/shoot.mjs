// Screenshot the layout harness in several configurations.
//
//   node harness/shoot.mjs            (from apps/reader)
//   node harness/shoot.mjs desktop    (only configurations whose name contains "desktop")
//
// Starts its own Vite dev server on a free port, opens harness/layout.html in
// Chrome (falls back to Playwright's bundled Chromium), and writes to
// harness/shots/:
//   <name>-overview.png        the whole grid at device scale 1
//   <name>-<canvas id>.png     every page and surface at device scale 2
//   <name>.txt                 the text report (layout time, measure, blocks per page)
import { chromium } from "@playwright/test";
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const out = join(here, "shots");
const port = 5300 + Math.floor(Math.random() * 400);

const configs = [
  { name: "desktop-spread", query: "w=560&h=820&spread=1" },
  { name: "phone-single", query: "w=360&h=640" },
  { name: "phone-351", query: "w=351&h=700" },
  { name: "dark-spread", query: "w=560&h=820&spread=1&theme=dark&state=lit:true&quiz=star_quiz:1&choice=fork:2" },
  { name: "large-type", query: "w=560&h=820&spread=1&font=1.4&lh=relaxed" },
  { name: "cliff", query: "w=560&h=820&spread=1&state=route:cliff,courage:3&quiz=star_quiz:0&choice=fork:0" },
  { name: "cover-band", query: "w=560&h=820&cover=band&only=surfaces", pages: false },
  { name: "cover-minimal", query: "w=560&h=820&cover=minimal", pages: false },
  { name: "cover-modern", query: "w=560&h=820&cover=modern&title=" + encodeURIComponent("An Unreasonably Long Title for a Short Book About Harbours"), pages: false },
  { name: "cover-image", query: "w=560&h=820&cover=classic&coverimage=pic", pages: false },
  { name: "cover-bleed", query: "w=560&h=820&cover=modern&coverimage=pic&bleed=1", pages: false },
  { name: "tiny", query: "w=200&h=300&font=1.6" },
  { name: "perf", query: "w=560&h=820&spread=1&perf=7000", pages: false, surfaces: false },
];

function startVite() {
  const child = spawn("npx", ["vite", "--port", String(port), "--strictPort", "--host", "127.0.0.1"], { cwd: root, stdio: ["ignore", "pipe", "pipe"] });
  const ready = new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("vite did not start in 30 s")), 30000);
    const onData = (buf) => {
      if (String(buf).includes(`:${port}`)) {
        clearTimeout(timer);
        resolve();
      }
    };
    child.stdout.on("data", onData);
    child.stderr.on("data", (b) => process.stderr.write(b));
    child.on("exit", (code) => reject(new Error(`vite exited with ${code}`)));
  });
  return { child, ready };
}

async function launch() {
  try {
    return await chromium.launch({ channel: "chrome" });
  } catch {
    return await chromium.launch();
  }
}

const filter = process.argv[2];
mkdirSync(out, { recursive: true });
const vite = startVite();
let browser;
try {
  await vite.ready;
  browser = await launch();
  for (const cfg of configs) {
    if (filter && !cfg.name.includes(filter)) continue;
    const url = `http://127.0.0.1:${port}/harness/layout.html?${cfg.query}`;
    for (const scale of [1, 2]) {
      const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, deviceScaleFactor: scale });
      const page = await context.newPage();
      page.on("pageerror", (e) => console.error(`[${cfg.name}] page error:`, e.message));
      await page.goto(url);
      await page.waitForFunction(() => window.__harness && window.__harness.ready, null, { timeout: 60000 });
      const state = await page.evaluate(() => window.__harness);
      if (state.error) throw new Error(`${cfg.name}: ${state.error}`);
      if (scale === 1) {
        writeFileSync(join(out, `${cfg.name}.txt`), state.info.join("\n") + "\n");
        await page.screenshot({ path: join(out, `${cfg.name}-overview.png`), fullPage: true });
        console.log(`${cfg.name}: ${state.info[1]}`);
        if (state.perf) console.log(`${cfg.name}: ${state.info.find((l) => l.startsWith("perf"))}`);
      } else {
        const ids = await page.evaluate(() => Array.from(document.querySelectorAll("canvas"), (c) => c.id));
        for (const id of ids) {
          if (cfg.pages === false && id.startsWith("page-")) continue;
          if (cfg.surfaces === false && id.startsWith("surface-")) continue;
          if (cfg.pages !== false && id.startsWith("surface-") && cfg.name !== "desktop-spread" && cfg.name !== "dark-spread") continue;
          await page.locator(`#${id}`).screenshot({ path: join(out, `${cfg.name}-${id}.png`) });
        }
      }
      await context.close();
    }
  }
} finally {
  if (browser) await browser.close();
  vite.child.kill();
}
