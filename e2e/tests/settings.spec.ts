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

/// The writes to the tree the page sends from now on, which the preferences
/// of the device must leave empty: they never reach the server.
const treeWrites = (page: import("@playwright/test").Page, treeId: string) => {
    const writes: string[] = [];
    page.on("request", (r) => {
        if (r.url().endsWith(`/api/v1/trees/${treeId}`) && r.method() !== "GET") {
            writes.push(`${r.method()} ${r.url()}`);
        }
    });
    return writes;
};

/// A Yes / No preference of the settings pages, named `name`.
const yesNo = (page: import("@playwright/test").Page, name: string, choice: "Yes" | "No") =>
    page.getByRole("group", { name, exact: true }).getByRole("button", { name: choice, exact: true });

test("the entry options of the app settings shape the person form and the linking panel", async ({ page, tree }) => {
    const writes = treeWrites(page, tree.treeId);
    await page.goto("/settings");
    await page.getByRole("button", { name: "Entry Options", exact: true }).click();
    await page.getByLabel("Input date format").selectOption({ label: "YYYY-MM-DD" });
    for (const option of ["Automatic uppercase for surnames", "Suggest existing persons"]) {
        await yesNo(page, option, "No").click();
        await expect(yesNo(page, option, "No")).toHaveAttribute("aria-pressed", "true");
    }

    // Kept on the device: a reload, and the tree's own settings, show the same choices.
    await page.reload();
    await page.getByRole("button", { name: "Entry Options", exact: true }).click();
    await expect(page.getByLabel("Input date format")).toHaveValue("iso");
    await page.goto(`/trees/${tree.treeId}/settings`);
    await page.getByRole("button", { name: "Entry Options", exact: true }).click();
    await expect(yesNo(page, "Suggest existing persons", "No")).toHaveAttribute("aria-pressed", "true");

    // A date field starts with its year, and a surname keeps its case.
    await page.goto(`/trees/${tree.treeId}`);
    const card = page.locator("g.ped-card").filter({ hasText: /Bernard\s*ASHDOWN/ }).first();
    await card.click({ button: "right" });
    await page.locator(".context-menu").getByRole("menuitem", { name: "Edit individual", exact: true }).click();
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
    await page.locator(".context-menu").getByRole("menuitem", { name: "Add spouse", exact: true }).click();
    const panel = page.locator(".linking-card");
    await expect(panel.getByRole("button", { name: "Create New Person as Spouse" })).toBeVisible();
    await expect(panel.locator(".search-person")).toHaveCount(0);
    expect(writes).toEqual([]);
});

test("a long hint never squeezes the Yes / No buttons of its row", async ({ page }) => {
    // Narrow enough for every hint to wrap, wide enough to keep hint and
    // buttons on one row.
    await page.setViewportSize({ width: 800, height: 800 });
    await page.goto("/settings");
    for (const section of ["Names", "Date Display", "Entry Options"]) {
        await page.getByRole("button", { name: section, exact: true }).click();
        const groups = page.locator(".settings-content .theme-toggle-group");
        await expect(groups.first()).toBeVisible();
        for (const group of await groups.all()) {
            const cut = await group.evaluate((el) => el.scrollWidth - el.clientWidth);
            expect(cut, `${section}: ${await group.getAttribute("aria-label")}`).toBe(0);
        }
    }
});

test("the date display of the tree's global preferences reaches the person page and the pedigree", async ({ page, request, tree }) => {
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
    const writes = treeWrites(page, tree.treeId);

    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    await expect(page.getByText("3 Feb 1922").first()).toBeVisible();

    await page.goto(`/trees/${tree.treeId}/settings`);
    await page.getByRole("button", { name: "Date Display", exact: true }).click();
    const preview = page.locator(".settings-date-preview");
    await expect(preview).toContainText("12 Mar 1822");

    // Each choice applies on the spot, and the preview follows it.
    await page.getByLabel("Date format").selectOption({ label: "12/03/1842" });
    await expect(preview).toContainText("12/03/1822");
    await yesNo(page, "Event symbols", "Yes").click();
    await expect(preview).toContainText("* 1799 + ca 1867");

    // The person page writes the event's date in the chosen format…
    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    await expect(page.getByText("03/02/1922").first()).toBeVisible();
    await expect(page.getByText("3 Feb 1922")).toHaveCount(0);

    // …and the pedigree card its lifespan behind the birth symbol.
    await page.getByRole("button", { name: "Tree view" }).click();
    await expect(page.locator("g.ped-card").filter({ hasText: /Anchor\s*ASHDOWN/ }).first()).toContainText("* 1900");

    // The app settings show the same preference.
    await page.goto("/settings");
    await page.getByRole("button", { name: "Date Display", exact: true }).click();
    await expect(page.getByLabel("Date format")).toHaveValue("numeric");
    await expect(yesNo(page, "Event symbols", "Yes")).toHaveAttribute("aria-pressed", "true");
    expect(writes).toEqual([]);
});

test("a GEDZIP export can be downloaded again after the page is reloaded", async ({ page, tree }) => {
    // Playwright cannot drive Chromium's save picker: without it, the page
    // falls back to a browser download, which it can.
    await page.addInitScript(() => {
        Object.defineProperty(window, "showSaveFilePicker", { value: undefined, configurable: true });
    });
    const openExport = () => page.getByRole("button", { name: "Export Tree", exact: true }).click();
    await page.goto(`/trees/${tree.treeId}/settings`);
    await openExport();
    await page.locator(".settings-export-format").selectOption("gedzip");
    let download = page.waitForEvent("download");
    await page.getByRole("button", { name: "Download .gdz" }).click();
    expect((await download).suggestedFilename()).toMatch(/\.gdz$/);
    const again = page.getByRole("button", { name: "Download again" });
    await expect(again).toBeVisible();

    // A reload forgets what the page did; the server still keeps the archive.
    await page.reload();
    await openExport();
    await expect(again).toBeVisible();
    await expect(page.locator(".settings-export-info", { hasText: "available until" })).toContainText(
        /GEDZIP archive of [\d.]+ [KM]?B/,
    );
    download = page.waitForEvent("download");
    await again.click();
    const saved = await download;
    expect(saved.suggestedFilename()).toMatch(/\.gdz$/);
    expect(await saved.failure()).toBeNull();
});

test("a GEDCOM export is written for the chosen destination program and says what it lost", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/settings`);
    await page.getByRole("button", { name: "Export Tree", exact: true }).click();
    const destination = page.getByLabel("Destination program");
    // Standard GEDCOM 5.5.1 first, and chosen.
    await expect(destination).toHaveValue("generic-551");
    await expect(destination.locator("option").first()).toHaveText("Standard GEDCOM 5.5.1");
    await expect(page.getByText("still to be verified")).toBeHidden();

    // A program: its description, and the hint its profile still needs checking.
    await destination.selectOption("geneanet");
    await expect(page.getByText("Geneanet’s online import")).toBeVisible();
    await expect(page.getByText("still to be verified against its latest version")).toBeVisible();
    const download = page.waitForEvent("download");
    await page.getByRole("button", { name: "Download .ged" }).click();
    expect((await download).suggestedFilename()).toMatch(/\.ged$/);
    await expect(
        page.getByText(/Written for Geneanet without losing anything|lost information for Geneanet/),
    ).toBeVisible();

    // A GEDZIP is standard GEDCOM 7.0: no destination to choose.
    await page.locator(".settings-export-format").selectOption("gedzip");
    await expect(destination).toBeHidden();
});

test("the app settings switch the theme and the language, and keep both on the device", async ({ page }) => {
    await page.goto("/settings");
    const title = page.locator(".td-bc-current");
    await expect(title).toHaveText("Settings");
    const theme = (label: string) =>
        page.locator(".theme-picker-option").filter({ has: page.locator(".theme-picker-label").getByText(label, { exact: true }) });
    const background = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);

    // Appearance opens first: the dark theme repaints the whole page.
    const light = theme("Light");
    const dark = theme("Dark");
    await light.click();
    await expect(light).toHaveAttribute("aria-pressed", "true");
    const lightBackground = await background();
    await dark.click();
    await expect(dark).toHaveAttribute("aria-pressed", "true");
    await expect(light).toHaveAttribute("aria-pressed", "false");
    await expect.poll(background).not.toBe(lightBackground);

    // The language rewrites the interface at once.
    await page.getByRole("button", { name: "Language", exact: true }).click();
    await page.locator(".lang-option", { hasText: "Français" }).click();
    await expect(title).toHaveText("Paramètres");
    await expect(page.getByRole("button", { name: "Apparence", exact: true })).toBeVisible();

    await page.reload();
    await expect(title).toHaveText("Paramètres");
    await expect(theme("Sombre")).toHaveAttribute("aria-pressed", "true");
    expect(await background()).not.toBe(lightBackground);
});
