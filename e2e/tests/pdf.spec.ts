// A stored PDF opens in the PDF viewer, drawn by pdf.js: its pages scroll,
// its text is searchable, and its gallery tile shows its first page.

import { apiUrl, expect, test } from "./fixtures";

/// A fictitious two-page PDF of text in a standard font, written by hand:
/// no scan, so the server takes no thumbnail of it and the client draws one.
function textPdf(): Buffer {
    const objects: string[] = [];
    const add = (body: string) => objects.push(body);
    add("<< /Type /Catalog /Pages 2 0 R >>");
    add("<< /Type /Pages /Kids [4 0 R 6 0 R] /Count 2 >>");
    add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    for (const [index, number] of [1, 2].entries()) {
        const content =
            `BT /F1 24 Tf 72 720 Td (Fictitious parish register - page ${number}) Tj ET\n` +
            "BT /F1 14 Tf 72 680 Td (Baptism of Jean Exemple, son of Pierre Exemple) Tj ET\n";
        add(
            `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] ` +
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

test("a PDF opens in the viewer, where its text can be searched", async ({ page, request, tree }) => {
    const base = `${apiUrl}/api/v1/trees/${tree.treeId}`;
    const document = await request.post(`${base}/media/document`, { data: { title: "Fictitious register" } });
    expect(document.ok(), await document.text()).toBeTruthy();
    const documentId = (await document.json()).id as string;
    const uploaded = await request.post(`${base}/media/upload`, {
        multipart: {
            file: { name: "register.pdf", mimeType: "application/pdf", buffer: textPdf() },
            document_id: documentId,
        },
    });
    expect(uploaded.ok(), await uploaded.text()).toBeTruthy();
    expect((await uploaded.json()).thumbnail_key).toBeNull();
    const linked = await request.post(`${base}/media-links`, {
        data: { media_id: documentId, person_id: tree.anchorId, sort_order: 0 },
    });
    expect(linked.ok(), await linked.text()).toBeTruthy();

    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    const tile = page.locator(".media-tile", { hasText: "Fictitious register" });
    // The tile draws the first page itself, as a picture of the session.
    await expect(tile.locator(".pdf-preview img")).toHaveAttribute("src", /^blob:/);

    await tile.locator(".media-thumb").click();
    const viewer = page.locator(".pdf-viewer");
    await expect(viewer.locator(".pdfViewer .page")).toHaveCount(2);
    await expect(viewer.locator(".textLayer").first()).toContainText("Fictitious parish register");
    await expect(viewer.getByText("Page 1 of 2")).toBeVisible();

    const search = viewer.getByRole("searchbox", { name: "Search the document" });
    await search.fill("Exemple");
    await expect(viewer.getByRole("status")).toHaveText("1 of 4");
    await search.press("Enter");
    await expect(viewer.getByRole("status")).toHaveText("2 of 4");
    await search.fill("Nowhere");
    await expect(viewer.getByRole("status")).toHaveText("No match");

    // The file is still offered as it is.
    await expect(page.getByRole("button", { name: "Download file" })).toBeVisible();
});
