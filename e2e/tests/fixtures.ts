// The suite's `test`: Playwright's, with checks every test gets for free and
// helpers that seed trees through the REST API.

import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

import { test as base, expect, type APIRequestContext, type Page } from "@playwright/test";

const repository = resolve(import.meta.dirname, "..", "..");
export const apiUrl = `http://127.0.0.1:${process.env.E2E_API_PORT ?? "18080"}`;

/// The fictitious tree every test starts from: `family_blocks_gedcom(3)` of
/// the API integration tests, thirty persons in three unrelated families.
const fixtureGedcom = readFileSync(join(import.meta.dirname, "..", "fixtures", "family-blocks.ged"), "utf8");

/// The first segment of every translation key ("common", "person_form", …),
/// read from the English table. A visible word that starts with one of these
/// and a dot is a key that reached the page untranslated.
const keyPrefixes = [
    ...new Set(
        [...readFileSync(join(repository, "crates", "oxidgene-ui", "src", "i18n", "en.rs"), "utf8").matchAll(
            /"([a-z][a-z0-9_]*)\.[a-z0-9_.]+"\s*,/g,
        )].map((match) => match[1]),
    ),
];
const rawKey = new RegExp(`\\b(?:${keyPrefixes.join("|")})\\.[a-z0-9_]+(?:\\.[a-z0-9_]+)*\\b`);

/// Console noise that is not an application error: the debug bundle's
/// hot-reload client, which looks for `dx serve`.
const ignoredConsole = [/_dioxus/];

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
        async ({ page }, use) => {
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
            expect(errors, "console errors").toEqual([]);
        },
        { auto: true },
    ],
});

export { expect };
