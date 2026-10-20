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
        await expect(menu.getByRole("menuitem", { name: item, exact: true })).toBeVisible();
    }

    // Clicking outside the menu closes it.
    await page.locator(".context-menu-backdrop").click({ position: { x: 5, y: 5 } });
    await expect(menu).toBeHidden();
});

test("opens the selected card's menu from its trigger, and gives the focus back on Escape", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}`);

    // The focus card is selected at first: its trigger is shown, and only it.
    const trigger = page.locator("g.ped-card-focus").getByRole("button", { name: /^Actions for / });
    await expect(trigger).toHaveCount(1);
    await expect(trigger).toHaveAttribute("aria-haspopup", "menu");
    await expect(trigger).toHaveAttribute("aria-expanded", "false");

    await trigger.click();
    const menu = page.locator(".context-menu");
    await expect(menu).toBeVisible();
    await expect(trigger).toHaveAttribute("aria-expanded", "true");
    await expect(menu.getByRole("menuitem", { name: "Edit individual", exact: true })).toBeVisible();

    await page.keyboard.press("Escape");
    await expect(menu).toBeHidden();
    await expect(trigger).toBeFocused();
    await expect(trigger).toHaveAttribute("aria-expanded", "false");
});

test("closes the person form on Escape and on a press outside it", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}`);
    const card = page.locator("g.ped-card").filter({ hasText: /Bernard\s*ASHDOWN/ }).first();
    const form = page.locator(".person-form-modal");

    await card.click({ button: "right" });
    await page.locator(".context-menu").getByRole("menuitem", { name: "Edit individual", exact: true }).click();
    await expect(form).toBeVisible();
    await expect(form.getByRole("heading", { name: "Bernard Ashdown" })).toBeVisible();
    // Escape reaches the form from any control in it, a name field included:
    // its suggestion list takes Escape only while it lists something.
    await form.locator(".form-group", { hasText: "Given Names *" }).locator("input").focus();
    await page.keyboard.press("Escape");
    await expect(form).toBeHidden();

    await card.click({ button: "right" });
    await page.locator(".context-menu").getByRole("menuitem", { name: "Edit individual", exact: true }).click();
    await expect(form).toBeVisible();
    // The backdrop dismisses on press, so press its corner, away from the form.
    await page.locator(".modal-backdrop").click({ position: { x: 5, y: 5 } });
    await expect(form).toBeHidden();
});

test("pans the chart from anywhere on the canvas, again and again", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}`);
    const card = page.locator("g.ped-card").filter({ hasText: /Bernard\s*ASHDOWN/ }).first();
    await expect(card).toBeVisible();
    const inner = page.locator(".pedigree-viewport .pedigree-inner");
    const offset = async () =>
        inner.evaluate(element => {
            const matrix = new DOMMatrix(getComputedStyle(element).transform);
            return [matrix.e, matrix.f];
        });
    // At a hand's pace: the chart follows the pointer from the frame after
    // the press.
    const drag = async ([x, y]: number[]) => {
        await page.mouse.move(x, y);
        await page.mouse.down();
        for (let step = 1; step <= 6; step++) {
            await page.mouse.move(x + 5 * step, y + (10 * step) / 3);
            await page.waitForTimeout(20);
        }
        await page.mouse.up();
    };
    const pans = async (from: () => Promise<number[]>) => {
        const before = await offset();
        await drag(await from());
        await expect.poll(async () => (await offset())[0] - before[0]).toBeCloseTo(30, 0);
        expect((await offset())[1] - before[1]).toBeCloseTo(20, 0);
    };
    // A point of the chart between the cards, where nothing listens of its own.
    const between = () =>
        page.evaluate(() => {
            const svg = document.querySelector(".pedigree-viewport svg");
            const rect = svg?.getBoundingClientRect();
            if (!svg || !rect) return [];
            for (let y = rect.top + 4; y < rect.bottom; y += 7) {
                for (let x = rect.left + 4; x < rect.right; x += 7) {
                    if (document.elementFromPoint(x, y) === svg) return [x, y];
                }
            }
            return [];
        });
    const corner = async () => {
        const box = await page.locator(".pedigree-viewport").boundingBox();
        return box ? [box.x + 8, box.y + 8] : [];
    };
    const onCard = async () => {
        const box = await card.boundingBox();
        return box ? [box.x + box.width / 2, box.y + box.height / 2] : [];
    };

    // Each drag ends by removing its move listeners: the next one, wherever
    // it starts, must still be heard.
    await pans(between);
    await pans(between);
    await pans(corner);
    await pans(onCard);
    await pans(between);
    // Dragging a card pans without making the person the focus.
    await expect(page).not.toHaveURL(/person=/);
});

test("goes back to each person made the focus, one at a time", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}`);
    const cards = page.locator("g.ped-card");
    const focus = page.locator(".ev-panel .evp-person");

    await cards.filter({ hasText: /Bernard\s*ASHDOWN/ }).first().click();
    await expect(focus).toContainText(/Bernard\s*Ashdown/i);
    await cards.filter({ hasText: /Dorian\s*ASHDOWN/ }).first().click();
    await expect(focus).toContainText(/Dorian\s*Ashdown/i);

    await page.goBack();
    await expect(focus).toContainText(/Bernard\s*Ashdown/i);
    await page.goBack();
    await expect(focus).toContainText(/Anchor\s*Ashdown/i);
    await page.goForward();
    await expect(focus).toContainText(/Bernard\s*Ashdown/i);
});

test("opens the selected card's menu from the keyboard, with the context-menu key on the canvas", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}`);

    await page.locator(".pedigree-viewport").focus();
    await page.keyboard.press("Shift+F10");
    const menu = page.locator(".context-menu");
    await expect(menu).toBeVisible();
    await expect(menu.getByRole("menuitem", { name: "Edit individual", exact: true })).toBeVisible();

    await page.keyboard.press("Escape");
    await expect(menu).toBeHidden();
});

test("clicking the root's disc of the wheel selects it without a new page of history", async ({ page, tree }) => {
    await page.goto("/settings");
    await page.getByRole("button", { name: "Pedigree", exact: true }).click();
    await page.getByRole("button", { name: /^Ancestor wheel/ }).click();
    await page.goto(`/trees/${tree.treeId}`);

    const root = page.locator("g.fan-root");
    await expect(root).toHaveCount(1);
    const box = await root.boundingBox();
    expect(box).not.toBeNull();
    const before = await page.evaluate(() => history.length);

    // Inside the disc, clear of the ⋮ trigger at its centre.
    await page.mouse.click(box!.x + box!.width * 0.75, box!.y + box!.height / 2);
    await expect(page.locator(".context-menu")).toBeHidden();
    expect(await page.evaluate(() => history.length)).toBe(before);
});
