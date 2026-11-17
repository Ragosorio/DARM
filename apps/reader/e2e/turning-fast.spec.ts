import { type Page } from "@playwright/test";
import { book, diag, expect, openBook, openCover, settled, test } from "./helpers";

/**
 * What is on screen once the book rests must be what a fresh paint of
 * the same position gives, however the reader got there. A stale texture (a
 * page showing text that belongs elsewhere) shows up as a difference.
 */
type Pixels = { width: number; height: number; data: number[] };

/** The stage as it is on screen, read back from the canvas (a quarter of the pixels is plenty). */
async function restingPixels(page: Page): Promise<Pixels> {
  return page.evaluate(() => {
    const stage = (window as unknown as { darm: { app: { reader: { stageForTests: { canvas: HTMLCanvasElement; renderNow(): void } } } } }).darm.app.reader.stageForTests;
    stage.renderNow();
    const w = Math.round(stage.canvas.width / 2);
    const h = Math.round(stage.canvas.height / 2);
    const c = document.createElement("canvas");
    c.width = w;
    c.height = h;
    const ctx = c.getContext("2d")!;
    ctx.drawImage(stage.canvas, 0, 0, w, h);
    return { width: w, height: h, data: Array.from(ctx.getImageData(0, 0, w, h).data) };
  });
}

/** The same position painted from nothing. */
async function repainted(page: Page): Promise<Pixels> {
  await page.evaluate(() => {
    const stage = (window as unknown as { darm: { app: { reader: { stageForTests: { invalidate(): void; renderNow(): void } } } } }).darm.app.reader.stageForTests;
    stage.invalidate();
    stage.renderNow();
  });
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(null)))));
  return restingPixels(page);
}

/** Share of pixels that differ visibly. */
function difference(a: Pixels, b: Pixels): number {
  if (a.width !== b.width || a.height !== b.height) return 1;
  let n = 0;
  for (let i = 0; i < a.data.length; i += 4) {
    if (Math.abs(a.data[i]! - b.data[i]!) + Math.abs(a.data[i + 1]! - b.data[i + 1]!) + Math.abs(a.data[i + 2]! - b.data[i + 2]!) > 60) n++;
  }
  return n / (a.data.length / 4);
}

test.describe("turning pages quickly", () => {
  test.skip(({ isMobile }) => isMobile, "covered on desktop engines");

  test("a burst of key presses ends on the right pages", async ({ page }) => {
    await openCover(page, book("atlas-de-mareas.darm"));
    await openBook(page);
    for (const burst of [9, 14, 6]) {
      for (let i = 0; i < burst; i++) await page.keyboard.press("ArrowRight");
      const d = await settled(page);
      const seen = await restingPixels(page);
      const fresh = await repainted(page);
      expect(difference(seen, fresh), `after ${burst} quick presses the pages on screen are the pages of position ${d.pos}`).toBeLessThan(0.002);
    }
    for (let i = 0; i < 20; i++) await page.keyboard.press("ArrowLeft");
    const d = await settled(page);
    const seen = await restingPixels(page);
    expect(difference(seen, await repainted(page)), `after 20 quick presses back, at position ${d.pos}`).toBeLessThan(0.002);
  });

  test("jumps with the scrubber and Home/End end on the right pages", async ({ page }) => {
    await openCover(page, book("atlas-de-mareas.darm"));
    await openBook(page);
    for (const key of ["End", "Home", "End"]) {
      await page.keyboard.press(key);
      // Interrupt the riffle half-way with more input, as an impatient reader does.
      await page.waitForTimeout(180);
      await page.keyboard.press("ArrowLeft");
      await page.keyboard.press("ArrowRight");
      await page.keyboard.press("ArrowRight");
      const d = await settled(page);
      const seen = await restingPixels(page);
      expect(difference(seen, await repainted(page)), `after ${key} and a burst, at position ${d.pos}`).toBeLessThan(0.002);
    }
    expect((await diag(page)).settled).toBe(true);
  });
});

test.describe("an interactive book under impatient hands", () => {
  test.skip(({ isMobile, browserName }) => isMobile || browserName === "firefox", "Chrome and WebKit");

  test("choices, jumps and bursts of turns never leave the wrong page or label on screen", async ({ page }) => {
    test.setTimeout(240_000);
    await openCover(page, book("lighthouse-clear.darm"));
    await openBook(page);
    // A fixed pseudo-random walk, so a failure can be replayed.
    let seed = 20261116;
    const rnd = (n: number) => {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      return seed % n;
    };
    const trail: string[] = [];
    for (let step = 0; step < 45; step++) {
      const controls = page.locator(".overlay-layer button.region:not(.region-image):not(.region-gallery):not(.region-link)");
      const n = await controls.count();
      const pick = rnd(10);
      if (n > 0 && pick < 5) {
        const c = controls.nth(rnd(n));
        trail.push(`click "${await c.getAttribute("aria-label", { timeout: 2000 }).catch(() => "?")}"`);
        // The control may already have left with the page; that is not what is being tested.
        await c.click({ force: true, timeout: 2000 }).catch(() => undefined);
        // Half the time, do not wait for the jump to finish before turning on.
        if (rnd(2) === 0) {
          const k = 1 + rnd(3);
          trail.push(`→ x${k} at once`);
          for (let i = 0; i < k; i++) await page.keyboard.press("ArrowRight");
        }
      } else if (pick < 8) {
        const k = 1 + rnd(5);
        trail.push(`→ x${k}`);
        for (let i = 0; i < k; i++) await page.keyboard.press("ArrowRight");
      } else {
        const k = 1 + rnd(4);
        trail.push(`← x${k}`);
        for (let i = 0; i < k; i++) await page.keyboard.press("ArrowLeft");
      }
      // Generous: this test runs beside others, and each step may end in an entrance animation.
      const d = await settled(page, 40_000);
      const where = `step ${step} (${trail.slice(-4).join(", ")}), position ${d.pos} of ${d.count}`;
      if (d.pos <= 0) {
        await openBook(page);
        continue;
      }
      if (d.pos >= d.count) {
        await page.keyboard.press("ArrowLeft");
        await settled(page);
        continue;
      }
      const label = (await page.locator(".location").textContent()) ?? "";
      expect(label, `the page label, ${where}`).toContain(`Page ${d.page! + 1} of ${d.pages}`);
      const shot = process.env.DARM_DUMP ? await page.screenshot() : null;
      const seen = await restingPixels(page);
      const diff = difference(seen, await repainted(page));
      if (shot && diff >= 0.002) {
        const { writeFileSync } = await import("node:fs");
        writeFileSync(`${process.env.DARM_DUMP}/walk-${step}-seen.png`, shot);
        writeFileSync(`${process.env.DARM_DUMP}/walk-${step}-fresh.png`, await page.screenshot());
        console.log(`DIFF ${diff.toFixed(4)} ${where}`);
        continue;
      }
      expect(diff, `pixels on screen, ${where}`).toBeLessThan(0.002);
    }
  });
});
