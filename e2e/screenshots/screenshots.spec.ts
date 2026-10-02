// The screenshots of the README and of docs/features.md, taken from the
// fictitious Landrevel family (family.ts) on a throwaway server.
//
// Run by `just screenshots`, never by `just e2e`: it lives outside the
// suite's test directory and has its own configuration. Each capture lands in
// target/screenshots/raw/; scripts/screenshots.py then optimises them into
// assets/screenshots/ and assembles the README carousel.
//
// Everything that could change between two runs is pinned, so that a run
// reproduces the committed images: the viewport, the language, the theme,
// the data (seeded), the browser clock, and the timestamps the API returns,
// which are rewritten to one fixed instant.

import { mkdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

import type { APIRequestContext, Page } from "@playwright/test";

import { apiUrl, expect, importFile, test } from "../tests/fixtures";
import { cast, familyArchive, firstCousin } from "./family";

const repository = resolve(import.meta.dirname, "..", "..");
const raw = join(repository, "target", "screenshots", "raw");

/// The instant every API timestamp is rewritten to, and the browser's clock
/// two hours later: "Modified 2 hours ago" on every run.
const RECORDED_AT = "2026-09-28T14:30:00Z";
const NOW = new Date("2026-09-28T16:30:00Z");
const TIMESTAMP = /"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})"/g;

interface Ids {
    treeId: string;
    root: string;
    weddingGroom: string;
    cousin: string;
}

async function personId(request: APIRequestContext, treeId: string, given: string, surname: string, born?: number): Promise<string> {
    const query = new URLSearchParams({ surname, given_names: given, limit: "25" });
    const response = await request.get(`${apiUrl}/api/v1/trees/${treeId}/persons/search?${query}`);
    expect(response.ok(), await response.text()).toBeTruthy();
    const body = (await response.json()) as { entries: Array<{ person_id: string; given_names: string; birth_year?: string | null }> };
    const match = body.entries.find((entry) => entry.given_names === given && (born === undefined || entry.birth_year === String(born)));
    expect(match, `${given} ${surname}`).toBeTruthy();
    return match!.person_id;
}

async function json<T>(request: APIRequestContext, method: "get" | "post" | "put", path: string, data?: unknown): Promise<T> {
    const response = await request[method](`${apiUrl}/api/v1${path}`, data === undefined ? undefined : { data });
    expect(response.ok(), `${method} ${path}: ${await response.text()}`).toBeTruthy();
    return (await response.json()) as T;
}

/// The trees of the home page: the Landrevel family, and two smaller ones.
async function seed(request: APIRequestContext): Promise<Ids> {
    const tree = await json<{ id: string }>(request, "post", "/trees", {
        name: "Landrevel family",
        description: "A fictitious family of Cornouaille, for demonstration",
    });
    await importFile(request, tree.id, familyArchive(), "gedzip");
    const root = await personId(request, tree.id, cast.root.given, cast.root.surname);
    const father = await personId(request, tree.id, cast.father.given, cast.father.surname);
    const mother = await personId(request, tree.id, cast.mother.given, cast.mother.surname);
    const spouse = await personId(request, tree.id, cast.spouse.given, cast.spouse.surname);
    const grandfather = await personId(request, tree.id, cast.grandfather.given, cast.grandfather.surname);
    const cousin = await personId(request, tree.id, firstCousin.given, firstCousin.surname, firstCousin.born);
    const [groom, bride] = await Promise.all(
        cast.weddingCouple.map((person) => personId(request, tree.id, person.given, person.surname)),
    );
    await json(request, "put", `/trees/${tree.id}`, { sosa_root_person_id: root, self_person_id: root });

    // The bride and groom of the wedding photograph are portrayed by their
    // identification on it.
    for (const person of [groom, bride]) {
        const found = await json<Array<{ id: string }> | { edges: Array<{ node: { id: string } }> }>(
            request, "get", `/trees/${tree.id}/vignettes?person_id=${person}`);
        const vignettes = Array.isArray(found) ? found : found.edges.map((edge) => edge.node);
        await json(request, "put", `/trees/${tree.id}/persons/${person}/portrait`, { vignette_id: vignettes[0].id });
    }

    // Notes on four relatives, for the tree card's recently modified persons.
    for (const [person, text] of [
        [grandfather, "Wove linen for the merchants of Locronan; his loom is mentioned in the inventory taken after his death."],
        [mother, "Worked for a Quimper dressmaker before her marriage."],
        [spouse, "Kept a milliner's shop in Lorient until 1923."],
        [father, "Member of the carpenters' mutual aid society of Quimper."],
    ] as const) {
        await json(request, "post", `/trees/${tree.id}/notes`, { person_id: person, text });
    }

    // Last, the root's own edits: they head the tree card's recently modified
    // persons, and give the root a history to compare.
    const names = await json<Array<{ id: string; is_primary: boolean }> | { edges: Array<{ node: { id: string; is_primary: boolean } }> }>(
        request, "get", `/trees/${tree.id}/persons/${root}/names`);
    const list = Array.isArray(names) ? names : names.edges.map((edge) => edge.node);
    const primary = list.find((name) => name.is_primary)!;
    await json(request, "put", `/trees/${tree.id}/persons/${root}/names/${primary.id}`, { given_names: "Étienne Marie" });
    await json(request, "post", `/trees/${tree.id}/events`, {
        person_id: root,
        event_type: "occupation",
        description: "Headmaster",
        date_value: "1945",
    });

    // Tags for the media library's tag cloud.
    const media = await json<{ edges: Array<{ node: { id: string; title?: string } }> }>(request, "get", `/trees/${tree.id}/media?first=100`);
    for (const { node } of media.edges) {
        const tags = /register/i.test(node.title ?? "")
            ? ["parish register", "18th century"]
            : /postcard/i.test(node.title ?? "")
              ? ["postcard", "Lorient"]
              : /wedding/i.test(node.title ?? "")
                ? ["wedding", "studio portrait"]
                : ["studio portrait"];
        for (const tag of tags) await json(request, "post", `/trees/${tree.id}/media/${node.id}/tags`, { tag });
    }

    const other = await json<{ id: string }>(request, "post", "/trees", {
        name: "Ashdown and Birchley",
        description: "Three unrelated families, imported from GEDCOM",
    });
    await importFile(request, other.id, readFileSync(join(repository, "e2e", "fixtures", "family-blocks.ged")), "gedcom");
    await json(request, "post", "/trees", { name: "Daulac line", description: "To start: the family of Joseph Daulac" });

    return { treeId: tree.id, root, weddingGroom: groom, cousin };
}

/// Pin the browser's clock and every timestamp the API returns.
async function pin(page: Page, theme: string): Promise<void> {
    await page.clock.setFixedTime(NOW);
    await page.addInitScript((value) => {
        localStorage.setItem("oxidgene-lang", "en");
        localStorage.setItem("oxidgene-theme", value);
    }, theme);
    await page.route(`${apiUrl}/api/**`, async (route) => {
        const response = await route.fetch();
        if (!(response.headers()["content-type"] ?? "").includes("json")) {
            await route.fulfill({ response });
            return;
        }
        const body = (await response.text()).replace(TIMESTAMP, `"${RECORDED_AT}"`);
        await route.fulfill({ response, body });
    });
}

/// Wait until the page has settled — no request in flight, fonts loaded,
/// transitions over — and save it under `name`.
async function shoot(page: Page, name: string): Promise<void> {
    await page.waitForLoadState("networkidle");
    await page.evaluate(() => document.fonts.ready);
    await page.waitForFunction(() => [...document.images].every((image) => image.complete));
    // The charts fit their viewport in an animation of their own.
    await page.waitForTimeout(1200);
    await page.waitForLoadState("networkidle");
    await page.screenshot({ path: join(raw, `${name}.png`), animations: "disabled", caret: "hide" });
}

test.describe.configure({ mode: "serial" });

let ids: Ids;

test.beforeAll(async ({ playwright }) => {
    mkdirSync(raw, { recursive: true });
    const request = await playwright.request.newContext();
    ids = await seed(request);
    await request.dispose();
});

test("home", async ({ page }) => {
    await pin(page, "light");
    await page.goto("/");
    await expect(page.getByText("Landrevel family", { exact: true })).toBeVisible();
    await expect(page.getByText("Étienne Marie", { exact: true }).first()).toBeVisible();
    await shoot(page, "home");
});

test("pedigree", async ({ page }) => {
    await pin(page, "light");
    await page.addInitScript(() => {
        localStorage.setItem("oxidgene-pedigree-defaults", JSON.stringify({ ancestor_levels: 3, descendant_levels: 2 }));
    });
    await page.goto(`/trees/${ids.treeId}`);
    // The SOSA root opens selected, its events in the side panel.
    await expect(page.locator("g.ped-card-focus").filter({ hasText: /Étienne Marie\s*LANDREVEL/ }).first()).toBeVisible();
    await shoot(page, "pedigree");
});

test("fan chart, dark", async ({ page }) => {
    await pin(page, "dark");
    await page.addInitScript(() => {
        localStorage.setItem("oxidgene-pedigree-view", JSON.stringify("fan"));
        localStorage.setItem("oxidgene-pedigree-defaults", JSON.stringify({ ancestor_levels: 6, descendant_levels: 2 }));
    });
    await page.goto(`/trees/${ids.treeId}`);
    await expect(page.locator(".fan-seg").first()).toBeVisible();
    await shoot(page, "fan-chart-dark");
});

test("hourglass, medieval", async ({ page }) => {
    await pin(page, "light");
    await page.addInitScript(() => {
        localStorage.setItem("oxidgene-pedigree-view", JSON.stringify("hourglass"));
        localStorage.setItem("oxidgene-pedigree-theme", JSON.stringify("medieval"));
        localStorage.setItem("oxidgene-pedigree-defaults", JSON.stringify({ ancestor_levels: 3, descendant_levels: 2 }));
    });
    await page.goto(`/trees/${ids.treeId}`);
    await expect(page.getByText("LANDREVEL").first()).toBeVisible();
    await shoot(page, "hourglass-medieval");
});

test("person", async ({ page }) => {
    await pin(page, "light");
    await page.goto(`/trees/${ids.treeId}/persons/${ids.root}`);
    await expect(page.getByRole("heading", { level: 1, name: "Étienne Marie Landrevel" })).toBeVisible();
    await shoot(page, "person");
    await page.getByRole("heading", { name: "Family Connections" }).evaluate((heading) => heading.scrollIntoView({ block: "start" }));
    await shoot(page, "person-timeline");
});

test("couple", async ({ page }) => {
    await pin(page, "light");
    await page.goto(`/trees/${ids.treeId}/persons/${ids.weddingGroom}`);
    await page.getByRole("button", { name: "Couple view" }).click();
    await expect(page).toHaveURL(/\/couples\//);
    await shoot(page, "couple");
});

test("search", async ({ page }) => {
    await pin(page, "light");
    await page.goto(`/trees/${ids.treeId}/search?last=LANDREVEL&first=&origin=`);
    await expect(page.getByText(/\d+ results/).first()).toBeVisible();
    await page.getByRole("button", { name: "Filters", exact: false }).first().click();
    await page.getByText("With media", { exact: true }).click();
    await expect(page.getByText("8 results")).toBeVisible();
    await page.getByRole("button", { name: "Filters", exact: false }).first().click();
    await shoot(page, "search");
});

test("statistics", async ({ page }) => {
    await pin(page, "light");
    await page.goto(`/trees/${ids.treeId}/statistics`);
    await page.getByRole("tab", { name: "Overview" }).click();
    await shoot(page, "statistics-overview");
    await page.getByRole("tab", { name: "Places" }).click();
    await shoot(page, "statistics-map");
    await page.getByRole("tab", { name: "Names and occupations" }).click();
    await shoot(page, "statistics-charts");
});

test("tools", async ({ page }) => {
    await pin(page, "light");
    await page.goto(`/trees/${ids.treeId}/tools`);
    await page.getByRole("tab", { name: "Anomalies" }).click();
    await shoot(page, "tools-anomalies");
    await page.getByRole("tab", { name: /duplicates/i }).click();
    await shoot(page, "tools-duplicates");
});

test("kinship", async ({ page }) => {
    await pin(page, "light");
    await page.goto(`/trees/${ids.treeId}/kinship?from=${ids.root}&to=${ids.cousin}`);
    await expect(page.getByText(/relationships? found/)).toBeVisible();
    await shoot(page, "kinship");
});

test("dictionary and media library", async ({ page }) => {
    await pin(page, "light");
    await page.goto(`/trees/${ids.treeId}/dictionary`);
    await expect(page.getByText("LANDREVEL").first()).toBeVisible();
    await page.getByPlaceholder("Filter...").fill("LANDREVEL");
    await page.getByRole("button", { name: "▼" }).first().click();
    await shoot(page, "dictionary");
    await page.getByRole("tab", { name: "Media" }).click();
    await shoot(page, "media-library");
});

test("history", async ({ page }) => {
    await pin(page, "light");
    await page.goto(`/trees/${ids.treeId}/persons/${ids.root}/history`);
    await expect(page.getByRole("heading", { level: 1, name: /History of/ })).toBeVisible();
    // The current version against the imported one: both edits at once.
    await page.getByRole("combobox").selectOption({ label: "Version 1" });
    await shoot(page, "history");
});

test("settings", async ({ page }) => {
    await pin(page, "dark");
    await page.goto("/settings");
    await shoot(page, "app-settings-dark");
});
