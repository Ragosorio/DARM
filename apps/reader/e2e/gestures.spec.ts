import { book, diag, drag, expect, openBook, openCover, pageRects, settled, test } from "./helpers";

test.describe("turning pages by hand", () => {
  test.skip(({ isMobile }) => isMobile, "mouse gestures are covered on desktop projects; touch has its own spec");

  test("mouse: drag to turn, short drag falls back, drag back, clicks on the page edges", async ({ page }, info) => {
    await openCover(page, book("kitchen-sink.darm"));
    await openBook(page);
    await page.keyboard.press("ArrowRight");
    let d = await settled(page);
    const start = d.pos;
    const r = await pageRects(page);
    const y = r.right.y + r.right.height * 0.8;

    // Hold the page half-way: the leaf is in the air and follows the pointer.
    await drag(page, [r.right.x + r.right.width * 0.92, y], [r.right.x + r.right.width * 0.1, y], { release: false });
    d = await diag(page);
    expect(d.settled, "a held page is not at rest").toBe(false);
    expect(d.pos).toBe(start);
    await info.attach("page held in mid-turn", { body: await page.screenshot(), contentType: "image/png" });
    // Carry it across and let go: the turn completes.
    await page.mouse.move(r.left!.x + r.left!.width * 0.3, y, { steps: 10 });
    await page.mouse.up();
    d = await settled(page);
    expect(d.pos).toBe(start + 1);

    // A short drag that is released returns the page.
    await drag(page, [r.right.x + r.right.width * 0.92, y], [r.right.x + r.right.width * 0.72, y], { steps: 30 });
    d = await settled(page);
    expect(d.pos).toBe(start + 1);

    // Dragging the left page back.
    await drag(page, [r.left!.x + r.left!.width * 0.08, y], [r.right.x + r.right.width * 0.7, y]);
    d = await settled(page);
    expect(d.pos).toBe(start);

    // A quick flick turns the page even though it is let go before the middle.
    await drag(page, [r.right.x + r.right.width * 0.9, y], [r.right.x + r.right.width * 0.45, y], { steps: 4 });
    d = await settled(page);
    expect(d.pos).toBe(start + 1);

    // Clicking the outer part of a page turns it; the inner part toggles the controls.
    await page.mouse.click(r.right.x + r.right.width * 0.9, r.right.y + r.right.height * 0.5);
    d = await settled(page);
    expect(d.pos).toBe(start + 2);
    await page.mouse.click(r.left!.x + r.left!.width * 0.1, r.left!.y + r.left!.height * 0.5);
    d = await settled(page);
    expect(d.pos).toBe(start + 1);
    const hiddenBefore = await page.locator(".reader").evaluate((el) => el.classList.contains("chrome-hidden"));
    await page.mouse.click(r.right.x + r.right.width * 0.25, r.right.y + r.right.height * 0.93);
    await expect.poll(() => page.locator(".reader").evaluate((el) => el.classList.contains("chrome-hidden"))).toBe(!hiddenBefore);
    expect((await diag(page)).pos).toBe(start + 1);
  });

  test("the cover itself can be dragged open, and rapid key presses fan several leaves", async ({ page }) => {
    let d = await openCover(page, book("atlas-de-mareas.darm"));
    const r = await pageRects(page);
    await drag(page, [r.right.x + r.right.width * 0.85, r.right.y + r.right.height * 0.5], [r.right.x - r.right.width * 0.4, r.right.y + r.right.height * 0.5], { steps: 24 });
    d = await settled(page);
    expect(d.pos).toBe(1);

    for (let i = 0; i < 5; i++) {
      await page.keyboard.press("ArrowRight");
      await page.waitForTimeout(70);
    }
    const flying = await page.evaluate(() => (window as unknown as { darm: { app: { reader: { stageForTests: { controller: { flyers: unknown[] } } } } } }).darm.app.reader.stageForTests.controller.flyers.length);
    expect(flying, "several leaves are in the air at once").toBeGreaterThanOrEqual(3);
    d = await settled(page);
    expect(d.pos).toBe(6);

    // Reversal in mid-air: the same leaf turns round.
    await page.keyboard.press("ArrowRight");
    await page.waitForTimeout(180);
    await page.keyboard.press("ArrowLeft");
    d = await settled(page);
    expect(d.pos).toBe(6);

    // Home / End jump with a riffle and arrive.
    await page.keyboard.press("End");
    d = await settled(page);
    expect(d.pos).toBe(d.count);
    await page.keyboard.press("Home");
    d = await settled(page);
    expect(d.page).toBeGreaterThanOrEqual(0);
    await page.keyboard.press("Control+Home");
    d = await settled(page);
    expect(d.pos).toBe(0);
  });
});
