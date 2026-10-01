import { expect, rawKeysOn, test } from "./fixtures";

test("every tree page's sidebar leads to the statistics and the tools", async ({ page, tree }) => {
    const base = `/trees/${tree.treeId}`;
    const person = `${base}/persons/${tree.anchorId}`;

    // The couple page's address comes from the person page's sidebar.
    await page.goto(person);
    await page.getByRole("button", { name: "Couple view" }).click();
    await expect(page).toHaveURL(/\/couples\/[0-9a-f-]{36}$/);
    const couple = new URL(page.url()).pathname;

    const treePages = [
        base,
        person,
        couple,
        `${person}/history`,
        `${base}/search?last=Ashdown&first=&origin=`,
        `${base}/kinship?from=${tree.anchorId}&to=`,
        `${base}/dictionary`,
        `${base}/statistics`,
        `${base}/tools`,
        `${base}/settings`,
    ];
    for (const path of treePages) {
        for (const [button, target] of [
            ["Statistics", "/statistics"],
            ["Tools", "/tools"],
        ]) {
            await page.goto(path);
            const sidebar = page.locator("nav.tree-icon-sidebar");
            await expect(sidebar, `sidebar of ${path}`).toBeVisible();
            await sidebar.getByRole("button", { name: button, exact: true }).click();
            await expect(page, `${button} from ${path}`).toHaveURL(new RegExp(`${base}${target}$`));
        }
        await page.goto(path);
        await expect(page.locator("nav.tree-icon-sidebar")).toBeVisible();
        expect(await rawKeysOn(page), `untranslated keys on ${path}`).toEqual([]);
    }
});

test("the profile sidebar leads to every view of the tree", async ({ page, tree }) => {
    const base = `/trees/${tree.treeId}`;
    const person = `${base}/persons/${tree.anchorId}`;
    const sidebar = page.locator("nav.tree-icon-sidebar");
    const targets: Array<[string, RegExp]> = [
        ["Couple view", /\/couples\/[0-9a-f-]{36}$/],
        ["Tree view", new RegExp(`${base}\\?person=${tree.anchorId}$`)],
        ["Dictionary", new RegExp(`${base}/dictionary$`)],
        ["Settings", new RegExp(`${base}/settings$`)],
    ];
    for (const [button, target] of targets) {
        await page.goto(person);
        await sidebar.getByRole("button", { name: button, exact: true }).click();
        await expect(page, button).toHaveURL(target);
    }

    await page.goto(person);
    await sidebar.getByRole("button", { name: "Add person", exact: true }).click();
    await expect(page.locator(".person-form-modal")).toBeVisible();
    await page.locator(".person-form-modal").getByRole("button", { name: "Cancel", exact: true }).click();
    await expect(page.locator(".person-form-modal")).toBeHidden();

    // From a tool page, the tree view returns to the pedigree.
    await page.goto(`${base}/dictionary`);
    await sidebar.getByRole("button", { name: "Tree view", exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`${base}(\\?.*)?$`));
});
