import { expect, test } from "./fixtures";

test("draws the SOSA root's ancestors and opens a card's context menu", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}`);

    const cards = page.locator("g.ped-card");
    // The root, its parents and its four grandparents, all from block 0.
    for (const name of [/Anchor\s*ASHDOWN/, /Bernard\s*ASHDOWN/, /Clara\s*BIRCHLEY/, /Dorian\s*ASHDOWN/, /Clara\s*DUNMORE/]) {
        await expect(cards.filter({ hasText: name }).first()).toBeVisible();
    }

    await cards.filter({ hasText: /Bernard\s*ASHDOWN/ }).first().click({ button: "right" });
    const menu = page.locator(".context-menu");
    await expect(menu).toBeVisible();
    for (const item of ["Edit individual", "Add spouse", "Add child", "Add sibling", "Relationship with…"]) {
        await expect(menu.getByRole("button", { name: item })).toBeVisible();
    }

    // Clicking outside the menu closes it.
    await page.locator(".context-menu-backdrop").click({ position: { x: 5, y: 5 } });
    await expect(menu).toBeHidden();
});

test("closes the person form on Escape and on a press outside it", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}`);
    const card = page.locator("g.ped-card").filter({ hasText: /Bernard\s*ASHDOWN/ }).first();
    const form = page.locator(".person-form-modal");

    await card.click({ button: "right" });
    await page.locator(".context-menu").getByRole("button", { name: "Edit individual" }).click();
    await expect(form).toBeVisible();
    await expect(form.getByRole("heading", { name: "Bernard Ashdown" })).toBeVisible();
    // Escape reaches the form from any control in it, a name field included:
    // its suggestion list takes Escape only while it lists something.
    await form.locator(".form-group", { hasText: "Given Names *" }).locator("input").focus();
    await page.keyboard.press("Escape");
    await expect(form).toBeHidden();

    await card.click({ button: "right" });
    await page.locator(".context-menu").getByRole("button", { name: "Edit individual" }).click();
    await expect(form).toBeVisible();
    // The backdrop dismisses on press, so press its corner, away from the form.
    await page.locator(".modal-backdrop").click({ position: { x: 5, y: 5 } });
    await expect(form).toBeHidden();
});
