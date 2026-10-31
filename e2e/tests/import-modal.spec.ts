import { readFileSync } from "node:fs";
import { join } from "node:path";

import { apiUrl, expect, test } from "./fixtures";
import type { Page } from "@playwright/test";

const fixture = readFileSync(join(import.meta.dirname, "..", "fixtures", "family-blocks.ged"));

/// Opens the import modal of the tree called `name`, from its card's menu.
async function openImport(page: Page, name: string) {
    await page.goto("/");
    await page.locator(".home-search-input").fill(name);
    const card = page.locator(".tree-card").filter({ has: page.getByText(name, { exact: true }) });
    await card.getByRole("button", { name: `Actions for ${name}` }).click();
    // A plain click: the menu holds still while the page around it moves.
    await page.getByRole("menuitem", { name: "Import", exact: true }).click();
    const modal = page.locator(".import-modal");
    await expect(modal.getByText(`Import into “${name}”`)).toBeVisible();
    await modal.locator("#oxidgene-file-import-input").setInputFiles({
        name: "family-blocks.ged",
        mimeType: "application/octet-stream",
        buffer: fixture,
    });
    await expect(modal.locator(".import-drop-name")).toHaveText("family-blocks.ged");
    return modal;
}

test("a GEDCOM file imported from the modal is followed to its end", async ({ page, request }) => {
    const name = `E2E modal import ${Date.now()}`;
    const created = await request.post(`${apiUrl}/api/v1/trees`, { data: { name } });
    expect(created.ok()).toBeTruthy();
    const treeId = (await created.json()).id as string;
    // The status reads after the first one wait on the server for a change.
    const waits: string[] = [];
    page.on("request", (req) => {
        const url = new URL(req.url());
        if (url.pathname.includes(`/trees/${treeId}/import-jobs/`) && url.searchParams.has("since")) {
            waits.push(url.searchParams.get("wait") ?? "");
        }
    });

    const modal = await openImport(page, name);
    await modal.getByRole("button", { name: "Import", exact: true }).click();

    await expect(modal.getByRole("heading", { name: "Import complete" })).toBeVisible({ timeout: 30_000 });
    const people = modal.locator(".import-stat").filter({ has: page.getByText("people", { exact: true }) });
    await expect(people.locator(".import-stat-value")).toHaveText("30");
    expect(waits.every((wait) => wait === "25"), `long polls ask to wait 25 s: ${waits}`).toBe(true);
});

test("an import is cancelled from the modal, which can then be closed", async ({ page, request }) => {
    const name = `E2E modal cancel ${Date.now()}`;
    const created = await request.post(`${apiUrl}/api/v1/trees`, { data: { name } });
    expect(created.ok()).toBeTruthy();
    const treeId = (await created.json()).id as string;

    // The job is played: it runs until it is asked to cancel, then unwinds
    // for a few reads before it reports itself cancelled.
    const jobId = "01900000-0000-7000-8000-0000000ca9ce";
    let version = 0;
    let cancelAsked = false;
    let readsSinceCancel = 0;
    const status = () => {
        version += 1;
        const phase = !cancelAsked ? "database" : readsSinceCancel++ < 3 ? "cancelling" : "cancelled";
        return { version: `v${version}`, phase, done: 0, total: 0 };
    };
    await page.route(new RegExp(`/trees/${treeId}/import-jobs\\?`), (route) =>
        route.fulfill({ status: 202, json: { job_id: jobId } }),
    );
    await page.route(new RegExp(`/import-jobs/${jobId}/cancel`), (route) => {
        cancelAsked = true;
        return route.fulfill({ json: status() });
    });
    await page.route(new RegExp(`/import-jobs/${jobId}(\\?|$)`), async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 200));
        await route.fulfill({ json: status() });
    });

    const modal = await openImport(page, name);
    await modal.getByRole("button", { name: "Import", exact: true }).click();
    await expect(modal.getByText("Writing genealogy…")).toBeVisible();
    // Running, the modal cannot be dismissed.
    await expect(modal.locator(".person-form-close")).toBeDisabled();

    await modal.getByRole("button", { name: "Cancel import" }).click();
    const confirm = page.getByRole("dialog", { name: "Cancel this import?" });
    await expect(confirm.getByText(/people, families, events, sources and media/)).toBeVisible();
    await confirm.getByRole("button", { name: "Cancel the import" }).click();
    await expect(confirm).toBeHidden();
    await expect(modal.getByRole("button", { name: "Cancel import" })).toHaveCount(0);

    await expect(modal.getByText("Import cancelled. The tree is unchanged.")).toBeVisible();
    await expect(modal.locator(".person-form-close")).toBeEnabled();
    await modal.locator(".person-form-close").click();
    await expect(modal).toBeHidden();
});
