import { readFileSync } from "node:fs";

import { apiUrl, createTag, expect, personIdOf, tagPerson, test } from "./fixtures";

/// The number a statistics tile shows, found by the label under its value.
function tileValue(page: import("@playwright/test").Page, label: string) {
    return page
        .locator(".stats-tile")
        .filter({ has: page.getByText(label, { exact: true }) })
        .locator(".stats-tile-value");
}

/// Whether a request starts an export: not the reads the section makes when it opens.
function isExportRequest(req: import("@playwright/test").Request): boolean {
    const path = new URL(req.url()).pathname;
    return path.endsWith("/gedcom/export") || (req.method() === "POST" && path.endsWith("/export-jobs"));
}

/// The GEDCOM records of a `kind` in a written export: its xref lines.
function records(gedcom: string, kind: string): string[] {
    return [...gedcom.matchAll(new RegExp(`^0 @[^@]+@ ${kind}$`, "gm"))].map((match) => match[0]);
}

test("statistics limited to a tag count only the persons carrying it", async ({ page, request, tree }) => {
    const tagId = await createTag(request, tree.treeId, "Fictional mark");
    await tagPerson(request, tree.treeId, tagId, tree.anchorId);
    await tagPerson(request, tree.treeId, tagId, await personIdOf(request, tree.treeId, "Dorian"));

    await page.goto(`/trees/${tree.treeId}/statistics`);
    await expect(tileValue(page, "persons")).toHaveText("30");

    const asked = page.waitForRequest((req) => req.url().includes("/statistics?") && req.url().includes(`tag=${tagId}`));
    await page.locator("#stats-tag").selectOption(tagId);
    await asked;
    // Two persons carry the tag: the figures count them alone.
    await expect(tileValue(page, "persons")).toHaveText("2");
    // The tree's sources are not about persons: the note says they stay whole.
    await expect(page.getByText("Sources are counted over the whole tree, whatever the tag.")).toBeVisible();

    // The choice is kept with the page's history entry: going to another page
    // and back takes it up again.
    await page.getByRole("button", { name: "Tools", exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/trees/${tree.treeId}/tools$`));
    await page.goBack();
    await expect(page).toHaveURL(new RegExp(`/trees/${tree.treeId}/statistics$`));
    await expect(page.locator("#stats-tag")).toHaveValue(tagId);
    await expect(tileValue(page, "persons")).toHaveText("2");
});

test("anomalies limited to a tag ask for the tag's persons and count only their anomalies", async ({ page, request, tree }) => {
    const tagId = await createTag(request, tree.treeId, "Fictional mark");
    await tagPerson(request, tree.treeId, tagId, tree.anchorId);

    const everyone = await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/anomalies`)).json();
    const tagged = await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/anomalies?tag=${tagId}`)).json();
    const sum = (body: { rules: Array<{ count: number }> }) => body.rules.reduce((total, rule) => total + rule.count, 0);
    expect(sum(tagged)).toBeLessThanOrEqual(sum(everyone));

    await page.goto(`/trees/${tree.treeId}/tools`);
    await page.getByRole("tab", { name: "Anomalies" }).click();
    const asked = page.waitForRequest((req) => req.url().includes("/anomalies?") && req.url().includes(`tag=${tagId}`));
    await page.locator("#tools-anomalies-tag").selectOption(tagId);
    await asked;
    await expect(page.locator("#tools-anomalies-tag")).toHaveValue(tagId);
});

test("a branch export holds only the persons it names", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/settings`);
    await page.getByRole("button", { name: "Export Tree", exact: true }).click();
    await page.locator("#export-scope").selectOption("branch");
    // Nothing is exported before a root person is chosen.
    await expect(page.getByText("Choose the root person or the tag to export.")).toBeHidden();

    await page.getByPlaceholder("Search for the root person").fill("Dorian");
    await page.getByRole("button", { name: /^Ashdown Dorian\b/ }).first().click();
    // Their children only, Dorian included: Bernard, not the grandchildren or the spouse.
    await page.locator("#export-scope-depth").fill("1");

    const download = page.waitForEvent("download");
    await page.getByRole("button", { name: "Download .ged" }).click();
    const gedcom = readFileSync((await (await download).path()) as string, "utf8");
    expect(records(gedcom, "INDI")).toHaveLength(2);
    expect(gedcom).toContain("Dorian /Ashdown/");
    expect(gedcom).toContain("Bernard /Ashdown/");
    expect(gedcom).not.toContain("Anchor /Ashdown/");
    expect(gedcom).not.toContain("Alice /Coldwell/");
});

test("a tag export holds only the persons carrying the tag", async ({ page, request, tree }) => {
    const tagId = await createTag(request, tree.treeId, "Fictional mark");
    await tagPerson(request, tree.treeId, tagId, tree.anchorId);
    await tagPerson(request, tree.treeId, tagId, await personIdOf(request, tree.treeId, "Dorian"));

    await page.goto(`/trees/${tree.treeId}/settings`);
    await page.getByRole("button", { name: "Export Tree", exact: true }).click();
    await page.locator("#export-scope").selectOption("tag");
    await page.locator("#export-scope-tag").selectOption(tagId);

    const download = page.waitForEvent("download");
    await page.getByRole("button", { name: "Download .ged" }).click();
    const gedcom = readFileSync((await (await download).path()) as string, "utf8");
    expect(records(gedcom, "INDI")).toHaveLength(2);
    expect(gedcom).toContain("Anchor /Ashdown/");
    expect(gedcom).toContain("Dorian /Ashdown/");
    expect(gedcom).not.toContain("Bernard /Ashdown/");
});

test("a branch export names what is missing and sends no request until it is complete", async ({ page, tree }) => {
    const exportRequests: string[] = [];
    page.on("request", (req) => {
        if (isExportRequest(req)) exportRequests.push(req.url());
    });

    await page.goto(`/trees/${tree.treeId}/settings`);
    await page.getByRole("button", { name: "Export Tree", exact: true }).click();
    await page.locator("#export-scope").selectOption("branch");

    // No root person yet: the export says so and makes no request.
    await page.getByRole("button", { name: "Download .ged" }).click();
    await expect(page.getByText("Choose the root person or the tag to export.")).toBeVisible();
    expect(exportRequests).toHaveLength(0);

    await page.getByPlaceholder("Search for the root person").fill("Dorian");
    await page.getByRole("button", { name: /^Ashdown Dorian\b/ }).first().click();

    // An out-of-range generations value is shown as typed, explained, and blocks the export.
    await page.locator("#export-scope-depth").fill("31");
    await page.getByRole("button", { name: "Download .ged" }).click();
    await expect(page.getByText("Enter a number of generations from 1 to 30, or leave the field empty.").first()).toBeVisible();
    await expect(page.locator("#export-scope-depth")).toHaveValue("31");
    expect(exportRequests).toHaveLength(0);
});

test("a tag export says when no person carries a tag yet and sends no request", async ({ page, tree }) => {
    const exportRequests: string[] = [];
    page.on("request", (req) => {
        if (isExportRequest(req)) exportRequests.push(req.url());
    });

    await page.goto(`/trees/${tree.treeId}/settings`);
    await page.getByRole("button", { name: "Export Tree", exact: true }).click();
    await page.locator("#export-scope").selectOption("tag");
    await expect(page.getByLabel("Tag", { exact: true })).toHaveAttribute("id", "export-scope-tag");
    await expect(page.getByText("No person carries a tag yet.")).toBeVisible();

    await page.getByRole("button", { name: "Download .ged" }).click();
    await expect(page.getByText("Choose the root person or the tag to export.")).toBeVisible();
    expect(exportRequests).toHaveLength(0);
});
