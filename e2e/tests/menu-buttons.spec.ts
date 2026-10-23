// Where the ⋮ menu triggers sit, at a desktop's width and at a phone's
// (docs/ui-common.md §4.8): at the end of an event's line and of each of its
// citations' lines, in one column; at a note's top right; at the left edge
// of a media tile's picture; under the pedigree's selected card only; never
// out of the window, and never pushing the page sideways.

import type { APIRequestContext, Locator, Page } from "@playwright/test";

import { apiUrl, expect, test } from "./fixtures";

async function seed(request: APIRequestContext, treeId: string, personId: string) {
    const post = async (path: string, data: object) => {
        const created = await request.post(`${apiUrl}/api/v1/trees/${treeId}${path}`, { data });
        expect(created.ok(), await created.text()).toBeTruthy();
        return (await created.json()).id as string;
    };
    const event = await post("/events", { event_type: "baptism", date_value: "1900", person_id: personId });
    const first = await post("/sources", { title: "Fictional parish register of a long-named village" });
    const second = await post("/sources", { title: "Fictional census" });
    await post("/citations", { source_id: first, event_id: event, page: "f. 12" });
    await post("/citations", { source_id: second, event_id: event, page: "p. 3" });
    await post("/notes", {
        text: "Fictional first line of a note\nA second line\nA third line that runs on for a while",
        person_id: personId,
    });
}

async function box(locator: Locator) {
    const found = await locator.boundingBox();
    expect(found, "the element is laid out").not.toBeNull();
    return found!;
}

async function noSidewaysScroll(page: Page) {
    const [scroll, client] = await page.evaluate(() => [
        document.documentElement.scrollWidth,
        document.documentElement.clientWidth,
    ]);
    expect(scroll, "the page does not scroll sideways").toBeLessThanOrEqual(client + 1);
}

for (const viewport of [
    { width: 1280, height: 900 },
    { width: 390, height: 844 },
]) {
    test.describe(`at ${viewport.width}×${viewport.height}`, () => {
        test.use({ viewport });

        test("an event's and its citations' ⋮ share the end of their lines, and a note's sits at its top right", async ({
            page,
            request,
            tree,
        }) => {
            await seed(request, tree.treeId, tree.anchorId);
            await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);

            // The baptism seeded here, among the fixture's other events.
            const event = page
                .locator(".pd-ev-body")
                .filter({ has: page.locator(".pd-ev-source-row", { hasText: "Fictional census" }) });
            const citations = event.locator(".pd-ev-source-row");
            await expect(citations).toHaveCount(2);
            const eventTrigger = await box(event.locator(".pd-ev-row .menu-button").first());
            for (const row of await citations.all()) {
                const trigger = await box(row.locator(".menu-button"));
                // Same column as the event's ⋮, one per line.
                expect(Math.abs(trigger.x + trigger.width - (eventTrigger.x + eventTrigger.width))).toBeLessThanOrEqual(2);
                const line = await box(row);
                expect(trigger.y).toBeGreaterThanOrEqual(line.y - 1);
                expect(trigger.y + trigger.height).toBeLessThanOrEqual(line.y + line.height + 1);
                expect(trigger.x + trigger.width).toBeLessThanOrEqual(viewport.width);
            }

            const note = page.locator(".note-entry").filter({ hasText: "Fictional first line of a note" });
            const noteBox = await box(note);
            const noteTrigger = await box(note.locator(":scope > .menu-button-wrap .menu-button"));
            const body = await box(note.locator(".note-body"));
            // At the top right of the note, beside its text and not under it.
            expect(noteTrigger.x).toBeGreaterThanOrEqual(body.x + body.width - 1);
            expect(noteTrigger.y).toBeLessThan(body.y + 20);
            expect(noteTrigger.x + noteTrigger.width).toBeLessThanOrEqual(noteBox.x + noteBox.width);

            // The ⋮ opens the same record menu as before.
            await citations.first().locator(".menu-button").click();
            await expect(page.locator(".record-tag-menu")).toBeVisible();
            await page.keyboard.press("Escape");

            // The header's badges and buttons stay inside its card.
            const header = await box(page.locator(".card.page-header").first());
            for (const button of await page.locator(".pd-header-buttons .btn").all()) {
                const placed = await box(button);
                expect(placed.x + placed.width).toBeLessThanOrEqual(header.x + header.width + 1);
            }
            await noSidewaysScroll(page);
        });

        test("a short citation's ⋮ still ends its line, under the event's", async ({ page, request, tree }) => {
            const post = async (path: string, data: object) => {
                const created = await request.post(`${apiUrl}/api/v1/trees/${tree.treeId}${path}`, { data });
                expect(created.ok(), await created.text()).toBeTruthy();
                return (await created.json()).id as string;
            };
            const event = await post("/events", { event_type: "burial", date_value: "1950", person_id: tree.anchorId });
            const source = await post("/sources", { title: "Fictional ledger" });
            await post("/citations", { source_id: source, event_id: event });
            await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
            const body = page.locator(".pd-ev-body").filter({ has: page.locator(".pd-ev-source-row", { hasText: "Fictional ledger" }) });
            const eventTrigger = await box(body.locator(".pd-ev-row .menu-button").first());
            const trigger = await box(body.locator(".pd-ev-source-row .menu-button"));
            expect(Math.abs(trigger.x + trigger.width - (eventTrigger.x + eventTrigger.width))).toBeLessThanOrEqual(2);
        });

        test("the pedigree shows its ⋮ under the selected card only, inside the window", async ({ page, tree }) => {
            await page.goto(`/trees/${tree.treeId}`);
            const triggers = page.locator(".chart-menu-button");
            await expect(triggers).toHaveCount(1);
            // The frame, not the card's group, which holds the ⋮ too.
            const card = await box(page.locator("g.ped-card-focus .ped-card-rect").first());
            const trigger = await box(triggers.first());
            // Below the card and clear of it, centred on it.
            expect(trigger.y).toBeGreaterThanOrEqual(card.y + card.height - 1);
            expect(Math.abs(trigger.x + trigger.width / 2 - (card.x + card.width / 2))).toBeLessThanOrEqual(2);

            await triggers.first().click();
            await expect(page.locator(".context-menu")).toBeVisible();
            await page.keyboard.press("Escape");
            await expect(triggers.first()).toBeFocused();
            await noSidewaysScroll(page);
        });

        test("the topbar's history arrows carry no ⋮", async ({ page, tree }) => {
            await page.goto(`/trees/${tree.treeId}/persons/${tree.anchorId}`);
            await expect(page.locator(".nav-history-btn")).toHaveCount(2);
            await expect(page.locator(".nav-history .menu-button")).toHaveCount(0);
        });
    });
}
