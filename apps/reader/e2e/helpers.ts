import { type Page, expect, test as base } from "@playwright/test";
import { join } from "node:path";
import { TMP } from "./global-setup";

export const book = (name: string) => join(TMP, name);

export interface Diagnostics {
  renderer: "webgl2" | "canvas2d";
  pages: number;
  page: number | null;
  pos: number;
  count: number;
  settled: boolean;
  spread: boolean;
  layoutMs: number;
  frameTimes: number[];
  state: Record<string, number | boolean | string>;
  textureBytes: number;
}

/** Every test fails on an uncaught page error or a console error, and on any request leaving this machine. */
export const test = base.extend<{ strict: void }>({
  strict: [
    async ({ page }, use) => {
      const problems: string[] = [];
      page.on("pageerror", (e) => problems.push(`pageerror: ${e.message}`));
      page.on("console", (m) => {
        // WebKit's own media controls log this under Playwright's phone emulation; it is not the app's.
        if (m.type() === "error" && !m.text().startsWith("Button failed to load, iconName")) problems.push(`console.error: ${m.text()}`);
      });
      page.on("request", (r) => {
        const u = new URL(r.url());
        if (!["127.0.0.1", "localhost"].includes(u.hostname) && !["blob:", "data:"].includes(u.protocol)) {
          problems.push(`network request left the machine: ${r.url()}`);
        }
      });
      await use();
      expect(problems, "page errors / console errors / external requests").toEqual([]);
    },
    { auto: true },
  ],
});

export { expect };

export async function diag(page: Page): Promise<Diagnostics> {
  return page.evaluate(() => (window as unknown as { darm: { diagnostics(): unknown } }).darm.diagnostics()) as Promise<Diagnostics>;
}

/** Wait until the book has stopped moving. */
export async function settled(page: Page, timeout = 15_000): Promise<Diagnostics> {
  await expect.poll(async () => (await diag(page))?.settled === true, { timeout }).toBe(true);
  // One more frame so overlays for the resting pages exist.
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(null)))));
  return diag(page);
}

/** Add a file to the library through the real file input. A single file opens straight away. */
export async function importBook(page: Page, file: string): Promise<void> {
  await page.goto("/");
  await page.setInputFiles("#import-input", file);
}

/** Import and wait for the cover. */
export async function openCover(page: Page, file: string): Promise<Diagnostics> {
  await importBook(page, file);
  await expect(page.locator(".reader")).toBeVisible();
  await expect(page.locator(".reader")).toHaveAttribute("data-at", "front");
  return settled(page);
}

/** From the cover, open the book and wait for the first page. */
export async function openBook(page: Page): Promise<Diagnostics> {
  await page.getByRole("button", { name: /open the book|continue reading/i }).click();
  await expect(page.locator(".reader")).toHaveAttribute("data-at", "page");
  return settled(page);
}

/**
 * Make sure the top and bottom bars are on screen. On phones they hide while
 * reading; Escape brings them back (as does a tap in the middle of the page).
 */
export async function showBars(page: Page): Promise<void> {
  if (await page.locator(".reader.chrome-hidden").count()) await page.keyboard.press("Escape");
  await expect(page.locator(".reader.chrome-hidden")).toHaveCount(0);
}

/** Jump straight to the page that holds a block (no animation), for tests that are not about turning. */
export async function showBlock(page: Page, block: string): Promise<Diagnostics> {
  await page.evaluate((id) => {
    const r = (window as unknown as { darm: { app: { reader: { layout: { blockPages: Map<string, number> }; goToPage(p: number, t: string): void } } } }).darm.app.reader;
    const p = r.layout.blockPages.get(id);
    if (p === undefined) throw new Error(`block ${id} is not in the layout`);
    r.goToPage(p, "none");
  }, block);
  return settled(page);
}

/** Jump straight to the page that carries an interactive control with this label. */
export async function showControl(page: Page, label: string): Promise<Diagnostics> {
  await page.evaluate((label) => {
    const r = (window as unknown as { darm: { app: { reader: { layout: { pages: { index: number; regions: { label?: string }[] }[] }; goToPage(p: number, t: string): void } } } }).darm.app.reader;
    const p = r.layout.pages.find((pg) => pg.regions.some((x) => x.label === label));
    if (!p) throw new Error(`no control labelled "${label}" in the book in its current state`);
    r.goToPage(p.index, "none");
  }, label);
  return settled(page);
}

export function percentile(values: number[], p: number): number {
  if (values.length === 0) return NaN;
  const s = [...values].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))]!;
}

/** Drag across the stage with the mouse, in small steps like a hand would. */
export async function drag(page: Page, from: [number, number], to: [number, number], opts: { steps?: number; release?: boolean } = {}): Promise<void> {
  const steps = opts.steps ?? 16;
  await page.mouse.move(from[0], from[1]);
  await page.mouse.down();
  for (let i = 1; i <= steps; i++) {
    await page.mouse.move(from[0] + ((to[0] - from[0]) * i) / steps, from[1] + ((to[1] - from[1]) * i) / steps);
    await page.waitForTimeout(12);
  }
  if (opts.release !== false) await page.mouse.up();
}

/** Rectangles of the resting pages, in viewport CSS px. */
export async function pageRects(page: Page): Promise<{ left: { x: number; y: number; width: number; height: number } | null; right: { x: number; y: number; width: number; height: number } }> {
  return page.evaluate(() => {
    const r = (window as unknown as { darm: { app: { reader: { stageForTests: { pageRects(): { left: DOMRect | null; right: DOMRect }; canvas: HTMLCanvasElement } } } } }).darm.app.reader;
    const rects = r.stageForTests.pageRects();
    const box = r.stageForTests.canvas.getBoundingClientRect();
    const abs = (q: { x: number; y: number; width: number; height: number } | null) => (q ? { x: q.x + box.left, y: q.y + box.top, width: q.width, height: q.height } : null);
    return { left: abs(rects.left), right: abs(rects.right)! };
  });
}
