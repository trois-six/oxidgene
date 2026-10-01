import { expect, test } from "./fixtures";
import type { Page } from "@playwright/test";

/// What sticks out of the viewport sideways: the document itself, and any
/// child of the page's main area (docs/development.md §4). Overflow inside a
/// scrolling container, such as the pedigree canvas, is the container's.
async function horizontalOverflow(page: Page): Promise<string[]> {
    return page.evaluate(() => {
        const width = document.documentElement.clientWidth;
        const found: string[] = [];
        if (document.documentElement.scrollWidth > width + 1) {
            found.push(`document ${document.documentElement.scrollWidth}px > ${width}px`);
        }
        for (const element of document.querySelectorAll("main > *, main > * > *")) {
            // Fixed decorations, such as the home page's gears, bleed off the
            // edge on purpose and never scroll.
            if (getComputedStyle(element).position === "fixed") continue;
            const rect = element.getBoundingClientRect();
            if (rect.width > 0 && (rect.left < -1 || rect.right > width + 1)) {
                found.push(`${element.tagName.toLowerCase()}.${element.className} [${Math.round(rect.left)}, ${Math.round(rect.right)}]`);
            }
        }
        return found;
    });
}

for (const viewport of [
    { width: 1280, height: 800 },
    { width: 390, height: 844 },
]) {
    test(`no page overflows sideways at ${viewport.width}×${viewport.height}`, async ({ page, tree }) => {
        await page.setViewportSize(viewport);
        const base = `/trees/${tree.treeId}`;
        const pages = [
            "/",
            base,
            `${base}/persons/${tree.anchorId}`,
            `${base}/search?last=Ashdown&first=&origin=`,
            `${base}/dictionary`,
            `${base}/statistics`,
            `${base}/tools`,
            `${base}/settings`,
            "/settings",
        ];
        for (const path of pages) {
            await page.goto(path);
            await expect(page.locator("main")).toBeVisible();
            // Let the page's data arrive and lay out.
            await page.waitForLoadState("networkidle");
            expect(await horizontalOverflow(page), `overflow on ${path}`).toEqual([]);
        }
    });
}
