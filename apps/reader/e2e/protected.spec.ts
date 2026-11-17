import { book, expect, importBook, openBook, settled, test } from "./helpers";
import { LIGHTHOUSE_PASSWORD, PASSWORD } from "./global-setup";

test.describe("protected books and hostile files", () => {
  test.skip(({ isMobile }) => isMobile, "covered on desktop engines");

  test("password: wrong is refused, right opens; nothing of the book is stored in the clear", async ({ page }) => {
    await importBook(page, book("kitchen-sink.locked.darm"));
    const dialog = page.getByRole("dialog", { name: "Unlock The Kitchen Sink" });
    await expect(dialog).toBeVisible();
    await expect(page.locator(".reader"), "nothing is shown before the book is unlocked").toHaveCount(0);

    await dialog.getByLabel("Password").fill("definitely wrong");
    await dialog.getByRole("button", { name: "Unlock" }).click();
    await expect(page.getByRole("alert")).toHaveText("That password or key does not open this book. Try again.");
    await expect(page.locator(".reader")).toHaveCount(0);

    await page.getByLabel("Password").fill(PASSWORD);
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "front");
    const d = await openBook(page);
    expect(d.page).toBe(0);
    await page.keyboard.press("ArrowRight");
    await settled(page);

    // The library keeps the encrypted file and no cover image of a protected book.
    await page.getByRole("button", { name: "Back to the library" }).click();
    const card = page.locator(".card").first();
    await expect(card.locator(".card-lock")).toBeVisible();
    await expect(card.locator("img.card-cover")).toHaveCount(0);
    const stored = await page.evaluate(async () => {
      const db = await new Promise<IDBDatabase>((res, rej) => {
        const r = indexedDB.open("darm-library");
        r.onsuccess = () => res(r.result);
        r.onerror = () => rej(r.error);
      });
      const all = <T>(store: string) =>
        new Promise<T[]>((res, rej) => {
          const r = db.transaction(store).objectStore(store).getAll();
          r.onsuccess = () => res(r.result as T[]);
          r.onerror = () => rej(r.error);
        });
      // Stored as a Blob, or as plain bytes in engines that cannot keep Blobs in IndexedDB.
      const files = await all<Blob | { bytes: ArrayBuffer }>("files");
      const first = files[0]!;
      const text = new TextDecoder("latin1").decode(first instanceof Blob ? await first.arrayBuffer() : first.bytes);
      const books = await all<{ cover?: Blob }>("books");
      const state = JSON.stringify(await all<unknown>("state"));
      return { leaks: ["The ferry came in", "Polaris", "res/pic", "courage"].filter((s) => text.includes(s)), cover: books[0]!.cover !== undefined, state };
    });
    expect(stored.leaks, "the stored file is still encrypted").toEqual([]);
    expect(stored.cover).toBe(false);
    expect(stored.state).not.toContain(PASSWORD);

    // Reopening asks for the password again: it was never saved.
    await page.locator(".card-open").first().click();
    await expect(page.getByRole("dialog", { name: "Unlock The Kitchen Sink" })).toBeVisible();
    await page.getByRole("button", { name: "Cancel" }).click();
    await expect(page.locator(".shelf")).toBeVisible();
  });

  test("key file: a hidden-title book opens only with its key", async ({ page }) => {
    await importBook(page, book("kitchen-sink.keyed.darm"));
    const dialog = page.getByRole("dialog", { name: "Unlock Protected book" });
    await expect(dialog, "title and author are not readable without the key").toBeVisible();
    await expect(dialog).not.toContainText("Kitchen");
    await expect(dialog.getByLabel("Password"), "this book has no password slot").toHaveCount(0);
    await dialog.locator('input[type="file"]').setInputFiles({ name: "wrong.darmkey", mimeType: "application/json", buffer: Buffer.from('{"not":"a key"}') });
    await expect(page.getByRole("alert")).toHaveText("That file is not a DARM key file.");
    await page.locator('.modal input[type="file"]').setInputFiles(book("kitchen-sink.darmkey"));
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "front");
    await openBook(page);
  });

  test("the showcase book with full-strength key derivation unlocks in the browser", async ({ page }) => {
    await importBook(page, book("the-lighthouse-protocol.darm"));
    await page.getByLabel("Password").fill(LIGHTHOUSE_PASSWORD);
    const t0 = Date.now();
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "front", { timeout: 30_000 });
    test.info().annotations.push({ type: "unlock-ms (Argon2id 64 MiB, 3 passes, in WebAssembly + open)", description: String(Date.now() - t0) });
    await openBook(page);
  });

  test("damaged, altered, truncated and fake files are rejected with a clear reason", async ({ page }) => {
    await page.goto("/");
    const cases: [string, RegExp][] = [
      ["tampered.darm", /damaged or was modified/i],
      ["truncated.darm", /incomplete/i],
      ["garbage.darm", /not a DARM book/i],
      ["bad-header.darm", /damaged or was modified|not a DARM book|incomplete/i],
    ];
    for (const [file, reason] of cases) {
      await page.setInputFiles("#import-input", book(file));
      const alert = page.getByRole("alertdialog");
      await expect(alert, file).toContainText(reason);
      await expect(alert).toContainText(file);
      await alert.getByRole("button", { name: "Close" }).click();
      await expect(page.locator(".card"), `${file} must not be added to the library`).toHaveCount(0);
      await expect(page.locator(".reader")).toHaveCount(0);
    }
    // The app is still usable afterwards.
    await page.setInputFiles("#import-input", book("kitchen-sink.darm"));
    await expect(page.locator(".reader")).toBeVisible();
  });
});
