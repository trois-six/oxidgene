import { apiUrl, expect, test } from "./fixtures";

test("creates, renames and deletes a tree from the home page", async ({ page }) => {
    const name = `E2E home ${Date.now()}`;
    const renamed = `${name} renamed`;
    await page.goto("/");

    await page.locator(".home-toolbar").getByRole("button", { name: "New Tree" }).click();
    const createDialog = page.locator(".home-create-modal");
    await createDialog.getByPlaceholder("e.g. Martin Family Tree").fill(name);
    await createDialog.getByRole("button", { name: "Create" }).click();
    await expect(createDialog).toBeHidden();

    const card = page.locator(".tree-card").filter({ has: page.locator(".tree-card-name", { hasText: name }) });
    await expect(card).toHaveCount(1);

    await card.getByRole("button", { name: `Actions for ${name}` }).click();
    await page.getByRole("menuitem", { name: "Rename", exact: true }).click();
    const renameDialog = page.locator(".home-create-modal");
    await expect(renameDialog.getByRole("heading", { name: "Rename Tree" })).toBeVisible();
    await renameDialog.getByPlaceholder("e.g. Martin Family Tree").fill(renamed);
    await renameDialog.getByRole("button", { name: "Save" }).click();
    await expect(renameDialog).toBeHidden();

    const renamedCard = page
        .locator(".tree-card")
        .filter({ has: page.locator(".tree-card-name", { hasText: renamed }) });
    await expect(renamedCard).toHaveCount(1);

    await renamedCard.getByRole("button", { name: `Actions for ${renamed}` }).click();
    await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
    await expect(page.getByText(`Delete "${renamed}"? This action cannot be undone.`)).toBeVisible();
    await page.locator(".modal-actions, .confirm-dialog").getByRole("button", { name: "Delete" }).click();
    await expect(renamedCard).toHaveCount(0);

    await page.reload();
    await expect(page.locator(".tree-card-name", { hasText: name })).toHaveCount(0);
});

test("sorts the trees by name either way and lays them out as a list", async ({ page, request }) => {
    const token = `sortcheck${Date.now()}`;
    for (const name of [`${token} Able`, `${token} Zulu`]) {
        const created = await request.post(`${apiUrl}/api/v1/trees`, { data: { name } });
        expect(created.ok()).toBeTruthy();
    }
    await page.goto("/");
    await page.locator(".home-search-input").fill(token);
    const names = page.locator(".tree-card-name");
    await expect(names).toHaveCount(2);

    await page.locator(".home-sort-select").selectOption({ label: "Name Z → A" });
    await expect(names).toHaveText([new RegExp(`${token} Zulu`, "i"), new RegExp(`${token} Able`, "i")]);
    await page.locator(".home-sort-select").selectOption({ label: "Name A → Z" });
    await expect(names).toHaveText([new RegExp(`${token} Able`, "i"), new RegExp(`${token} Zulu`, "i")]);

    const grid = page.locator(".trees-grid");
    await expect(grid).not.toHaveClass(/trees-list/);
    await page.getByRole("button", { name: "List of trees" }).click();
    await expect(grid).toHaveClass(/trees-list/);
    await expect(page.getByRole("button", { name: "List of trees" })).toHaveAttribute("aria-pressed", "true");
});

test("a tree card shows its ⋮ without hover, and the menu opens from it", async ({ page, tree }) => {
    await page.goto("/");
    const card = page.locator(".tree-card").filter({ has: page.locator(".tree-card-name", { hasText: tree.name }) });
    await expect(card).toHaveCount(1);

    await page.mouse.move(0, 0);
    const trigger = card.getByRole("button", { name: `Actions for ${tree.name}` });
    await expect(trigger).toBeVisible();
    await trigger.click();
    await expect(trigger).toHaveAttribute("aria-expanded", "true");
    await expect(page.getByRole("menuitem", { name: "Rename", exact: true })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(trigger).toBeFocused();
});

/// A fictitious import job, which only the routes below know of.
const playedJobId = "01900000-0000-7000-8000-00000000c0de";

test("a tree's menu stays open while another tree imports, and once that import ends", async ({ page, request, tree }) => {
    const created = await request.post(`${apiUrl}/api/v1/trees`, { data: { name: `${tree.name} importing` } });
    expect(created.ok()).toBeTruthy();
    const importingId = (await created.json()).id as string;

    // The other tree's import is played by the routes: the list reports it
    // running until its job, which moves on with every read, has completed.
    let reads = 0;
    let ended = false;
    await page.route(/\/api\/v1\/trees\?/, async (route) => {
        const response = await route.fetch();
        const body = await response.json();
        for (const edge of body.edges) {
            if (edge.node.id === importingId && !ended) {
                edge.node.import_in_progress = true;
                edge.node.import_job_id = playedJobId;
            }
        }
        await route.fulfill({ response, json: body });
    });
    await page.route(new RegExp(`/import-jobs/${playedJobId}`), async (route) => {
        reads += 1;
        await new Promise((resolve) => setTimeout(resolve, 150));
        ended = reads >= 15;
        await route.fulfill({
            json: { version: `v${reads}`, phase: ended ? "completed" : "database", done: reads, total: 15 },
        });
    });

    await page.goto("/");
    await page.locator(".home-search-input").fill(tree.name);
    const importingCard = page
        .locator(".tree-card")
        .filter({ has: page.getByText(`${tree.name} importing`, { exact: true }) });
    await expect(importingCard.locator(".tree-card-import-overlay")).toBeVisible();

    const card = page.locator(".tree-card").filter({ has: page.getByText(tree.name, { exact: true }) });
    await card.getByRole("button", { name: `Actions for ${tree.name}` }).click();
    const rename = page.getByRole("menuitem", { name: "Rename", exact: true });
    await expect(rename).toBeVisible();

    // Through the import's progress, its end, and the list read again then.
    const deadline = Date.now() + 10_000;
    let over = false;
    while (Date.now() < deadline && !over) {
        expect(await rename.isVisible(), `the menu is still open after ${reads} job reads`).toBe(true);
        over = ended && (await importingCard.locator(".tree-card-import-overlay").count()) === 0;
        await page.waitForTimeout(100);
    }
    expect(over, "the played import ended and its card came back").toBe(true);
    await page.waitForTimeout(1_000);
    await expect(rename).toBeVisible();
    await rename.click();
    await expect(page.locator(".home-create-modal").getByRole("heading", { name: "Rename Tree" })).toBeVisible();
});

test("a menu opened as the page appears stays open on its trigger, and closes when the window is resized", async ({ page, tree }) => {
    await page.goto(`/`);
    const card = page.locator(".tree-card").filter({ has: page.getByText(tree.name, { exact: true }) });
    const trigger = card.getByRole("button", { name: `Actions for ${tree.name}` });
    // At once, while the grid's entrance still runs: a transform there made
    // the grid the menu's containing block, drew the menu off its trigger,
    // and closed it when the entrance ended.
    await trigger.click({ force: true });
    const rename = page.getByRole("menuitem", { name: "Rename", exact: true });
    await expect(rename).toBeVisible();
    await page.waitForTimeout(1_500);
    await expect(rename).toBeVisible();
    await expect(trigger).toHaveAttribute("aria-expanded", "true");

    const at = await trigger.boundingBox();
    const menu = await page.locator(".context-menu").boundingBox();
    expect(at && menu, "the trigger and its menu are drawn").toBeTruthy();
    if (at && menu) {
        expect(Math.abs(menu.y - (at.y + at.height)), "the menu opens under its trigger").toBeLessThan(24);
    }
    // A click on the menu needs it to hold still, as it now does.
    await rename.click();
    await expect(page.locator(".home-create-modal").getByRole("heading", { name: "Rename Tree" })).toBeVisible();
    await page.keyboard.press("Escape");

    await trigger.click();
    await expect(rename).toBeVisible();
    await page.setViewportSize({ width: 1000, height: 700 });
    await expect(rename).toBeHidden();
});
