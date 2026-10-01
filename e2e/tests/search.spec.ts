import { expect, test } from "./fixtures";

test("filters, sorts and switches the view of the search results", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/search?last=Ashdown&first=&origin=`);
    const results = page.getByRole("link", { name: /^Ashdown / });
    await expect(page.getByText("5 results")).toBeVisible();
    await expect(results).toHaveCount(5);

    // Sort: oldest birth first puts the grandfather at the top.
    await page.locator(".sr-sort select").selectOption({ label: "Birth date ↑" });
    await expect(results.first()).toHaveAccessibleName(/^Ashdown Dorian/);
    await page.locator(".sr-sort select").selectOption({ label: "Name A → Z" });
    await expect(results.first()).toHaveAccessibleName(/^Ashdown Anchor/);

    // A filter narrows the results and shows as a removable chip.
    await page.getByRole("button", { name: /Filters/ }).click();
    await page.locator(".sr-filter-sex").getByRole("button", { name: "Female", exact: true }).click();
    await expect(results).toHaveCount(1);
    await expect(results.first()).toHaveAccessibleName(/^Ashdown Delia/);
    const chip = page.locator(".sr-active-filters .sr-filter-chip");
    await expect(chip).toHaveCount(1);
    await chip.click();
    await expect(chip).toHaveCount(0);
    await expect(results).toHaveCount(5);

    // The grid view draws each result with its own small pedigree.
    await page.getByTitle("Pedigree grid view").click();
    await expect(page.locator(".sr-grid .sr-grid-card")).toHaveCount(5);
    await page.getByTitle("List view").click();
    await expect(page.locator(".sr-grid")).toHaveCount(0);
    await expect(results).toHaveCount(5);
});
