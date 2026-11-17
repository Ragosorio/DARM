import { book, diag, expect, openBook, openCover, settled, showBars, showBlock, test } from "./helpers";

test.describe("state, choices, quizzes and branching", () => {
  test("choices change the route, conditions re-flow the page, reset starts over, state persists", async ({ page }) => {
    await openCover(page, book("kitchen-sink.darm"));
    let d = await openBook(page);
    const linearPages = d.pages;
    expect(d.state).toEqual({ courage: 1, lit: false, route: "shore" });

    d = await showBlock(page, "fork");
    const button = (name: string | RegExp) => page.getByRole("button", { name });
    await expect(button("Climb to the lighthouse")).toBeVisible();
    await expect(button("Wade into the sea caves"), "an option whose condition is false is not offered").toHaveCount(0);
    await expect(page.locator(".page-overlay article").filter({ hasText: "You stay on the shingle with the gulls." })).toHaveCount(1);

    // A button changes state; the page re-flows: a new option and a different passage appear.
    await button("Light the lantern").click();
    d = await settled(page);
    expect(d.state.lit).toBe(true);
    await expect(button("Wade into the sea caves")).toBeVisible();
    await expect(page.locator(".page-overlay article").filter({ hasText: "The lantern hisses. Courage: 1." }), "elif branch with {courage} interpolated").toHaveCount(1);

    // Quiz: the first answer stands; a wrong one does not run on_correct.
    await showBlock(page, "star_quiz");
    await button("Sirius").click();
    d = await settled(page);
    expect(d.state.courage).toBe(1);
    await expect(page.locator(".page-overlay article").filter({ hasText: "Polaris sits within a degree" }), "the explanation is shown after answering").toHaveCount(1);
    await expect(button("Polaris")).toHaveAttribute("aria-disabled", "true");
    await button("Polaris").click({ force: true });
    expect((await settled(page)).state.courage).toBe(1);

    // Reset restores every variable and clears recorded answers, and jumps to the start.
    await showBlock(page, "restart");
    await button("Start again").click();
    d = await settled(page);
    expect(d.state).toEqual({ courage: 1, lit: false, route: "shore" });
    await expect(page.locator(".page-overlay article").filter({ hasText: "The ferry came in sideways" }), "`goto start` lands on the first page after the front matter").toHaveCount(1);
    await showBlock(page, "star_quiz");
    await button("Polaris").click();
    d = await settled(page);
    expect(d.state.courage, "on_correct ran").toBe(2);

    // Taking the cliff route adds a chapter (with a fixed-layout plate) to the book and jumps into it.
    await showBlock(page, "fork");
    await button("Climb to the lighthouse").click();
    await expect.poll(async () => (await diag(page)).state.route).toBe("cliff");
    d = await settled(page);
    expect(d.state).toEqual({ courage: 3, lit: false, route: "cliff" });
    expect(d.pages).toBeGreaterThan(linearPages);
    await expect(page.locator(".page-overlay article").filter({ hasText: "The path up is cut into the rock" })).toHaveCount(1);
    await showBlock(page, "plate");
    await expect(button("Back to the quay"), "a button placed by hand on a fixed page").toBeVisible();
    await button("Back to the quay").click();
    d = await settled(page);
    await expect(page.locator(".page-overlay article").filter({ hasText: "The quay" }).first()).toBeVisible();
    await showBars(page);
    await page.getByRole("button", { name: "Contents" }).click();
    await expect(page.getByRole("navigation", { name: "Table of contents" }).getByRole("button", { name: /The cliff/ })).toBeVisible();
    await page.keyboard.press("Escape");

    // Changing the choice changes the route back: the chapter leaves the book again.
    await showBlock(page, "fork");
    await expect(button(/Climb to the lighthouse \(selected\)/)).toBeVisible();
    await button("Stay on the shingle").click();
    d = await settled(page);
    expect(d.state.route).toBe("shore");
    expect(d.pages).toBe(linearPages);

    // State survives closing and reopening the book.
    await button("Light the lantern").click();
    await settled(page);
    await page.waitForTimeout(700);
    await page.reload();
    await page.locator(".card-open").first().click();
    await expect(page.locator(".reader")).toHaveAttribute("data-at", "page", { timeout: 15_000 });
    d = await settled(page);
    expect(d.state).toEqual({ courage: 3, lit: true, route: "shore" });
    await expect(button(/Stay on the shingle \(selected\)/)).toBeVisible();
  });

  test("interactive content never traps the reader", async ({ page }) => {
    await openCover(page, book("kitchen-sink.darm"));
    await openBook(page);
    let d = await showBlock(page, "fork");
    const at = d.pos;
    // Without answering anything, the page still turns both ways and the contents still open.
    await page.keyboard.press("ArrowRight");
    d = await settled(page);
    expect(d.pos).toBe(at + 1);
    await page.keyboard.press("ArrowLeft");
    await page.keyboard.press("ArrowLeft");
    d = await settled(page);
    expect(d.pos).toBe(at - 1);
    await showBars(page);
    await page.getByRole("button", { name: "Contents" }).click();
    await expect(page.getByRole("dialog", { name: "Contents" })).toBeVisible();
  });
});
