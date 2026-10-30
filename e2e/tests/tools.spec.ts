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

test("the ancestry completeness counts the SOSA root's ancestors and can show only what is missing", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/tools`);
    await page.getByRole("tab", { name: "Ancestry completeness" }).click();
    const summary = page.locator("table.tools-generations tbody tr");
    // The fixture's block 0: three complete generations above nobody.
    await expect(summary.filter({ hasText: "Generation 1" })).toContainText("1 / 1");
    await expect(summary.filter({ hasText: "Generation 3" })).toContainText("4 / 4");
    await expect(summary.filter({ hasText: "Generation 4" })).toContainText("0 / 8");

    const generation = (n: number) => page.locator("details.tools-generation").filter({ has: page.locator("summary", { hasText: `Generation ${n} ·` }) });
    await expect(generation(2).locator("tr", { hasText: "Bernard Ashdown" })).toContainText("2");
    await expect(generation(4).getByText("Missing ancestor")).toHaveCount(8);
    // Generation 3 records every key fact: it leaves the missing-only list.
    await expect(generation(3)).toHaveCount(1);
    await page.getByLabel("Show only what is missing").check();
    await expect(generation(3)).toHaveCount(0);
    await expect(generation(4).getByText("Missing ancestor")).toHaveCount(8);
});

test("the places not located are listed with their uses, and who uses one unfolds under it", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/tools`);
    await page.getByRole("tab", { name: "Places not located" }).click();
    await expect(page.getByText("5 places could not be located")).toBeVisible();
    const row = page.locator("table.tools-places tbody tr").filter({ hasText: "Westford, Shire" });
    await expect(row.locator(".tools-col-fact")).toHaveText("6");

    const uses = row.getByRole("button", { name: "Who uses it" });
    await uses.click();
    await expect(uses).toHaveAttribute("aria-expanded", "true");
    const usage = page.locator("tr.tools-place-usage");
    await expect(usage.locator(".dict-accordion-name", { hasText: "Ashdown Dorian" })).toBeVisible();
    await uses.click();
    await expect(usage).toHaveCount(0);
});

test("the date converter reads a date in every calendar", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/tools`);
    await page.getByRole("tab", { name: "Date converter" }).click();
    await page.locator(".pf-date-dd").fill("22");
    await page.locator(".pf-date-mm").fill("09");
    await page.locator(".pf-date-yyyy").fill("1792");
    const result = (calendar: string) => page.locator(".tools-converter-result").filter({ has: page.getByText(calendar, { exact: true }) });
    await expect(result("Gregorian").locator(".tools-converter-date")).toHaveText("22 Sep 1792");
    await expect(result("Julian").locator(".tools-converter-date")).toHaveText("11 Sep 1792");
    await expect(result("Hebrew").locator(".tools-converter-date")).toHaveText("6 Tishrei 5553");
    await expect(result("Republican").locator(".tools-converter-date")).toHaveText("1 Vendémiaire year I");
    await expect(page.getByText("Day of the week: Saturday")).toBeVisible();
});

test("a date written in Latin is read back into the date and written out part by part", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/tools`);
    await page.getByRole("tab", { name: "Date in words" }).click();
    await expect(page.getByText("Enter a date above.")).toBeVisible();
    await page.getByRole("textbox", { name: "Read a written date" }).fill("die secunda mensis Februarii anno Domini MDCL");
    await page.getByRole("button", { name: "Read", exact: true }).click();

    await expect(page.locator(".pf-date-dd")).toHaveValue("2");
    await expect(page.locator(".pf-date-mm")).toHaveValue("2");
    await expect(page.locator(".pf-date-yyyy")).toHaveValue("1650");
    const parts = page.locator("table.tools-words-parts");
    await expect(parts.locator("tr", { hasText: "Year" })).toContainText("millesimo sexcentesimo quinquagesimo = MDCL");
    await expect(parts.locator("tr", { hasText: "Month" })).toContainText("Februarii");
});
