import { apiUrl, expect, importGedcom, test } from "./fixtures";

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
    await importGedcom(request, tree.treeId, singletons(30));

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

test("the media tab pages like every other list: numbered, with first and last", async ({ page, request, tree }) => {
    for (let i = 1; i <= 30; i++) {
        const title = `Fictitious scan ${String(i).padStart(2, "0")}`;
        const created = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}/media/document`, { data: { title } });
        expect(created.ok()).toBeTruthy();
    }

    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Media", exact: true }).click();
    const pager = page.locator("nav.pager");
    await expect(pager.getByRole("button", { name: "First page", exact: true })).toBeVisible();
    await expect(pager.getByRole("button", { name: "Last page", exact: true })).toBeVisible();
    await expect(pager.locator(".pager-num.is-current:visible")).toHaveText("1");

    // A page is jumped to, not walked to.
    await pager.getByRole("button", { name: "Page 2", exact: true }).click();
    await expect(pager.locator(".pager-num.is-current:visible")).toHaveText("2");
    await expect(page.getByText("Fictitious scan 30", { exact: true })).toBeVisible();
    await expect(page.getByText("Fictitious scan 01", { exact: true })).toHaveCount(0);
    await pager.getByRole("button", { name: "First page", exact: true }).click();
    await expect(page.getByText("Fictitious scan 01", { exact: true })).toBeVisible();
});

test("an occupation is renamed on every event recording it, from its ⋮", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Occupations", exact: true }).click();
    const row = page.locator(".dict-row").filter({ hasText: "Weaver" }).first();
    await row.getByRole("button", { name: /^Actions for / }).click();
    await page.getByRole("menuitem", { name: "Edit", exact: true }).click();

    const dialog = page.getByRole("dialog", { name: "Edit occupation" });
    await dialog.getByRole("textbox").fill("Fictitious hand weaver");
    await expect(dialog.getByText(/Renames the occupation of/)).toBeVisible();
    await dialog.getByRole("button", { name: "Rename", exact: true }).click();
    await expect(dialog).toBeHidden();
    await expect(page.locator(".dict-row").filter({ hasText: "Fictitious hand weaver" })).toBeVisible();
    await expect(page.locator(".dict-row-value", { hasText: /^Weaver$/ })).toHaveCount(0);
});

test("a place entered twice is merged from its editor, keeping the coordinates only one had", async ({ page, request, tree }) => {
    const created = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}/places`, {
        data: { name: "NORTHFIELD, SHIRE", latitude: 47.25, longitude: -1.5 },
    });
    expect(created.status()).toBe(201);
    const duplicate = (await created.json()).id as string;

    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Places", exact: true }).click();
    const row = page.locator(".dict-row").filter({ hasText: /NORTHFIELD, SHIRE/ });
    await row.getByRole("button", { name: /^Actions for / }).click();
    await page.getByRole("menuitem", { name: "Edit", exact: true }).click();

    const dialog = page.getByRole("dialog", { name: "Edit place" });
    await expect(dialog.getByText(/is another place of the tree/)).toBeVisible();
    // The two already shared the name: merging is the user's choice.
    const toggle = dialog.getByRole("checkbox", { name: /Merge with/ });
    await expect(toggle).not.toBeChecked();
    await toggle.check();
    // The name is picked, the coordinates only this one has are kept.
    await dialog.getByRole("radio").nth(1).check();
    await dialog.getByRole("button", { name: "Merge", exact: true }).click();
    await expect(dialog).toBeHidden();

    await expect(page.locator(".dict-row").filter({ hasText: /northfield, shire/i })).toHaveCount(1);
    expect((await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/places/${duplicate}`)).status()).toBe(404);
    const kept = (await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/places?name=Northfield, Shire`)).json()) as {
        edges: Array<{ node: { name: string; latitude: number | null } }>;
    };
    expect(kept.edges.map((edge) => [edge.node.name, edge.node.latitude])).toEqual([["Northfield, Shire", 47.25]]);
});

test("a repository renamed to another's name is merged into it", async ({ page, request, tree }) => {
    for (const data of [
        { name: "Fictional county archive", address: "1 Imaginary Road" },
        { name: "Old fictional archive", phone: "000 000" },
    ]) {
        const created = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}/repositories`, { data });
        expect(created.status()).toBe(201);
    }

    await page.goto(`/trees/${tree.treeId}/dictionary`);
    await page.getByRole("tab", { name: "Repositories", exact: true }).click();
    const row = page.locator(".dict-row").filter({ hasText: "Old fictional archive" });
    await row.getByRole("button", { name: /^Actions for / }).click();
    await page.getByRole("menuitem", { name: "Edit", exact: true }).click();

    const dialog = page.getByRole("dialog", { name: "Edit repository" });
    await dialog.getByRole("textbox").first().fill("Fictional County Archive");
    // A rename into another's name proposes the merge.
    await expect(dialog.getByRole("checkbox", { name: /Merge with/ })).toBeChecked();
    await dialog.getByRole("button", { name: "Merge", exact: true }).click();
    await expect(dialog).toBeHidden();

    const listed = (await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/repositories`)).json()) as {
        edges: Array<{ node: { name: string; address: string | null; phone: string | null } }>;
    };
    expect(listed.edges.map((edge) => edge.node)).toEqual([
        expect.objectContaining({ name: "Fictional county archive", address: "1 Imaginary Road", phone: "000 000" }),
    ]);
});

test("each tab and each record has an address of its own", async ({ page, request, tree }) => {
    const repository = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}/repositories`, {
        data: { name: "Fictional parish archive" },
    });
    expect(repository.status()).toBe(201);
    const repositoryId = (await repository.json()).id as string;
    const base = `${apiUrl}/api/v1/trees/${tree.treeId}`;
    const sources = (await (await request.get(`${base}/sources`)).json()) as { edges: Array<{ node: { id: string; title: string } }> };
    const sourceId = sources.edges.map((edge) => edge.node).find((source) => source.title === "Parish register 1")?.id;
    expect(sourceId, "the fixture's Parish register 1").toBeTruthy();
    const places = (await (await request.get(`${base}/places?first=100`)).json()) as { edges: Array<{ node: { id: string; name: string } }> };
    const placeId = places.edges.map((edge) => edge.node).find((place) => place.name.includes("Westford"))?.id;
    expect(placeId, "the fixture's Westford").toBeTruthy();

    // A tab's address opens the page on it.
    for (const [path, tab, entry] of [
        ["sources", "Sources", "Parish register 0"],
        ["places", "Places", "Northfield"],
        ["repositories", "Repositories", "Fictional parish archive"],
    ]) {
        await page.goto(`/trees/${tree.treeId}/dictionary/${path}`);
        await expect(page.getByRole("tab", { name: tab, exact: true })).toHaveAttribute("aria-selected", "true");
        await expect(page.locator(".dict-row", { hasText: entry }).first()).toBeVisible();
    }

    // A record's address opens its row, unfolded and focused.
    for (const [path, id, row] of [
        ["sources", sourceId, `dict-source-${sourceId}`],
        ["places", placeId, `dict-place-${placeId}`],
        ["repositories", repositoryId, `dict-repository-${repositoryId}`],
    ]) {
        await page.goto(`/trees/${tree.treeId}/dictionary/${path}/${id}`);
        const opened = page.locator(`#${row}`);
        await expect(opened).toBeFocused();
        await expect(opened).toContainText("▲");
    }
});

test("a family name unfolds on its descent by branches, or on its persons in alphabetical order", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/dictionary`);
    // The row is the control: Enter on it unfolds it.
    await page.locator(".dict-row").filter({ hasText: "Ashdown" }).first().press("Enter");

    const views = page.locator(".dict-descent-views");
    await expect(views.getByRole("tab", { name: "Branches (1)", exact: true })).toHaveAttribute("aria-selected", "true");
    const branches = page.locator(".dict-descent-branch");
    await expect(branches).toHaveCount(1);
    const lines = branches.locator(".dict-descent-line");
    // The oldest bearer, his son, and his son's three children by birth then record order.
    await expect(lines).toHaveCount(5);
    await expect(lines.nth(0)).toContainText("Dorian ASHDOWN");
    await expect(lines.nth(0)).toContainText("1840-1900");
    await expect(lines.nth(0)).toContainText("&1851");
    await expect(lines.nth(0)).toContainText("Alice COLDWELL");
    await expect(lines.nth(1)).toContainText("Bernard ASHDOWN");
    await expect(lines.nth(2)).toContainText("Anchor ASHDOWN");
    await expect(lines.nth(3)).toContainText("Delia ASHDOWN");
    await expect(lines.nth(4)).toContainText("Arthur ASHDOWN");
    // The SOSA root and his ancestors are marked, his wife and sister are not.
    await expect(lines.nth(0).locator(".pd-sosa-mark")).toHaveCount(2);
    await expect(lines.nth(2).locator(".pd-sosa-mark")).toHaveCount(1);
    await expect(lines.nth(3).locator(".pd-sosa-mark")).toHaveCount(0);

    // The alphabetical list is the other view of the same row.
    await views.getByRole("tab", { name: "Alphabetical order", exact: true }).click();
    await expect(page.locator("a.dict-accordion-item")).toHaveCount(5);
    await expect(branches).toHaveCount(0);
    await views.getByRole("tab", { name: "Branches (1)", exact: true }).click();

    // Each name opens the person's page.
    await lines.nth(3).getByRole("link", { name: "Delia ASHDOWN", exact: true }).click();
    await expect(page).toHaveURL(/\/persons\//);
});
