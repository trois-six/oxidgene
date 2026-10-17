import { apiUrl, expect, test } from "./fixtures";

test("a click on an anomaly's row opens its subject's profile", async ({ page, request, tree }) => {
    // The anchor, born in 1900, recorded dead in 1850.
    const death = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}/events`, {
        data: { person_id: tree.anchorId, event_type: "death", date_value: "1850" },
    });
    expect(death.ok(), await death.text()).toBeTruthy();

    await page.goto(`/trees/${tree.treeId}/tools`);
    await page.getByRole("tab", { name: "Anomalies" }).click();
    const rule = page.locator("details.tools-rule", { hasText: "Death before birth" });
    await rule.locator("summary").click();
    const row = rule.locator("tr.tools-anomaly-row", { hasText: "Anchor" }).first();

    // Not on the name, which is a link of its own: on what the rule measured.
    await row.locator("td.tools-detail").click();
    await expect(page).toHaveURL(new RegExp(`/persons/${tree.anchorId}$`));
    await expect(page.getByRole("heading", { level: 1, name: "Anchor Ashdown" })).toBeVisible();

    // The keyboard opens it the same way.
    await page.goBack();
    await rule.locator("summary").click();
    await row.focus();
    await page.keyboard.press("Enter");
    await expect(page).toHaveURL(new RegExp(`/persons/${tree.anchorId}$`));
});
