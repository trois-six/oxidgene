import { apiUrl, expect, importGedcom, test } from "./fixtures";
import type { APIRequestContext } from "@playwright/test";

/// Create the tag `label` in `treeId` and return its id.
async function createTag(request: APIRequestContext, treeId: string, label: string): Promise<string> {
    const created = await request.post(`${apiUrl}/api/v1/trees/${treeId}/tags`, { data: { label } });
    expect(created.ok(), await created.text()).toBeTruthy();
    return (await created.json()).id as string;
}

/// Put the tag on a person, through the record route the picker uses.
async function tagPerson(request: APIRequestContext, treeId: string, tagId: string, personId: string): Promise<void> {
    const put = await request.put(`${apiUrl}/api/v1/trees/${treeId}/tags/${tagId}/records/person/${personId}`);
    expect(put.status()).toBe(204);
}

/// The id of the fixture person whose given names are `given`.
async function personIdOf(request: APIRequestContext, treeId: string, given: string): Promise<string> {
    const page = (await (await request.get(`${apiUrl}/api/v1/trees/${treeId}/profiles?first=100`)).json()) as {
        edges: Array<{ node: { person_id: string; primary_name?: { given_names?: string } } }>;
    };
    const id = page.edges.map((edge) => edge.node).find((p) => p.primary_name?.given_names === given)?.person_id;
    expect(id, `the fixture's ${given}`).toBeTruthy();
    return id as string;
}

test("highlights a tag's carriers in the tree and in another chart", async ({ page, request, tree }) => {
    const tagId = await createTag(request, tree.treeId, "Fictional mark");
    await tagPerson(request, tree.treeId, tagId, tree.anchorId);
    await tagPerson(request, tree.treeId, tagId, await personIdOf(request, tree.treeId, "Dorian"));

    await page.goto(`/trees/${tree.treeId}`);
    const cards = page.locator("g.ped-card");
    const anchor = cards.filter({ hasText: /Anchor\s*ASHDOWN/ }).first();
    const dorian = cards.filter({ hasText: /Dorian\s*ASHDOWN/ }).first();
    const bernard = cards.filter({ hasText: /Bernard\s*ASHDOWN/ }).first();
    await expect(bernard).toBeVisible();
    // Nothing is marked while no tag is highlighted.
    await expect(page.locator(".ped-tag-dot")).toHaveCount(0);

    const dialog = page.getByRole("dialog", { name: "Highlight a tag" });
    await page.getByRole("button", { name: "Highlight a tag", exact: true }).click();
    await dialog.locator(".tag-highlight-option", { hasText: "Fictional mark" }).click();
    await expect(dialog).toBeHidden();

    await expect(page.locator(".tag-highlight-chip")).toContainText("Fictional mark");
    await expect(anchor.locator(".ped-tag-dot")).toHaveCount(1);
    await expect(dorian.locator(".ped-tag-dot")).toHaveCount(1);
    await expect(bernard.locator(".ped-tag-dot")).toHaveCount(0);
    await expect(anchor).not.toHaveClass(/ped-tag-dim/);
    await expect(dorian).not.toHaveClass(/ped-tag-dim/);
    await expect(bernard).toHaveClass(/ped-tag-dim/);

    // Turning the dimming off keeps the marks and undims the others.
    await page.getByRole("button", { name: "Highlight a tag", exact: true }).click();
    await dialog.getByRole("checkbox", { name: "Dim the others" }).uncheck();
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(bernard).not.toHaveClass(/ped-tag-dim/);
    await expect(anchor.locator(".ped-tag-dot")).toHaveCount(1);

    // The chip's × ends the highlight: no marks are left.
    await page.getByRole("button", { name: "End the highlight of “Fictional mark”" }).click();
    await expect(page.locator(".tag-highlight-chip")).toHaveCount(0);
    await expect(page.locator(".ped-tag-dot")).toHaveCount(0);

    // The same highlight in the fan chart, chosen in App Settings, which
    // the tree page reads on its next load.
    await page.goto("/settings");
    await page.getByRole("button", { name: "Pedigree", exact: true }).click();
    const fan = page.getByRole("button", { name: /^Fan chart/ });
    await fan.click();
    await expect(fan).toHaveAttribute("aria-pressed", "true");
    await page.goto(`/trees/${tree.treeId}`);
    await page.getByRole("button", { name: "Highlight a tag", exact: true }).click();
    await dialog.locator(".tag-highlight-option", { hasText: "Fictional mark" }).click();
    await expect(page.locator(".tag-highlight-chip")).toContainText("Fictional mark");
    await expect(page.locator(".ped-tag-dot")).toHaveCount(2);
    await expect(page.locator(".ped-tag-dim").first()).toBeAttached();
});

test("tags the search results from the keyboard", async ({ page, request, tree }) => {
    const tagId = await createTag(request, tree.treeId, "Fictional kin");
    await page.goto(`/trees/${tree.treeId}/search?last=Ashdown&first=&origin=`);
    await expect(page.getByText("5 results")).toBeVisible();

    await page.getByRole("button", { name: "Tag the results…", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Tag persons" });
    await expect(dialog.getByText("5 persons in this selection")).toBeVisible();
    const row = dialog.getByRole("group", { name: "Fictional kin" });
    await expect(row.locator(".tag-bulk-count")).toHaveText("0 of 5");

    // Tab from the filter field reaches the tag's add action; Enter adds.
    await dialog.getByRole("textbox", { name: "Filter or create a tag" }).focus();
    await page.keyboard.press("Tab");
    await expect(row.getByRole("button", { name: "Add “Fictional kin” to all" })).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(row.locator(".tag-bulk-count")).toHaveText("5 of 5");
    await expect(row.getByRole("button", { name: "Add “Fictional kin” to all" })).toBeDisabled();
    const carriers = await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/tags/${tagId}/records?kind=person&first=10`);
    expect((await carriers.json()).total_count).toBe(5);

    // Tab from the add action reaches the remove action; Space removes.
    await page.keyboard.press("Tab");
    await expect(row.getByRole("button", { name: "Remove “Fictional kin” from all" })).toBeFocused();
    await page.keyboard.press(" ");
    await expect(row.locator(".tag-bulk-count")).toHaveText("0 of 5");
    await expect(row.getByRole("button", { name: "Remove “Fictional kin” from all" })).toBeDisabled();
});

test("asks before a bulk change over 100 persons", async ({ page, request }) => {
    // One import seeds 101 persons of one name: a single request, not 101.
    const individuals = Array.from({ length: 101 }, (_, index) => [
        `0 @I${index + 1}@ INDI`,
        "1 NAME Wren /Mosswick/",
        "1 SEX U",
    ]).flat();
    const gedcom = ["0 HEAD", "1 GEDC", "2 VERS 5.5.1", "1 CHAR UTF-8", ...individuals, "0 TRLR"].join("\n");
    const created = await request.post(`${apiUrl}/api/v1/trees`, { data: { name: "E2E Mosswick bulk" } });
    expect(created.ok()).toBeTruthy();
    const treeId = (await created.json()).id as string;
    await importGedcom(request, treeId, gedcom);
    await createTag(request, treeId, "Fictional kin");

    await page.goto(`/trees/${treeId}/search?last=Mosswick&first=&origin=`);
    await expect(page.getByText("101 results")).toBeVisible();
    await page.getByRole("button", { name: "Tag the results…", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Tag persons" });
    await expect(dialog.getByText("101 persons in this selection")).toBeVisible();
    const row = dialog.getByRole("group", { name: "Fictional kin" });

    await row.getByRole("button", { name: "Add “Fictional kin” to all" }).click();
    await expect(dialog.getByText("This changes 101 persons at once.")).toBeVisible();
    await expect(row.locator(".tag-bulk-count")).toHaveText("0 of 101");
    await dialog.getByRole("button", { name: "Apply", exact: true }).click();
    await expect(row.locator(".tag-bulk-count")).toHaveText("101 of 101");
});

test("tags an event and a source from the keyboard menu, and sees the dots", async ({ page, request, tree }) => {
    await createTag(request, tree.treeId, "Fictional doubt");
    await createTag(request, tree.treeId, "Fictional checked");

    // The birth of the fixture's root, in the timeline of the person profile.
    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    const birth = page.locator("li[id^='pd-event-']").filter({ hasText: "Birth" }).first();
    await expect(birth).toBeVisible();
    await expect(birth.locator(".tag-dots")).toHaveCount(0);

    // Shift+F10 on the focused row opens its menu; Tags… (focused) opens the picker.
    await birth.focus();
    await page.keyboard.press("Shift+F10");
    const tagsEntry = page.locator(".record-tag-menu").getByRole("button", { name: "Tags…", exact: true });
    await expect(tagsEntry).toBeFocused();
    await page.keyboard.press("Enter");
    const filter = page.getByRole("textbox", { name: "Filter or create a tag" });
    await expect(filter).toBeFocused();
    await page.keyboard.type("Fictional doubt");
    await expect(page.getByRole("option", { name: "Fictional doubt" })).toBeVisible();
    await page.keyboard.press("Enter");
    await expect(birth.locator(".tag-dot-link")).toHaveAttribute("title", "Fictional doubt");
    await page.keyboard.press("Escape");
    await expect(filter).toBeHidden();
    await expect(birth).toBeFocused();

    // The source of the same fixture, in the Sources tab of the dictionary.
    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Sources", exact: true }).click();
    const source = page.locator(".dict-row").filter({ hasText: "Parish register 0" }).first();
    await expect(source).toBeVisible();
    await expect(source.locator(".tag-dot-link")).toHaveCount(0);

    await source.focus();
    await page.keyboard.press("Shift+F10");
    await expect(tagsEntry).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(filter).toBeFocused();
    await page.keyboard.type("Fictional checked");
    await expect(page.getByRole("option", { name: "Fictional checked" })).toBeVisible();
    await page.keyboard.press("Enter");
    await expect(source.locator(".tag-dot-link")).toHaveAttribute("title", "Fictional checked");
    await page.keyboard.press("Escape");
    await expect(source).toBeFocused();
});
