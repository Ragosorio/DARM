import { type ChildProcess, spawn } from "node:child_process";
import { cpSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { DARM, ROOT, TMP } from "./global-setup";
import { book, expect, settled, test } from "./helpers";

const READER = join(ROOT, "apps", "reader", "dist");

/** Start `darm open|dev … --no-browser` and return its URL. */
async function start(args: string[]): Promise<{ url: string; child: ChildProcess }> {
  const child = spawn(DARM, [...args, "--no-browser", "--reader", READER], { stdio: ["ignore", "pipe", "pipe"] });
  // Drain stderr: build output must never fill the pipe and stall the server.
  let log = "";
  child.stderr!.on("data", (d: Buffer) => {
    log = (log + d.toString()).slice(-4000);
  });
  (child as ChildProcess & { log: () => string }).log = () => log;
  const lines = createInterface({ input: child.stdout! });
  const url = await new Promise<string>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("darm did not print its address")), 60_000);
    lines.on("line", (line) => {
      const m = /reader: (http:\/\/127\.0\.0\.1:\d+\/\S+)/.exec(line);
      if (m) {
        clearTimeout(timer);
        resolve(m[1]!);
      }
    });
    child.on("exit", (code) => reject(new Error(`darm exited with ${code}`)));
  });
  return { url, child };
}

test.describe("the CLI's local reader", () => {
  test.skip(({ browserName, isMobile }) => browserName !== "chromium" || isMobile, "one engine is enough for the server");

  test("`darm open book.darm` serves the reader and the book", async ({ page }) => {
    const { url, child } = await start(["open", book("kitchen-sink.darm")]);
    try {
      await page.goto(url);
      await expect(page.locator(".reader")).toHaveAttribute("data-at", "front");
      await page.getByRole("button", { name: "Open the book" }).click();
      await expect(page.locator(".reader")).toHaveAttribute("data-at", "page");
      const d = await settled(page);
      expect(d.page).toBe(0);
      // The production build carries a Content-Security-Policy that forbids foreign script and connections.
      const csp = await page.locator('meta[http-equiv="Content-Security-Policy"]').getAttribute("content");
      expect(csp).toContain("script-src 'self' 'wasm-unsafe-eval'");
      expect(csp).toContain("object-src 'none'");
    } finally {
      child.kill();
    }
  });

  test("`darm dev` rebuilds on save, reloads the reader in place, and shows build errors", async ({ page }) => {
    test.setTimeout(120_000);
    const project = join(TMP, "dev-project");
    cpSync(join(ROOT, "apps/reader/tests/fixtures/kitchen-sink"), project, { recursive: true });
    const source = join(project, "book.darml");
    const { url, child } = await start(["dev", project]);
    try {
      await page.goto(url);
      await expect(page.locator(".reader")).toHaveAttribute("data-at", "front");
      await page.getByRole("button", { name: "Open the book" }).click();
      await settled(page);
      await page.keyboard.press("ArrowRight");
      await page.keyboard.press("ArrowRight");
      await settled(page);
      await expect(page.locator(".page-overlay article").filter({ hasText: "The ferry came in sideways" })).toHaveCount(1);

      // Edit the source: the open page updates without losing the place.
      const original = readFileSync(source, "utf8");
      writeFileSync(source, original.replace("The ferry came in sideways", "The ferry came in backwards"));
      await expect(page.locator(".page-overlay article").filter({ hasText: "The ferry came in backwards" })).toHaveCount(1, { timeout: 30_000 });

      // Break it: the diagnostics appear over the last good build.
      writeFileSync(source, original.replace('text "The first thing anyone tells you', 'txet "The first thing anyone tells you'));
      const banner = page.locator(".dev-diagnostics");
      await expect(banner).toContainText("error[E0301]: unknown element `txet`", { timeout: 30_000 });
      await expect(banner).toContainText("did you mean `text`?");

      // Fix it: the banner goes away.
      writeFileSync(source, original);
      await expect(banner).toHaveCount(0, { timeout: 30_000 });
    } finally {
      child.kill();
    }
  });
});
