import { type Page } from "@playwright/test";
import { book, expect, openBook, openCover, settled, showBlock, test } from "./helpers";

/**
 * Visual snapshots of deterministic states. Baselines live in
 * e2e/snapshots/<project>/ and are created with `npm run e2e -- --update-snapshots`.
 * Turning poses are frozen through the stage's test hook, so a snapshot of a
 * page in mid-air is as reproducible as one at rest.
 */
type Hooks = { darm: { app: { reader: { stageForTests: { controller: { next(): boolean; flyers: Record<string, unknown>[] }; advance(s: number): void; renderNow(): void } } } } };

async function freeze(page: Page, pose: { p: number; lambda: number; kappa: number; vGrab: number; count?: number }): Promise<void> {
  await page.evaluate((pose) => {
    const st = (window as unknown as Hooks).darm.app.reader.stageForTests;
    for (let i = 0; i < (pose.count ?? 1); i++) st.controller.next();
    st.controller.flyers.forEach((f, i) => {
      Object.assign(f, { p: Math.max(0.05, pose.p - i * 0.17), heldP: Math.max(0.05, pose.p - i * 0.17), held: true, lambda: pose.lambda, lambdaTarget: pose.lambda, kappa: pose.kappa, kappaTarget: pose.kappa, vGrab: pose.vGrab, v: 0 });
    });
    st.renderNow();
  }, pose);
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(null)))));
}

test.describe("visual snapshots", () => {
  test.skip(({ browserName, isMobile }) => browserName === "firefox" || isMobile, "baselines are kept for Chrome and WebKit desktop");

  test("covers, spreads, dark mode, pages in mid-turn", async ({ page }) => {
    const chrome = [page.locator(".chrome"), page.locator(".cover-hint")];
    await openCover(page, book("kitchen-sink.darm"));
    await expect(page).toHaveScreenshot("01-front-cover.png", { mask: chrome });

    await openBook(page);
    await expect(page).toHaveScreenshot("02-title-spread.png", { mask: chrome });

    await showBlock(page, "quay");
    await expect(page).toHaveScreenshot("03-figure-spread.png", { mask: chrome });

    await showBlock(page, "fork");
    await expect(page).toHaveScreenshot("04-interactive-spread.png", { mask: chrome });

    // A dragged page, peeled from its lower corner.
    await showBlock(page, "quay");
    await freeze(page, { p: 0.42, lambda: 2.4, kappa: -0.3, vGrab: 0.1 });
    await expect(page).toHaveScreenshot("05-curl-in-hand.png", { mask: chrome });
    await page.reload();

    // Several thrown leaves fanned out.
    await page.locator(".card-open").first().click();
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "page", { timeout: 15_000 });
    await settled(page);
    await page.keyboard.press("Home");
    await settled(page);
    await freeze(page, { p: 0.74, lambda: -0.42, kappa: 0.05, vGrab: 0.5, count: 3 });
    await expect(page).toHaveScreenshot("06-fan-of-leaves.png", { mask: chrome });
    await page.reload();

    // Dark mode.
    await page.locator(".card-open").first().click();
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "page", { timeout: 15_000 });
    await settled(page);
    await page.getByRole("button", { name: "Reading settings" }).click();
    await page.getByRole("radio", { name: "Night" }).click();
    await page.keyboard.press("Escape");
    await showBlock(page, "quay");
    await expect(page).toHaveScreenshot("07-dark-spread.png", { mask: chrome });

    // Back cover.
    await page.keyboard.press("End");
    await settled(page);
    await expect(page).toHaveScreenshot("08-back-cover.png", { mask: chrome });
  });

  test("a page in mid-turn really differs from the page at rest (curvature, shadow, reverse side)", async ({ page }) => {
    await openCover(page, book("kitchen-sink.darm"));
    await openBook(page);
    await showBlock(page, "quay");
    const rest = await page.screenshot();
    await freeze(page, { p: 0.5, lambda: 2.4, kappa: 0, vGrab: 0.5 });
    const turning = await page.screenshot();
    expect(turning.equals(rest)).toBe(false);
    // The canvas has pixels darker than anything on the resting right-hand page: the cast shadow.
    const stats = await page.evaluate(() => {
      const st = (window as unknown as Hooks).darm.app.reader.stageForTests;
      const c = document.querySelector<HTMLCanvasElement>(".stage-canvas")!;
      // Read the WebGL canvas in the same task as the draw, before the browser presents and clears it.
      st.renderNow();
      const o = document.createElement("canvas");
      o.width = c.width;
      o.height = c.height;
      const ctx = o.getContext("2d")!;
      ctx.drawImage(c, 0, 0);
      // A strip of the right-hand page beside the lifted leaf.
      const data = ctx.getImageData(Math.round(o.width * 0.56), Math.round(o.height * 0.3), Math.round(o.width * 0.3), Math.round(o.height * 0.3)).data;
      let min = 255;
      let max = 0;
      for (let i = 0; i < data.length; i += 4) {
        const l = (data[i]! + data[i + 1]! + data[i + 2]!) / 3;
        min = Math.min(min, l);
        max = Math.max(max, l);
      }
      return { min, max };
    });
    expect(stats.max - stats.min, "the lifted leaf is shaded and casts a shadow").toBeGreaterThan(60);
  });
});
