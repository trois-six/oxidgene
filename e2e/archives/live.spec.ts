// The browser half of the live checks (docs/archives.md §9.1), run by
// `just archives-live [archive id]` after the Rust test of the native
// collections:
//
// - steps 1 to 3 of every collection whose portal admits only a browser,
//   through `archives-live-bridge`, the Rust check whose requests run in
//   this page;
// - step 4 of every resolved collection: the target opens and the portal's
//   viewer shows the cited view, after its reuse licence if it asks for one
//   (a cookie banner is refused as the desktop window refuses it);
// - step 5 for a `display: "iiif"` archive: the picture and the thumbnail
//   are images of the resolved proportions.
//
// Each archive is one test. It fails on a drift only: an unreachable or a
// challenged portal is annotated and left to the scheduled workflow.

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

import { expect, test, type Page, type Request, type Response, type Route } from "@playwright/test";

import { runBridge } from "./bridge";
import { type ArchiveReport, type CollectionReport, type Failure, type Opening, type Signature, antiBotName, antiBotPage, drift, nativeReport, record, summary, worst } from "./report";
import { type Viewer, viewers } from "./viewers";

const root = fileURLToPath(new URL("../..", import.meta.url));
const reportDir = process.env.OXIDGENE_LIVE_REPORT_DIR || path.join(root, "target/archives-live");
const bridge = process.env.OXIDGENE_LIVE_BRIDGE || path.join(root, "target/debug/archives-live-bridge");
const only = process.env.OXIDGENE_LIVE_ARCHIVE || "";

// How long a viewer may take to show its view, licence included.
const VIEWER_TIMEOUT_MS = 30_000;

// The desktop window's cookie-consent script (docs/archives.md §6.1), run
// as the window runs it: it refuses a recognized banner, never accepts, so
// that no banner hides the viewer. It posts nothing here.
const consentDir = path.join(root, "apps/oxidgene-desktop/src/archives");
const consentScript = `(() => {\nconst consent = ${readFileSync(path.join(consentDir, "consent.json"), "utf8")};\n${readFileSync(path.join(consentDir, "consent.js"), "utf8")}\n})();`;

// The archives to check, as the Rust catalogue lists them.
const archives = execFileSync(bridge, only ? ["--list", only] : ["--list"], { encoding: "utf8" })
    .split("\n")
    .filter(Boolean);

function unreachable(step: Failure["step"], expected: string, error: unknown): Failure {
    const received = error instanceof Error && error.name === "TimeoutError" ? "timeout" : "network error";
    return { step, outcome: "unreachable", expected, received };
}

// The first or last number an element shows: an input's value or its text
// (`12`, `/ 46`, `5/267`).
async function numberShown(page: Page, selector: string, which: "first" | "last"): Promise<number | null> {
    const element = page.locator(selector).first();
    if ((await element.count()) === 0) return null;
    const text = await element
        .evaluate((node) => (node instanceof HTMLInputElement ? node.value : (node.textContent ?? "")))
        .catch(() => "");
    const numbers = text.match(/\d+/g) ?? [];
    const number = which === "first" ? numbers[0] : numbers[numbers.length - 1];
    return number === undefined ? null : Number(number);
}

// An image request: by the browser's resource type, or by its file
// extension for one a viewer fetches by script.
const IMAGE_PATH = /\.(jpe?g|png|gif|webp|tiff?|jp2)$/i;

function isImageRequest(request: Request): boolean {
    return request.resourceType() === "image" || IMAGE_PATH.test(new URL(request.url()).pathname);
}

// Step 4: the target opens on the cited view, with the viewer's image
// requests aborted where the viewer shows its view without them.
async function open(page: Page, opening: Opening): Promise<Failure | null> {
    const viewer = viewers[opening.platform];
    if (!viewer) return drift("opening", `a viewer of ${opening.platform} in e2e/archives/viewers.ts`, "none");
    if (!viewer.blockImages) return openViewer(page, opening, viewer);
    const block = (route: Route) => (isImageRequest(route.request()) ? route.abort() : route.fallback());
    await page.route("**/*", block);
    try {
        return await openViewer(page, opening, viewer);
    } finally {
        await page.unroute("**/*", block);
    }
}

// The anti-bot page, if any, that answered one of the viewer's own requests
// in place of its data (a manifest, a page list): an F5 script challenging
// a request of a headless browser leaves the viewer with no view to show.
function watchViewerRequests(page: Page): { guard: () => Signature | null; stop: () => void } {
    let found: Signature | null = null;
    const listener = (response: Response) => {
        const type = response.request().resourceType();
        if (found || (type !== "xhr" && type !== "fetch")) return;
        response
            .text()
            .then((body) => {
                found ??= antiBotPage(body);
            })
            .catch(() => undefined);
    };
    page.on("response", listener);
    return { guard: () => found, stop: () => page.off("response", listener) };
}

async function openViewer(page: Page, opening: Opening, viewer: Viewer): Promise<Failure | null> {
    const requests = watchViewerRequests(page);
    try {
        return await showView(page, opening, viewer, requests.guard);
    } finally {
        requests.stop();
    }
}

async function showView(page: Page, opening: Opening, viewer: Viewer, answered: () => Signature | null): Promise<Failure | null> {
    // A target behind a reuse licence: its entry first, where the licence
    // is accepted as a reader accepts it, then the target, as the desktop
    // window goes on to it (docs/archives.md §6.1).
    if (opening.licence) {
        const failure = await passLicence(page, opening.licence, viewer.licence);
        if (failure) return failure;
    }
    // A portal behind a challenge keeps the fragment, which carries the view,
    // only once its cookie is set (docs/archives.md §6.1): the bridge has
    // loaded its start page in this page already.
    try {
        // DOM ready only: a portal's third-party resource may hold `load`
        // back long after the viewer shows, and the loop below waits for it.
        const response = await page.goto(opening.url, { waitUntil: "domcontentloaded", timeout: VIEWER_TIMEOUT_MS });
        if ((response?.status() ?? 0) >= 500) return unreachable("opening", "the target page", new Error("status"));
        // Sent to another site: the portal refuses the identified agent.
        if (new URL(page.url()).origin !== new URL(opening.url).origin) {
            await page.goto("about:blank").catch(() => undefined);
            return { step: "opening", outcome: "challenged", expected: "the portal's viewer", received: "a redirect to another site" };
        }
    } catch (error) {
        return unreachable("opening", "the target page", error);
    }
    if (!viewer.view) return drift("opening", `the view of ${opening.platform} in e2e/archives/viewers.ts`, "none");
    const view = viewer.view;
    let shown: number | null = null;
    const deadline = Date.now() + VIEWER_TIMEOUT_MS;
    while (Date.now() < deadline) {
        if (viewer.licence) {
            const licence = page.locator(viewer.licence).first();
            if (await licence.isVisible().catch(() => false)) await licence.click();
        }
        shown = await numberShown(page, view, "first");
        if (shown === opening.view) break;
        await page.waitForTimeout(500);
    }
    if (shown === null) {
        const blank = await page.evaluate(() => !document.body || !document.body.innerText.trim()).catch(() => true);
        const guard = antiBotPage(await page.content().catch(() => ""));
        if (guard) return { step: "opening", outcome: "challenged", expected: "the portal's viewer", received: `an anti-bot page in place of the viewer: ${antiBotName(guard)}` };
        if (blank) return { step: "opening", outcome: "challenged", expected: "the portal's viewer", received: "a blank page in place of the viewer" };
        return drift("opening", `the viewer showing view ${opening.view}`, "no view number shown");
    }
    if (shown !== opening.view) {
        const guard = answered();
        if (guard) return { step: "opening", outcome: "challenged", expected: `view ${opening.view}`, received: `an anti-bot page answered the viewer's request: ${antiBotName(guard)}` };
        return drift("opening", `view ${opening.view}`, `view ${shown}`);
    }
    if (viewer.viewCount && opening.view_count !== null) {
        const count = await numberShown(page, viewer.viewCount, "last");
        if (count !== opening.view_count) return drift("opening", `${opening.view_count} views`, `${count ?? "no"} views`);
    }
    return null;
}

// The reuse licence a target stands behind: the entry leads to the licence
// page, whose button is clicked as a reader clicks it, and the page the
// portal shows next, behind the licence, must load.
async function passLicence(page: Page, licence: NonNullable<Opening["licence"]>, button?: string): Promise<Failure | null> {
    if (!button) return drift("opening", "the licence button in e2e/archives/viewers.ts", "none");
    try {
        await page.goto(licence.entry, { waitUntil: "domcontentloaded", timeout: VIEWER_TIMEOUT_MS });
    } catch (error) {
        return unreachable("opening", "the portal's entry", error);
    }
    const accept = page.locator(button).first();
    const shown = await accept.waitFor({ state: "visible", timeout: VIEWER_TIMEOUT_MS }).then(
        () => true,
        () => false,
    );
    if (!shown) {
        const guard = antiBotPage(await page.content().catch(() => ""));
        if (guard) return { step: "opening", outcome: "challenged", expected: "the reuse licence", received: `an anti-bot page in place of the entry: ${antiBotName(guard)}` };
        return drift("opening", "the reuse licence", "not shown");
    }
    await accept.click();
    const behind = (url: URL) => url.href.startsWith(licence.scope) && !url.href.startsWith(licence.page);
    const passed = await page.waitForURL(behind, { waitUntil: "domcontentloaded", timeout: VIEWER_TIMEOUT_MS }).then(
        () => true,
        () => false,
    );
    return passed ? null : drift("opening", "the page behind the reuse licence", "not shown");
}

interface Loaded {
    status?: number;
    type?: string;
    width?: number;
    height?: number;
    error?: string;
}

// One image as the page loads it: its status, type and pixel size. An image
// on another origin without CORS is read through an <img>, without its type.
async function loadImage(page: Page, url: string): Promise<Loaded> {
    return page
        .evaluate(async (url) => {
            try {
                const response = await fetch(url, { credentials: "include" });
                const type = response.headers.get("content-type") ?? "";
                if (!response.ok) return { status: response.status, type };
                const bitmap = await createImageBitmap(await response.blob());
                return { status: response.status, type, width: bitmap.width, height: bitmap.height };
            } catch {
                return await new Promise<Loaded>((resolve) => {
                    const image = new Image();
                    image.onload = () => resolve({ status: 200, width: image.naturalWidth, height: image.naturalHeight });
                    image.onerror = () => resolve({ error: "network" });
                    image.src = url;
                });
            }
        }, url)
        .catch(() => ({ error: "network" }));
}

// Step 5: the picture and the thumbnail are images no larger than the
// resolved size, the picture of the resolved proportions.
async function images(page: Page, image: NonNullable<Opening["image"]>): Promise<Failure | null> {
    const ratio = image.width / image.height;
    for (const [role, url] of [
        ["picture", image.picture],
        ["thumbnail", image.thumbnail],
    ] as const) {
        const loaded = await loadImage(page, url);
        if (loaded.error || (loaded.status ?? 0) >= 500) return unreachable("images", `the ${role}`, new Error(loaded.error));
        if (loaded.type !== undefined && !loaded.type.startsWith("image/")) return drift("images", `the ${role} as an image`, `${loaded.status} ${loaded.type || "untyped"}`);
        if (!loaded.width || !loaded.height) return drift("images", `the ${role} as an image`, `${loaded.status} without pixels`);
        // A thumbnail may be the portal's own, square or padded: only the
        // picture is held to the image's proportions.
        const reshaped = role === "picture" && Math.abs(loaded.width / loaded.height - ratio) > ratio * 0.02;
        if (reshaped || Math.max(loaded.width, loaded.height) > Math.max(image.width, image.height)) {
            return drift("images", `the ${role} within ${image.width}×${image.height}, same proportions`, `${loaded.width}×${loaded.height}`);
        }
    }
    return null;
}

async function browserSteps(page: Page, collection: CollectionReport): Promise<void> {
    if (collection.outcome !== "ok" || !collection.opening) return;
    const failure = (await open(page, collection.opening)) ?? (collection.opening.image ? await images(page, collection.opening.image) : null);
    if (failure) {
        collection.outcome = failure.outcome;
        collection.failure = failure;
    }
}

for (const archive of archives) {
    test(archive, async ({ page }) => {
        // Every document of the page, the start pages and the targets alike.
        await page.addInitScript({ content: consentScript });
        const bridged = await runBridge(bridge, archive, page);
        const native = bridged.native.length > 0 ? nativeReport(reportDir, archive) : [];
        const missing = bridged.native.filter((index) => !native?.some((collection) => collection.index === index));
        if (missing.length > 0) {
            throw new Error(`no native report of ${archive}'s collections ${missing.join(", ")} in ${reportDir}: run \`just archives-live\``);
        }
        const collections = [...(native ?? []), ...bridged.collections].sort((a, b) => a.index - b.index);
        for (const collection of collections) await browserSteps(page, collection);

        const report: ArchiveReport = { archive, outcome: worst(collections.map((c) => c.outcome)), collections };
        record(reportDir, report);
        const text = summary(report);
        if (report.outcome !== "ok") test.info().annotations.push({ type: report.outcome, description: text });
        expect(report.outcome, text).not.toBe("drift");
    });
}
