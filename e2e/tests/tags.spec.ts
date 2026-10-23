import { apiUrl, createTag, expect, expectedConsoleError, importGedcom, personIdOf, tagPerson, tagVocabularyRequests, test } from "./fixtures";
import type { APIRequestContext, Locator } from "@playwright/test";

/// Create one source per title in `treeId`, in batches, and return their ids in
/// the order of `titles`.
async function seedSources(request: APIRequestContext, treeId: string, titles: string[]): Promise<string[]> {
    const ids: string[] = [];
    for (let start = 0; start < titles.length; start += 26) {
        const batch = titles.slice(start, start + 26);
        const created = await Promise.all(
            batch.map((title) => request.post(`${apiUrl}/api/v1/trees/${treeId}/sources`, { data: { title } })),
        );
        for (const response of created) {
            expect(response.status()).toBe(201);
            ids.push((await response.json()).id as string);
        }
    }
    return ids;
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

test("filters a long tag list from a field focused on opening, folding case and accents", async ({ page, request, tree }) => {
    // Twelve tags, past the threshold of the search field. Each one sits on
    // the anchor, so each one is listed; only the accented one also sits on
    // Dorian.
    const labels = ["À vérifier", ...Array.from({ length: 11 }, (_, index) => `Fictional tag ${index + 1}`)];
    const ids: string[] = [];
    for (const label of labels) {
        ids.push(await createTag(request, tree.treeId, label));
    }
    for (const tagId of ids) {
        await tagPerson(request, tree.treeId, tagId, tree.anchorId);
    }
    await tagPerson(request, tree.treeId, ids[0], await personIdOf(request, tree.treeId, "Dorian"));

    await page.goto(`/trees/${tree.treeId}`);
    const cards = page.locator("g.ped-card");
    const dorian = cards.filter({ hasText: /Dorian\s*ASHDOWN/ }).first();
    await expect(dorian).toBeVisible();

    const dialog = page.getByRole("dialog", { name: "Highlight a tag" });
    await page.getByRole("button", { name: "Highlight a tag", exact: true }).click();
    const field = dialog.getByRole("textbox", { name: "Filter the tags" });
    await expect(field).toBeVisible();
    await expect(field).toBeFocused();
    await expect(dialog.locator(".tag-highlight-option")).toHaveCount(12);

    // Part of a label, in other case and without the accents, leaves the one tag.
    await field.fill("A VERIFIER");
    await expect(dialog.locator(".tag-highlight-option")).toHaveCount(1);
    await expect(dialog.locator(".tag-highlight-option")).toContainText("À vérifier");

    // A text that matches no tag says so, and the title and the dimming
    // checkbox stay in the dialog.
    await field.fill("zzz");
    await expect(dialog.locator(".tag-highlight-option")).toHaveCount(0);
    await expect(dialog.getByText("No matching tag.")).toBeVisible();
    await expect(dialog.getByRole("checkbox", { name: "Dim the others" })).toBeVisible();

    await field.fill("a verifier");
    await dialog.locator(".tag-highlight-option", { hasText: "À vérifier" }).click();
    await expect(dialog).toBeHidden();
    await expect(page.locator(".tag-highlight-chip")).toContainText("À vérifier");
    await expect(dorian.locator(".ped-tag-dot")).toHaveCount(1);
    await expect(dorian).not.toHaveClass(/ped-tag-dim/);

    // Reopening the dialog shows the whole list again, with the field empty.
    await page.getByRole("button", { name: "Highlight a tag", exact: true }).click();
    await expect(dialog.getByRole("textbox", { name: "Filter the tags" })).toHaveValue("");
    await expect(dialog.locator(".tag-highlight-option")).toHaveCount(12);
});

test("keeps the highlight dialog plain for three tags, with no search field", async ({ page, request, tree }) => {
    for (const label of ["Fictional one", "Fictional two", "Fictional three"]) {
        await tagPerson(request, tree.treeId, await createTag(request, tree.treeId, label), tree.anchorId);
    }

    await page.goto(`/trees/${tree.treeId}`);
    const dialog = page.getByRole("dialog", { name: "Highlight a tag" });
    await page.getByRole("button", { name: "Highlight a tag", exact: true }).click();
    await expect(dialog.locator(".tag-highlight-option")).toHaveCount(3);
    await expect(dialog.getByRole("textbox", { name: "Filter the tags" })).toHaveCount(0);
    await expect(dialog.getByRole("checkbox", { name: "Dim the others" })).toBeVisible();
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
    await expect(birth.locator(".tag-badges")).toHaveCount(0);

    // Shift+F10 on the focused row opens its menu; Tags… (focused) opens the picker.
    await birth.focus();
    await page.keyboard.press("Shift+F10");
    const tagsEntry = page.locator(".record-tag-menu").getByRole("menuitem", { name: "Tags…", exact: true });
    await expect(tagsEntry).toBeFocused();
    await page.keyboard.press("Enter");
    const filter = page.getByRole("textbox", { name: "Filter or create a tag" });
    await expect(filter).toBeFocused();
    await page.keyboard.type("Fictional doubt");
    await expect(page.getByRole("option", { name: "Fictional doubt" })).toBeVisible();
    await page.keyboard.press("Enter");
    await expect(birth.locator(".tag-badge")).toHaveAttribute("title", "Fictional doubt");
    await page.keyboard.press("Escape");
    await expect(filter).toBeHidden();
    await expect(birth).toBeFocused();

    // The source of the same fixture, in the Sources tab of the dictionary.
    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Sources", exact: true }).click();
    const source = page.locator(".dict-row").filter({ hasText: "Parish register 0" }).first();
    await expect(source).toBeVisible();
    await expect(source.locator(".tag-badge")).toHaveCount(0);

    // A source's menu opens on Edit, so Tags… is reached by its own focus.
    await source.focus();
    await page.keyboard.press("Shift+F10");
    await page.locator(".record-tag-menu").getByRole("menuitem", { name: "Tags…", exact: true }).focus();
    await expect(tagsEntry).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(filter).toBeFocused();
    await page.keyboard.type("Fictional checked");
    await expect(page.getByRole("option", { name: "Fictional checked" })).toBeVisible();
    await page.keyboard.press("Enter");
    await expect(source.locator(".tag-badge")).toHaveAttribute("title", "Fictional checked");
    await page.keyboard.press("Escape");
    await expect(source).toBeFocused();
});

test("tags a note and a place from the keyboard menu, and sees the dots", async ({ page, request, tree }) => {
    await createTag(request, tree.treeId, "Fictional note mark");
    await createTag(request, tree.treeId, "Fictional place mark");
    const note = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}/notes`, {
        data: { text: "Fictional note about the root", person_id: tree.anchorId },
    });
    expect(note.status()).toBe(201);
    const filter = page.getByRole("textbox", { name: "Filter or create a tag" });
    const tagsEntry = page.locator(".record-tag-menu").getByRole("menuitem", { name: "Tags…", exact: true });

    // The note of the person's profile, in the Notes card.
    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    const entry = page.locator("[id^='note-entry-']").filter({ hasText: "Fictional note about the root" }).first();
    await expect(entry).toBeVisible();
    await expect(entry.locator(".tag-badges")).toHaveCount(0);

    await entry.focus();
    await page.keyboard.press("Shift+F10");
    await expect(tagsEntry).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(filter).toBeFocused();
    await page.keyboard.type("Fictional note mark");
    await expect(page.getByRole("option", { name: "Fictional note mark" })).toBeVisible();
    await page.keyboard.press("Enter");
    await expect(entry.locator(".tag-badge")).toHaveAttribute("title", "Fictional note mark");
    await page.keyboard.press("Escape");
    await expect(entry).toBeFocused();

    // The place of the fixture, in the Places tab of the dictionary.
    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Places", exact: true }).click();
    const place = page.locator(".dict-row").filter({ hasText: "Northfield" }).first();
    await expect(place).toBeVisible();
    await expect(place.locator(".tag-badge")).toHaveCount(0);

    // A place's menu opens on Edit, as a source's does.
    await place.focus();
    await page.keyboard.press("Shift+F10");
    await expect(page.locator(".record-tag-menu").getByRole("menuitem", { name: "Edit", exact: true })).toBeFocused();
    await tagsEntry.focus();
    await expect(tagsEntry).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(filter).toBeFocused();
    await page.keyboard.type("Fictional place mark");
    await expect(page.getByRole("option", { name: "Fictional place mark" })).toBeVisible();
    await page.keyboard.press("Enter");
    await expect(place.locator(".tag-badge")).toHaveAttribute("title", "Fictional place mark");
    await page.keyboard.press("Escape");
    await expect(place).toBeFocused();
});

test("opens a tagged source, place and repository on their own rows from the Tags tab", async ({ page, request, tree }) => {
    // More sources than the drill-down's flat list takes (250), sharing one
    // title prefix, so the Sources tab takes at least one prefix level to
    // reach the target.
    const sourceTitles = Array.from({ length: 260 }, (_, index) => `Fictional register ${String(index + 1).padStart(3, "0")}`);
    const sourceIds = await seedSources(request, tree.treeId, sourceTitles);
    const deepIndex = sourceTitles.indexOf("Fictional register 200");
    const sourceId = sourceIds[deepIndex];

    const repository = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}/repositories`, {
        data: { name: "Fictional archive" },
    });
    expect(repository.status()).toBe(201);
    const repositoryId = (await repository.json()).id as string;

    const places = (await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/places?first=100`)).json()) as {
        edges: Array<{ node: { id: string; name: string } }>;
    };
    const placeId = places.edges.map((edge) => edge.node).find((place) => place.name.includes("Northfield"))?.id;
    expect(placeId, "the fixture's Northfield").toBeTruthy();

    const records = [
        { label: "Fictional deep mark", kind: "source", id: sourceId, title: "Fictional register 200", row: `dict-source-${sourceId}` },
        { label: "Fictional place mark", kind: "place", id: placeId, title: "Northfield", row: `dict-place-${placeId}` },
        { label: "Fictional repository mark", kind: "repository", id: repositoryId, title: "Fictional archive", row: `dict-repository-${repositoryId}` },
    ];
    for (const record of records) {
        const tagId = await createTag(request, tree.treeId, record.label);
        const put = await request.put(`${apiUrl}/api/v1/trees/${tree.treeId}/tags/${tagId}/records/${record.kind}/${record.id}`);
        expect(put.status()).toBe(204);

        // The tag's own page unfolds it: the record's link opens its row.
        await page.goto(`/trees/${tree.treeId}/dictionary/tags/${tagId}`);
        await page.getByRole("link", { name: record.title }).click();

        const row = page.locator(`#${record.row}`);
        await expect(row).toBeVisible({ timeout: 15_000 });
        await expect(row).toBeInViewport();
        await expect(row).toBeFocused({ timeout: 15_000 });
        await expect(row).toContainText("▲");
    }
});

test("a click or long press on a source's tag badge opens the tag without toggling the row", async ({ page, request, tree }) => {
    const [sourceId] = await seedSources(request, tree.treeId, ["Fictional badge register"]);
    const tagId = await createTag(request, tree.treeId, "Fictional badge mark");
    const put = await request.put(`${apiUrl}/api/v1/trees/${tree.treeId}/tags/${tagId}/records/source/${sourceId}`);
    expect(put.status()).toBe(204);

    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Sources", exact: true }).click();
    const row = page.locator(`#dict-source-${sourceId}`);
    await expect(row).toBeVisible();
    await expect(row).toContainText("▼");
    const badge = row.locator(".tag-badge");
    await expect(badge).toHaveAttribute("title", "Fictional badge mark");

    // A long press on the badge does not open the row's record menu.
    const box = await badge.boundingBox();
    expect(box).not.toBeNull();
    await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
    await page.mouse.down();
    await page.waitForTimeout(700);
    await expect(page.locator(".record-tag-menu")).toHaveCount(0);
    await page.mouse.up();

    // The click opens the tag's page; the row was not unfolded on the way.
    await expect(page).toHaveURL(new RegExp(`/dictionary/tags/${tagId}$`));
    await page.goBack();
    await page.getByRole("tab", { name: "Sources", exact: true }).click();
    await expect(page.locator(`#dict-source-${sourceId}`)).toContainText("▼");
    await expect(page.locator(`#dict-source-${sourceId}`)).not.toContainText("▲");
});

test("opens a record's link on the tab root when the id is not a record's", async ({ page, tree }) => {
    // A link with no record id behind it: the Sources tab opens at its root,
    // with no request for the record and no error.
    await page.goto(`/trees/${tree.treeId}/dictionary/sources/not-a-record`);
    await expect(page.getByRole("tab", { name: "Sources", exact: true })).toHaveAttribute("aria-selected", "true");
    await expect(page.locator(".dict-row").first()).toBeVisible();
});

test("opens a tagged source whose title is shared by more sources than the flat list takes", async ({ page, request, tree }) => {
    // Every source shares one title: no character after it splits them, so the
    // Sources tab must show the final list at that title, with the tagged row
    // in it, rather than a single group the tab could not open.
    const sourceIds = await seedSources(request, tree.treeId, Array.from({ length: 260 }, () => "Fictional ledger"));
    const sourceId = sourceIds[200];

    const tagId = await createTag(request, tree.treeId, "Fictional shared mark");
    const put = await request.put(`${apiUrl}/api/v1/trees/${tree.treeId}/tags/${tagId}/records/source/${sourceId}`);
    expect(put.status()).toBe(204);

    await page.goto(`/trees/${tree.treeId}/dictionary/tags/${tagId}`);
    await page.getByRole("link", { name: "Fictional ledger", exact: true }).click();

    const row = page.locator(`#dict-source-${sourceId}`);
    await expect(row).toBeVisible({ timeout: 15_000 });
    await expect(row).toBeInViewport();
    await expect(row).toBeFocused({ timeout: 15_000 });
    await expect(row).toContainText("▲");
});

test("opens a tagged place on a later page of its letter", async ({ page, request, tree }) => {
    // Thirty places share one letter, so the one at index 26 sits on the second
    // page of that letter (25 per page): the tab must change page to show it.
    const names = Array.from({ length: 30 }, (_, index) => `Fictional hamlet ${String(index + 1).padStart(2, "0")}`);
    const placeIds: string[] = [];
    for (const name of names) {
        const created = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}/places`, { data: { name } });
        expect(created.status()).toBe(201);
        placeIds.push((await created.json()).id as string);
    }
    const placeId = placeIds[26];

    const tagId = await createTag(request, tree.treeId, "Fictional hamlet mark");
    const put = await request.put(`${apiUrl}/api/v1/trees/${tree.treeId}/tags/${tagId}/records/place/${placeId}`);
    expect(put.status()).toBe(204);

    await page.goto(`/trees/${tree.treeId}/dictionary/tags/${tagId}`);
    await page.getByRole("link", { name: names[26], exact: true }).click();

    const row = page.locator(`#dict-place-${placeId}`);
    await expect(row).toBeVisible({ timeout: 15_000 });
    await expect(row).toBeInViewport();
    await expect(row).toBeFocused({ timeout: 15_000 });
    await expect(row).toContainText("▲");
});

test("opens the tab root, without a request error, for a source deleted since it was tagged", async ({ page, request, tree }) => {
    const [deletedId] = await seedSources(request, tree.treeId, ["Fictional deleted register"]);
    await seedSources(request, tree.treeId, ["Fictional surviving register"]);
    const deleted = await request.delete(`${apiUrl}/api/v1/trees/${tree.treeId}/sources/${deletedId}`);
    expect(deleted.ok(), await deleted.text()).toBeTruthy();

    // A well-formed id that is no source of the tree: the Sources tab opens at
    // its root and no request for the id fails (the console guard is active).
    await page.goto(`/trees/${tree.treeId}/dictionary/sources/${deletedId}`);
    await expect(page.getByRole("tab", { name: "Sources", exact: true })).toHaveAttribute("aria-selected", "true");
    await expect(page.locator(`#dict-source-${deletedId}`)).toHaveCount(0);
});

test("opens the tab root, without a request error, for an unknown source id", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/dictionary/sources/0195c0de-0000-7000-8000-0000000000fe`);
    await expect(page.getByRole("tab", { name: "Sources", exact: true })).toHaveAttribute("aria-selected", "true");
});

test("opens the tab root, without a request error, for an unknown place id", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/dictionary/places/0195c0de-0000-7000-8000-0000000000fd`);
    await expect(page.getByRole("tab", { name: "Places", exact: true })).toHaveAttribute("aria-selected", "true");
    await expect(page.locator(".dict-row").first()).toBeVisible();
});

test("opens the tab root, without a request error, for an unknown repository id", async ({ page, request, tree }) => {
    const repository = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}/repositories`, {
        data: { name: "Fictional listed archive" },
    });
    expect(repository.status()).toBe(201);

    await page.goto(`/trees/${tree.treeId}/dictionary/repositories/0195c0de-0000-7000-8000-0000000000fc`);
    await expect(page.getByRole("tab", { name: "Repositories", exact: true })).toHaveAttribute("aria-selected", "true");
    await expect(page.locator(".dict-row").first()).toBeVisible();
});

test("unfolds a linked repository once, not again when its tab is shown later", async ({ page, request, tree }) => {
    const repository = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}/repositories`, {
        data: { name: "Fictional reading room" },
    });
    expect(repository.status()).toBe(201);
    const repositoryId = (await repository.json()).id as string;

    const tagId = await createTag(request, tree.treeId, "Fictional room mark");
    const put = await request.put(`${apiUrl}/api/v1/trees/${tree.treeId}/tags/${tagId}/records/repository/${repositoryId}`);
    expect(put.status()).toBe(204);

    await page.goto(`/trees/${tree.treeId}/dictionary/tags/${tagId}`);
    await page.getByRole("link", { name: "Fictional reading room", exact: true }).click();
    const row = page.locator(`#dict-repository-${repositoryId}`);
    await expect(row).toContainText("▲", { timeout: 15_000 });

    // Leaving the tab and coming back shows the row as the reader left it:
    // folded, not unfolded and focused again by the link.
    await page.getByRole("tab", { name: "Sources", exact: true }).click();
    await page.getByRole("tab", { name: "Repositories", exact: true }).click();
    await expect(row).toBeVisible();
    await expect(row).not.toContainText("▲");
});

test("replays the Sources tab's levels on browser back and forward after a tagged source's link", async ({ page, request, tree }) => {
    // The same shape as the deep-source case: the target sits under the
    // "FICTIONAL REGISTER 2" branch of the level "FICTIONAL REGISTER ".
    const sourceTitles = Array.from({ length: 260 }, (_, index) => `Fictional register ${String(index + 1).padStart(3, "0")}`);
    const sourceIds = await seedSources(request, tree.treeId, sourceTitles);
    const sourceId = sourceIds[sourceTitles.indexOf("Fictional register 200")];

    const tagId = await createTag(request, tree.treeId, "Fictional history mark");
    const put = await request.put(`${apiUrl}/api/v1/trees/${tree.treeId}/tags/${tagId}/records/source/${sourceId}`);
    expect(put.status()).toBe(204);

    await page.goto(`/trees/${tree.treeId}/dictionary/tags/${tagId}`);
    await page.getByRole("link", { name: "Fictional register 200", exact: true }).click();
    const row = page.locator(`#dict-source-${sourceId}`);
    await expect(row).toContainText("▲", { timeout: 15_000 });
    const activeCrumb = page.locator(".dict-src-breadcrumb .dict-src-crumb.active");
    await expect(activeCrumb).toHaveText("FICTIONAL REGISTER 2");

    // Back leaves the record's route for the tag's page.
    await page.goBack();
    await expect(page).toHaveURL(new RegExp(`/dictionary/tags/${tagId}$`));

    // Forward replays the levels to the same row: the breadcrumb and the
    // drill level read as before, and the row is reachable again.
    await page.goForward();
    await expect(page).toHaveURL(new RegExp(`/dictionary/sources/${sourceId}$`));
    await expect(page.getByRole("tab", { name: "Sources", exact: true })).toHaveAttribute("aria-selected", "true");
    await expect(activeCrumb).toHaveText("FICTIONAL REGISTER 2", { timeout: 15_000 });
    await expect(row).toBeVisible({ timeout: 15_000 });
    await expect(row).toBeFocused({ timeout: 15_000 });
    await expect(row).toContainText("▲");

    // The root crumb leads back to the tab's root, whose letters are shown.
    await page.locator(".dict-src-breadcrumb .dict-src-crumb").first().click();
    await expect(page.getByRole("button", { name: "F", exact: true })).toBeVisible();
    await expect(page.locator(".dict-src-groups-label")).toBeVisible();
});

test("drills through a level whose exact-title group sits beside its siblings", async ({ page, request, tree }) => {
    // One source titled exactly "Fictional ledger" beside 300 sources that
    // continue it: the level stays a branch, its exact-title group is chosen
    // like any other and lists the exact source alone, and the sibling group
    // drills on as usual.
    const titles = ["Fictional ledger", ...Array.from({ length: 300 }, (_, index) => `Fictional ledger: Book ${index + 1}`)];
    await seedSources(request, tree.treeId, titles);

    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Sources", exact: true }).click();
    // The root's letters: the tree's own "P" sources and the "F" of the ledgers,
    // whose levels skip down to the ledger title on their own.
    await page.getByRole("button", { name: "F", exact: true }).click();
    const exact = page.getByRole("button", { name: "FICTIONAL LEDGER", exact: true });
    await expect(exact).toBeEnabled();
    await exact.click();

    // The exact group is a final list: its one source, and the breadcrumb says
    // what the list holds.
    await expect(page.locator(".dict-src-breadcrumb .dict-src-crumb.active")).toHaveText("Titled exactly “FICTIONAL LEDGER”");
    await expect(page.locator(".dict-row")).toHaveCount(1);
    await expect(page.locator(".dict-row").first()).toContainText("Fictional ledger");
    await expect(page.locator(".dict-src-groups-label")).toHaveCount(0);

    // Back on the level by its breadcrumb, the sibling group is a branch too:
    // its groups show, not the 300 sources flattened.
    await page.locator(".dict-src-breadcrumb .dict-src-crumb").nth(1).click();
    const book = page.getByRole("button", { name: "FICTIONAL LEDGER:", exact: true });
    await expect(book).toBeEnabled();
    await book.click();
    await expect(page.locator(".dict-src-groups-label")).toBeVisible();
    await expect(page.locator(".dict-row")).toHaveCount(0);
    await page.getByRole("button", { name: "FICTIONAL LEDGER: BOOK 1", exact: true }).click();
    await expect(page.locator(".dict-row")).toHaveCount(111);
});

test("opens a tagged source whose exact title sits beside its siblings in the drill-down", async ({ page, request, tree }) => {
    // The same level: the tagged source is the one titled exactly "Fictional
    // ledger", so the Sources tab reaches it through the exact-title group.
    const titles = ["Fictional ledger", ...Array.from({ length: 300 }, (_, index) => `Fictional ledger: Book ${index + 1}`)];
    const sourceIds = await seedSources(request, tree.treeId, titles);
    const sourceId = sourceIds[0];

    const tagId = await createTag(request, tree.treeId, "Fictional exact mark");
    const put = await request.put(`${apiUrl}/api/v1/trees/${tree.treeId}/tags/${tagId}/records/source/${sourceId}`);
    expect(put.status()).toBe(204);

    await page.goto(`/trees/${tree.treeId}/dictionary/tags/${tagId}`);
    await page.getByRole("link", { name: "Fictional ledger", exact: true }).click();

    const row = page.locator(`#dict-source-${sourceId}`);
    await expect(row).toBeVisible({ timeout: 15_000 });
    await expect(row).toBeInViewport();
    await expect(row).toBeFocused({ timeout: 15_000 });
    await expect(row).toContainText("▲");
    await expect(page.locator(".dict-src-breadcrumb .dict-src-crumb.active")).toHaveText("Titled exactly “FICTIONAL LEDGER”");
});

test("shows the selected person's badges and each tagged event's in the events panel", async ({ page, request, tree }) => {
    const personTag = await createTag(request, tree.treeId, "Fictional panel person");
    await tagPerson(request, tree.treeId, personTag, tree.anchorId);
    const eventTag = await createTag(request, tree.treeId, "Fictional panel event");
    const profile = (await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/profiles/${tree.anchorId}`)).json()) as {
        birth: { event_id: string };
    };
    const put = await request.put(`${apiUrl}/api/v1/trees/${tree.treeId}/tags/${eventTag}/records/event/${profile.birth.event_id}`);
    expect(put.status()).toBe(204);

    await page.goto(`/trees/${tree.treeId}`);
    const panel = page.locator(".ev-panel");
    await expect(panel.locator(".evp-name .tag-badge").first()).toHaveAttribute("title", "Fictional panel person");
    const taggedEvent = panel.locator(".ev-item").filter({ has: page.locator(".tag-badge[title='Fictional panel event']") });
    await expect(taggedEvent).toHaveCount(1);
    await expect(taggedEvent.locator(".tag-badge")).toHaveCount(1);
});

test("shows a person's twelve long tags as three badges and a +9 without changing row heights", async ({ page, request, tree }) => {
    const searchRow = () => page.locator(".search-person-entry").filter({ hasText: "Anchor" }).first();
    const birthRow = () => page.locator("li[id^='pd-event-']").filter({ hasText: "Birth" }).first();
    const header = () => page.locator(".pd-header-actions .tag-badges.is-wrap");

    // The untagged heights are the reference: the badges must not change them.
    await page.goto(`/trees/${tree.treeId}/search?last=Ashdown&first=&origin=`);
    await expect(searchRow()).toBeVisible();
    const searchBefore = (await searchRow().boundingBox())?.height ?? 0;
    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    await expect(birthRow()).toBeVisible();
    const birthBefore = (await birthRow().boundingBox())?.height ?? 0;

    // Twelve tags of long labels, on the person and on the birth.
    const profile = (await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/profiles/${tree.anchorId}`)).json()) as {
        birth: { event_id: string };
    };
    for (let index = 1; index <= 12; index += 1) {
        const tagId = await createTag(request, tree.treeId, `Fictional long badge label number ${index}`);
        await tagPerson(request, tree.treeId, tagId, tree.anchorId);
        const put = await request.put(`${apiUrl}/api/v1/trees/${tree.treeId}/tags/${tagId}/records/event/${profile.birth.event_id}`);
        expect(put.status()).toBe(204);
    }

    await page.goto(`/trees/${tree.treeId}/search?last=Ashdown&first=&origin=`);
    await expect(searchRow()).toBeVisible();
    await expect(searchRow().locator(".tag-badge")).toHaveCount(3);
    await expect(searchRow().locator(".tag-badge-more")).toHaveText("+9");
    const searchAfter = (await searchRow().boundingBox())?.height ?? 0;
    expect(Math.abs(searchAfter - searchBefore)).toBeLessThan(1);

    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    await expect(birthRow()).toBeVisible();
    await expect(birthRow().locator(".tag-badge")).toHaveCount(3);
    await expect(birthRow().locator(".tag-badge-more")).toHaveText("+9");
    const birthAfter = (await birthRow().boundingBox())?.height ?? 0;
    expect(Math.abs(birthAfter - birthBefore)).toBeLessThan(1);

    // The header shows two badges in at most two lines, then a toggle below them.
    await expect(header().locator(".tag-badge")).toHaveCount(2);
    const rowHeight = (await header().boundingBox())?.height ?? 0;
    expect(rowHeight).toBeLessThan(60);
    const toggle = page.getByRole("button", { name: "+10 more", exact: true });
    await expect(toggle).toHaveAttribute("aria-expanded", "false");
    await toggle.click();
    await expect(header().locator(".tag-badge")).toHaveCount(12);
    await expect(page.getByRole("button", { name: "Show fewer tags", exact: true })).toHaveAttribute("aria-expanded", "true");

    // At a phone width the toggle stays visible and clickable, collapsed and expanded.
    await page.setViewportSize({ width: 360, height: 800 });
    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    await expect(header().locator(".tag-badge")).toHaveCount(2);
    const phoneToggle = page.getByRole("button", { name: "+10 more", exact: true });
    await expect(phoneToggle).toBeVisible();
    await expect(phoneToggle).toBeInViewport();
    await phoneToggle.click();
    await expect(header().locator(".tag-badge")).toHaveCount(12);
    await expect(page.getByRole("button", { name: "Show fewer tags", exact: true })).toBeVisible();

    // At a desktop width the badges sit in the trailing column, between the
    // SOSA badges and the buttons, and the name keeps its own box.
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    await expect(header().locator(".tag-badge")).toHaveCount(2);
    const boxOf = async (selector: string) => (await page.locator(selector).first().boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
    const sosaBox = await boxOf(".pd-header-sosa");
    const chipsBox = await boxOf(".pd-header-actions .tag-badges-header");
    const buttonsBox = await boxOf(".pd-header-buttons");
    const nameBox = await boxOf(".pd-header-main h1");
    const actionsBox = await boxOf(".pd-header-actions");
    expect(chipsBox.y).toBeGreaterThanOrEqual(sosaBox.y + sosaBox.height - 1);
    expect(chipsBox.y + chipsBox.height).toBeLessThanOrEqual(buttonsBox.y + 1);
    expect(nameBox.x + nameBox.width).toBeLessThanOrEqual(actionsBox.x + 1);

    // No page scrolls sideways, at a phone width or a desktop one.
    for (const viewport of [
        { width: 360, height: 800 },
        { width: 1280, height: 800 },
    ]) {
        await page.setViewportSize(viewport);
        for (const path of [`/trees/${tree.treeId}/search?last=Ashdown&first=&origin=`, `/trees/${tree.treeId}/persons/${tree.anchorId}`]) {
            await page.goto(path);
            await page.waitForLoadState("networkidle");
            const scroll = await page.evaluate(() => [document.documentElement.scrollWidth, document.documentElement.clientWidth]);
            expect(scroll[0], `sideways scroll on ${path} at ${viewport.width}px`).toBeLessThanOrEqual(scroll[1] + 1);
        }
    }
});

/// The classes and the computed styles that decide how a dictionary element
/// looks, read from the page: what a row or a usage list is made of.
async function looksOf(element: Locator) {
    return element.evaluate((node) => {
        const style = getComputedStyle(node);
        const properties = [
            "display", "padding", "margin", "gap", "align-items", "color", "background-color",
            "border-top-color", "border-top-width", "border-radius", "font-size", "font-weight",
            "font-family", "text-decoration-line", "cursor",
        ];
        const computed: Record<string, string> = {};
        for (const property of properties) computed[property] = style.getPropertyValue(property);
        return { tag: node.tagName, classes: [...node.classList].sort(), computed };
    });
}

test("removes a record from a tag's unfolded list from the keyboard, and drops the kind when none is left", async ({ page, request, tree }) => {
    const tagId = await createTag(request, tree.treeId, "Fictional kin");
    await tagPerson(request, tree.treeId, tagId, tree.anchorId);
    await tagPerson(request, tree.treeId, tagId, await personIdOf(request, tree.treeId, "Dorian"));

    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Tags", exact: true }).click();
    const row = page.locator(".dict-row").filter({ hasText: "Fictional kin" });
    await expect(row.locator(".dict-row-count")).toHaveText("2 persons");
    await row.click();
    const dorian = page.getByRole("link", { name: /Dorian/ });
    const anchor = page.getByRole("link", { name: /Anchor/ });
    await expect(dorian).toBeVisible();
    await expect(anchor).toBeVisible();
    // Each person's line is the usage list's: the name in capitals, then the lifespan.
    await expect(dorian.locator(".dict-accordion-name")).toContainText("ASHDOWN Dorian");
    await expect(page.locator(".dict-accordion-group").filter({ hasText: "Persons" })).toHaveCount(1);

    // Tab from the person's link reaches its remove button; Enter takes it off.
    const removeDorian = page.getByRole("button", { name: /^Remove .*Dorian.* from Fictional kin$/ });
    const removeAnchor = page.getByRole("button", { name: /^Remove .*Anchor.* from Fictional kin$/ });
    await dorian.focus();
    await page.keyboard.press("Tab");
    await expect(removeDorian).toBeFocused();
    await page.keyboard.press("Enter");

    // The line goes, the count reads one person, and the focus moves to the
    // other person's remove button.
    await expect(dorian).toHaveCount(0);
    await expect(row.locator(".dict-row-count")).toHaveText("1 person");
    await expect(removeAnchor).toBeFocused();
    const kept = await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/tags/${tagId}/records?kind=person&first=10`);
    const keptPage = await kept.json();
    expect(keptPage.total_count).toBe(1);
    expect(keptPage.edges[0].node.id).toBe(tree.anchorId);

    // Removing the last record drops the Persons group, and the row says so.
    await page.keyboard.press("Enter");
    await expect(anchor).toHaveCount(0);
    await expect(page.locator(".dict-accordion-group")).toHaveCount(0);
    await expect(row.locator(".dict-row-count")).toHaveText("No record yet");
    await expect(page.getByText("No record yet", { exact: true }).last()).toBeVisible();
    const emptied = await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/tags/${tagId}/records?kind=person&first=10`)).json();
    expect(emptied.total_count).toBe(0);

    // The tag itself stays in the vocabulary.
    const tags = (await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/tags`)).json()) as Array<{ id: string }>;
    expect(tags.map((tag) => tag.id)).toContain(tagId);
});

test("brings a line back with the error and the focus when its removal is refused, and shows a record put back after a removal", async ({ page, request, tree }) => {
    const tagId = await createTag(request, tree.treeId, "Fictional refusal");
    await tagPerson(request, tree.treeId, tagId, tree.anchorId);
    await tagPerson(request, tree.treeId, tagId, await personIdOf(request, tree.treeId, "Dorian"));

    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Tags", exact: true }).click();
    const row = page.locator(".dict-row").filter({ hasText: "Fictional refusal" });
    await row.click();
    const dorian = page.getByRole("link", { name: /Dorian/ });
    const anchor = page.getByRole("link", { name: /Anchor/ });
    const removeAnchor = page.getByRole("button", { name: /^Remove .*Anchor.* from Fictional refusal$/ });
    const removeDorian = page.getByRole("button", { name: /^Remove .*Dorian.* from Fictional refusal$/ });
    await expect(anchor).toBeVisible();

    // The server refuses the removal: the line leaves at once, then comes
    // back with the error under the kind's heading, and the focus returns to it.
    test.info().annotations.push({ type: expectedConsoleError, description: "status of 500" });
    const refusedUrl = `**/api/v1/trees/${tree.treeId}/tags/${tagId}/records/person/${tree.anchorId}`;
    await page.route(refusedUrl, async (route) => {
        if (route.request().method() !== "DELETE") {
            await route.fallback();
            return;
        }
        await route.fulfill({
            status: 500,
            headers: { "access-control-allow-origin": "*" },
            contentType: "application/json",
            body: JSON.stringify({ error: { code: "internal", message: "refused" } }),
        });
    });
    await removeAnchor.click();
    await expect(anchor).toBeVisible();
    await expect(row.locator("xpath=..").locator(".dict-accordion").getByRole("alert")).toHaveText("The tag could not be saved. Try again.");
    await expect(removeAnchor).toBeFocused();
    await expect(row.locator(".dict-row-count")).toHaveText("2 persons");
    const kept = await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/tags/${tagId}/records?kind=person&first=10`)).json();
    expect(kept.total_count).toBe(2);

    // Once the server accepts it, the line goes; a record put back on the tag
    // meanwhile shows again when the next removal reads the list.
    await page.unroute(refusedUrl);
    await removeAnchor.click();
    await expect(anchor).toHaveCount(0);
    await expect(row.locator("xpath=..").locator(".dict-accordion").getByRole("alert")).toHaveCount(0);
    await tagPerson(request, tree.treeId, tagId, tree.anchorId);
    await removeDorian.click();
    await expect(dorian).toHaveCount(0);
    await expect(anchor).toBeVisible();
    await expect(row.locator(".dict-row-count")).toHaveText("1 person");
});

test("draws the Tags tab's rows and unfolded lists with the Occupations tab's classes and styles", async ({ page, request, tree }) => {
    const tagId = await createTag(request, tree.treeId, "Fictional looks");
    await tagPerson(request, tree.treeId, tagId, tree.anchorId);
    await tagPerson(request, tree.treeId, tagId, await personIdOf(request, tree.treeId, "Dorian"));

    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Occupations", exact: true }).click();
    const occupation = page.locator(".dict-row").filter({ hasText: "Weaver" });
    await occupation.click();
    const occupationList = occupation.locator("xpath=..").locator(".dict-accordion");
    await expect(occupationList.locator("a.dict-accordion-item").first()).toBeVisible();
    await page.mouse.move(0, 0);
    const occupationLooks = {
        row: await looksOf(occupation),
        value: await looksOf(occupation.locator(".dict-row-value")),
        main: await looksOf(occupation.locator(".dict-row-main")),
        count: await looksOf(occupation.locator(".dict-row-count")),
        chevron: await looksOf(occupation.locator(".dict-row-chevron")),
        list: await looksOf(occupationList),
        item: await looksOf(occupationList.locator("a.dict-accordion-item").first()),
        name: await looksOf(occupationList.locator("a.dict-accordion-item").first().locator(".dict-accordion-name")),
        dates: await looksOf(occupationList.locator("a.dict-accordion-item").first().locator(".dict-accordion-dates")),
    };

    await page.getByRole("tab", { name: "Tags", exact: true }).click();
    const tag = page.locator(".dict-row").filter({ hasText: "Fictional looks" });
    await tag.click();
    const tagList = tag.locator("xpath=..").locator(".dict-accordion");
    const dorian = tagList.locator("a.dict-accordion-item", { hasText: "Dorian" });
    await expect(dorian).toBeVisible();
    await page.mouse.move(0, 0);
    const tagLooks = {
        row: await looksOf(tag),
        value: await looksOf(tag.locator(".dict-row-value")),
        main: await looksOf(tag.locator(".dict-row-main")),
        count: await looksOf(tag.locator(".dict-row-count")),
        chevron: await looksOf(tag.locator(".dict-row-chevron")),
        list: await looksOf(tagList),
        item: await looksOf(dorian),
        name: await looksOf(dorian.locator(".dict-accordion-name")),
        dates: await looksOf(dorian.locator(".dict-accordion-dates")),
    };

    expect(tagLooks.row).toEqual(occupationLooks.row);
    expect(tagLooks.value).toEqual(occupationLooks.value);
    expect(tagLooks.main).toEqual(occupationLooks.main);
    expect(tagLooks.count).toEqual(occupationLooks.count);
    expect(tagLooks.chevron).toEqual(occupationLooks.chevron);
    expect(tagLooks.list).toEqual(occupationLooks.list);
    expect(tagLooks.item).toEqual(occupationLooks.item);
    expect(tagLooks.name).toEqual(occupationLooks.name);
    expect(tagLooks.dates).toEqual(occupationLooks.dates);
});

test("puts the colour dot inline before the label, and asks before the tag is deleted", async ({ page, request, tree }) => {
    const tagId = await createTag(request, tree.treeId, "Fictional dot");
    await tagPerson(request, tree.treeId, tagId, tree.anchorId);

    await page.goto(`/trees/${tree.treeId}/dictionary/tags/${tagId}`);
    const row = page.locator(".dict-row").filter({ hasText: "Fictional dot" });
    await expect(row).toBeVisible();

    // The dot sits on the label's line, before the label, the way a badge's does.
    const dot = await row.locator(".tag-dot").boundingBox();
    const label = await row.locator(".dict-row-value").boundingBox();
    expect(dot, "the colour dot").not.toBeNull();
    expect(label, "the label").not.toBeNull();
    const dotCentre = dot!.y + dot!.height / 2;
    const labelCentre = label!.y + label!.height / 2;
    expect(Math.abs(dotCentre - labelCentre)).toBeLessThan(label!.height / 2);
    expect(dot!.x + dot!.width).toBeLessThanOrEqual(label!.x + 1);

    // Delete is in the row's ⋮ menu, and it asks before it deletes.
    await row.getByRole("button", { name: /^Actions for / }).click();
    await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Delete the tag" });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    await expect(dialog).toBeHidden();
    await expect(row).toBeVisible();
    const tags = (await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/tags`)).json()) as Array<{ id: string }>;
    expect(tags.map((tag) => tag.id)).toContain(tagId);
});

test("lists a tagged event with its translated type, date, place, description and persons, and × takes it off", async ({ page, request, tree }) => {
    const created = async (path: string, data: object): Promise<string> => {
        const response = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}${path}`, { data });
        expect(response.ok(), await response.text()).toBeTruthy();
        return (await response.json()).id as string;
    };
    const tagId = await createTag(request, tree.treeId, "Fictional ledger");
    const dorian = await personIdOf(request, tree.treeId, "Dorian");
    const bernard = await personIdOf(request, tree.treeId, "Bernard");
    const place = await created("/places", { name: "Fictionville" });
    const occupation = await created("/events", {
        event_type: "occupation",
        person_id: dorian,
        date_value: "1938",
        date_qualifier: "exact",
        place_id: place,
        description: "Fictional inspector",
    });
    const family = await created("/families", {});
    await created(`/families/${family}/spouses`, { person_id: bernard, role: "husband", sort_order: 0 });
    await created(`/families/${family}/spouses`, { person_id: dorian, role: "wife", sort_order: 1 });
    const marriage = await created("/events", { event_type: "marriage", family_id: family, date_value: "1860", date_qualifier: "exact" });
    for (const event of [occupation, marriage]) {
        const put = await request.put(`${apiUrl}/api/v1/trees/${tree.treeId}/tags/${tagId}/records/event/${event}`);
        expect(put.status()).toBe(204);
    }

    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Tags", exact: true }).click();
    await page.locator(".dict-row").filter({ hasText: "Fictional ledger" }).click();

    // Each event line names its type translated, its date and place, its
    // description, then the person, or both spouses of the couple, with the
    // lifespan a person line shows. No raw enum value is listed.
    const occupationLine = page.locator(".dict-accordion-line").filter({ hasText: "Fictional inspector" });
    const marriageLine = page.locator(".dict-accordion-line").filter({ hasText: "Marriage" });
    await expect(occupationLine).toContainText("Occupation — Fictional inspector · 1938 · Fictionville");
    await expect(occupationLine).toContainText("ASHDOWN Dorian");
    await expect(occupationLine.locator(".dict-accordion-dates")).toHaveText("1840-1900");
    await expect(marriageLine).toContainText("Marriage · 1860");
    await expect(marriageLine.locator(".dict-accordion-name")).toHaveCount(3);
    await expect(marriageLine).toContainText("ASHDOWN Bernard");
    await expect(marriageLine).toContainText("ASHDOWN Dorian");
    await expect(page.locator(".dict-accordion-group")).not.toContainText(/occupation|marriage/);

    // The × takes the event off the tag, named by the line's own text.
    await page.getByRole("button", { name: "Remove Occupation — Fictional inspector · 1938 · Fictionville from Fictional ledger", exact: true }).click();
    await expect(occupationLine).toHaveCount(0);
    await expect(marriageLine).toBeVisible();
});

test("a dictionary source row shows its ⋮ without hover and opens the same tag menu", async ({ page, request, tree }) => {
    await createTag(request, tree.treeId, "Fictional trigger");

    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Sources", exact: true }).click();
    const source = page.locator(".dict-row").filter({ hasText: "Parish register 0" }).first();
    await expect(source).toBeVisible();

    // The trigger is visible while the pointer is away from the row.
    await page.mouse.move(0, 0);
    const trigger = source.getByRole("button", { name: /^Actions for / });
    await expect(trigger).toBeVisible();
    await expect(trigger).toHaveAttribute("aria-haspopup", "menu");
    await expect(trigger).toHaveAttribute("aria-expanded", "false");

    // The menu holds Edit, then Tags…, in the order every record menu uses.
    await trigger.click();
    await expect(trigger).toHaveAttribute("aria-expanded", "true");
    const menu = page.locator(".record-tag-menu");
    expect(await menu.getByRole("menuitem").allInnerTexts()).toEqual(["Edit", "Tags…"]);
    const editEntry = menu.getByRole("menuitem", { name: "Edit", exact: true });
    await expect(editEntry).toBeFocused();
    const tagsEntry = menu.getByRole("menuitem", { name: "Tags…", exact: true });
    await page.keyboard.press("Escape");
    await expect(tagsEntry).toBeHidden();
    await expect(trigger).toBeFocused();
    await expect(source.locator(".dict-row-action")).toHaveCount(0);
    await expect(source.locator(".tag-badge")).toHaveCount(0);
});

test("a tag row's ⋮ and its right click, Shift+F10 and long press open the same Edit and Delete menu", async ({ page, request, tree }) => {
    await createTag(request, tree.treeId, "Fictional tag menu");

    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Tags", exact: true }).click();
    const row = page.locator(".dict-row").filter({ hasText: "Fictional tag menu" }).first();
    await expect(row).toBeVisible();

    await page.mouse.move(0, 0);
    const trigger = row.getByRole("button", { name: "Actions for Fictional tag menu", exact: true });
    await expect(trigger).toBeVisible();
    await trigger.click();
    await expect(trigger).toHaveAttribute("aria-expanded", "true");
    const menu = page.locator(".menu-button-wrap .context-menu");
    const fromTrigger = await menu.getByRole("menuitem").allInnerTexts();
    expect(fromTrigger).toEqual(["Edit", "Delete"]);
    await page.keyboard.press("Escape");
    await expect(trigger).toBeFocused();

    await row.click({ button: "right" });
    await expect(menu).toBeVisible();
    expect(await menu.getByRole("menuitem").allInnerTexts()).toEqual(fromTrigger);
    await page.keyboard.press("Escape");
    await expect(menu).toBeHidden();

    await row.focus();
    await page.keyboard.press("Shift+F10");
    await expect(menu).toBeVisible();
    expect(await menu.getByRole("menuitem").allInnerTexts()).toEqual(fromTrigger);
    await page.keyboard.press("Escape");
    await expect(menu).toBeHidden();
});

test("a family name's ⋮ opens its Edit menu, as a right click does", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Family Names", exact: true }).click();
    const row = page.locator(".dict-row").filter({ hasText: /ashdown/i }).first();
    await expect(row).toBeVisible();

    await page.mouse.move(0, 0);
    const trigger = row.getByRole("button", { name: /^Actions for / });
    await expect(trigger).toBeVisible();
    await trigger.click();
    const menu = page.locator(".menu-button-wrap .context-menu");
    const fromTrigger = await menu.getByRole("menuitem").allInnerTexts();
    expect(fromTrigger).toEqual(["Edit"]);
    await page.keyboard.press("Escape");
    await expect(trigger).toBeFocused();

    await row.click({ button: "right" });
    await expect(menu).toBeVisible();
    expect(await menu.getByRole("menuitem").allInnerTexts()).toEqual(fromTrigger);
    await page.keyboard.press("Escape");
    await expect(menu).toBeHidden();
});

test("a dictionary place and repository row show their ⋮ without hover, and it opens the menu a right click opens", async ({ page, request, tree }) => {
    await createTag(request, tree.treeId, "Fictional place trigger");
    const repository = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}/repositories`, {
        data: { name: "Fictional trigger archive" },
    });
    expect(repository.status()).toBe(201);

    const rows: Array<[tab: string, text: string]> = [
        ["Places", "Northfield"],
        ["Repositories", "Fictional trigger archive"],
    ];
    for (const [tab, text] of rows) {
        await page.goto(`/trees/${tree.treeId}/dictionary`);
        await page.getByRole("tab", { name: tab, exact: true }).click();
        const row = page.locator(".dict-row").filter({ hasText: text }).first();
        await expect(row).toBeVisible();

        // The trigger is visible while the pointer is away from the row.
        await page.mouse.move(0, 0);
        const trigger = row.getByRole("button", { name: /^Actions for / });
        await expect(trigger).toBeVisible();
        await expect(trigger).toHaveAttribute("aria-expanded", "false");

        // The ⋮ opens the tag menu, and a right click on the row opens the same entries.
        await trigger.click();
        await expect(trigger).toHaveAttribute("aria-expanded", "true");
        const menu = page.locator(".record-tag-menu");
        const fromTrigger = await menu.getByRole("menuitem").allInnerTexts();
        await page.keyboard.press("Escape");
        await expect(menu).toBeHidden();
        await expect(trigger).toBeFocused();

        await row.click({ button: "right" });
        await expect(menu).toBeVisible();
        expect(await menu.getByRole("menuitem").allInnerTexts()).toEqual(fromTrigger);
        await page.keyboard.press("Escape");
        await expect(menu).toBeHidden();
    }
});

test("a profile event shows its ⋮ and the same tag menu as Shift+F10", async ({ page, request, tree }) => {
    await createTag(request, tree.treeId, "Fictional menu twin");

    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    const birth = page.locator("li[id^='pd-event-']").filter({ hasText: "Birth" }).first();
    await expect(birth).toBeVisible();

    await page.mouse.move(0, 0);
    const trigger = birth.getByRole("button", { name: "Actions for Birth", exact: true });
    await expect(trigger).toBeVisible();

    // The ⋮ opens the menu that Shift+F10 opens on the same row.
    await trigger.click();
    const tagsEntry = page.locator(".record-tag-menu").getByRole("menuitem", { name: "Tags…", exact: true });
    await expect(tagsEntry).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(trigger).toBeFocused();

    await birth.focus();
    await page.keyboard.press("Shift+F10");
    await expect(tagsEntry).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(birth).toBeFocused();
});

test("the pedigree asks for the tag vocabulary only when its highlight list opens", async ({ page, request, tree }) => {
    const tagId = await createTag(request, tree.treeId, "Fictional mark");
    await tagPerson(request, tree.treeId, tagId, tree.anchorId);
    const asked = tagVocabularyRequests(page, tree.treeId);

    await page.goto(`/trees/${tree.treeId}`);
    await expect(page.locator("g.ped-card").first()).toBeVisible();
    expect(asked).toEqual([]);

    const dialog = page.getByRole("dialog", { name: "Highlight a tag" });
    await page.getByRole("button", { name: "Highlight a tag", exact: true }).click();
    await expect(dialog.locator(".tag-highlight-option", { hasText: "Fictional mark" })).toBeVisible();
    expect(asked).toHaveLength(1);
    // Opening the list again reads nothing: the vocabulary stays for the page.
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await page.getByRole("button", { name: "Highlight a tag", exact: true }).click();
    await expect(dialog.locator(".tag-highlight-option", { hasText: "Fictional mark" })).toBeVisible();
    expect(asked).toHaveLength(1);
});

test("a pedigree opened with a highlight from its history draws the marks in the tag's colour", async ({ page, request, tree }) => {
    const tagId = await createTag(request, tree.treeId, "Fictional mark");
    await tagPerson(request, tree.treeId, tagId, tree.anchorId);
    const anchor = page.locator("g.ped-card").filter({ hasText: /Anchor\s*ASHDOWN/ }).first();

    // The route names the root, so the entry it is left on keeps the highlight:
    // a route rewritten on load would clear the entry's saved view.
    await page.goto(`/trees/${tree.treeId}?person=${tree.anchorId}`);
    await expect(anchor).toBeVisible();
    await page.getByRole("button", { name: "Highlight a tag", exact: true }).click();
    await page
        .getByRole("dialog", { name: "Highlight a tag" })
        .locator(".tag-highlight-option", { hasText: "Fictional mark" })
        .click();
    await expect(anchor.locator(".ped-tag-dot")).toHaveCount(1);

    await page.getByRole("button", { name: "Statistics", exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/trees/${tree.treeId}/statistics$`));
    await page.goBack();
    await expect(page).toHaveURL(new RegExp(`/trees/${tree.treeId}(\\?|$)`));

    // The restored highlight needs the tag's colour at once: the chip, the
    // carriers' marks and their colour come back without any further choice.
    await expect(page.locator(".tag-highlight-chip")).toContainText("Fictional mark");
    await expect(anchor.locator(".ped-tag-dot")).toHaveCount(1);
    const tags = (await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/tags`)).json()) as Array<{ id: string; color: number }>;
    const color = tags.find((tag) => tag.id === tagId)?.color;
    await expect(anchor.locator(".ped-tag-dot")).toHaveAttribute("style", new RegExp(`fill:var\\(--tag-${color}\\)`));
    await expect(page.locator("g.ped-card").filter({ hasText: /Dorian\s*ASHDOWN/ }).first()).toHaveClass(/ped-tag-dim/);
});

test("a restored highlight draws no mark in a fallback colour while the tag's colour is unread", async ({ page, request, tree }) => {
    test.setTimeout(120_000);
    const tagId = await createTag(request, tree.treeId, "Fictional mark");
    await tagPerson(request, tree.treeId, tagId, tree.anchorId);
    const anchor = page.locator("g.ped-card").filter({ hasText: /Anchor\s*ASHDOWN/ }).first();

    await page.goto(`/trees/${tree.treeId}?person=${tree.anchorId}`);
    await expect(anchor).toBeVisible();
    await page.getByRole("button", { name: "Highlight a tag", exact: true }).click();
    await page
        .getByRole("dialog", { name: "Highlight a tag" })
        .locator(".tag-highlight-option", { hasText: "Fictional mark" })
        .click();
    await expect(anchor.locator(".ped-tag-dot")).toHaveCount(1);

    // Leave the page on its history entry, then let the API client's 30 s
    // response cache expire, so going back reads the vocabulary again.
    await page.getByRole("button", { name: "Statistics", exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/trees/${tree.treeId}/statistics$`));
    await page.waitForTimeout(31_000);

    // Hold the vocabulary of the restored page while its carriers come back:
    // the marks must wait for the tag's colour rather than take a fallback one.
    let arrived!: () => void;
    const vocabularyAsked = new Promise<void>((resolve) => (arrived = resolve));
    let release!: () => void;
    const released = new Promise<void>((resolve) => (release = resolve));
    await page.route(`**/api/v1/trees/${tree.treeId}/tags`, async (route) => {
        arrived();
        await released;
        await route.continue();
    });
    const carriersRead = page.waitForResponse((res) => res.url().includes(`/tags/${tagId}/records`) && res.ok());

    await page.goBack();
    await expect(page).toHaveURL(new RegExp(`/trees/${tree.treeId}(\\?|$)`));
    await vocabularyAsked;
    await carriersRead;
    await expect(anchor.locator(".ped-tag-dot")).toHaveCount(0);
    await expect(page.locator(".ped-tag-dot")).toHaveCount(0);

    release();
    await expect(page.locator(".tag-highlight-chip")).toContainText("Fictional mark");
    const tags = (await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/tags`)).json()) as Array<{ id: string; color: number }>;
    const color = tags.find((tag) => tag.id === tagId)?.color;
    await expect(anchor.locator(".ped-tag-dot")).toHaveCount(1);
    await expect(anchor.locator(".ped-tag-dot")).toHaveAttribute("style", new RegExp(`fill:var\\(--tag-${color}\\)`));
});

test("the highlight list reads the vocabulary again after a refused read, and not after a good one", async ({ page, request, tree }) => {
    const tagId = await createTag(request, tree.treeId, "Fictional mark");
    await tagPerson(request, tree.treeId, tagId, tree.anchorId);
    const asked = tagVocabularyRequests(page, tree.treeId);

    // The first read is refused: the list says so, and the next opening asks again.
    test.info().annotations.push({ type: expectedConsoleError, description: "status of 500" });
    let refusals = 0;
    await page.route(`**/api/v1/trees/${tree.treeId}/tags`, async (route) => {
        if (route.request().method() !== "GET" || refusals++ > 0) {
            await route.fallback();
            return;
        }
        await route.fulfill({
            status: 500,
            headers: { "access-control-allow-origin": "*" },
            contentType: "application/json",
            body: JSON.stringify({ error: { code: "internal", message: "refused" } }),
        });
    });

    await page.goto(`/trees/${tree.treeId}`);
    await expect(page.locator("g.ped-card").first()).toBeVisible();
    const dialog = page.getByRole("dialog", { name: "Highlight a tag" });
    await page.getByRole("button", { name: "Highlight a tag", exact: true }).click();
    await expect(dialog.getByRole("alert")).toHaveText("The tag could not be saved. Try again.");
    expect(asked).toHaveLength(1);
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();

    await page.getByRole("button", { name: "Highlight a tag", exact: true }).click();
    await expect(dialog.locator(".tag-highlight-option", { hasText: "Fictional mark" })).toBeVisible();
    expect(asked).toHaveLength(2);
    // A good read is kept: opening the list again reads nothing.
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await page.getByRole("button", { name: "Highlight a tag", exact: true }).click();
    await expect(dialog.locator(".tag-highlight-option", { hasText: "Fictional mark" })).toBeVisible();
    expect(asked).toHaveLength(2);
});

test("a restored highlight whose tag read fails says the tag is unavailable, draws no mark, and is marked once read again", async ({ page, request, tree }) => {
    test.setTimeout(120_000);
    const tagId = await createTag(request, tree.treeId, "Fictional mark");
    await tagPerson(request, tree.treeId, tagId, tree.anchorId);
    const anchor = page.locator("g.ped-card").filter({ hasText: /Anchor\s*ASHDOWN/ }).first();

    await page.goto(`/trees/${tree.treeId}?person=${tree.anchorId}`);
    await expect(anchor).toBeVisible();
    await page.getByRole("button", { name: "Highlight a tag", exact: true }).click();
    await page
        .getByRole("dialog", { name: "Highlight a tag" })
        .locator(".tag-highlight-option", { hasText: "Fictional mark" })
        .click();
    await expect(anchor.locator(".ped-tag-dot")).toHaveCount(1);

    // Leave on the history entry, let the API client's cache expire, and refuse
    // the vocabulary on the way back.
    await page.getByRole("button", { name: "Statistics", exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/trees/${tree.treeId}/statistics$`));
    await page.waitForTimeout(31_000);
    test.info().annotations.push({ type: expectedConsoleError, description: "status of 500" });
    await page.route(`**/api/v1/trees/${tree.treeId}/tags`, async (route) => {
        if (route.request().method() !== "GET") {
            await route.fallback();
            return;
        }
        await route.fulfill({
            status: 500,
            headers: { "access-control-allow-origin": "*" },
            contentType: "application/json",
            body: JSON.stringify({ error: { code: "internal", message: "refused" } }),
        });
    });
    await page.goBack();
    await expect(page).toHaveURL(new RegExp(`/trees/${tree.treeId}(\\?|$)`));

    // The chip says the tag is unavailable and no card is marked: the marks
    // need the tag's colour, which the refused read did not give.
    await expect(page.locator(".tag-highlight-chip")).toContainText("Tag unavailable");
    await expect(page.locator(".ped-tag-dot")).toHaveCount(0);

    // Opening the list asks for the vocabulary again: the chip takes the label
    // and the mark comes back in the tag's colour.
    await page.unroute(`**/api/v1/trees/${tree.treeId}/tags`);
    await page.getByRole("button", { name: "Highlight a tag", exact: true }).click();
    await expect(page.getByRole("dialog", { name: "Highlight a tag" }).locator(".tag-highlight-option", { hasText: "Fictional mark" })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator(".tag-highlight-chip")).toContainText("Fictional mark");
    await expect(anchor.locator(".ped-tag-dot")).toHaveCount(1);
});
