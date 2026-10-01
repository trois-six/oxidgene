import { expect, test } from "./fixtures";

test("traces the relationship between two persons", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/kinship?from=${tree.anchorId}&to=`);
    await expect(page.getByText("Choose the person to trace the relationship to.")).toBeVisible();

    await page.getByPlaceholder("Search for a person…").fill("Dorian");
    await page.getByRole("button", { name: /^Ashdown Dorian/ }).click();

    await expect(page).toHaveURL(/to=[0-9a-f-]{36}/);
    await expect(page.getByText("1 relationship found")).toBeVisible();
    await expect(page.getByText("Grandfather").first()).toBeVisible();

    // Swapping the two persons reverses the relationship.
    await page.getByRole("button", { name: "Swap the two persons" }).click();
    await expect(page.getByText("Grandson").first()).toBeVisible();
});
