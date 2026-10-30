// The suite's `test`: Playwright's, with checks every test gets for free and
// helpers that seed trees through the REST API.

import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

import { test as base, expect, type APIRequestContext, type Page } from "@playwright/test";

const repository = resolve(import.meta.dirname, "..", "..");
export const apiUrl = `http://127.0.0.1:${process.env.E2E_API_PORT ?? "18080"}`;

/// Whether the browser request to `url` is an API call. The path tells, not
/// the origin alone: a deployment serves the bundle and the API from one
/// origin (`E2E_EXTERNAL`, docs/development.md §2.7).
export function isApiCall(url: string): boolean {
    return url.startsWith(`${apiUrl}/api/`) || url.startsWith(`${apiUrl}/graphql`);
}

/// The fictitious tree every test starts from: `family_blocks_gedcom(3)` of
/// the API integration tests, thirty persons in three unrelated families.
const fixtureGedcom = readFileSync(join(import.meta.dirname, "..", "fixtures", "family-blocks.ged"), "utf8");

/// The first segment of every translation key ("common", "person_form", …),
/// read from the English locale document. A visible word that starts with one
/// of these and a dot is a key that reached the page untranslated.
const keyPrefixes = [
    ...new Set(
        Object.keys(
            JSON.parse(readFileSync(join(repository, "assets", "i18n", "en.json"), "utf8")).translations,
        ).map((key) => key.split(".")[0]),
    ),
];
const rawKey = new RegExp(`\\b(?:${keyPrefixes.join("|")})\\.[a-z0-9_]+(?:\\.[a-z0-9_]+)*\\b`);

/// Console noise that is not an application error: the debug bundle's
/// hot-reload client, which looks for `dx serve`.
const ignoredConsole = [/_dioxus/];

/// The annotation type that lets a test expect one console error, by a part of
/// its text, for a request the test refuses on purpose.
export const expectedConsoleError = "expected console error";

export interface SeededTree {
    treeId: string;
    /// Block 0's root, "Anchor Ashdown", the tree's SOSA root.
    anchorId: string;
    name: string;
}

/// Import `gedcom` into tree `treeId` the way the application does: upload it
/// as an import job, then wait for the backend's worker to complete it.
export async function importGedcom(request: APIRequestContext, treeId: string, gedcom: string): Promise<void> {
    await importFile(request, treeId, Buffer.from(gedcom, "utf8"), "gedcom");
}

/// Import a genealogy file of `format` into tree `treeId` as an import job,
/// and wait for the backend's worker to complete it.
export async function importFile(
    request: APIRequestContext,
    treeId: string,
    bytes: Buffer,
    format: "gedcom" | "gedzip" | "geneweb",
): Promise<void> {
    const started = await request.post(`${apiUrl}/api/v1/trees/${treeId}/import-jobs?format=${format}`, {
        headers: { "content-type": "application/octet-stream" },
        data: bytes,
    });
    expect(started.status(), await started.text()).toBe(202);
    const jobId = (await started.json()).job_id as string;
    await expect
        .poll(
            async () => {
                const status = await request.get(`${apiUrl}/api/v1/trees/${treeId}/import-jobs/${jobId}`);
                expect(status.ok()).toBeTruthy();
                const phase = (await status.json()).phase as string;
                expect(phase, "the fixture import failed").not.toBe("failed");
                return phase;
            },
            { message: "the fixture import completes", timeout: 30_000, intervals: [100, 250, 500] },
        )
        .toBe("completed");
}

/// Create a tree named `name` from the fixture GEDCOM and make block 0's
/// root its SOSA root, the way `family_blocks_tree` does in the Rust tests.
export async function seedTree(request: APIRequestContext, name: string): Promise<SeededTree> {
    const created = await request.post(`${apiUrl}/api/v1/trees`, { data: { name } });
    expect(created.ok()).toBeTruthy();
    const treeId = (await created.json()).id as string;
    await importGedcom(request, treeId, fixtureGedcom);
    // One page holds the whole fixture (30 persons).
    const page = (await (await request.get(`${apiUrl}/api/v1/trees/${treeId}/profiles?first=100`)).json()) as {
        edges: Array<{ node: { person_id: string; primary_name?: { given_names?: string } } }>;
    };
    const anchorId = page.edges.map((edge) => edge.node).find((p) => p.primary_name?.given_names === "Anchor")?.person_id;
    expect(anchorId, "the fixture's block 0 root").toBeTruthy();
    const updated = await request.put(`${apiUrl}/api/v1/trees/${treeId}`, {
        data: { sosa_root_person_id: anchorId },
    });
    expect(updated.ok()).toBeTruthy();
    return { treeId, anchorId: anchorId as string, name };
}

/// Create the tag `label` in `treeId` and return its id.
export async function createTag(request: APIRequestContext, treeId: string, label: string): Promise<string> {
    const created = await request.post(`${apiUrl}/api/v1/trees/${treeId}/tags`, { data: { label } });
    expect(created.ok(), await created.text()).toBeTruthy();
    return (await created.json()).id as string;
}

/// Records the GET requests the page makes for the tree's tag vocabulary, in
/// the order they start. Attach before the page is opened.
export function tagVocabularyRequests(page: Page, treeId: string): string[] {
    const asked: string[] = [];
    page.on("request", (req) => {
        const url = new URL(req.url());
        if (req.method() === "GET" && url.pathname === `/api/v1/trees/${treeId}/tags`) asked.push(req.url());
    });
    return asked;
}

/// Put the tag on a person, through the record route the picker uses.
export async function tagPerson(request: APIRequestContext, treeId: string, tagId: string, personId: string): Promise<void> {
    const put = await request.put(`${apiUrl}/api/v1/trees/${treeId}/tags/${tagId}/records/person/${personId}`);
    expect(put.status()).toBe(204);
}

/// The id of the fixture person whose given names are `given`.
export async function personIdOf(request: APIRequestContext, treeId: string, given: string): Promise<string> {
    const page = (await (await request.get(`${apiUrl}/api/v1/trees/${treeId}/profiles?first=100`)).json()) as {
        edges: Array<{ node: { person_id: string; primary_name?: { given_names?: string } } }>;
    };
    const id = page.edges.map((edge) => edge.node).find((p) => p.primary_name?.given_names === given)?.person_id;
    expect(id, `the fixture's ${given}`).toBeTruthy();
    return id as string;
}

/// The visible text of the page that looks like an untranslated key.
export async function rawKeysOn(page: Page): Promise<string[]> {
    const text = await page.locator("body").innerText();
    return text
        .split(/\s+/)
        .filter((word) => rawKey.test(word));
}

type Fixtures = {
    /// A fresh fixture tree of the test's own, free to change.
    tree: SeededTree;
    /// Fails the test on a console error, an uncaught exception, or a raw
    /// translation key left on the page.
    pageGuards: void;
};

export const test = base.extend<Fixtures>({
    tree: async ({ request }, use, testInfo) => {
        await use(await seedTree(request, `E2E ${testInfo.title.slice(0, 40)} ${testInfo.workerIndex}-${Date.now()}`));
    },
    pageGuards: [
        async ({ page }, use, testInfo) => {
            // Hermetic: the debug bundle's development-toast font is the
            // only outside request, and no test may depend on the network.
            await page.route(/^https?:\/\/(?!127\.0\.0\.1[:/])/, (route) => route.abort());
            // The debug bundle's hot-reload client dials `dx serve`; a
            // static server cannot answer, and the client would then cover
            // the page with a "being rebuilt" notice. Keep it quietly open.
            await page.routeWebSocket(/\/_dioxus/, () => {});
            const errors: string[] = [];
            page.on("console", (message) => {
                if (message.type() !== "error") return;
                const text = message.text();
                const local = message.location().url.startsWith("http://127.0.0.1");
                if (!local || ignoredConsole.some((pattern) => pattern.test(text))) return;
                errors.push(text);
            });
            page.on("pageerror", (error) => errors.push(`uncaught: ${error.message}`));
            await use();
            if (!page.isClosed() && page.url().startsWith("http")) {
                expect(await rawKeysOn(page), "untranslated keys on the page").toEqual([]);
            }
            // A test that refuses a request on purpose names the console line
            // that refusal makes, as an "expected console error" annotation.
            const expected = testInfo.annotations
                .filter((annotation) => annotation.type === expectedConsoleError)
                .map((annotation) => annotation.description ?? "");
            const unexpected = errors.filter((text) => !expected.some((part) => text.includes(part)));
            expect(unexpected, "console errors").toEqual([]);
        },
        { auto: true },
    ],
});

export { expect };
