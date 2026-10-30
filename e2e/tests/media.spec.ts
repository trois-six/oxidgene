// The media viewer on an address of its own, /trees/:tree_id/media/:media_id
// (docs/ui-dictionary.md §20): the Tags tab links a document there.

import type { APIRequestContext } from "@playwright/test";

import { apiUrl, createTag, expect, test } from "./fixtures";
import { png } from "./requests";

/// Files a fictitious document of `pages` generated pictures, attached to
/// `personId`, and returns its id.
async function document(request: APIRequestContext, treeId: string, personId: string, title: string, pages: number): Promise<string> {
    const base = `${apiUrl}/api/v1/trees/${treeId}`;
    const created = await request.post(`${base}/media/document`, { data: { title } });
    expect(created.ok(), await created.text()).toBeTruthy();
    const id = (await created.json()).id as string;
    for (let index = 0; index < pages; index++) {
        const uploaded = await request.post(`${base}/media/upload`, {
            multipart: {
                file: { name: `page-${index}.png`, mimeType: "image/png", buffer: png(300, 420, 70 + index) },
                document_id: id,
            },
        });
        expect(uploaded.ok(), await uploaded.text()).toBeTruthy();
    }
    const linked = await request.post(`${base}/media-links`, { data: { media_id: id, person_id: personId } });
    expect(linked.ok(), await linked.text()).toBeTruthy();
    return id;
}

test("a document opened from its address shows its pages, and closing it lands on the dictionary", async ({ page, request, tree }) => {
    const mediaId = await document(request, tree.treeId, tree.anchorId, "Fictitious census sheet", 2);

    await page.goto(`/trees/${tree.treeId}/media/${mediaId}`);
    const viewer = page.locator(".media-viewer");
    await expect(viewer.locator(".cropper-title")).toHaveText("Fictitious census sheet");
    await expect(page.locator("nav.td-bc").getByRole("link", { name: "Dictionary" })).toBeVisible();
    await expect(viewer.getByText("Page 1 of 2")).toBeVisible();
    await expect(viewer.locator(".media-viewer-main img").first()).toBeVisible();

    // The pager turns to the second page.
    await viewer.locator(".media-pager .pager-num", { hasText: "2" }).click();
    await expect(viewer.getByText("Page 2 of 2")).toBeVisible();

    // Opened from an address alone, there is nowhere to go back to.
    await viewer.getByRole("button", { name: "Close" }).first().click();
    await expect(page).toHaveURL(new RegExp(`/trees/${tree.treeId}/dictionary$`));
});

test("a document's link in the Tags tab opens the viewer, and closing it goes back to the tag", async ({ page, request, tree }) => {
    const mediaId = await document(request, tree.treeId, tree.anchorId, "Fictitious school photograph", 1);
    const tagId = await createTag(request, tree.treeId, "Fictional album");
    const put = await request.put(`${apiUrl}/api/v1/trees/${tree.treeId}/tags/${tagId}/records/media/${mediaId}`);
    expect(put.status()).toBe(204);

    await page.goto(`/trees/${tree.treeId}/dictionary/tags/${tagId}`);
    await page.getByRole("link", { name: "Fictitious school photograph" }).click();
    await expect(page).toHaveURL(new RegExp(`/trees/${tree.treeId}/media/${mediaId}$`));
    const viewer = page.locator(".media-viewer");
    await expect(viewer.locator(".cropper-title")).toHaveText("Fictitious school photograph");

    await viewer.getByRole("button", { name: "Close" }).first().click();
    await expect(page).toHaveURL(new RegExp(`/trees/${tree.treeId}/dictionary/tags/${tagId}$`));
});
