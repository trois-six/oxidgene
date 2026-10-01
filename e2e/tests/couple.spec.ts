import { expect, test, type SeededTree } from "./fixtures";
import type { Locator, Page } from "@playwright/test";

/// Open the couple page of Anchor's parents, through the person page and the
/// sidebar, then their union form.
async function openParentsUnionForm(page: Page, tree: SeededTree): Promise<Locator> {
    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    await page.getByRole("link", { name: "Bernard Ashdown" }).first().click();
    await expect(page.getByRole("heading", { level: 1, name: "Bernard Ashdown" })).toBeVisible();
    await page.getByRole("button", { name: "Couple view" }).click();
    await expect(page).toHaveURL(/\/couples\/[0-9a-f-]{36}$/);
    await expect(page.getByText("Bernard Ashdown & Clara Birchley").first()).toBeVisible();

    await page.getByRole("button", { name: "Edit couple" }).click();
    const form = page.locator(".union-form-modal");
    await expect(form).toBeVisible();
    await expect(form.locator(".uf-child-row")).toHaveCount(3);
    return form;
}

function childRow(form: Locator, name: string): Locator {
    return form.locator(".uf-child-row").filter({ hasText: name });
}

async function stageDetachment(form: Locator, name: string): Promise<void> {
    await childRow(form, name).getByRole("button", { name: "Detach" }).click();
    await expect(form.getByText(`Detach ${name} from this union?`)).toBeVisible();
    await form.getByRole("button", { name: "Confirm" }).click();
    await expect(childRow(form, name)).toHaveClass(/pending-detach/);
}

test("the union form stages a child's detachment and undoes it", async ({ page, tree }) => {
    let form = await openParentsUnionForm(page, tree);
    await stageDetachment(form, "Delia Ashdown");
    await childRow(form, "Delia Ashdown").getByRole("button", { name: "Undo" }).click();
    await expect(childRow(form, "Delia Ashdown")).not.toHaveClass(/pending-detach/);
    await form.getByRole("button", { name: "Save", exact: true }).click();
    await expect(form).toBeHidden();

    await page.getByRole("button", { name: "Edit couple" }).click();
    form = page.locator(".union-form-modal");
    await expect(form.locator(".uf-child-row")).toHaveCount(3);
});

// Known defect: the form sends the child's person id where
// `DELETE /families/{family_id}/children/{child_id}` expects the id of the
// family-child link, so saving answers 404 and nothing is detached. Remove
// `test.fail` once the union form is fixed.
test("saving a staged detachment removes the child from the union", async ({ page, tree }) => {
    test.fail();
    let form = await openParentsUnionForm(page, tree);
    await stageDetachment(form, "Delia Ashdown");
    await form.getByRole("button", { name: "Save", exact: true }).click();
    await expect(form).toBeHidden();

    await page.getByRole("button", { name: "Edit couple" }).click();
    form = page.locator(".union-form-modal");
    await expect(form.locator(".uf-child-row")).toHaveCount(2);
    await expect(childRow(form, "Delia Ashdown")).toHaveCount(0);
});
