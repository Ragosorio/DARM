import { book, diag, expect, openBook, openCover, settled, showBars, test } from "./helpers";

test.describe("reading a book from cover to cover", () => {
  test("opens on the front cover, opens with a real animation, reaches the back cover", async ({ page }) => {
    let d = await openCover(page, book("kitchen-sink.darm"));
    expect(d.pos, "a book always starts closed on its front cover").toBe(0);
    expect(d.page).toBeNull();
    expect(d.pages).toBeGreaterThan(8);
    await expect(page.getByRole("button", { name: "Open the book" })).toBeVisible();

    d = await openBook(page);
    expect(d.pos).toBe(1);
    expect(d.page).toBe(0);
    // The opening was animated frame by frame, not a jump cut.
    expect(d.frameTimes.length, "frames drawn while the cover was opening").toBeGreaterThan(12);

    // Turn every sheet with the keyboard.
    const positions = [d.pos];
    for (let i = 0; i < d.count + 2; i++) {
      await page.keyboard.press("ArrowRight");
      const n = await settled(page);
      if (n.pos === positions[positions.length - 1]) break;
      positions.push(n.pos);
    }
    d = await diag(page);
    expect(positions).toEqual(Array.from({ length: d.count }, (_, i) => i + 1));
    expect(d.pos, "the last turn closes the book on its back cover").toBe(d.count);
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "back");
    await expect(page.getByRole("button", { name: /read again/i })).toBeVisible();

    // The back cover is not a dead end.
    await page.keyboard.press("ArrowLeft");
    d = await settled(page);
    expect(d.pos).toBe(d.count - 1);
    await page.keyboard.press("ArrowRight");
    await settled(page);
    await page.getByRole("button", { name: /read again/i }).click();
    d = await settled(page);
    expect(d.page).not.toBeNull();
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "page");
  });

  test("remembers the reading position across restarts and reopens there from the cover", async ({ page }) => {
    await openCover(page, book("kitchen-sink.darm"));
    let d = await openBook(page);
    for (let i = 0; i < 3; i++) {
      await page.keyboard.press("ArrowRight");
      d = await settled(page);
    }
    const saved = d.page!;
    expect(saved).toBeGreaterThan(0);
    await page.waitForTimeout(700); // debounced save

    await page.reload();
    const card = page.locator(".card-open").first();
    await expect(card).toBeVisible();
    await expect(card).toContainText("% read");
    await expect(card.locator("img.card-cover"), "the shelf shows the book's own cover").toBeVisible();
    await card.click();

    // Cover first, then the book opens by itself to the saved page.
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "front");
    await expect(page.getByRole("button", { name: /continue reading/i })).toBeVisible();
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "page", { timeout: 15_000 });
    d = await settled(page);
    expect(d.page).toBe(saved);

    // Finishing the book is remembered too.
    await page.keyboard.press("End");
    d = await settled(page);
    expect(d.pos).toBe(d.count);
    await page.waitForTimeout(700);
    await showBars(page);
    await page.getByRole("button", { name: "Back to the library" }).click();
    await expect(page.locator(".card-open").first()).toContainText("Finished");
  });

  test("bookmarks, contents and search navigate the book", async ({ page }) => {
    await openCover(page, book("kitchen-sink.darm"));
    let d = await openBook(page);

    // Contents.
    await showBars(page);
    await page.getByRole("button", { name: "Contents" }).click();
    const toc = page.getByRole("navigation", { name: "Table of contents" });
    await expect(toc.getByRole("button", { name: /Arrival/ })).toBeVisible();
    await expect(toc.getByRole("button", { name: /The cliff/ }), "a chapter behind an unmet condition is not listed").toHaveCount(0);
    await toc.getByRole("button", { name: /What the tide left/ }).click();
    d = await settled(page);
    await expect(page.locator(".page-overlay article").filter({ hasText: "What the tide left" })).toHaveCount(1);
    const tidePage = d.page!;

    // Bookmark this page, go elsewhere, come back through the bookmark.
    await showBars(page);
    await page.getByRole("button", { name: "Bookmark this page" }).click();
    await expect(page.getByRole("button", { name: "Remove bookmark" })).toHaveAttribute("aria-pressed", "true");
    await page.keyboard.press("Home");
    d = await settled(page);
    expect(d.page).not.toBe(tidePage);
    await showBars(page);
    await page.getByRole("button", { name: "Contents" }).click();
    await page.locator(".bookmarks .toc-line").first().click();
    d = await settled(page);
    expect(d.page).toBe(tidePage);

    // Search is case- and diacritic-insensitive and jumps to the hit.
    await page.keyboard.press("/");
    await page.getByRole("searchbox").fill("POLARIS");
    await expect(page.locator(".search-hit").first()).toBeVisible();
    await expect(page.locator(".search-hit mark").first()).toHaveText("Polaris");
    await page.locator(".search-hit").first().click();
    d = await settled(page);
    await expect(page.getByRole("button", { name: "Polaris" })).toBeVisible();
    // Text on a branch the reader has not taken is not revealed by search.
    await page.keyboard.press("/");
    await page.getByRole("searchbox").fill("handwriting");
    await expect(page.locator(".panel [role=status]")).toHaveText("Nothing found.");
    await page.keyboard.press("Escape");

    // Settings: larger type reflows the book but keeps the place.
    const before = (await diag(page)).pages;
    const reading = (await page.locator(".page-overlay article").first().innerText()).split(/\s+/).slice(0, 5).join(" ");
    await showBars(page);
    await page.getByRole("button", { name: "Reading settings" }).click();
    await page.getByRole("button", { name: "Larger text" }).click();
    await page.getByRole("button", { name: "Larger text" }).click();
    await page.keyboard.press("Escape");
    d = await settled(page);
    expect(d.pages).toBeGreaterThan(before);
    expect((await page.locator(".page-overlay article").allInnerTexts()).join(" "), "the passage that was at the top of the spread is still on screen").toContain(reading);
  });
});
