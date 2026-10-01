import { expect, test } from "./fixtures";

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

    await card.getByTitle("Tree actions").click();
    await page.getByRole("button", { name: "Rename" }).click();
    const renameDialog = page.locator(".home-create-modal");
    await expect(renameDialog.getByRole("heading", { name: "Rename Tree" })).toBeVisible();
    await renameDialog.getByPlaceholder("e.g. Martin Family Tree").fill(renamed);
    await renameDialog.getByRole("button", { name: "Save" }).click();
    await expect(renameDialog).toBeHidden();

    const renamedCard = page
        .locator(".tree-card")
        .filter({ has: page.locator(".tree-card-name", { hasText: renamed }) });
    await expect(renamedCard).toHaveCount(1);

    await renamedCard.getByTitle("Tree actions").click();
    await page.getByRole("button", { name: "Delete" }).click();
    await expect(page.getByText(`Delete "${renamed}"? This action cannot be undone.`)).toBeVisible();
    await page.locator(".modal-actions, .confirm-dialog").getByRole("button", { name: "Delete" }).click();
    await expect(renamedCard).toHaveCount(0);

    await page.reload();
    await expect(page.locator(".tree-card-name", { hasText: name })).toHaveCount(0);
});
