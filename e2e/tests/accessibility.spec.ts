// The accessibility pass of docs/ui-common.md §5: axe-core over the main
// pages and over an open dialog, and the keyboard behaviour axe cannot see —
// the modal's focus trap and restore, and the pedigree's arrow keys.

import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";

import { expect, test, type SeededTree } from "./fixtures";

/// The rules checked: WCAG 2.1 levels A and AA, and axe's best practices.
const tags = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "best-practice"];

/// Rules left out, each for a reason recorded here.
const disabledRules = [
    // Contrast is decided per theme by the unit test of `theme.rs`, on the
    // theme's tokens and with its recorded thresholds (ui-common.md §5); axe
    // measures the one theme on screen and every tint mixed from it.
    "color-contrast",
    // A rule axe marks experimental and leaves off by default; selecting
    // rules by tag turns it on. It wants the text drawn in a control to be
    // a substring of its accessible name: a chart's card is named by its
    // person's full name, while it draws that name wrapped or shortened
    // beside the SOSA number and the years.
    "label-content-name-mismatch",
];

/// The violations of the page as it stands: the rule, the element's
/// selector, and the start of its markup.
async function violations(page: Page): Promise<string[]> {
    const results = await new AxeBuilder({ page }).withTags(tags).disableRules(disabledRules).analyze();
    return results.violations.flatMap((violation) =>
        violation.nodes.map((node) => `${violation.id}: ${node.target.join(" ")} — ${node.html.slice(0, 200)}`),
    );
}

/// The main pages: a name, an address, and what tells the page has drawn.
const pages: Array<[string, (tree: SeededTree) => string, string]> = [
    ["home", () => "/", ".tree-card-name"],
    ["pedigree", (tree) => `/trees/${tree.treeId}`, "[data-chart-item]"],
    ["person", (tree) => `/trees/${tree.treeId}/persons/${tree.anchorId}`, "h1"],
    ["search", (tree) => `/trees/${tree.treeId}/search?last=Ashdown&first=&origin=`, ".sr-count"],
    ["dictionary", (tree) => `/trees/${tree.treeId}/dictionary`, ".dict-row"],
    ["statistics", (tree) => `/trees/${tree.treeId}/statistics`, ".stats-tile"],
    ["tools", (tree) => `/trees/${tree.treeId}/tools`, "[role=tab]"],
    ["tree settings", (tree) => `/trees/${tree.treeId}/settings`, ".settings-content"],
    ["application settings", () => "/settings", ".theme-picker"],
];

for (const [name, address, drawn] of pages) {
    test(`the ${name} page has no accessibility violation`, async ({ page, tree }) => {
        await page.goto(address(tree));
        await expect(page.locator(drawn).first()).toBeVisible();
        expect(await violations(page)).toEqual([]);
    });
}

test("an open dialog has no accessibility violation, traps the focus and gives it back", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
    const edit = page.getByRole("button", { name: "Edit", exact: true }).first();
    await edit.focus();
    await page.keyboard.press("Enter");

    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();
    expect(await violations(page)).toEqual([]);

    // The dialog is modal: the focus is inside it, the page behind is inert
    // and Tab never leaves the dialog.
    expect(await dialog.evaluate((element) => element.matches("dialog:modal"))).toBe(true);
    for (let presses = 0; presses < 40; presses += 1) {
        await page.keyboard.press("Tab");
        const inside = await page.evaluate(() => {
            const active = document.activeElement;
            // The browser's own interface takes its turn in the Tab order.
            return !active || active === document.body || !!active.closest("dialog");
        });
        expect(inside, `Tab press ${presses + 1} stays in the dialog`).toBe(true);
    }

    // Escape closes it — after confirming nothing was typed — and the
    // focus returns to the button that opened it.
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    await expect(edit).toBeFocused();
});

test("the arrow keys travel between the pedigree's cards and Enter makes one the focus", async ({ page, tree }) => {
    await page.goto(`/trees/${tree.treeId}`);
    const cards = page.locator("[data-chart-item]");
    await expect(cards.first()).toBeVisible();

    // One card is the chart's stop in the Tab order: the selected person's.
    const stop = page.locator('[data-chart-item][tabindex="0"]');
    await expect(stop).toHaveCount(1);
    await expect(stop).toHaveAttribute("aria-label", /Anchor/);
    await stop.focus();

    // An arrow moves to another card, named after its person.
    await page.keyboard.press("ArrowRight");
    const focused = page.locator("[data-chart-item]:focus");
    await expect(focused).toHaveCount(1);
    const name = await focused.getAttribute("aria-label");
    expect(name).toBeTruthy();
    expect(name).not.toMatch(/Anchor/);

    // Enter does what a click does: that person becomes the chart's focus,
    // and the keyboard stays in the chart, on their card.
    await page.keyboard.press("Enter");
    await expect(page).toHaveURL(/[?&]person=/);
    await expect(page.locator('[data-chart-item][tabindex="0"]')).toHaveAttribute("aria-label", name as string);
    await expect(page.locator("[data-chart-item]:focus")).toHaveAttribute("aria-label", name as string);
});
