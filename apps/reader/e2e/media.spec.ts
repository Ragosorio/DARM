import { type Page } from "@playwright/test";
import { book, expect, openBook, openCover, settled, test } from "./helpers";

async function showRegion(page: Page, type: string): Promise<void> {
  await page.evaluate((t) => {
    const r = (window as unknown as { darm: { app: { reader: { layout: { pages: { index: number; regions: { type: string }[] }[] }; goToPage(p: number, tr: string): void } } } }).darm.app.reader;
    const p = r.layout.pages.find((pg) => pg.regions.some((x) => x.type === t));
    if (!p) throw new Error(`no ${t} region in the book`);
    r.goToPage(p.index, "none");
  }, type);
  await settled(page);
}

test.describe("media packaged in the book", () => {
  test.skip(({ isMobile }) => isMobile, "covered on desktop engines");

  test("video plays from the package with captions; turning the page stops it", async ({ page }) => {
    await openCover(page, book("kitchen-sink.darm"));
    await openBook(page);
    await showRegion(page, "video");
    const video = page.locator(".region-video video");
    await expect(video).toBeVisible();
    await expect(video).toHaveAttribute("aria-label", "A slow diagonal gradient from navy to gold");
    expect(await video.getAttribute("src"), "media is served from the package, not the network").toMatch(/^blob:/);
    await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.readyState), { timeout: 15_000 }).toBeGreaterThanOrEqual(1);
    const duration = await video.evaluate((v: HTMLVideoElement) => v.duration);
    expect(duration).toBeGreaterThan(3.5);
    expect(duration).toBeLessThan(4.6);
    expect(await video.evaluate((v: HTMLVideoElement) => [v.videoWidth, v.videoHeight])).toEqual([480, 270]);
    expect(await video.getAttribute("poster")).toMatch(/^blob:/);
    expect(await video.evaluate((v: HTMLVideoElement) => v.hasAttribute("controls"))).toBe(true);
    // Caption tracks: English and Spanish, from WebVTT files inside the package.
    await expect.poll(() => video.evaluate((v: HTMLVideoElement) => [...v.textTracks].map((t) => `${t.language}:${t.label}`).sort())).toEqual(["en:English", "es:Español"]);

    await video.evaluate(async (v: HTMLVideoElement) => {
      v.muted = true;
      await v.play();
    });
    await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime), { timeout: 15_000 }).toBeGreaterThan(0.4);
    // The first declared track (English) is the default one and its cues are loaded.
    await expect.poll(() => video.evaluate((v: HTMLVideoElement) => [v.textTracks[0]!.language, v.textTracks[0]!.cues?.length ?? 0])).toEqual(["en", 2]);
    // Seeking works (the blob is a random-access slice of the book file).
    await video.evaluate((v: HTMLVideoElement) => (v.currentTime = 3));
    await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.currentTime)).toBeGreaterThanOrEqual(2.9);

    await page.keyboard.press("ArrowRight");
    await settled(page);
    await expect(page.locator(".region-video video"), "media does not keep playing on a page that was turned").toHaveCount(0);
  });

  test("audio player, image viewer and gallery", async ({ page }) => {
    await openCover(page, book("kitchen-sink.darm"));
    await openBook(page);
    await showRegion(page, "audio");
    const audio = page.locator(".region-audio audio");
    await page.getByRole("button", { name: "Play A single tone" }).click();
    await expect.poll(() => audio.evaluate((a: HTMLAudioElement) => a.currentTime), { timeout: 15_000 }).toBeGreaterThan(0.25);
    expect(await audio.evaluate((a: HTMLAudioElement) => a.src)).toMatch(/^blob:/);
    await page.getByRole("button", { name: "Pause A single tone" }).click();
    await expect.poll(() => audio.evaluate((a: HTMLAudioElement) => a.paused)).toBe(true);
    await expect(page.locator(".audio-transcript")).toContainText("A steady low tone");

    await showRegion(page, "image");
    await page.getByRole("button", { name: "Enlarge image" }).first().click();
    const box = page.getByRole("dialog", { name: "Image" });
    await expect(box).toBeVisible();
    await expect(box.locator("img")).toHaveAttribute("alt", "A low sun over a dark sea");
    await expect.poll(() => box.locator("img").evaluate((i: HTMLImageElement) => i.naturalWidth)).toBe(360);
    await page.keyboard.press("Escape");
    await expect(box).toHaveCount(0);

    await showRegion(page, "gallery");
    await page.getByRole("button", { name: "Open gallery of 2 images" }).click();
    const gallery = page.getByRole("dialog", { name: "Gallery" });
    await expect(gallery.locator(".lightbox-count")).toHaveText("1 / 2");
    await gallery.getByRole("button", { name: "Next image" }).click();
    await expect(gallery.locator(".lightbox-count")).toHaveText("2 / 2");
    await expect(gallery.locator("img")).toHaveAttribute("alt", /chart with a red hill/);
    await page.keyboard.press("ArrowRight");
    await expect(gallery.locator(".lightbox-count")).toHaveText("1 / 2");
    await gallery.getByRole("button", { name: "Close" }).click();
    await expect(gallery).toHaveCount(0);
  });

  test("external links ask before leaving and show the real address", async ({ page, context }) => {
    await openCover(page, book("kitchen-sink.darm"));
    await openBook(page);
    await showRegion(page, "link");
    await page.getByRole("button", { name: "an external one (external link)" }).click();
    const dialog = page.getByRole("alertdialog", { name: "Open external link" });
    await expect(dialog).toContainText("example.org/tides");
    await dialog.getByRole("button", { name: "Stay" }).click();
    await expect(dialog).toHaveCount(0);
    expect(context.pages().length, "nothing was opened").toBe(1);
    // An internal link jumps inside the book.
    await page.getByRole("button", { name: "link to the fork" }).click();
    await settled(page);
    await expect(page.getByRole("button", { name: "Climb to the lighthouse" })).toBeVisible();
  });
});
