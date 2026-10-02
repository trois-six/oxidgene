import { apiUrl, expect, test } from "./fixtures";

test("the tree's date display settings reach the person page and the pedigree", async ({ page, request, tree }) => {
    // A fully dated event, which the fixture's year-only dates lack.
    const event = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}/events`, {
        data: {
            event_type: "residence",
            date_value: "3 FEB 1922",
            date_qualifier: "exact",
            calendar: "gregorian",
            person_id: tree.anchorId,
        },
    });
    expect(event.ok(), await event.text()).toBeTruthy();

    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    await expect(page.getByText("3 Feb 1922").first()).toBeVisible();

    await page.goto(`/trees/${tree.treeId}/settings`);
    await page.getByRole("button", { name: "Date Display", exact: true }).click();
    const preview = page.locator(".settings-date-preview");
    await expect(preview).toContainText("12 Mar 1822");

    // Each choice is saved on the spot, and the preview follows it.
    const saved = () =>
        page.waitForResponse((r) => r.url().endsWith(`/api/v1/trees/${tree.treeId}`) && r.request().method() === "PUT");
    let save = saved();
    await page.getByLabel("Date format").selectOption({ label: "12/03/1842" });
    expect((await save).ok()).toBeTruthy();
    await expect(preview).toContainText("12/03/1822");

    save = saved();
    await page.locator(".settings-card", { hasText: "Event symbols" }).getByRole("button", { name: "Yes" }).click();
    expect((await save).ok()).toBeTruthy();
    await expect(preview).toContainText("* 1799 + ca 1867");

    const stored = await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}`)).json();
    expect([stored.date_format, stored.date_symbols]).toEqual(["numeric", true]);

    // The person page writes the event's date in the tree's format…
    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    await expect(page.getByText("03/02/1922").first()).toBeVisible();
    await expect(page.getByText("3 Feb 1922")).toHaveCount(0);

    // …and the pedigree card its lifespan behind the birth symbol.
    await page.getByRole("button", { name: "Tree view" }).click();
    await expect(page.locator("g.ped-card").filter({ hasText: /Anchor\s*ASHDOWN/ }).first()).toContainText("* 1900");
});
