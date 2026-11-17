import { book, diag, expect, openBook, openCover, settled, showBlock, test } from "./helpers";

test.describe("accessibility and reduced motion", () => {
  test.skip(({ isMobile }) => isMobile, "covered on desktop engines");

  test("reduced motion: no flight, but every way of navigating still works", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await openCover(page, book("kitchen-sink.darm"));
    await page.getByRole("button", { name: "Open the book" }).click();
    let d = await settled(page);
    expect(d.pos).toBe(1);
    expect(d.frameTimes.length, "no animation frames were needed").toBe(0);
    await page.keyboard.press("ArrowRight");
    d = await diag(page);
    expect(d.pos, "the change is immediate").toBe(2);
    expect(d.settled).toBe(true);
    // A swipe still turns the page.
    const box = (await page.locator(".stage-canvas").boundingBox())!;
    await page.mouse.move(box.x + box.width * 0.8, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width * 0.6, box.y + box.height / 2, { steps: 5 });
    await page.mouse.up();
    expect((await settled(page)).pos).toBe(3);
    await page.keyboard.press("End");
    d = await settled(page);
    expect(d.pos).toBe(d.count);
    // The setting can also be forced from inside the reader, whatever the system says.
    await page.emulateMedia({ reducedMotion: "no-preference" });
    await page.keyboard.press("Home");
    await settled(page);
    await page.getByRole("button", { name: "Reading settings" }).click();
    await page.getByRole("radio", { name: "Reduced" }).click();
    await page.keyboard.press("Escape");
    const before = (await diag(page)).pos;
    await page.keyboard.press("ArrowRight");
    d = await diag(page);
    expect([d.pos, d.settled]).toEqual([before + 1, true]);
  });

  test("keyboard only: reach and operate everything; screen-reader outline of the page", async ({ page, browserName }) => {
    // Safari moves focus to buttons with Option+Tab unless "Keyboard navigation" is enabled in macOS settings.
    const tab = browserName === "webkit" ? "Alt+Tab" : "Tab";
    await openCover(page, book("kitchen-sink.darm"));
    // Enter opens the book from its cover.
    await page.keyboard.press("Enter");
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "page");
    await settled(page);
    await showBlock(page, "fork");

    // Tab walks the controls on the page in reading order, with a visible focus ring.
    await page.locator("body").click({ position: { x: 5, y: 400 } });
    const focused = () => page.evaluate(() => document.activeElement?.getAttribute("aria-label") ?? document.activeElement?.tagName);
    const seen: (string | undefined)[] = [];
    for (let i = 0; i < 4; i++) {
      await page.keyboard.press(tab);
      seen.push(await focused());
    }
    expect(seen).toEqual(["Climb to the lighthouse", "Stay on the shingle", "Light the lantern", expect.any(String)]);
    await page.keyboard.press(browserName === "webkit" ? "Alt+Shift+Tab" : "Shift+Tab");
    const ring = await page.evaluate(() => {
      const s = getComputedStyle(document.activeElement!);
      return { style: s.outlineStyle, width: parseFloat(s.outlineWidth) };
    });
    expect(ring.style).not.toBe("none");
    expect(ring.width).toBeGreaterThanOrEqual(2);
    // Space activates the focused control instead of turning the page.
    const pos = (await diag(page)).pos;
    await page.keyboard.press("Space");
    const d = await settled(page);
    expect(d.state.lit).toBe(true);
    expect(d.pos).toBe(pos);

    // The page has a real text outline for assistive technology.
    const outline = page.locator('.page-overlay article[aria-label^="Page"]');
    await expect(outline.filter({ hasText: "The path forks at the breakwater." })).toHaveCount(1);
    await showBlock(page, "quay");
    await expect(page.locator(".page-overlay article h3", { hasText: "The quay" })).toHaveCount(1);
    await expect(page.locator('.page-overlay article [role="img"][aria-label="A low sun over a dark sea"]').first()).toBeAttached();
    await expect(page.locator('[role="status"][aria-live="polite"]').first()).toHaveText(/Page \d+ of \d+/);

    // Every button has an accessible name; interactive targets are comfortably large.
    const unnamed = await page.evaluate(() => [...document.querySelectorAll("button")].filter((b) => !(b.getAttribute("aria-label") || b.textContent?.trim())).length);
    expect(unnamed).toBe(0);
    await showBlock(page, "fork");
    const sizes = await page.evaluate(() => [...document.querySelectorAll("button.region-option, button.region-button, button.region-answer")].map((b) => b.getBoundingClientRect().height));
    expect(sizes.length).toBeGreaterThan(3);
    expect(Math.min(...sizes)).toBeGreaterThanOrEqual(40);
    // Panels trap focus and close with Escape, returning focus.
    await page.keyboard.press("t");
    await expect(page.getByRole("dialog", { name: "Contents" })).toBeVisible();
    expect(await page.evaluate(() => document.activeElement?.closest(".panel") !== null)).toBe(true);
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog", { name: "Contents" })).toHaveCount(0);
  });

  test("without WebGL the Canvas 2D renderer still reads, turns and interacts", async ({ page }) => {
    await page.goto("/?renderer=2d");
    await page.setInputFiles("#import-input", book("kitchen-sink.darm"));
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "front");
    let d = await settled(page);
    expect(d.renderer).toBe("canvas2d");
    d = await openBook(page);
    expect(d.frameTimes.length).toBeGreaterThan(10);
    await page.keyboard.press("ArrowRight");
    d = await settled(page);
    expect(d.pos).toBe(2);
    await showBlock(page, "fork");
    await page.getByRole("button", { name: "Light the lantern" }).click();
    expect((await settled(page)).state.lit).toBe(true);
    // Something book-coloured was actually drawn.
    const drawn = await page.locator(".stage-canvas").evaluate((c: HTMLCanvasElement) => {
      const ctx = c.getContext("2d")!;
      // In the outer margin of the right-hand page, where there is only paper.
      const px = ctx.getImageData(Math.round(c.width * 0.855), Math.round(c.height * 0.5), 1, 1).data;
      return px[0]! + px[1]! + px[2]!;
    });
    expect(drawn).toBeGreaterThan(500);
  });
});
