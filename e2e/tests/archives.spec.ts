// A cited register opens on its archive's portal, for every archive alike,
// without the offer to attach its views, which is disabled for now
// (docs/archives.md §6.3); with the offer on, the cited views of an archive
// whose images OxidGene may use attach as a document from beside the source
// (§6.2, §6.4); and the "Find in the archives" dialog completes a citation
// (§6.5). No archive portal is contacted: the backend's archive-target answer
// is stubbed, and the archive's pages and pictures are served by the test.

import type { APIRequestContext, Page } from "@playwright/test";

import { apiUrl, expect, test } from "./fixtures";

/// A fictitious citation of a catalogued `display: "iiif"` archive, naming
/// the right half of view 5 of 13.
const CITATION = "AD37 - Exampleville - (aucun) - N - 1900 - 3E1/2 - vue 5d/13";
const ARCHIVE = "https://archives.example.invalid";

/// The smallest PNG: one transparent pixel.
const PIXEL = Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==",
    "base64",
);

/// The backend's answer for `view`, as the Mnesys adapter shapes it.
function target(view: number) {
    return {
        kind: "view",
        url: `${ARCHIVE}/ark:/00000/a1/${view}`,
        views: [
            {
                view,
                url: `${ARCHIVE}/ark:/00000/a1/${view}`,
                ark: `${ARCHIVE}/ark:/00000/a1/${view}`,
                image: {
                    picture: `${ARCHIVE}/iiif/ark:/00000/a1/${view}/full/max/0/default.jpg`,
                    thumbnail: `${ARCHIVE}/images/${view}_thumbnail.jpg`,
                    width: 1200,
                    height: 800,
                },
            },
        ],
        view_count: 13,
        call_number: "3E1/2",
        attribution: `Archives départementales d'Indre-et-Loire, 3E1/2, vue ${view}`,
    };
}

/// Rewrites "Parish register 0", which the fixture cites on every birth, as
/// a normalized citation of the archive, and stubs the backend's
/// archive-target answer. Returns the views asked for, `null` for the cited
/// one.
async function citeTheArchive(page: Page, request: APIRequestContext, treeId: string) {
    const sources = await (
        await request.get(`${apiUrl}/api/v1/trees/${treeId}/sources?title=Parish%20register%200`)
    ).json();
    const source = sources.edges[0].node;
    const renamed = await request.put(`${apiUrl}/api/v1/trees/${treeId}/sources/${source.id}`, {
        data: { title: CITATION },
    });
    expect(renamed.ok()).toBeTruthy();

    const asked: Array<number | null> = [];
    await page.context().route(`${ARCHIVE}/**`, (route) => route.fulfill({ contentType: "image/png", body: PIXEL }));
    await page.route("**/sources/*/archive-target", async (route) => {
        const view = (route.request().postDataJSON()?.view ?? null) as number | null;
        asked.push(view);
        await route.fulfill({ json: target(view ?? 5) });
    });
    return asked;
}

test("a cited register opens on the portal, with no offer to attach its views", async ({ page, request, tree }) => {
    const asked = await citeTheArchive(page, request, tree.treeId);

    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    // Every birth of the timeline cites the register: the first opens on
    // the portal, in a tab of its own, like any archive's.
    const opened = page.waitForEvent("popup");
    await page.getByRole("button", { name: CITATION }).first().click();
    const tab = await opened;
    await tab.waitForURL(`${ARCHIVE}/ark:/00000/a1/5`);
    expect(asked).toEqual([null]);
    await expect(page.locator(".media-viewer")).toHaveCount(0);
    // Viewing a cited source is not where documents are collected: the
    // offer to attach is off (`ATTACH_OFFERED`).
    await expect(page.locator(".pd-ev-source-link").first()).toBeVisible();
    await expect(page.getByRole("button", { name: "Attach as a document" })).toHaveCount(0);
});

// The offer to attach is off (`ATTACH_OFFERED` in oxidgene-ui's
// archive_viewer, docs/archives.md §6.3): this runs again once it is turned
// on, with the free browsing of the archives (docs/roadmap.md) — the test
// above then fails, as a reminder.
test.skip("with the offer on, an iiif archive's cited view attaches as a document", async ({
    page,
    request,
    tree,
}) => {
    const asked = await citeTheArchive(page, request, tree.treeId);

    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    const opened = page.waitForEvent("popup");
    await page.getByRole("button", { name: CITATION }).first().click();
    await (await opened).waitForURL(`${ARCHIVE}/ark:/00000/a1/5`);
    expect(asked).toEqual([null]);

    // Nothing is written until the form is saved.
    await page.getByRole("button", { name: "Attach as a document" }).first().click();
    const form = page.locator(".document-form-modal");
    await expect(form.locator(".form-group", { hasText: "Title" }).locator("input").first()).toHaveValue("3E1/2, view 5");
    expect(asked).toEqual([null, null]);
    const before = await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/media`)).json();
    expect(before.edges).toHaveLength(0);
    // The next view of the register, resolved on the click, and only then.
    await form.getByRole("button", { name: "Add the next view" }).click();
    await expect(form.locator(".form-group", { hasText: "Title" }).locator("input").first()).toHaveValue("3E1/2, views 5-6");
    expect(asked).toEqual([null, null, 6]);
    await form.getByRole("button", { name: "Save", exact: true }).click();
    await expect(form).toBeHidden();

    const documents = await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/media`)).json();
    expect(documents.edges).toHaveLength(1);
    const document = documents.edges[0].node;
    expect(document.title).toBe("3E1/2, views 5-6");
    expect(document.document_category).toBe("civil_record");
    expect(document.source_media_type).toBe("manuscript");
    const pages = await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/media/${document.id}/pages`)).json();
    expect(pages).toHaveLength(2);
    expect(pages[0].file_path).toBe(`${ARCHIVE}/iiif/ark:/00000/a1/5/full/max/0/default.jpg`);
    expect(pages[0].thumbnail_url).toBe(`${ARCHIVE}/images/5_thumbnail.jpg`);
    expect([pages[0].width, pages[0].height]).toEqual([1200, 800]);
    expect(pages[1].file_path).toBe(`${ARCHIVE}/iiif/ark:/00000/a1/6/full/max/0/default.jpg`);
});

test("a citation missing its locality opens the Find in the archives dialog, which may keep what the reader adds", async ({
    page,
    request,
    tree,
}) => {
    // A baptism without a place, cited from the short form of a catalogued
    // archive: the archive, the kind, the year and the view are known, the
    // locality is not.
    const post = async (path: string, data: object) => {
        const created = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}${path}`, { data });
        expect(created.ok()).toBeTruthy();
        return (await created.json()).id as string;
    };
    const event = await post("/events", { event_type: "baptism", date_value: "1900", person_id: tree.anchorId });
    const source = await post("/sources", { title: "AD37, 3E1/2" });
    const citation = await post("/citations", { source_id: source, event_id: event, page: "vue 5" });

    const asked: unknown[] = [];
    await page.context().route(`${ARCHIVE}/**`, (route) => route.fulfill({ contentType: "image/png", body: PIXEL }));
    await page.route("**/sources/*/archive-target", async (route) => {
        asked.push(route.request().postDataJSON()?.parts ?? null);
        await route.fulfill({ json: target(5) });
    });

    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    await page.getByRole("button", { name: "AD37, 3E1/2 — vue 5" }).click();

    const dialog = page.getByRole("dialog", { name: "Find in the archives" });
    await expect(dialog.locator(".form-group", { hasText: "Kind of register" }).locator("select")).toHaveValue("B");
    await expect(dialog.locator(".form-group", { hasText: "Year" }).locator("input")).toHaveValue("1900");
    await expect(dialog.locator(".form-group", { hasText: "View" }).locator("input")).toHaveValue("5");
    // Without a locality, the search waits for it.
    await dialog.getByRole("button", { name: "Search" }).click();
    await expect(dialog.getByRole("alert")).toBeVisible();
    expect(asked).toEqual([]);

    await dialog.locator(".form-group", { hasText: "Locality" }).locator("input").fill("Exampleville");
    await dialog.getByRole("checkbox", { name: "Add these details to the citation" }).check();
    const opened = page.waitForEvent("popup");
    await dialog.getByRole("button", { name: "Search" }).click();
    await expect(dialog).toBeHidden();

    // The register opens on the portal with the reader's part, and only
    // theirs.
    await (await opened).waitForURL(`${ARCHIVE}/ark:/00000/a1/5`);
    expect(asked).toEqual([{ locality: "Exampleville", act: null, year: null, view: null }]);
    // Kept on the citation, at the end of its page.
    const pageOf = async () => {
        const listed = await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/citations?source_id=${source}`);
        const edges = (await listed.json()).edges as Array<{ node: { id: string; page: string } }>;
        return edges.find((edge) => edge.node.id === citation)?.node.page;
    };
    await expect.poll(pageOf).toBe("vue 5, Exampleville");
});
