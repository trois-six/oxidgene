// UI request budgets: the API requests every page makes, against the
// committed budget of e2e/budgets/requests.json.
//
// Drift it prevents: a page that starts fetching per card, a waterfall that
// gains a level, the same request sent twice, a payload that balloons — the
// regressions the per-page request study removed (docs/development.md §2.8).
//
// Each scenario loads a page cold (a fresh browser context: empty HTTP and
// client caches), navigates to it from a neighbour inside the application
// (warm), or switches a page's tabs, on a tree seeded with portraits, crops,
// a document, a note and a citation. Every API request is recorded through
// the DevTools protocol with 50 ms of latency added, as a slow network
// would. The structural metrics — requests, critical-path depth, duplicates,
// response bytes — are deterministic and blocking: requests, depth and
// duplicates may not exceed their budget, bytes not by more than a margin.
// The time to the page's content is reported in
// test-results/request-report.json, never compared: a runner's timing is
// noise.
//
// After an intended change, rewrite the budget and commit it, saying why in
// the commit: `E2E_BUDGET_UPDATE=1 just e2e tests/request-budgets.spec.ts`.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import type { Browser, Locator, Page } from "@playwright/test";

import { expect, test } from "./fixtures";
import { type Metrics, type PicturedTree, Recorder, metrics, picturedTree, template } from "./requests";

const BUDGET_FILE = join(import.meta.dirname, "..", "budgets", "requests.json");
const REPORT_FILE = join(import.meta.dirname, "..", "test-results", "request-report.json");
const UPDATE = !!process.env.E2E_BUDGET_UPDATE;
/// Response bytes vary with what the fixture's pictures compress to.
const BYTES_MARGIN = 0.25;
const LATENCY_MS = 50;

interface Target {
    url: (tree: PicturedTree) => string;
    /// What appears once the page has drawn its content.
    content: string;
    /// The pedigree's chart, stored where the page reads it.
    view?: string;
}

const pedigree = (view: string): Target => ({
    url: (t) => `/trees/${t.treeId}`,
    content: ["tree", "lineage", "descendant-lineage", "hourglass", "bowtie"].includes(view) ? "g.ped-card" : ".fan-seg",
    view,
});
const PAGES = {
    home: { url: () => "/", content: ".tree-card-name" },
    pedigree: pedigree("tree"),
    search: { url: (t) => `/trees/${t.treeId}/search?last=Ashdown&first=&origin=`, content: ".search-person-result" },
    person: { url: (t) => `/trees/${t.treeId}/persons/${t.anchorId}`, content: ".pd-header-top" },
    history: { url: (t) => `/trees/${t.treeId}/persons/${t.anchorId}/history`, content: ".ph-version" },
    couple: { url: (t) => `/trees/${t.treeId}/couples/${t.familyId}`, content: ".cp-grid .cp-cell" },
    kinship: { url: (t) => `/trees/${t.treeId}/kinship?from=${t.anchorId}&to=${t.kinId}`, content: ".kin-path" },
    dictionary: { url: (t) => `/trees/${t.treeId}/dictionary`, content: ".dict-row" },
    statistics: { url: (t) => `/trees/${t.treeId}/statistics`, content: ".stats-tile, .stats-card, .stats-section" },
    tools: { url: (t) => `/trees/${t.treeId}/tools`, content: ".tools-rule, .tools-category, .stats-table" },
    settings: { url: (t) => `/trees/${t.treeId}/settings`, content: ".settings-section" },
    appSettings: { url: () => "/settings", content: ".settings-section" },
} satisfies Record<string, Target>;

type Scenario =
    | { name: string; kind: "cold"; target: Target }
    | { name: string; kind: "warm"; from: Target; target: Target }
    | { name: string; kind: "tabs"; target: Target; tab: string; label: string; content: string };

const scenarios: Scenario[] = [
    { name: "home", kind: "cold", target: PAGES.home },
    ...["tree", "wheel", "fan", "descendant-wheel", "descendant-fan", "lineage", "descendant-lineage", "hourglass", "bowtie"].map(
        (view): Scenario => ({ name: `pedigree:${view}`, kind: "cold", target: pedigree(view) }),
    ),
    ...(["search", "person", "history", "couple", "kinship", "dictionary", "statistics", "tools", "settings", "appSettings"] as const).map(
        (page): Scenario => ({ name: page, kind: "cold", target: PAGES[page] }),
    ),
    ...(
        [
            ["home", "pedigree"],
            ["pedigree", "home"],
            ["pedigree", "person"],
            ["person", "pedigree"],
            ["pedigree", "search"],
            ["search", "person"],
            ["person", "history"],
            ["person", "couple"],
            ["person", "kinship"],
            ["pedigree", "dictionary"],
            ["pedigree", "statistics"],
            ["pedigree", "tools"],
            ["pedigree", "settings"],
            ["home", "appSettings"],
        ] as const
    ).map(([to, from]): Scenario => ({ name: `${to}<-${from}`, kind: "warm", from: PAGES[from], target: PAGES[to] })),
    ...tabs("dictionary", '[role="tab"]', ["Sources", "Places", "Occupations", "Family Names"], ".dict-row"),
    ...tabs("statistics", '[role="tab"]', ["Population", "Families", "Places", "Growth", "Overview"], ".stats-content *"),
    ...tabs("tools", '[role="tab"]', ["Places not located", "Ancestry completeness", "Potential duplicates", "Anomalies"], ".tools-content *"),
];

function tabs(page: keyof typeof PAGES, tab: string, labels: string[], content: string): Scenario[] {
    return labels.map((label) => ({ name: `${page}:${label}`, kind: "tabs", target: PAGES[page], tab, label, content }));
}

interface Measured extends Metrics {
    /// Milliseconds to the content, with the added latency: reported only.
    contentMs: number;
    endpoints: string[];
}

async function openPage(browser: Browser, view?: string): Promise<{ page: Page; recorder: Recorder; close: () => Promise<void> }> {
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, locale: "en-US", timezoneId: "UTC" });
    if (view) {
        await context.addInitScript((v) => {
            try {
                localStorage.setItem("oxidgene-pedigree-view", JSON.stringify(v));
            } catch {}
        }, view);
    }
    const page = await context.newPage();
    await page.route(/^https?:\/\/(?!127\.0\.0\.1[:/])/, (route) => route.abort());
    await page.routeWebSocket(/\/_dioxus/, () => {});
    const recorder = await Recorder.attach(page, LATENCY_MS);
    return { page, recorder, close: () => context.close() };
}

async function shown(locator: Locator): Promise<number> {
    const start = Date.now();
    await expect(locator.first()).toBeVisible({ timeout: 30_000 });
    return Date.now() - start;
}

async function measure(browser: Browser, tree: PicturedTree, scenario: Scenario): Promise<Measured> {
    const { page, recorder, close } = await openPage(browser, scenario.target.view);
    try {
        let contentMs: number;
        if (scenario.kind === "cold") {
            const start = Date.now();
            await page.goto(scenario.target.url(tree), { waitUntil: "commit" });
            await shown(page.locator(scenario.target.content));
            contentMs = Date.now() - start;
        } else {
            const first = scenario.kind === "warm" ? scenario.from : scenario.target;
            await page.goto(first.url(tree), { waitUntil: "commit" });
            await shown(page.locator(first.content));
            await recorder.settle(page);
            recorder.reset();
            const start = Date.now();
            if (scenario.kind === "warm") {
                await page.evaluate((url) => {
                    history.pushState({}, "", url);
                    window.dispatchEvent(new PopStateEvent("popstate", { state: {} }));
                }, scenario.target.url(tree));
                await shown(page.locator(scenario.target.content));
            } else {
                await page.locator(scenario.tab, { hasText: scenario.label }).first().click();
                await shown(page.locator(scenario.content));
            }
            contentMs = Date.now() - start;
        }
        await recorder.settle(page);
        const recorded = recorder.all();
        return { ...metrics(recorded), contentMs, endpoints: recorded.map(template) };
    } finally {
        await close();
    }
}

function overBudget(measured: Metrics, budget: Metrics | undefined): string[] {
    if (!budget) return ["no budget: run with E2E_BUDGET_UPDATE=1 and commit e2e/budgets/requests.json"];
    const problems: string[] = [];
    for (const key of ["requests", "depth", "duplicates"] as const) {
        if (measured[key] > budget[key]) problems.push(`${key} ${measured[key]} > ${budget[key]}`);
    }
    if (measured.bytes > budget.bytes * (1 + BYTES_MARGIN)) {
        problems.push(`bytes ${measured.bytes} > ${budget.bytes} + ${BYTES_MARGIN * 100}%`);
    }
    return problems;
}

// One test walks every scenario, so one tree and one report serve them all
// and a failing page does not hide the others: each is a soft assertion.
test("every page keeps within its request budget", async ({ browser, playwright }, testInfo) => {
    test.setTimeout(30 * 60_000);
    const budgets: Record<string, Metrics> = UPDATE ? {} : JSON.parse(readFileSync(BUDGET_FILE, "utf8"));
    const request = await playwright.request.newContext();
    const tree = await picturedTree(request, `E2E request budgets ${Date.now()}`);
    await request.dispose();

    const report: Record<string, Measured> = {};
    for (const scenario of scenarios) {
        let measured = await measure(browser, tree, scenario);
        let problems = UPDATE ? [] : overBudget(measured, budgets[scenario.name]);
        if (problems.length > 0) {
            // A request still in flight at the settle deadline, or a racing
            // refresh: measure once more and keep the smaller.
            const again = await measure(browser, tree, scenario);
            if (again.requests <= measured.requests) measured = again;
            problems = overBudget(measured, budgets[scenario.name]);
        }
        report[scenario.name] = measured;
        expect.soft(problems, `${scenario.name}: ${measured.endpoints.join(", ")}`).toEqual([]);
    }
    for (const name of Object.keys(budgets)) {
        expect.soft(report[name], `${name} is in the budget file but no scenario measures it`).toBeDefined();
    }

    mkdirSync(join(REPORT_FILE, ".."), { recursive: true });
    writeFileSync(REPORT_FILE, JSON.stringify(report, null, 2) + "\n");
    await testInfo.attach("request-report", { path: REPORT_FILE, contentType: "application/json" });
    if (UPDATE) {
        const updated = Object.fromEntries(
            Object.entries(report).map(([name, { requests, depth, duplicates, bytes }]) => [name, { requests, depth, duplicates, bytes }]),
        );
        mkdirSync(join(BUDGET_FILE, ".."), { recursive: true });
        writeFileSync(BUDGET_FILE, JSON.stringify(updated, null, 2) + "\n");
    }
});
