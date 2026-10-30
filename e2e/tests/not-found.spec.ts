import { expect, test } from "./fixtures";

test("an unknown address shows the not-found page, which leads home", async ({ page, tree }) => {
    await page.goto("/no-such/page");
    await expect(page.getByRole("heading", { level: 1, name: "404 — Not Found" })).toBeVisible();
    // The page names the address it could not find.
    await expect(page.getByText('The page "/no-such/page" does not exist.')).toBeVisible();

    await page.getByRole("button", { name: "Go Home", exact: true }).click();
    await expect(page).toHaveURL(/\/$/);
    await expect(page.locator(".tree-card-name", { hasText: tree.name })).toBeVisible();
});
