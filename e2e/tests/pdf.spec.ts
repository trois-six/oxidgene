// A stored PDF opens in the PDF viewer, drawn by pdf.js: its pages scroll,
// its text is searchable, and its gallery tile shows its first page.

import type { APIRequestContext } from "@playwright/test";
import { apiUrl, expect, test } from "./fixtures";

/// A fictitious PDF of text in a standard font, written by hand: no scan, so
/// the server takes no thumbnail of it and the client draws one. Two portrait
/// pages unless told otherwise.
function textPdf(pages = 2, [width, height] = [612, 792]): Buffer {
    const numbers = Array.from({ length: pages }, (_, index) => index + 1);
    const objects: string[] = [];
    const add = (body: string) => objects.push(body);
    add("<< /Type /Catalog /Pages 2 0 R >>");
    add(`<< /Type /Pages /Kids [${numbers.map(n => `${2 + n * 2} 0 R`).join(" ")}] /Count ${pages} >>`);
    add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    for (const [index, number] of numbers.entries()) {
        const content =
            `BT /F1 24 Tf 72 720 Td (Fictitious parish register - page ${number}) Tj ET\n` +
            "BT /F1 14 Tf 72 680 Td (Baptism of Jean Exemple, son of Pierre Exemple) Tj ET\n";
        add(
            `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${width} ${height}] ` +
                `/Resources << /Font << /F1 3 0 R >> >> /Contents ${5 + index * 2} 0 R >>`,
        );
        add(`<< /Length ${content.length} >>\nstream\n${content}\nendstream`);
    }
    let pdf = "%PDF-1.4\n";
    const offsets: number[] = [];
    objects.forEach((body, index) => {
        offsets.push(pdf.length);
        pdf += `${index + 1} 0 obj\n${body}\nendobj\n`;
    });
    const xref = pdf.length;
    pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
    for (const offset of offsets) pdf += `${String(offset).padStart(10, "0")} 00000 n \n`;
    pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
    return Buffer.from(pdf, "latin1");
}

/// Files `pdf` as the document `title`, attached to the tree's anchor person.
async function attachPdf(request: APIRequestContext, tree: { treeId: string; anchorId: string }, title: string, pdf: Buffer) {
    const base = `${apiUrl}/api/v1/trees/${tree.treeId}`;
    const document = await request.post(`${base}/media/document`, { data: { title } });
    expect(document.ok(), await document.text()).toBeTruthy();
    const documentId = (await document.json()).id as string;
    const uploaded = await request.post(`${base}/media/upload`, {
        multipart: {
            file: { name: "register.pdf", mimeType: "application/pdf", buffer: pdf },
            document_id: documentId,
        },
    });
    expect(uploaded.ok(), await uploaded.text()).toBeTruthy();
    expect((await uploaded.json()).thumbnail_key).toBeNull();
    const linked = await request.post(`${base}/media-links`, {
        data: { media_id: documentId, person_id: tree.anchorId, sort_order: 0 },
    });
    expect(linked.ok(), await linked.text()).toBeTruthy();
}

test("a PDF opens in the viewer, where its text can be searched", async ({ page, request, tree }) => {
    await attachPdf(request, tree, "Fictitious register", textPdf());

    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    const tile = page.locator(".media-tile", { hasText: "Fictitious register" });
    // The tile draws the first page itself, as a picture of the session.
    await expect(tile.locator(".pdf-preview img")).toHaveAttribute("src", /^blob:/);

    await tile.locator(".media-thumb").click();
    const viewer = page.locator(".pdf-viewer");
    await expect(viewer.locator(".pdfViewer .page")).toHaveCount(2);
    await expect(viewer.locator(".textLayer").first()).toContainText("Fictitious parish register");
    // Its pages are counted and turned as a document's: beside the title,
    // and by the viewer's pager.
    await expect(page.locator(".cropper-head").getByText("Page 1 of 2")).toBeVisible();

    const search = viewer.getByRole("searchbox", { name: "Search" });
    await search.fill("Exemple");
    await expect(viewer.getByRole("status")).toHaveText("1 of 4");
    await search.press("Enter");
    await expect(viewer.getByRole("status")).toHaveText("2 of 4");
    await search.fill("Nowhere");
    await expect(viewer.getByRole("status")).toHaveText("No match");
    await page.locator(".media-pager .pager-num", { hasText: "2" }).click();
    await expect(page.locator(".cropper-head").getByText("Page 2 of 2")).toBeVisible();

    // The file is still offered as it is.
    await expect(page.getByRole("button", { name: "Download file" })).toBeVisible();
});

test("at a phone's width the viewer's pager, count and pages fit its pane", async ({ page, request, tree }) => {
    await attachPdf(request, tree, "Fictitious register of a long title", textPdf(4, [960, 540]));
    await page.setViewportSize({ width: 300, height: 700 });
    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    await page.locator(".media-tile", { hasText: "Fictitious register" }).locator(".media-thumb").click();
    const viewer = page.locator(".pdf-viewer");
    await expect(viewer.locator(".pdfViewer .page")).toHaveCount(4);

    // Every number shown lies whole inside the pager, none under a step
    // button, and the last page is among them.
    const pager = page.locator(".media-pager");
    await expect(pager.locator(".pager-num:visible", { hasText: /^4$/ })).toBeVisible();
    const clipped = await pager.evaluate(nav => {
        const row = nav.getBoundingClientRect();
        const steps = [...nav.querySelectorAll(".pager-btn")]
            .map(button => button.getBoundingClientRect())
            .filter(rect => rect.width > 0);
        const found: string[] = [];
        for (const number of nav.querySelectorAll(".pager-num")) {
            const rect = number.getBoundingClientRect();
            if (rect.width === 0) continue;
            const strip = number.parentElement!;
            if (rect.left < row.left || rect.right > row.right || strip.scrollWidth > strip.clientWidth) {
                found.push(`${number.textContent} clipped`);
            }
            if (steps.some(step => rect.left < step.right && step.left < rect.right)) {
                found.push(`${number.textContent} under a step button`);
            }
        }
        return found;
    });
    expect(clipped).toEqual([]);

    // The count beside the title stays on one line; the title gives way.
    const count = page.locator(".cropper-head .media-pager-count");
    await expect(count).toHaveText("Page 1 of 4");
    expect(await count.evaluate(span => span.getClientRects().length)).toBe(1);

    // A fitted page is no wider than the pane it scrolls in.
    const scroll = viewer.locator(".pdf-viewer-scroll");
    await expect.poll(() => scroll.evaluate(box => box.scrollWidth - box.clientWidth)).toBeLessThanOrEqual(1);
});
