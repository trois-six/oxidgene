// Trace continuity in the browser: each page display is one trace.
//
// Drift it prevents: a request a page sends outside the page's load trace —
// a plain `use_resource`, a request started before the trace opens — which
// then reaches the backend as a trace of its own, or with no trace context
// at all, so the backend's spans cannot be told apart by screen
// (docs/cross-cutting.md, logs and traces; the backend side of the chain is
// `trace_continuity_test`).
//
// The bundle exports traces only when an OTLP endpoint is configured, so the
// test serves a runtime configuration naming one and answers its exports
// itself. On every display — a cold load, then navigations inside the
// application — every API request must carry a `traceparent`, all of one
// display must share its trace id, and two displays must not share one. A
// display the client cache serves whole sends nothing, and is skipped.

import type { Page } from "@playwright/test";

import { apiUrl, expect, test } from "./fixtures";

const DISPLAYS: Array<{ name: string; url: (t: { treeId: string; anchorId: string }) => string; content: string }> = [
    { name: "home", url: () => "/", content: ".tree-card-name" },
    { name: "pedigree", url: (t) => `/trees/${t.treeId}`, content: "g.ped-card" },
    { name: "person", url: (t) => `/trees/${t.treeId}/persons/${t.anchorId}`, content: ".pd-header-top" },
    { name: "search", url: (t) => `/trees/${t.treeId}/search?last=Ashdown&first=&origin=`, content: ".search-person-result" },
    { name: "statistics", url: (t) => `/trees/${t.treeId}/statistics`, content: ".stats-tile, .stats-card, .stats-section" },
    { name: "home again", url: () => "/", content: ".tree-card-name" },
];

/// Wait until no API request has started for `quietMs`.
async function quiet(page: Page, count: () => number, quietMs = 800): Promise<void> {
    let last = -1;
    let since = Date.now();
    for (let i = 0; i < 200; i++) {
        await page.waitForTimeout(100);
        if (count() !== last) {
            last = count();
            since = Date.now();
        } else if (Date.now() - since >= quietMs) {
            return;
        }
    }
}

test("every page display sends its API requests in one trace of its own", async ({ page, tree }) => {
    // The deployed bundle reads its OTLP endpoint here; the API origin is
    // already allowed by the Content-Security-Policy.
    await page.route("**/runtime-config.js", (route) =>
        route.fulfill({ contentType: "text/javascript", body: `globalThis.OXIDGENE_OTLP_ENDPOINT = "${apiUrl}";` }),
    );
    await page.route(`${apiUrl}/v1/traces`, (route) =>
        route.fulfill({ status: 200, headers: { "access-control-allow-origin": "*" }, body: "" }),
    );
    const requests: Array<{ url: string; traceparent: string | undefined }> = [];
    page.on("request", (request) => {
        const url = request.url();
        if (!url.startsWith(apiUrl) || url.includes("/v1/traces") || request.method() === "OPTIONS") return;
        requests.push({ url: url.slice(apiUrl.length), traceparent: request.headers().traceparent });
    });

    let previous: string | null = null;
    let traced = 0;
    for (const [index, display] of DISPLAYS.entries()) {
        const before = requests.length;
        if (index === 0) {
            await page.goto(display.url(tree));
        } else {
            await page.evaluate((url) => {
                history.pushState({}, "", url);
                window.dispatchEvent(new PopStateEvent("popstate", { state: {} }));
            }, display.url(tree));
        }
        await expect(page.locator(display.content).first()).toBeVisible();
        await quiet(page, () => requests.length);

        const sent = requests.slice(before);
        // A display the client cache serves whole sends nothing to trace.
        if (sent.length === 0) continue;
        traced += 1;
        const untraced = sent.filter((r) => !r.traceparent).map((r) => r.url);
        expect(untraced, `${display.name}: requests without a traceparent`).toEqual([]);
        const traces = new Set(sent.map((r) => r.traceparent!.split("-")[1]));
        expect([...traces], `${display.name}: its requests span several traces: ${sent.map((r) => `${r.url} ${r.traceparent}`).join(", ")}`).toHaveLength(1);
        const [trace] = traces;
        expect(trace, `${display.name} reuses the previous display's trace`).not.toBe(previous);
        previous = trace;
    }
    expect(traced, "displays that sent API requests").toBeGreaterThanOrEqual(4);
});
