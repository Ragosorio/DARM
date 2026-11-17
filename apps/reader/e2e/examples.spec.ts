import { book, diag, expect, importBook, openBook, openCover, settled, showBlock, showControl, test } from "./helpers";
import { LIGHTHOUSE_PASSWORD } from "./global-setup";

test.describe("the example books", () => {
  test.skip(({ isMobile, browserName }) => isMobile || browserName === "firefox", "smoke-tested in Chrome and WebKit");

  test("Atlas de mareas (editorial, Spanish): every page turns, search ignores accents", async ({ page }) => {
    let d = await openCover(page, book("atlas-de-mareas.darm"));
    expect(d.pages).toBeGreaterThan(25);
    d = await openBook(page);
    // Leaf through the whole book without waiting for each landing.
    for (let i = 1; i < d.count; i++) {
      await page.keyboard.press("ArrowRight");
      if (i % 6 === 0) await settled(page);
    }
    d = await settled(page);
    expect(d.pos).toBe(d.count);
    await page.keyboard.press("Home");
    await settled(page);

    await page.keyboard.press("/");
    await page.getByRole("searchbox").fill("mareografo");
    await expect(page.locator(".search-hit").first()).toBeVisible();
    await expect(page.locator(".search-hit mark").first()).toHaveText(/mare[oó]grafo/i);
    const hits = await page.locator(".search-hit").count();
    expect(hits).toBeGreaterThan(2);
    await page.keyboard.press("Escape");

    // Two-level contents, in Spanish.
    await page.getByRole("button", { name: "Contents" }).click();
    expect(await page.locator(".toc-2").count()).toBeGreaterThan(8);
    expect(await page.locator(".toc-1").count()).toBeGreaterThan(8);
  });

  test("The Lighthouse Protocol (interactive, encrypted): a route to an ending, restart, another route", async ({ page }) => {
    await importBook(page, book("the-lighthouse-protocol.darm"));
    await page.getByLabel("Password").fill(LIGHTHOUSE_PASSWORD);
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "front", { timeout: 30_000 });
    let d = await openBook(page);
    expect(d.state).toEqual({ oil: 2, half_flame: false, answered: false, duty: "lamp", route: "tower" });
    const button = (name: string | RegExp) => page.getByRole("button", { name });
    const visibleChapters = async () =>
      page.evaluate(() => (window as unknown as { darm: { app: { reader: { layout: { chapters: { id: string }[] } } } } }).darm.app.reader.layout.chapters.map((c) => c.id));

    // Route 1: the bell house, then down to the landing.
    await showBlock(page, "first_duty");
    await button("Go down to the bell house and wind the bell").click();
    await expect.poll(async () => (await diag(page)).state.duty).toBe("bell");
    await settled(page);
    expect(await visibleChapters()).toContain("bell_house");
    expect(await visibleChapters()).not.toContain("lamp_room");
    await showBlock(page, "the_signal");
    await button("Leave the lamp and go down the chain to the landing").click();
    await expect.poll(async () => (await diag(page)).state.route).toBe("landing");
    d = await settled(page);
    let chapters = await visibleChapters();
    expect(chapters.filter((c) => c.startsWith("ending_"))).toEqual(["ending_landing"]);

    // Restart from the ending.
    await showControl(page, "Wind the night back");
    await button("Wind the night back").click();
    await expect.poll(async () => (await diag(page)).state.duty).toBe("lamp");
    d = await settled(page);
    expect(d.state).toEqual({ oil: 2, half_flame: false, answered: false, duty: "lamp", route: "tower" });

    // Route 2: the lamp room, answer the signal.
    await showBlock(page, "first_duty");
    await button("Climb to the lamp room and see to the oil").click();
    await settled(page);
    expect(await visibleChapters()).toContain("lamp_room");
    await showBlock(page, "the_signal");
    await expect(button("Leave the lamp and go down the chain to the landing"), "only offered to the bell-ringer").toHaveCount(0);
    await button("Take the lanyard and answer: three long").click();
    await expect.poll(async () => (await diag(page)).state.answered).toBe(true);
    d = await settled(page);
    expect(d.state.oil).toBe(1);
    chapters = await visibleChapters();
    expect(chapters).toContain("the_answer");
    expect(chapters).not.toContain("the_log");
    expect(chapters.filter((c) => c.startsWith("ending_"))).toEqual(["ending_answer"]);

    // The book's video is inside the encrypted package and still plays.
    const hasVideo = await page.evaluate(() => {
      const r = (window as unknown as { darm: { app: { reader: { layout: { pages: { index: number; regions: { type: string }[] }[] }; goToPage(p: number, t: string): void } } } }).darm.app.reader;
      const p = r.layout.pages.find((pg) => pg.regions.some((x) => x.type === "video"));
      if (p) r.goToPage(p.index, "none");
      return p !== undefined;
    });
    expect(hasVideo).toBe(true);
    await settled(page);
    const video = page.locator(".region-video video").first();
    await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.readyState), { timeout: 20_000 }).toBeGreaterThanOrEqual(1);
    expect(await video.evaluate((v: HTMLVideoElement) => v.duration)).toBeGreaterThan(8);
  });
});
