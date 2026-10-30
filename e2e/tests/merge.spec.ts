// The merge wizard (docs/ui-merge.md): from a pair of the potential
// duplicates, and from a profile's "Merge with…".

import type { APIRequestContext } from "@playwright/test";

import { apiUrl, expect, importGedcom, test } from "./fixtures";

/// A second record of the fixture's Dorian Ashdown: same name, birth year
/// and birthplace, no family. Returns its id.
async function duplicateOfDorian(request: APIRequestContext, treeId: string): Promise<string> {
    await importGedcom(
        request,
        treeId,
        "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n2 FORM LINEAGE-LINKED\n1 CHAR UTF-8\n" +
            "0 @D1@ INDI\n1 NAME Dorian /Ashdown/\n1 SEX M\n1 BIRT\n2 DATE 1840\n2 PLAC Westford, Shire\n1 OCCU Miller\n" +
            "0 TRLR\n",
    );
    const page = (await (await request.get(`${apiUrl}/api/v1/trees/${treeId}/profiles?first=100`)).json()) as {
        edges: Array<{ node: { person_id: string; primary_name?: { display_name?: string }; families_as_spouse?: unknown[] } }>;
    };
    const dorians = page.edges.map((edge) => edge.node).filter((p) => p.primary_name?.display_name === "Dorian Ashdown");
    expect(dorians).toHaveLength(2);
    const duplicate = dorians.find((p) => (p.families_as_spouse ?? []).length === 0);
    expect(duplicate, "the imported duplicate").toBeTruthy();
    return duplicate!.person_id;
}

async function dorianCount(request: APIRequestContext, treeId: string): Promise<number> {
    const page = (await (await request.get(`${apiUrl}/api/v1/trees/${treeId}/profiles?first=100`)).json()) as {
        edges: Array<{ node: { primary_name?: { display_name?: string } } }>;
    };
    return page.edges.filter((edge) => edge.node.primary_name?.display_name === "Dorian Ashdown").length;
}

test("a potential duplicate is compared and merged from the tools", async ({ page, request, tree }) => {
    await duplicateOfDorian(request, tree.treeId);

    await page.goto(`/trees/${tree.treeId}/tools`);
    await page.getByRole("tab", { name: "Potential duplicates" }).click();
    // The fixture already holds look-alikes of its own (its families repeat
    // their given names); the pair of the two Dorian Ashdowns is this one.
    const pair = page.locator(".tools-pair").filter({ hasText: /Ashdown\s*Dorian/i });
    await expect(pair).toHaveCount(1);
    await expect(pair.locator(".tools-reasons")).toContainText("Same name");
    await pair.getByRole("button", { name: "Compare", exact: true }).click();

    const wizard = page.locator(".merge-card");
    await expect(wizard.getByRole("heading", { name: "Merge two records" })).toBeVisible();
    await expect(wizard.locator(".merge-step")).toHaveText("Step 2 of 3 · Compare and choose");
    await expect(wizard.locator(".merge-person")).toHaveCount(2);
    await expect(wizard.getByRole("button", { name: "Two different people" })).toBeVisible();
    await wizard.getByRole("button", { name: "Next", exact: true }).click();

    await expect(wizard.locator(".merge-step")).toHaveText("Step 3 of 3 · Review and confirm");
    await expect(wizard.locator(".merge-summary")).toContainText("Record kept: Dorian Ashdown");
    await expect(wizard.getByText("This cannot be undone")).toBeVisible();
    await wizard.getByRole("button", { name: "Merge", exact: true }).click();

    await expect(wizard).toBeHidden();
    // The duplicates are computed again: the pair has left the list.
    await expect(pair).toHaveCount(0);
    expect(await dorianCount(request, tree.treeId)).toBe(1);
});

test("a profile's Merge with… finds the other record, and the merged profile carries both records' facts", async ({ page, request, tree }) => {
    const duplicateId = await duplicateOfDorian(request, tree.treeId);

    await page.goto(`/trees/${tree.treeId}/persons/${duplicateId}`);
    await expect(page.getByRole("heading", { level: 1, name: "Dorian Ashdown" })).toBeVisible();
    await page.getByRole("button", { name: "Merge with…" }).click();

    const wizard = page.locator(".merge-card");
    await expect(wizard.locator(".merge-step")).toHaveText("Step 1 of 3 · Choose the other record");
    await wizard.getByPlaceholder("Search for person to merge...").fill("Dorian");
    // The other record: the one married, whose death is recorded.
    await wizard.getByRole("button", { name: /Dorian.*1900/ }).click();

    await expect(wizard.locator(".merge-step")).toHaveText("Step 2 of 3 · Compare and choose");
    // The profile's own record is the one kept by default.
    await expect(wizard.locator(".merge-person").first().getByRole("radio")).toBeChecked();
    await wizard.getByRole("button", { name: "Next", exact: true }).click();
    await wizard.getByRole("button", { name: "Merge", exact: true }).click();
    await expect(wizard).toBeHidden();

    // The kept record keeps its occupation and takes the other's death and family.
    await expect(page).toHaveURL(new RegExp(`/persons/${duplicateId}$`));
    await expect(page.getByRole("heading", { level: 1, name: "Dorian Ashdown" })).toBeVisible();
    await expect(page.getByText("Miller").first()).toBeVisible();
    await expect(page.getByRole("link", { name: "Alice Coldwell" }).first()).toBeVisible();
    expect(await dorianCount(request, tree.treeId)).toBe(1);
});
