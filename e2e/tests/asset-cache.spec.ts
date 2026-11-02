// Versioned pictures and the browser cache (docs/architecture.md §4.3).
//
// Every picture the application draws has an address that changes with its
// content, answered `immutable`: a page visited again draws its pictures
// from the browser's cache without asking the server anything.
//
// The first test checks that on a person page and a pedigree view, through
// the DevTools protocol: on the second visit — a full navigation in the same
// browser, after a page of another kind — no picture reaches the network.
//
// The second, opt-in (`E2E_ASSET_REPORT=1 just e2e tests/asset-cache.spec.ts`),
// measures both pages on a first and a second visit, with 50 ms of latency
// added to each request: the API requests that reached the network, their
// bytes as transferred, the pictures the cache answered, and the time to the
// page's content and to its last picture. It writes them to
// test-results/asset-cache-report.json and compares nothing: timings are a
// runner's noise.

import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import type { BrowserContext, CDPSession, Page } from "@playwright/test";

import { apiUrl, expect, test } from "./fixtures";
import { type PicturedTree, blockOutside, picturedTree } from "./requests";

const REPORT_FILE = join(import.meta.dirname, "..", "test-results", "asset-cache-report.json");
const LATENCY_MS = 50;

/// A picture of the API: a medium's thumbnail, card or file, a vignette's
/// crop, a user's avatar, or the batch that inlined pictures before them.
const PICTURE = /\/api\/v1\/(?:trees\/[^/]+\/(?:media\/[^/]+\/(?:thumbnail|card|file)|vignettes\/[^/]+\/(?:image|card)|image-data)|users\/[^/]+\/avatar)(?:\?|$)/;

interface Request {
    url: string;
    /// Wall-clock milliseconds; `skew` turns a protocol timestamp into one.
    start: number;
    skew: number;
    end: number | null;
    cached: boolean;
    transferred: number;
}

/// Records every request of a page through the DevTools protocol, cache
/// hits included, with `LATENCY_MS` added to each that reaches the network.
class Network {
    private requests = new Map<string, Request>();
    private constructor(readonly cdp: CDPSession) {}

    static async attach(context: BrowserContext, page: Page): Promise<Network> {
        const cdp = await context.newCDPSession(page);
        const network = new Network(cdp);
        await cdp.send("Network.enable");
        await cdp.send("Network.emulateNetworkConditions", {
            offline: false,
            latency: LATENCY_MS,
            downloadThroughput: -1,
            uploadThroughput: -1,
        });
        cdp.on("Network.requestWillBeSent", (event) => {
            network.requests.set(event.requestId, {
                url: event.request.url,
                start: event.wallTime * 1000,
                skew: (event.wallTime - event.timestamp) * 1000,
                end: null,
                cached: false,
                transferred: 0,
            });
        });
        cdp.on("Network.requestServedFromCache", (event) => {
            const request = network.requests.get(event.requestId);
            if (request) request.cached = true;
        });
        cdp.on("Network.responseReceived", (event) => {
            const request = network.requests.get(event.requestId);
            const response = event.response;
            if (request && (response.fromDiskCache || response.fromMemoryCache || response.fromPrefetchCache)) {
                request.cached = true;
            }
        });
        cdp.on("Network.loadingFinished", (event) => {
            const request = network.requests.get(event.requestId);
            if (request) {
                request.end = event.timestamp * 1000 + request.skew;
                request.transferred = event.encodedDataLength;
            }
        });
        cdp.on("Network.loadingFailed", (event) => {
            const request = network.requests.get(event.requestId);
            if (request) request.end = event.timestamp * 1000 + request.skew;
        });
        return network;
    }

    reset(): void {
        this.requests.clear();
    }

    api(): Request[] {
        return [...this.requests.values()].filter((r) => r.url.startsWith(`${apiUrl}/api/`) || r.url.startsWith(`${apiUrl}/graphql`));
    }

    /// Wait until no request has been in flight or started for `quietMs`.
    async settle(page: Page, quietMs = 800, maxMs = 20_000): Promise<void> {
        const deadline = Date.now() + maxMs;
        let quietSince: number | null = null;
        let count = -1;
        while (Date.now() < deadline) {
            await page.waitForTimeout(100);
            const all = [...this.requests.values()];
            if (all.every((r) => r.end !== null || r.cached) && all.length === count) {
                quietSince ??= Date.now();
                if (Date.now() - quietSince >= quietMs) return;
            } else {
                quietSince = null;
            }
            count = all.length;
        }
    }
}

interface Target {
    url: (tree: PicturedTree) => string;
    content: string;
}

const TARGETS: Record<string, Target> = {
    person: { url: (t) => `/trees/${t.treeId}/persons/${t.anchorId}`, content: ".pd-header-top" },
    pedigree: { url: (t) => `/trees/${t.treeId}`, content: "g.ped-card" },
};
const ELSEWHERE: Target = { url: (t) => `/trees/${t.treeId}/settings`, content: ".settings-section" };

interface Visit {
    /// API requests that reached the network, and their bytes as transferred.
    requests: number;
    bytes: number;
    /// Of those, the pictures (and the batch that inlined them).
    pictureRequests: number;
    pictureBytes: number;
    /// Pictures the browser's cache answered.
    cachedPictures: number;
    /// Milliseconds from the navigation to the content, and to the last
    /// picture's answer, network or cache.
    contentMs: number;
    picturesMs: number;
}

async function newPage(browser: import("@playwright/test").Browser): Promise<{ context: BrowserContext; page: Page; network: Network }> {
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, locale: "en-US", timezoneId: "UTC" });
    const page = await context.newPage();
    await page.routeWebSocket(/\/_dioxus/, () => {});
    const network = await Network.attach(context, page);
    await blockOutside(network.cdp);
    return { context, page, network };
}

async function visit(page: Page, network: Network, tree: PicturedTree, target: Target): Promise<Visit> {
    network.reset();
    const start = Date.now();
    await page.goto(target.url(tree), { waitUntil: "commit" });
    await expect(page.locator(target.content).first()).toBeVisible({ timeout: 30_000 });
    const contentMs = Date.now() - start;
    await network.settle(page);
    const api = network.api();
    const fetched = api.filter((r) => !r.cached);
    const pictures = api.filter((r) => PICTURE.test(r.url));
    const fetchedPictures = pictures.filter((r) => !r.cached);
    const last = Math.max(0, ...pictures.map((r) => (r.end ?? r.start) - start));
    return {
        requests: fetched.length,
        bytes: fetched.reduce((sum, r) => sum + r.transferred, 0),
        pictureRequests: fetchedPictures.length,
        pictureBytes: fetchedPictures.reduce((sum, r) => sum + r.transferred, 0),
        cachedPictures: pictures.length - fetchedPictures.length,
        contentMs,
        picturesMs: Math.round(last),
    };
}

/// The first and the second visit of `target`, a page of another kind
/// between them, each a full navigation in one browser.
async function twice(browser: import("@playwright/test").Browser, tree: PicturedTree, target: Target): Promise<[Visit, Visit]> {
    const { context, page, network } = await newPage(browser);
    try {
        const first = await visit(page, network, tree, target);
        await visit(page, network, tree, ELSEWHERE);
        const second = await visit(page, network, tree, target);
        return [first, second];
    } finally {
        await context.close();
    }
}

test.describe("versioned pictures", () => {
    test("a page visited again draws its pictures from the browser's cache", async ({ browser, playwright }) => {
        test.setTimeout(5 * 60_000);
        const request = await playwright.request.newContext();
        const tree = await picturedTree(request, `E2E asset cache ${Date.now()}`);
        await request.dispose();
        for (const [name, target] of Object.entries(TARGETS)) {
            const [first, second] = await twice(browser, tree, target);
            expect(first.pictureRequests, `${name}: the first visit fetches its pictures`).toBeGreaterThan(0);
            expect(second.pictureRequests, `${name}: the second visit fetches no picture`).toBe(0);
        }
    });

    test("report the requests, bytes and times of a first and a second visit", async ({ browser, playwright }, testInfo) => {
        test.skip(!process.env.E2E_ASSET_REPORT, "opt-in: E2E_ASSET_REPORT=1");
        test.setTimeout(15 * 60_000);
        const request = await playwright.request.newContext();
        const tree = await picturedTree(request, `E2E asset report ${Date.now()}`);
        await request.dispose();
        const runs = Number(process.env.E2E_ASSET_RUNS ?? "5");
        const report: Record<string, { first: Visit[]; second: Visit[] }> = {};
        for (const [name, target] of Object.entries(TARGETS)) {
            report[name] = { first: [], second: [] };
            for (let run = 0; run < runs; run++) {
                const [first, second] = await twice(browser, tree, target);
                report[name].first.push(first);
                report[name].second.push(second);
            }
        }
        mkdirSync(join(REPORT_FILE, ".."), { recursive: true });
        writeFileSync(REPORT_FILE, JSON.stringify(report, null, 2) + "\n");
        await testInfo.attach("asset-cache-report", { path: REPORT_FILE, contentType: "application/json" });
    });
});
