import { type Page } from "@playwright/test";
import { book, diag, expect, openBook, openCover, pageRects, settled, test } from "./helpers";

/** A one-finger swipe, delivered as touch pointer events (what the reader listens to). */
async function swipe(page: Page, from: [number, number], to: [number, number], steps = 12): Promise<void> {
  await page.evaluate(
    async ({ from, to, steps }) => {
      const el = document.querySelector<HTMLCanvasElement>(".stage-canvas")!;
      const fire = (type: string, x: number, y: number) =>
        el.dispatchEvent(new PointerEvent(type, { pointerId: 7, pointerType: "touch", isPrimary: true, clientX: x, clientY: y, button: 0, buttons: type === "pointerup" ? 0 : 1, bubbles: true, cancelable: true }));
      fire("pointerdown", from[0], from[1]);
      for (let i = 1; i <= steps; i++) {
        fire("pointermove", from[0] + ((to[0] - from[0]) * i) / steps, from[1] + ((to[1] - from[1]) * i) / steps);
        await new Promise((r) => requestAnimationFrame(r));
      }
      fire("pointerup", to[0], to[1]);
    },
    { from, to, steps },
  );
}

test.describe("phone: single page, touch", () => {
  test.skip(({ isMobile }) => !isMobile, "runs in the phone project");

  test("swipes and taps turn pages; rotation switches to a spread and keeps the place", async ({ page }) => {
    let d = await openCover(page, book("kitchen-sink.darm"));
    expect(d.spread, "a phone in portrait shows one page").toBe(false);
    d = await openBook(page);
    expect(d.page).toBe(0);
    const r = await pageRects(page);
    expect(r.left).toBeNull();
    const viewport = page.viewportSize()!;
    expect(r.right.width, "the page uses nearly the whole screen width").toBeGreaterThan(viewport.width * 0.88);
    const y = r.right.y + r.right.height * 0.6;

    await swipe(page, [r.right.x + r.right.width * 0.9, y], [r.right.x + r.right.width * 0.1, y]);
    d = await settled(page);
    expect(d.page).toBe(1);
    await swipe(page, [r.right.x + r.right.width * 0.15, y], [r.right.x + r.right.width * 0.95, y]);
    d = await settled(page);
    expect(d.page).toBe(0);
    // A hesitant swipe that comes back is not a page turn.
    await swipe(page, [r.right.x + r.right.width * 0.9, y], [r.right.x + r.right.width * 0.8, y], 20);
    d = await settled(page);
    expect(d.page).toBe(0);

    // Taps: right edge forward, left edge back, middle toggles the controls.
    await page.touchscreen.tap(r.right.x + r.right.width * 0.92, y);
    d = await settled(page);
    expect(d.page).toBe(1);
    await page.touchscreen.tap(r.right.x + r.right.width * 0.06, y);
    d = await settled(page);
    expect(d.page).toBe(0);
    const hidden = () => page.locator(".reader").evaluate((el) => el.classList.contains("chrome-hidden"));
    const before = await hidden();
    await page.touchscreen.tap(r.right.x + r.right.width * 0.5, r.right.y + r.right.height * 0.95);
    await expect.poll(hidden).toBe(!before);

    // Rotate to landscape: two pages, same place in the book.
    await page.keyboard.press("ArrowRight");
    await settled(page);
    await page.keyboard.press("ArrowRight");
    d = await settled(page);
    const article = await page.locator(".page-overlay article").first().innerText();
    await page.setViewportSize({ width: viewport.height, height: viewport.width });
    await expect.poll(async () => (await diag(page)).spread).toBe(true);
    d = await settled(page);
    expect(d.page).not.toBeNull();
    const after = await page.locator(".page-overlay article").allInnerTexts();
    const firstWords = article.split(/\s+/).slice(0, 6).join(" ");
    expect(after.join(" ").split(/\s+/).join(" "), "the passage that was being read is still on screen").toContain(firstWords);
  });
});
