import { apiUrl, expect, test } from "./fixtures";

test("editing a given name updates the profile, the breadcrumb and the pedigree", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    await expect(page.getByRole("heading", { level: 1, name: "Anchor Ashdown" })).toBeVisible();

    await page.getByRole("button", { name: "Edit", exact: true }).click();
    const form = page.locator(".person-form-modal");
    const given = form.locator(".form-group", { hasText: "Given Names *" }).locator("input");
    await expect(given).toHaveValue("Anchor");
    await given.fill("Keystone");
    await form.getByRole("button", { name: "Save", exact: true }).click();
    await expect(form).toBeHidden();

    await expect(page.getByRole("heading", { level: 1, name: "Keystone Ashdown" })).toBeVisible();
    await expect(page.locator("nav.td-bc").getByText("Keystone Ashdown")).toBeVisible();

    // The pedigree reads the refreshed projection, not a stale copy.
    await page.getByRole("button", { name: "Tree view" }).click();
    await expect(page.locator("g.ped-card").filter({ hasText: /Keystone\s*ASHDOWN/ }).first()).toBeVisible();
    await expect(page.locator("g.ped-card").filter({ hasText: /Anchor\s*ASHDOWN/ })).toHaveCount(0);
});

test("the history page compares a person's recorded versions", async ({ page, request, tree }) => {
    const names = await (await request.get(`${apiUrl}/api/v1/trees/${tree.treeId}/persons/${tree.anchorId}/names`)).json();
    const primary = (Array.isArray(names) ? names : names.edges.map((e: { node: unknown }) => e.node)).find(
        (n: { is_primary: boolean }) => n.is_primary,
    );
    const updated = await request.put(`${apiUrl}/api/v1/trees/${tree.treeId}/persons/${tree.anchorId}/names/${primary.id}`, {
        data: { given_names: "Keystone" },
    });
    expect(updated.ok()).toBeTruthy();

    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    await page.getByRole("button", { name: "History", exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/persons/${tree.anchorId}/history$`));
    await expect(page.getByRole("heading", { level: 1, name: "History of Keystone Ashdown" })).toBeVisible();
    await expect(page.getByRole("button", { name: /^Version 2\b/ })).toBeVisible();
    await expect(page.getByRole("button", { name: /^Version 1\b/ })).toBeVisible();
    // The latest version, compared with the one before it.
    await expect(page.getByRole("row", { name: "Given names Anchor Keystone" })).toBeVisible();
});
