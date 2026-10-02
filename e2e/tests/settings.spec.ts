import { apiUrl, expect, test } from "./fixtures";

/// The "+" of the first empty parent slot of the tree view.
const emptySlot = (page: import("@playwright/test").Page) =>
    page.locator("svg text.no-print").filter({ hasText: /^\+$/ }).first();

test("adding a parent offers the matching persons of the tree", async ({ page, request, tree }) => {
    const form = page.locator(".person-form-modal");
    const suggestions = form.locator(".pf-parent-suggestions");
    /// Opens the first empty parent slot and types a name someone of the
    /// slot's sex bears: the fixture's women are Claras, its men Bernards.
    const openAndType = async () => {
        await page.goto(`/trees/${tree.treeId}`);
        await emptySlot(page).click();
        await expect(form).toBeVisible();
        const given = form.locator(".form-group", { hasText: "Given Names *" }).locator("input");
        const female = form.getByRole("button", { name: "Female", exact: true });
        const mother = (await female.getAttribute("class"))?.includes("active");
        await given.fill(mother ? "Clara" : "Bernard");
        await expect(suggestions).toBeVisible();
    };

    // Dismissed, the panel leaves the form as it was.
    await openAndType();
    await suggestions.getByRole("button", { name: "Hide the suggestions" }).click();
    await expect(suggestions).toBeHidden();
    await expect(form).toBeVisible();

    // Linking a suggested person creates nobody.
    const count = async () =>
        (await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/persons?first=100`)).json()).edges.length;
    const before = await count();
    await openAndType();
    await suggestions.getByRole("button", { name: "Link this person" }).first().click();
    await expect(form).toBeHidden();
    expect(await count()).toBe(before);
});

test("the tree's entry options shape the person form and the linking panel", async ({ page, request, tree }) => {
    await page.goto(`/trees/${tree.treeId}/settings`);
    await page.getByRole("button", { name: "Entry Options", exact: true }).click();
    const saved = () =>
        page.waitForResponse((r) => r.url().endsWith(`/api/v1/trees/${tree.treeId}`) && r.request().method() === "PUT");
    let save = saved();
    await page.getByLabel("Input date format").selectOption({ label: "YYYY-MM-DD" });
    expect((await save).ok()).toBeTruthy();
    for (const card of ["Automatic uppercase for surnames", "Suggest existing persons"]) {
        save = saved();
        await page.locator(".settings-card", { hasText: card }).getByRole("button", { name: "No" }).click();
        expect((await save).ok()).toBeTruthy();
    }
    const stored = await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}`)).json();
    expect([stored.date_input_format, stored.surname_uppercase, stored.suggest_persons]).toEqual(["iso", false, false]);

    // A date field starts with its year, and a surname keeps its case.
    await page.goto(`/trees/${tree.treeId}`);
    const card = page.locator("g.ped-card").filter({ hasText: /Bernard\s*ASHDOWN/ }).first();
    await card.click({ button: "right" });
    await page.locator(".context-menu").getByRole("button", { name: "Edit individual" }).click();
    const form = page.locator(".person-form-modal");
    await expect(form.locator(".pf-date-row").first().locator(".pf-date-part").first()).toHaveAttribute("placeholder", "YYYY");
    const surname = form.locator(".form-group", { hasText: "Birth name *" }).locator("input").first();
    await surname.fill("Lowcase");
    await expect(surname).toHaveValue("Lowcase");

    // Creating a parent offers nobody of the tree either.
    await page.goto(`/trees/${tree.treeId}`);
    await emptySlot(page).click();
    await expect(form).toBeVisible();
    await form.locator(".form-group", { hasText: "Given Names *" }).locator("input").fill("Anchor");
    await page.waitForTimeout(800);
    await expect(form.locator(".pf-parent-suggestions")).toHaveCount(0);

    // Adding a spouse only offers a new person.
    await page.goto(`/trees/${tree.treeId}`);
    await card.click({ button: "right" });
    await page.locator(".context-menu").getByRole("button", { name: "Add spouse" }).click();
    const panel = page.locator(".linking-card");
    await expect(panel.getByRole("button", { name: "Create New Person as Spouse" })).toBeVisible();
    await expect(panel.locator(".search-person")).toHaveCount(0);
});

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
