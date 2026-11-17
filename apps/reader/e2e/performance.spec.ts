import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { book, diag, expect, openBook, openCover, percentile, settled, test } from "./helpers";

const RESULTS = join(dirname(fileURLToPath(import.meta.url)), "results");

test.describe("performance", () => {
  test.skip(({ isMobile }) => isMobile, "measured on desktop engines");
  test.describe.configure({ mode: "serial" });

  test("frame pacing while turning, layout time, memory", async ({ page, browserName }, info) => {
    const t0 = Date.now();
    await openCover(page, book("atlas-de-mareas.darm"));
    const loadMs = Date.now() - t0;
    let d = await openBook(page);
    // Ten ordinary turns, then a burst, then a long jump.
    for (let i = 0; i < 10; i++) {
      await page.keyboard.press("ArrowRight");
      d = await settled(page);
    }
    for (let i = 0; i < 5; i++) {
      await page.keyboard.press("ArrowRight");
      await page.waitForTimeout(60);
    }
    await settled(page);
    const t1 = Date.now();
    await page.keyboard.press("End");
    d = await settled(page);
    const jumpMs = Date.now() - t1;
    await page.keyboard.press("Home");
    d = await settled(page);

    const frames = d.frameTimes;
    const heap = await page.evaluate(() => (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory?.usedJSHeapSize ?? null);
    // Interaction latency: time from a key press to the first frame in which the page has moved.
    const latency = await page.evaluate(async () => {
      const st = (window as unknown as { darm: { app: { reader: { stageForTests: { controller: { flyers: { p: number }[] } } } } } }).darm.app.reader.stageForTests;
      const t = performance.now();
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
      for (;;) {
        await new Promise((r) => requestAnimationFrame(r));
        if (st.controller.flyers.some((f) => f.p > 0) || performance.now() - t > 1000) return performance.now() - t;
      }
    });
    await settled(page);

    const result = {
      project: info.project.name,
      browser: browserName,
      renderer: d.renderer,
      book: "atlas-de-mareas (reflow, spread)",
      pages: d.pages,
      frames: frames.length,
      frameMs: { median: percentile(frames, 50), p95: percentile(frames, 95), p99: percentile(frames, 99), max: Math.max(...frames) },
      framesOver33ms: frames.filter((f) => f > 33.4).length,
      layoutMs: Math.round(d.layoutMs * 10) / 10,
      importAndFirstPaintMs: loadMs,
      longJumpMs: jumpMs,
      keyToFirstMovingFrameMs: Math.round(latency * 10) / 10,
      textureMiB: Math.round((d.textureBytes / 1048576) * 10) / 10,
      jsHeapMiB: heap === null ? null : Math.round((heap / 1048576) * 10) / 10,
      viewport: page.viewportSize(),
      note: "headless run on the development machine; not a promise for other hardware",
    };
    mkdirSync(RESULTS, { recursive: true });
    writeFileSync(join(RESULTS, `perf-${info.project.name}.json`), JSON.stringify(result, null, 2));
    console.log(JSON.stringify(result));

    expect(frames.length).toBeGreaterThan(200);
    // Sanity bounds, deliberately loose: the suite must not flake on a busy CI machine.
    expect(result.frameMs.median, "median frame interval during turns").toBeLessThan(34);
    expect(result.layoutMs, "layout of the whole book").toBeLessThan(400);
    expect(result.keyToFirstMovingFrameMs).toBeLessThan(120);
    expect(jumpMs, "a jump across the whole book").toBeLessThan(4000);
    expect(d.textureBytes, "page textures stay within their budget").toBeLessThan(240 * 1024 * 1024);
  });
});
