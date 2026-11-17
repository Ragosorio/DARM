// Screenshots of the real reader for design work.
//
//   npm run dev                       (in another terminal: serves on :5173)
//   node harness/look.mjs OUT_DIR [atlas|lighthouse]
//
// Opens a book through the dev-only `?book=` URL, drives the stage with its
// deterministic `advance()` hook (so poses in mid-turn are reproducible), and
// writes PNGs of the cover, a spread, a page held in mid-turn, a fan of
// leaves, the panels and the back cover.
import { chromium, webkit } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { join } from "node:path";

const out = process.argv[2] ?? "harness/shots";
const which = process.argv[3] ?? "lighthouse";
const engine = process.env.ENGINE === "webkit" ? webkit : chromium;
const books = {
  atlas: "/e2e/.tmp/atlas-de-mareas.darm",
  lighthouse: "/e2e/.tmp/lighthouse-clear.darm",
};
mkdirSync(out, { recursive: true });

const browser = await engine.launch(engine === chromium ? { channel: "chrome" } : {});
const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2, colorScheme: "dark" });
page.on("pageerror", (e) => console.error("pageerror:", e.message));
await page.goto(`http://localhost:5173/?book=${books[which]}&name=${which}`);
await page.waitForFunction(() => window.darm?.app?.reader?.layout);
const shot = async (name) => {
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(null)))));
  await page.screenshot({ path: join(out, `${which}-${name}.png`) });
  console.log(`${which}-${name}.png`);
};
const run = (fn, arg) => page.evaluate(fn, arg);
const settle = () =>
  run(() => {
    const r = window.darm.app.reader;
    for (let i = 0; i < 40 && !window.darm.diagnostics().settled; i++) r.stageForTests.advance(0.25);
    r.stageForTests.advance(0.05);
  });

await settle();
await shot("1-cover");
// The cover in the middle of opening.
await run(() => {
  const r = window.darm.app.reader;
  r.open();
  r.stageForTests.advance(0.22);
});
await shot("2-opening");
await settle();
for (let i = 0; i < 4; i++) {
  await run(() => window.darm.app.reader.next());
  await settle();
}
await page.mouse.move(700, 450);
await shot("3-spread");
// A leaf in mid-turn.
await run(() => {
  const r = window.darm.app.reader;
  r.next();
  r.stageForTests.advance(0.11);
});
await shot("4-turning");
await settle();
// Three leaves in the air.
await run(() => {
  const r = window.darm.app.reader;
  r.next();
  r.stageForTests.advance(0.07);
  r.next();
  r.stageForTests.advance(0.07);
  r.next();
  r.stageForTests.advance(0.1);
});
await shot("5-fan");
await settle();
await page.getByRole("button", { name: "Reading settings" }).click();
await page.waitForTimeout(450);
await shot("6-settings");
await page.keyboard.press("Escape");
await page.getByRole("button", { name: "Contents" }).click();
await page.waitForTimeout(450);
await shot("7-contents");
await page.keyboard.press("Escape");
await run(() => window.darm.app.reader.goTo("end", "none"));
await settle();
await shot("8-back");
await page.goto("http://localhost:5173/");
await page.waitForTimeout(600);
await shot("9-library");
await browser.close();
