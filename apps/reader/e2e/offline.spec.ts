import { PASSWORD } from "./global-setup";
import { book, expect, openBook, settled, test } from "./helpers";

test.describe("offline", () => {
  test.skip(({ browserName, isMobile }) => browserName === "firefox" || isMobile, "Chrome and WebKit");

  test("with the network cut, a book is imported, read and its media played", async ({ page, context }) => {
    await page.goto("/");
    await expect(page.locator(".shelf")).toBeVisible();
    // Use the app once while online, as any reader has before going offline:
    // open a book (which loads the typefaces and hyphenation it needs), then return to the shelf.
    await page.setInputFiles("#import-input", book("kitchen-sink.locked.darm"));
    await page.getByLabel("Password").fill(PASSWORD);
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "front");
    await openBook(page);
    await page.waitForLoadState("networkidle");
    await page.getByRole("button", { name: "Back to the library" }).click();
    await expect(page.locator(".shelf")).toBeVisible();
    await context.setOffline(true);

    await page.setInputFiles("#import-input", book("kitchen-sink.darm"));
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "front");
    let d = await openBook(page);
    for (let i = 0; i < 4; i++) {
      await page.keyboard.press("ArrowRight");
      d = await settled(page);
    }
    expect(d.pos).toBe(5);
    await page.evaluate(() => {
      const r = (window as unknown as { darm: { app: { reader: { layout: { pages: { index: number; regions: { type: string }[] }[] }; goToPage(p: number, t: string): void } } } }).darm.app.reader;
      r.goToPage(r.layout.pages.find((pg) => pg.regions.some((x) => x.type === "video"))!.index, "none");
    });
    await settled(page);
    const video = page.locator(".region-video video");
    await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.readyState), { timeout: 15_000 }).toBeGreaterThanOrEqual(1);
    await video.evaluate(async (v: HTMLVideoElement) => {
      v.muted = true;
      await v.play();
    });
    await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime)).toBeGreaterThan(0.2);

    // The encrypted copy opens offline too: decryption happens on the device.
    await page.getByRole("button", { name: "Back to the library" }).click();
    await page.locator(".card", { has: page.locator(".card-lock") }).locator(".card-open").click();
    await page.getByLabel("Password").fill(PASSWORD);
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "front");
    await context.setOffline(false);
  });

  test("installed as a web app: after one visit it reloads and reads with no network at all", async ({ page, context }) => {
    await page.goto("/");
    await expect(page.locator(".shelf")).toBeVisible();
    // The service worker installs and caches the whole application.
    await page.evaluate(async () => {
      const reg = await navigator.serviceWorker.ready;
      if (!navigator.serviceWorker.controller) {
        await new Promise<void>((resolve) => navigator.serviceWorker.addEventListener("controllerchange", () => resolve(), { once: true }));
      }
      return reg.scope;
    });
    await page.setInputFiles("#import-input", book("atlas-de-mareas.darm"));
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "front");
    await openBook(page);
    await page.keyboard.press("ArrowRight");
    await settled(page);
    await page.waitForTimeout(700);

    await context.setOffline(true);
    await page.reload();
    const card = page.locator(".card-open").first();
    await expect(card, "the library survives an offline restart").toBeVisible();
    await card.click();
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "page", { timeout: 20_000 });
    const d = await settled(page);
    expect(d.page).toBeGreaterThan(0);
    // Spanish hyphenation patterns were precached with everything else.
    await page.keyboard.press("ArrowRight");
    await settled(page);
    await context.setOffline(false);
  });
});
