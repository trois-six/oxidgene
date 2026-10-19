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

test("tags a note and a place from the keyboard menu, and sees the dots", async ({ page, request, tree }) => {
    await createTag(request, tree.treeId, "Fictional note mark");
    await createTag(request, tree.treeId, "Fictional place mark");
    const note = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}/notes`, {
        data: { text: "Fictional note about the root", person_id: tree.anchorId },
    });
    expect(note.status()).toBe(201);
    const filter = page.getByRole("textbox", { name: "Filter or create a tag" });
    const tagsEntry = page.locator(".record-tag-menu").getByRole("button", { name: "Tags…", exact: true });

    // The note of the person's profile, in the Notes card.
    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    const entry = page.locator("[id^='note-entry-']").filter({ hasText: "Fictional note about the root" }).first();
    await expect(entry).toBeVisible();
    await expect(entry.locator(".tag-dots")).toHaveCount(0);

    await entry.focus();
    await page.keyboard.press("Shift+F10");
    await expect(tagsEntry).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(filter).toBeFocused();
    await page.keyboard.type("Fictional note mark");
    await expect(page.getByRole("option", { name: "Fictional note mark" })).toBeVisible();
    await page.keyboard.press("Enter");
    await expect(entry.locator(".tag-dot-link")).toHaveAttribute("title", "Fictional note mark");
    await page.keyboard.press("Escape");
    await expect(entry).toBeFocused();

    // The place of the fixture, in the Places tab of the dictionary.
    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Places", exact: true }).click();
    const place = page.locator(".dict-row").filter({ hasText: "Northfield" }).first();
    await expect(place).toBeVisible();
    await expect(place.locator(".tag-dot-link")).toHaveCount(0);

    await place.focus();
    await page.keyboard.press("Shift+F10");
    await expect(tagsEntry).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(filter).toBeFocused();
    await page.keyboard.type("Fictional place mark");
    await expect(page.getByRole("option", { name: "Fictional place mark" })).toBeVisible();
    await page.keyboard.press("Enter");
    await expect(place.locator(".tag-dot-link")).toHaveAttribute("title", "Fictional place mark");
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
