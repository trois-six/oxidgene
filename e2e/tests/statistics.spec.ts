import type { Page } from "@playwright/test";

import { expect, test } from "./fixtures";

/// The number a statistics tile shows, found by the label under its value.
function tileValue(page: Page, label: string) {
    return page
        .locator(".stats-tile")
        .filter({ has: page.getByText(label, { exact: true }) })
        .locator(".stats-tile-value");
}

/// The chart card titled `title`.
function card(page: Page, title: string) {
    return page.locator(".stats-card").filter({ has: page.locator(".stats-card-title", { hasText: title }) });
}

test("each statistics tab draws the fixture's figures", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/statistics`);

    // Overview: the fixture's counts.
    await expect(page.getByRole("tab", { name: "Overview" })).toHaveAttribute("aria-selected", "true");
    await expect(tileValue(page, "persons")).toHaveText("30");
    await expect(tileValue(page, "places")).toHaveText("5");
    await expect(tileValue(page, "sources")).toHaveText("3");
    await expect(page.getByRole("heading", { name: "Completeness" })).toBeVisible();

    await page.getByRole("tab", { name: "Population" }).click();
    await expect(card(page, "Age pyramid at death")).toBeVisible();

    await page.getByRole("tab", { name: "Families" }).click();
    await expect(card(page, "Unions by number of children")).toBeVisible();

    // Places: none of the fixture's places is located, so the map is empty
    // and the most used one heads the list.
    await page.getByRole("tab", { name: "Places" }).click();
    await expect(page.getByRole("heading", { name: "Map of the places" })).toBeVisible();
    await expect(page.locator(".stats-top-places li").first()).toContainText("Northfield");

    await page.getByRole("tab", { name: "Names and occupations" }).click();
    await expect(card(page, "10 most common family names")).toContainText("Ashdown");
    await expect(card(page, "10 most common occupations")).toContainText("Weaver");

    await page.getByRole("tab", { name: "Growth" }).click();
    await expect(page.getByRole("heading", { name: "Persons over time" })).toBeVisible();

    // Records and lists: a person of a list opens their profile.
    await page.getByRole("tab", { name: "Records and lists" }).click();
    const lists = page.locator(".stats-section").filter({ has: page.getByRole("heading", { name: "Notable records" }) });
    await lists.getByRole("tab", { name: "Longest lives" }).click();
    const first = lists.locator(".stats-table tr").first().getByRole("link").first();
    const name = (await first.innerText()).trim();
    await first.click();
    await expect(page).toHaveURL(/\/persons\/[0-9a-f-]{36}$/);
    await expect(page.getByRole("heading", { level: 1, name })).toBeVisible();
});
