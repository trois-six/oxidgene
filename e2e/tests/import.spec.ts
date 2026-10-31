import { readFileSync } from "node:fs";
import { join } from "node:path";

import { apiUrl, expect, test } from "./fixtures";

const fixture = readFileSync(join(import.meta.dirname, "..", "fixtures", "family-blocks.ged"));

test("a GEDCOM file is imported into a tree from its card's menu", async ({ page, request }) => {
    const name = `E2E import ${Date.now()}`;
    const created = await request.post(`${apiUrl}/api/v1/trees`, { data: { name } });
    expect(created.ok()).toBeTruthy();
    const treeId = (await created.json()).id as string;

    await page.goto("/");
    // Only this tree's card: the other tests' imports refresh the list.
    await page.locator(".home-search-input").fill(name);
    const card = page.locator(".tree-card").filter({ has: page.locator(".tree-card-name", { hasText: name }) });
    await card.getByRole("button", { name: `Actions for ${name}` }).click();
    await page.getByRole("menuitem", { name: "Import", exact: true }).click();

    const modal = page.locator(".import-modal");
    await expect(modal.getByText(`Import into “${name}”`)).toBeVisible();
    await expect(modal.getByRole("tab", { name: "A file" })).toBeVisible();
    await expect(modal.getByRole("tab", { name: "From Geneanet" })).toBeVisible();
    const start = modal.getByRole("button", { name: "Import", exact: true });
    await expect(start).toBeDisabled();

    await modal.locator("#oxidgene-file-import-input").setInputFiles({
        name: "family-blocks.ged",
        mimeType: "application/octet-stream",
        buffer: fixture,
    });
    await expect(modal.locator(".import-drop-name")).toHaveText("family-blocks.ged");
    await start.click();

    await expect(modal.getByRole("heading", { name: "Import complete" })).toBeVisible({ timeout: 30_000 });
    const people = modal.locator(".import-stat").filter({ has: page.getByText("people", { exact: true }) });
    await expect(people.locator(".import-stat-value")).toHaveText("30");

    const persons = await (await request.get(`${apiUrl}/api/v1/trees/${treeId}/persons?first=100`)).json();
    expect(persons.edges).toHaveLength(30);
});
