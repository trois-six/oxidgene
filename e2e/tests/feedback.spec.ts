// The feedback states of docs/cross-cutting.md §6: the error of a failed
// load and its retry, the read the network failed asked again by itself,
// the toast that says the outcome of a write — in the page and in an open
// dialog — and the network coming back.

import type { Page } from "@playwright/test";

import { expect, expectedConsoleError, test } from "./fixtures";

/// Refuses the home page's read of the trees while `refuses()` says so, as
/// a network that drops the request, and counts how many times it was asked.
async function refuseTreeList(page: Page, refuses: () => boolean): Promise<{ asked: number }> {
    const seen = { asked: 0 };
    await page.route(
        (url) => url.pathname === "/api/v1/trees",
        async (route) => {
            if (route.request().method() !== "GET") {
                await route.fallback();
                return;
            }
            seen.asked += 1;
            if (refuses()) {
                await route.abort("failed");
            } else {
                await route.fallback();
            }
        },
    );
    return seen;
}

/// The console line of a request the test dropped.
const droppedRequest = { type: expectedConsoleError, description: "Failed to load resource" };

test("a failed load says why, and its retry loads the page", async ({ page, tree }) => {
    test.info().annotations.push(droppedRequest);
    let down = true;
    const seen = await refuseTreeList(page, () => down);

    await page.goto("/");
    // The skeleton stands for the list while the read is asked again.
    await expect(page.locator(".home-main .skeleton")).toBeAttached();
    const failure = page.locator(".home-main").getByRole("alert");
    await expect(failure).toContainText("This could not be loaded.");
    await expect(failure).toContainText("The server could not be reached. Check the connection and try again.");
    // Asked three times before giving up: once, then twice more.
    expect(seen.asked).toBe(3);
    await expect(page.locator(".tree-card-name")).toHaveCount(0);

    down = false;
    await failure.getByRole("button", { name: "Retry", exact: true }).click();
    await expect(page.locator(".tree-card-name", { hasText: tree.name })).toBeVisible();
    await expect(failure).toHaveCount(0);
});

test("a read the network dropped once is asked again without a word", async ({ page, tree }) => {
    test.info().annotations.push(droppedRequest);
    let dropped = 0;
    const seen = await refuseTreeList(page, () => {
        dropped += 1;
        return dropped === 1;
    });

    await page.goto("/");
    await expect(page.locator(".tree-card-name", { hasText: tree.name })).toBeVisible();
    // The dropped request and the one that answered; the list is asked once
    // more whenever another test's tree ends its import meanwhile.
    expect(seen.asked).toBeGreaterThanOrEqual(2);
    await expect(page.locator(".home-main").getByRole("alert")).toHaveCount(0);
});

test("a page's failed load offers the way back to the trees", async ({ page, tree }) => {
    test.info().annotations.push(droppedRequest);
    // The person's page, whose own read is dropped for good.
    await page.route(
        (url) => url.pathname.startsWith(`/api/v1/trees/${tree.treeId}/persons/${tree.anchorId}`),
        (route) => route.abort("failed"),
    );
    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    const failure = page.locator(".sub-page-content").getByRole("alert");
    await expect(failure).toContainText("This could not be loaded.");
    await expect(failure.getByRole("button", { name: "Retry", exact: true })).toBeVisible();
    // The shared navigation stays, and the error leads out too.
    await expect(page.locator("nav.td-bc")).toBeVisible();
    await failure.getByRole("link", { name: "Go Home", exact: true }).click();
    await expect(page.locator(".tree-card-name", { hasText: tree.name })).toBeVisible();
});

test("the outcome of a write is said in a toast, which can be dismissed", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/settings`);
    const name = page.locator(".settings-tree-name-form input");
    await expect(name).toHaveValue(tree.name);
    await name.fill(`${tree.name} renamed`);
    await page.locator(".settings-tree-name-save").click();

    const toast = page.locator(".toast");
    await expect(toast).toHaveCount(1);
    await expect(toast).toHaveText(/Tree name saved\./);
    // Announced politely: the toast sits in a live region that was there
    // before it.
    await expect(page.getByRole("status").filter({ hasText: "Tree name saved." })).toBeVisible();

    await toast.getByRole("button", { name: "Dismiss", exact: true }).click();
    await expect(toast).toHaveCount(0);
});

test("a toast raised while a dialog is open is drawn in that dialog", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}`);
    await page.locator("svg text.no-print").filter({ hasText: /^\+$/ }).first().click();
    const form = page.locator(".person-form-modal");
    // A second Anchor Ashdown: the save succeeds, and the form stays open
    // behind the question about the homonym.
    await form.locator(".form-group", { hasText: "Birth name *" }).locator("input").fill("Ashdown");
    await form.locator(".form-group", { hasText: "Given Names *" }).locator("input").fill("Anchor");
    await form.getByRole("button", { name: "Create", exact: true }).click();
    await expect(page.getByRole("dialog", { name: "Same name found" })).toBeVisible();

    // The toast is drawn once, in the dialog opened last: the page and the
    // form under that dialog are inert.
    const shown = page.locator(".toast");
    await expect(shown).toHaveCount(1);
    await expect(shown).toHaveText(/Person saved\./);
    expect(await shown.evaluate((toast) => toast.closest("dialog")?.getAttribute("aria-label"))).toBe("Same name found");
    // It answers the pointer there: the click reaches its button.
    await shown.getByRole("button", { name: "Dismiss", exact: true }).click();
    await expect(page.locator(".toast")).toHaveCount(0);
    await expect(page.getByRole("dialog", { name: "Same name found" })).toBeVisible();
});

test("the network going and coming back is said, and a failed load is asked again", async ({ page, context, tree }) => {
    test.info().annotations.push(droppedRequest);
    let down = true;
    await refuseTreeList(page, () => down);
    await page.goto("/");
    const failure = page.locator(".home-main").getByRole("alert");
    await expect(failure).toBeVisible();

    await context.setOffline(true);
    await expect(page.locator(".toast-error")).toHaveText(/You are offline\./);

    down = false;
    await context.setOffline(false);
    await expect(page.locator(".toast-success")).toHaveText(/Connection restored\./);
    // No press on Retry: the reconnection asked again.
    await expect(page.locator(".tree-card-name", { hasText: tree.name })).toBeVisible();
    await expect(failure).toHaveCount(0);
});
