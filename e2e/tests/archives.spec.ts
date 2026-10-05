// OxidGene's viewer over an archive whose images it may show
// (docs/archives.md §6.3, §6.4). No archive portal is contacted: the
// backend's archive-target answer is stubbed in the page, and the archive's
// pictures are served by the test.

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

test("an iiif archive's cited view opens in OxidGene's viewer and attaches as a document", async ({
    page,
    request,
    tree,
}) => {
    // The fixture cites "Parish register 0" on every birth: rewrite it as a
    // normalized citation of the archive.
    const sources = await (
        await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/sources?title=Parish%20register%200`)
    ).json();
    const source = sources.edges[0].node;
    const renamed = await request.put(`${apiUrl}/api/v1/trees/${tree.treeId}/sources/${source.id}`, {
        data: { title: CITATION },
    });
    expect(renamed.ok()).toBeTruthy();

    const asked: Array<number | null> = [];
    await page.route(`${ARCHIVE}/**`, (route) => route.fulfill({ contentType: "image/png", body: PIXEL }));
    await page.route("**/sources/*/archive-target", async (route) => {
        const view = (route.request().postDataJSON()?.view ?? null) as number | null;
        asked.push(view);
        await route.fulfill({ json: target(view ?? 5) });
    });

    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    // Every birth of the timeline cites the register: open the first.
    await page.getByRole("button", { name: CITATION }).first().click();

    const viewer = page.locator(".media-viewer");
    await expect(viewer.locator("img.media-viewer-image")).toHaveAttribute("src", /\/a1\/5\/full\/max\//);
    // The cited half of the double page is marked; the credit links to the
    // archive's reuse terms.
    await expect(viewer.locator(".media-viewer-side.is-right")).toBeVisible();
    await expect(viewer.getByRole("link", { name: "Archives départementales d'Indre-et-Loire, 3E1/2, vue 5" })).toHaveAttribute(
        "href",
        "https://archives.touraine.fr/page/reutilisation",
    );
    await expect(viewer.getByText("View 5 of 13")).toBeVisible();
    await expect(viewer.getByRole("link", { name: "Open on the archive's site" })).toHaveAttribute(
        "href",
        `${ARCHIVE}/ark:/00000/a1/5`,
    );
    expect(asked).toEqual([null]);

    // The next view of the register, resolved on the click, and only then.
    await viewer.getByRole("button", { name: "Next view" }).click();
    await expect(viewer.getByText("View 6 of 13")).toBeVisible();
    await expect(viewer.locator("img.media-viewer-image")).toHaveAttribute("src", /\/a1\/6\/full\/max\//);
    await expect(viewer.locator(".media-viewer-side")).toHaveCount(0);
    // Back to the cited view: already loaded, no request.
    await viewer.getByRole("button", { name: "Previous view" }).click();
    await expect(viewer.getByText("View 5 of 13")).toBeVisible();
    expect(asked).toEqual([null, 6]);

    // Nothing is written until the form is saved.
    await viewer.getByRole("button", { name: "Attach as a document" }).click();
    const form = page.locator(".document-form-modal");
    await expect(form.locator(".form-group", { hasText: "Title" }).locator("input").first()).toHaveValue("3E1/2, view 5");
    const before = await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/media`)).json();
    expect(before.edges).toHaveLength(0);
    await form.getByRole("button", { name: "Save", exact: true }).click();
    await expect(form).toBeHidden();
    await expect(viewer.getByRole("button", { name: "Keep only the act" })).toBeVisible();

    const documents = await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/media`)).json();
    expect(documents.edges).toHaveLength(1);
    const document = documents.edges[0].node;
    expect(document.title).toBe("3E1/2, view 5");
    expect(document.document_category).toBe("civil_record");
    expect(document.source_media_type).toBe("manuscript");
    const pages = await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/media/${document.id}/pages`)).json();
    expect(pages).toHaveLength(1);
    expect(pages[0].file_path).toBe(`${ARCHIVE}/iiif/ark:/00000/a1/5/full/max/0/default.jpg`);
    expect(pages[0].thumbnail_url).toBe(`${ARCHIVE}/images/5_thumbnail.jpg`);
    expect([pages[0].width, pages[0].height]).toEqual([1200, 800]);
    expect(asked).toEqual([null, 6]);
});
