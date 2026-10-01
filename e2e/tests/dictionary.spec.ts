import { apiUrl, expect, test } from "./fixtures";

/// Thirty more persons, each with a family name of their own, so that the
/// family names fill more than one page of 25.
function singletons(count: number): string {
    let gedcom = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n2 FORM LINEAGE-LINKED\n1 CHAR UTF-8\n";
    for (let i = 1; i <= count; i++) {
        const n = String(i).padStart(2, "0");
        gedcom += `0 @P${n}@ INDI\n1 NAME Solo /Pagewell${n}/\n1 SEX U\n`;
    }
    return `${gedcom}0 TRLR\n`;
}

test("switches the dictionary tabs and pages through the family names", async ({ page, request, tree }) => {
    const imported = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}/gedcom/import`, {
        data: { gedcom: singletons(30) },
    });
    expect(imported.ok()).toBeTruthy();

    await page.goto(`/trees/${tree.treeId}/dictionary`);
    const entries = page.locator(".dict-total-count");
    const pages = page.locator("nav.pager");

    // Family names: the fixture's seven and the thirty singletons.
    await expect(entries).toHaveText("37 entries");
    await expect(page.getByText("Ashdown", { exact: true })).toBeVisible();
    await expect(pages.getByRole("button", { name: "Page 2", exact: true })).toBeVisible();
    await pages.getByRole("button", { name: "Page 2", exact: true }).click();
    await expect(page.getByText("Pagewell30", { exact: true })).toBeVisible();
    await expect(page.getByText("Ashdown", { exact: true })).toHaveCount(0);

    // The letter strip filters and returns to the first page.
    await page.getByRole("button", { name: "A", exact: true }).click();
    await expect(entries).toHaveText("1 entry");
    await expect(pages).toHaveCount(0);
    await page.getByRole("button", { name: "All", exact: true }).first().click();

    // Showing every entry removes the pager.
    await page.locator(".dict-page-size select").selectOption("all");
    await expect(pages).toHaveCount(0);
    await expect(page.getByText("Pagewell30", { exact: true })).toBeVisible();
    await expect(page.getByText("Ashdown", { exact: true })).toBeVisible();

    await page.getByRole("tab", { name: "Sources", exact: true }).click();
    await expect(page.getByText("Parish register 0", { exact: true })).toBeVisible();

    await page.getByRole("tab", { name: "Places", exact: true }).click();
    await expect(page.getByText("Northfield", { exact: false }).first()).toBeVisible();

    await page.getByRole("tab", { name: "Occupations", exact: true }).click();
    await expect(page.getByText("Weaver", { exact: true })).toBeVisible();

    await page.getByRole("tab", { name: "Media", exact: true }).click();
    await expect(page.getByText("Parish register 0", { exact: true })).toHaveCount(0);
});
