// The screenshots of the README, of docs/features.md and of the
// documentation site (site/), taken from the fictitious Landrevel family
// (family.ts) on a throwaway server, in English and in French: the site's
// two languages. The French captures land in target/screenshots/raw/fr/.
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

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

import type { APIRequestContext, Locator, Page } from "@playwright/test";

import { apiUrl, expect, importFile, test } from "../tests/fixtures";
import { archiveCitation, cast, familyArchive, firstCousin } from "./family";

const repository = resolve(import.meta.dirname, "..", "..");
const raw = join(repository, "target", "screenshots", "raw");
/// The pieces scripts/screenshots.py composes into one image, per language.
const parts = join(raw, "parts");

/// The languages of the captures: English, the README's and the default,
/// and French, the documentation site's other language.
const LANGUAGES = ["en", "fr"] as const;
type Language = (typeof LANGUAGES)[number];

const tables = Object.fromEntries(
    LANGUAGES.map((language) => [
        language,
        (JSON.parse(readFileSync(join(repository, "assets", "i18n", `${language}.json`), "utf8")) as { translations: Record<string, string> })
            .translations,
    ]),
) as Record<Language, Record<string, string>>;

/// The interface's text of `key` in `language`, its `{name}` placeholders
/// filled in from `values`.
function t(language: Language, key: string, values: Record<string, string | number> = {}): string {
    const text = tables[language][key];
    expect(text, `${language}: ${key}`).toBeDefined();
    return Object.entries(values).reduce((filled, [name, value]) => filled.replaceAll(`{${name}}`, String(value)), text);
}

/// A pattern matching the text of `key`, any number standing for `{count}`.
function counted(language: Language, key: string): RegExp {
    const text = t(language, key).replace(/[.*+?^${}()|[\]\\]/g, "\\$&").replace("\\{count\\}", "\\d+");
    return new RegExp(text);
}

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
    /// The wedding photograph, for the media viewer.
    weddingPhoto: string;
    /// The "Lorient" tag, carried by a person and a postcard.
    lorientTag: string;
    /// The civil registers of Quimper, cited by the root's birth.
    quimperRegisters: string;
    /// Quimper, the root's birthplace.
    quimper: string;
    /// The grandmother whose birth cites a register of a real archive.
    grandmother: string;
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
    const grandmother = await personId(request, tree.id, cast.grandmother.given, cast.grandmother.surname);
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

    // Tags for the media library's tag cloud: one vocabulary entry per label,
    // each put on the documents it describes.
    const media = await json<{ edges: Array<{ node: { id: string; title?: string } }> }>(request, "get", `/trees/${tree.id}/media?first=100`);
    const tagIds = new Map<string, string>();
    for (const { node } of media.edges) {
        const tags = /register/i.test(node.title ?? "")
            ? ["parish register", "18th century"]
            : /postcard/i.test(node.title ?? "")
              ? ["postcard", "Lorient"]
              : /wedding/i.test(node.title ?? "")
                ? ["wedding", "studio portrait"]
                : ["studio portrait"];
        for (const label of tags) {
            let tagId = tagIds.get(label);
            if (tagId === undefined) {
                tagId = (await json<{ id: string }>(request, "post", `/trees/${tree.id}/tags`, { label })).id;
                tagIds.set(label, tagId);
            }
            const response = await request.put(`${apiUrl}/api/v1/trees/${tree.id}/tags/${tagId}/records/media/${node.id}`);
            expect(response.ok(), `put tag ${label} on ${node.id}`).toBeTruthy();
        }
    }

    // Tags on a few persons, so the tree cards, the side panel and the profile
    // headers show badges. Same vocabulary as the media tags above.
    const personTags: Array<[string, string[]]> = [
        [grandfather, ["Weaver", "Locronan"]],
        [mother, ["Dressmaker"]],
        [spouse, ["Milliner", "Lorient"]],
        [father, ["Mutual aid society"]],
        [root, ["Headmaster", "Researcher"]],
    ];
    for (const [person, labels] of personTags) {
        for (const label of labels) {
            let tagId = tagIds.get(label);
            if (tagId === undefined) {
                tagId = (await json<{ id: string }>(request, "post", `/trees/${tree.id}/tags`, { label })).id;
                tagIds.set(label, tagId);
            }
            const response = await request.put(`${apiUrl}/api/v1/trees/${tree.id}/tags/${tagId}/records/person/${person}`);
            expect(response.ok(), `put tag ${label} on ${person}`).toBeTruthy();
        }
    }

    const other = await json<{ id: string }>(request, "post", "/trees", {
        name: "Ashdown and Birchley",
        description: "Three unrelated families, imported from GEDCOM",
    });
    await importFile(request, other.id, readFileSync(join(repository, "e2e", "fixtures", "family-blocks.ged")), "gedcom");
    await json(request, "post", "/trees", { name: "Daulac line", description: "To start: the family of Joseph Daulac" });

    const weddingPhoto = media.edges.find(({ node }) => /wedding/i.test(node.title ?? ""))?.node.id;
    expect(weddingPhoto, "the wedding photograph").toBeTruthy();
    const sources = await json<{ edges: Array<{ node: { id: string; title: string } }> }>(request, "get", `/trees/${tree.id}/sources?first=100`);
    const quimperRegisters = sources.edges.find(({ node }) => node.title.startsWith("Civil registers of Quimper"))?.node.id;
    expect(quimperRegisters, "the civil registers of Quimper").toBeTruthy();
    const places = await json<{ edges: Array<{ node: { id: string; name: string } }> }>(request, "get", `/trees/${tree.id}/places?first=100`);
    const quimper = places.edges.find(({ node }) => node.name.startsWith("Quimper,"))?.node.id;
    expect(quimper, "Quimper").toBeTruthy();

    return {
        treeId: tree.id,
        root,
        weddingGroom: groom,
        cousin,
        weddingPhoto: weddingPhoto!,
        lorientTag: tagIds.get("Lorient")!,
        quimperRegisters: quimperRegisters!,
        quimper: quimper!,
        grandmother,
    };
}

/// Pin the browser's clock, the language, the theme and every timestamp the
/// API returns.
async function pin(page: Page, language: Language, theme: string): Promise<void> {
    await page.clock.setFixedTime(NOW);
    await page.addInitScript(([lang, value]) => {
        localStorage.setItem("oxidgene-lang", lang);
        localStorage.setItem("oxidgene-theme", value);
    }, [language, theme]);
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

/// Wait until the page has settled: no request in flight, fonts loaded,
/// transitions over.
async function settle(page: Page): Promise<void> {
    await page.waitForLoadState("networkidle");
    await page.evaluate(() => document.fonts.ready);
    await page.waitForFunction(() => [...document.images].every((image) => image.complete));
    // The charts fit their viewport in an animation of their own.
    await page.waitForTimeout(1200);
    await page.waitForLoadState("networkidle");
}

/// Wait until the page has settled and save it under `name`, in the
/// language's folder.
async function shoot(page: Page, language: Language, name: string): Promise<void> {
    await settle(page);
    const folder = language === "en" ? raw : join(raw, language);
    mkdirSync(folder, { recursive: true });
    await page.screenshot({ path: join(folder, `${name}.png`), animations: "disabled", caret: "hide" });
}

/// Scroll the page so that `element` stands `top` pixels below the top of
/// the window.
async function scrollTo(element: Locator, top: number): Promise<void> {
    await element.evaluate((node, offset) => {
        let scroller = node.parentElement;
        while (scroller && !(scroller.scrollHeight > scroller.clientHeight && /auto|scroll/.test(getComputedStyle(scroller).overflowY))) {
            scroller = scroller.parentElement;
        }
        (scroller ?? document.scrollingElement!).scrollTop += node.getBoundingClientRect().top - offset;
    }, top);
}

/// What hides a register's content in its portal's tab: the viewer's
/// image, any other large picture or canvas (a strip of thumbnails), and the
/// tables of results, which may name the persons of an index. Strong enough
/// that no handwriting stays legible, light enough that the page still reads
/// as a register.
const REGISTER_BLUR = "blur(7px)";

/// The size the portal's tab is captured at, before the composition scales
/// it down into its window.
const PORTAL_VIEWPORT = { width: 1200, height: 750 };

/// How long the portal may take to open the register before its capture is
/// skipped.
const PORTAL_TIMEOUT = 45_000;

test.describe.configure({ mode: "serial" });

let ids: Ids;

test.beforeAll(async ({ playwright }) => {
    mkdirSync(raw, { recursive: true });
    const request = await playwright.request.newContext();
    ids = await seed(request);
    await request.dispose();
});

for (const language of LANGUAGES) {
    test(`home (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.goto("/");
        await expect(page.getByText("Landrevel family", { exact: true })).toBeVisible();
        await expect(page.getByText("Étienne Marie", { exact: true }).first()).toBeVisible();
        await shoot(page, language, "home");
    });

    test(`import (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.goto("/");
        const card = page.locator(".tree-card").filter({ has: page.locator(".tree-card-name", { hasText: "Landrevel family" }) });
        await card.getByRole("button", { name: t(language, "common.actions_for", { name: "Landrevel family" }) }).click();
        await page.getByRole("menuitem", { name: t(language, "common.import"), exact: true }).click();
        await expect(page.getByText(t(language, "import.tab_geneanet"))).toBeVisible();
        await shoot(page, language, "import");
    });

    test(`pedigree (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.addInitScript(() => {
            localStorage.setItem("oxidgene-pedigree-defaults", JSON.stringify({ ancestor_levels: 3, descendant_levels: 2 }));
        });
        await page.goto(`/trees/${ids.treeId}`);
        // The SOSA root opens selected, its events in the side panel.
        await expect(page.locator("g.ped-card-focus").filter({ hasText: /Étienne Marie\s*LANDREVEL/ }).first()).toBeVisible();
        await shoot(page, language, "pedigree");
    });

    test(`fan chart, dark (${language})`, async ({ page }) => {
        await pin(page, language, "dark");
        await page.addInitScript(() => {
            localStorage.setItem("oxidgene-pedigree-view", JSON.stringify("fan"));
            localStorage.setItem("oxidgene-pedigree-defaults", JSON.stringify({ ancestor_levels: 6, descendant_levels: 2 }));
        });
        await page.goto(`/trees/${ids.treeId}`);
        await expect(page.locator(".fan-seg").first()).toBeVisible();
        await shoot(page, language, "fan-chart-dark");
    });

    test(`hourglass, medieval (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.addInitScript(() => {
            localStorage.setItem("oxidgene-pedigree-view", JSON.stringify("hourglass"));
            localStorage.setItem("oxidgene-pedigree-theme", JSON.stringify("medieval"));
            localStorage.setItem("oxidgene-pedigree-defaults", JSON.stringify({ ancestor_levels: 3, descendant_levels: 2 }));
        });
        await page.goto(`/trees/${ids.treeId}`);
        await expect(page.getByText("LANDREVEL").first()).toBeVisible();
        await shoot(page, language, "hourglass-medieval");
    });

    test(`person (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.goto(`/trees/${ids.treeId}/persons/${ids.root}`);
        await expect(page.getByRole("heading", { level: 1, name: "Étienne Marie Landrevel" })).toBeVisible();
        await shoot(page, language, "person");
        await page.getByRole("heading", { name: t(language, "person.family_connections") }).evaluate((heading) => heading.scrollIntoView({ block: "start" }));
        await shoot(page, language, "person-timeline");
    });

    test(`person sections (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.goto(`/trees/${ids.treeId}/persons/${ids.root}`);
        await expect(page.getByRole("heading", { level: 1, name: "Étienne Marie Landrevel" })).toBeVisible();
        // The events, each with its sources and its documents.
        const events = page.getByRole("heading", { name: t(language, "person.events_section"), exact: true });
        await expect(page.locator(".pd-timeline img").first()).toBeVisible();
        await scrollTo(events, 64);
        await shoot(page, language, "events-timeline");
        // The Ancestors card's mini pedigree: the person, their parents and
        // grandparents, at the foot of the page.
        const ancestors = page.getByRole("heading", { name: t(language, "person.ancestors"), exact: true });
        await expect(page.locator(".pd-section").filter({ has: ancestors }).locator("svg").first()).toBeVisible();
        await scrollTo(ancestors, 64);
        await shoot(page, language, "mini-pedigree");
    });

    // A cited register opened on its archive's portal: the profile with the
    // source that was clicked, and the portal's tab it opened, which
    // scripts/screenshots.py composes into archive-register. The portal is
    // live: when it cannot be reached, challenges the browser or does not
    // open the register in time, the capture is skipped and the committed
    // image kept.
    test(`archive register (${language})`, async ({ page, context }) => {
        await pin(page, language, "light");
        await page.goto(`/trees/${ids.treeId}/persons/${ids.grandmother}`);
        await expect(page.getByRole("heading", { level: 1, name: /Marie-Jeanne Pennamen/i })).toBeVisible();
        const link = page.locator("button.pd-ev-source-link", { hasText: archiveCitation.source });
        await expect(link).toBeVisible();
        await scrollTo(link, 200);
        await settle(page);
        const folder = join(parts, language);
        mkdirSync(folder, { recursive: true });
        const box = (await link.boundingBox())!;
        await page.screenshot({ path: join(folder, "archive-register-app.png"), animations: "disabled", caret: "hide" });

        const opened = context.waitForEvent("page");
        await link.click();
        const tab = await opened;
        await tab.setViewportSize(PORTAL_VIEWPORT);
        try {
            await tab.waitForURL(archiveCitation.portal, { timeout: PORTAL_TIMEOUT });
            await tab.locator(archiveCitation.viewer).first().waitFor({ state: "visible", timeout: PORTAL_TIMEOUT });
        } catch {
            const message = `the portal did not open the register (${tab.url()}): archive-register keeps its committed image`;
            console.warn(`archive register (${language}): ${message}`);
            test.info().annotations.push({ type: "skipped", description: message });
            await tab.close();
            return;
        }
        // The register's content, blurred before anything is captured.
        await tab.addStyleTag({ content: `${archiveCitation.viewer}, table { filter: ${REGISTER_BLUR} !important; }` });
        await tab.waitForLoadState("networkidle", { timeout: PORTAL_TIMEOUT }).catch(() => {});
        await tab.evaluate((filter) => {
            for (const element of document.querySelectorAll<HTMLElement>("img, canvas, video")) {
                const { width, height } = element.getBoundingClientRect();
                if (width >= 64 && height >= 64) element.style.setProperty("filter", filter, "important");
            }
        }, REGISTER_BLUR);
        await tab.waitForTimeout(2000);
        await tab.screenshot({ path: join(folder, "archive-register-portal.png"), animations: "disabled", caret: "hide" });
        writeFileSync(join(folder, "archive-register.json"), `${JSON.stringify({
            link: { x: box.x, y: box.y, width: box.width, height: box.height },
            url: tab.url(),
        }, null, 4)}\n`);
        await tab.close();
    });

    test(`person form (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.goto(`/trees/${ids.treeId}/persons/${ids.root}`);
        await expect(page.getByRole("heading", { level: 1, name: "Étienne Marie Landrevel" })).toBeVisible();
        await page.getByRole("button", { name: t(language, "common.edit"), exact: true }).click();
        await expect(page.getByText(t(language, "person_form.subtitle_edit")).first()).toBeVisible();
        await shoot(page, language, "person-edit");
    });

    test(`couple (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.goto(`/trees/${ids.treeId}/persons/${ids.weddingGroom}`);
        await page.getByRole("button", { name: t(language, "pedigree.couple_view") }).click();
        await expect(page).toHaveURL(/\/couples\//);
        await shoot(page, language, "couple");
    });

    test(`search (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.goto(`/trees/${ids.treeId}/search?last=LANDREVEL&first=&origin=`);
        await expect(page.getByText(counted(language, "search.results_count_other")).first()).toBeVisible();
        const filters = page.getByRole("button", { name: t(language, "search.filters"), exact: false }).first();
        await filters.click();
        await page.getByText(t(language, "search.has_media"), { exact: true }).click();
        await expect(page.getByText(t(language, "search.results_count_other", { count: 8 }))).toBeVisible();
        await filters.click();
        await shoot(page, language, "search");
    });

    test(`statistics (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.goto(`/trees/${ids.treeId}/statistics`);
        await page.getByRole("tab", { name: t(language, "stats.tab.overview") }).click();
        await shoot(page, language, "statistics-overview");
        await page.getByRole("tab", { name: t(language, "stats.tab.places") }).click();
        await shoot(page, language, "statistics-map");
        await page.getByRole("tab", { name: t(language, "stats.tab.names") }).click();
        await shoot(page, language, "statistics-charts");
    });

    test(`tools (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.goto(`/trees/${ids.treeId}/tools`);
        await page.getByRole("tab", { name: t(language, "tools.tab.anomalies") }).click();
        await shoot(page, language, "tools-anomalies");
        await page.getByRole("tab", { name: t(language, "tools.tab.ancestry") }).click();
        await expect(page.locator("table.tools-generations")).toBeVisible();
        await shoot(page, language, "tools-ancestry");
        await page.getByRole("tab", { name: t(language, "tools.tab.duplicates") }).click();
        await shoot(page, language, "tools-duplicates");
        // The first pair, compared in the merge wizard.
        await page.getByRole("button", { name: t(language, "tools.duplicates.compare"), exact: true }).first().click();
        await expect(page.getByText(t(language, "merge.title"))).toBeVisible();
        await shoot(page, language, "merge");
    });

    test(`kinship (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.goto(`/trees/${ids.treeId}/kinship?from=${ids.root}&to=${ids.cousin}`);
        await expect(page.getByText(counted(language, "kinship.found_other")).or(page.getByText(counted(language, "kinship.found_one")))).toBeVisible();
        await shoot(page, language, "kinship");
    });

    test(`dictionary and media library (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.goto(`/trees/${ids.treeId}/dictionary`);
        await expect(page.getByText("LANDREVEL").first()).toBeVisible();
        await page.getByPlaceholder(t(language, "dictionary.filter_placeholder")).fill("LANDREVEL");
        // A click on the row unfolds it: the row is the control, not a chevron.
        await page.locator(".dict-row", { hasText: "LANDREVEL" }).first().click();
        await shoot(page, language, "dictionary");
        await page.getByRole("tab", { name: t(language, "dictionary.tab.media") }).click();
        await shoot(page, language, "media-library");
    });

    test(`dictionary records (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        // A source's and a place's own address opens their row, unfolded.
        await page.goto(`/trees/${ids.treeId}/dictionary/sources/${ids.quimperRegisters}`);
        await expect(page.locator(`#dict-source-${ids.quimperRegisters}`)).toBeFocused();
        await shoot(page, language, "dictionary-sources");
        await page.goto(`/trees/${ids.treeId}/dictionary/places/${ids.quimper}`);
        await expect(page.locator(`#dict-place-${ids.quimper}`)).toBeFocused();
        await shoot(page, language, "dictionary-places");
    });

    test(`tag (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.goto(`/trees/${ids.treeId}/dictionary/tags/${ids.lorientTag}`);
        await expect(page.getByRole("tab", { name: t(language, "dictionary.tab.tags") })).toHaveAttribute("aria-selected", "true");
        await expect(page.getByText(cast.spouse.given).first()).toBeVisible();
        await shoot(page, language, "tags");
    });

    test(`media viewer (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.goto(`/trees/${ids.treeId}/media/${ids.weddingPhoto}`);
        await expect(page.locator(".media-viewer .cropper-title")).toContainText("Joseph Daulac");
        await shoot(page, language, "media-viewer");
    });

    test(`history (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.goto(`/trees/${ids.treeId}/persons/${ids.root}/history`);
        await expect(page.getByRole("heading", { level: 1, name: t(language, "history.title", { name: "Étienne Marie Landrevel" }) })).toBeVisible();
        // The current version against the imported one: both edits at once.
        await page.getByRole("combobox").selectOption({ label: t(language, "history.version_n", { version: 1 }) });
        await shoot(page, language, "history");
    });

    test(`tree settings (${language})`, async ({ page }) => {
        await pin(page, language, "light");
        await page.goto(`/trees/${ids.treeId}/settings`);
        await expect(page.getByText("Landrevel family").first()).toBeVisible();
        await shoot(page, language, "tree-settings");
        // The thresholds the anomaly rules apply, one card per category.
        await page.getByRole("button", { name: t(language, "settings.anomalies"), exact: true }).click();
        await expect(page.getByRole("heading", { level: 2, name: t(language, "settings.anomalies"), exact: true })).toBeVisible();
        await shoot(page, language, "tree-settings-anomalies");
    });

    test(`settings (${language})`, async ({ page }) => {
        await pin(page, language, "dark");
        await page.goto("/settings");
        await shoot(page, language, "app-settings-dark");
    });
}
