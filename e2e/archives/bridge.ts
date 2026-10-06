// The page end of `archives-live-bridge` (crates/oxidgene-archives/src/live/
// bridge.rs): the Rust check of one archive sends JSON lines asking to load
// a portal's start page or to run one request with the page's own `fetch`,
// and this answers each in turn, as the desktop's archive window does
// (docs/archives.md §4.2). No adapter logic lives here: the requests, their
// order and every verdict come from Rust.

import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

import type { Page } from "@playwright/test";

import { type CollectionReport, antiBotPage } from "./report";

// The native transport's bound on one request (transport.rs, TIMEOUT).
const REQUEST_TIMEOUT_MS = 30_000;
// How long a portal page, a challenge's redirect included, may take.
const LOAD_TIMEOUT_MS = 30_000;

interface Connect {
    kind: "connect";
    start: string;
    origins: string[];
}

interface Fetch {
    kind: "fetch";
    ticket: number;
    method: string;
    url: string;
    headers: Record<string, string>;
    body: string | null;
}

interface Report {
    kind: "report";
    collections: CollectionReport[];
    // The collections the native test checks instead.
    native: number[];
}

type Message = Connect | Fetch | Report;

function originOf(url: string): string | null {
    try {
        return new URL(url).origin;
    } catch {
        return null;
    }
}

type PageState = "portal" | "challenge" | "blocked";

// What the page on screen is, as the desktop's archive window tells it
// (apps/oxidgene-desktop/src/archives/page.js): an anti-bot check or block
// by the adapters' signatures, otherwise the portal once it renders text or
// a frameset with a frame; null while it shows nothing or navigates.
async function pageState(page: Page, origins: string[]): Promise<PageState | null> {
    if (!origins.includes(originOf(page.url()) ?? "")) return null;
    try {
        const signature = antiBotPage(await page.content());
        if (signature) return signature.guard === "block" ? "blocked" : "challenge";
        const rendered = await page.evaluate(() => {
            const body = document.body;
            if (document.readyState !== "complete" || !body) return false;
            return body.localName === "frameset" ? !!body.querySelector("frame") : body.innerText.trim().length > 0;
        });
        return rendered ? "portal" : null;
    } catch {
        // The page is navigating, as a check does once passed.
        return null;
    }
}

// Loads the start page and waits until it is the portal's own page. A
// check that clears itself is waited out; one that stays, which only a
// reader may answer, ends the check `challenged`, as a block does at once.
async function connect(page: Page, start: string, origins: string[]): Promise<string | null> {
    const current = new URL(page.url());
    const wanted = new URL(start);
    // Already on the start page with its cookies: the check does not load it
    // again for every collection it resolves in.
    if (current.origin === wanted.origin && current.pathname === wanted.pathname && (await pageState(page, origins)) === "portal") {
        return null;
    }
    let status = 0;
    try {
        const response = await page.goto(start, { waitUntil: "load", timeout: LOAD_TIMEOUT_MS });
        status = response?.status() ?? 0;
    } catch (error) {
        return error instanceof Error && error.name === "TimeoutError" ? "timeout" : "network";
    }
    if (status >= 500) return "network";
    // A portal that sends an agent identifying itself to another site
    // refuses it, as a challenge does; the check goes no further there.
    if (!origins.includes(originOf(page.url()) ?? "")) {
        await page.goto("about:blank").catch(() => undefined);
        return "challenged";
    }
    const deadline = Date.now() + LOAD_TIMEOUT_MS;
    let checked = false;
    while (Date.now() < deadline) {
        const state = await pageState(page, origins);
        if (state === "blocked") return "challenged";
        // A refusal status over the portal's own page refuses it too.
        if (state === "portal") return status === 403 || status === 429 ? "challenged" : null;
        checked ||= state === "challenge";
        await page.waitForTimeout(500);
    }
    return checked ? "challenged" : "timeout";
}

// Runs one request as the page's own `fetch`, with the portal's cookies, as
// the desktop window does: none on another origin, whose CORS may then admit
// any origin (`*`).
async function pageFetch(page: Page, message: Fetch): Promise<Record<string, unknown>> {
    const options = {
        method: message.method,
        headers: message.headers,
        body: message.body,
        credentials: "same-origin" as const,
        redirect: "follow" as const,
        cache: "no-store" as const,
    };
    try {
        return await page.evaluate(
            async ({ url, options, timeout }) => {
                try {
                    const response = await fetch(url, { ...options, signal: AbortSignal.timeout(timeout) });
                    return { status: response.status, url: response.url, body: await response.text() };
                } catch (error) {
                    return { error: error instanceof Error && error.name === "TimeoutError" ? "timeout" : "network" };
                }
            },
            { url: message.url, options, timeout: REQUEST_TIMEOUT_MS },
        );
    } catch {
        return { error: "network" };
    }
}

// Runs the bridge binary on one archive and answers its messages in `page`
// until it reports its browser collections, naming the native ones.
export async function runBridge(binary: string, archive: string, page: Page): Promise<Omit<Report, "kind">> {
    const child = spawn(binary, [archive], { stdio: ["pipe", "pipe", "inherit"] });
    const exited = new Promise<number | null>((resolve) => child.on("close", resolve));
    const send = (answer: Record<string, unknown>) => child.stdin.write(`${JSON.stringify(answer)}\n`);
    let report = null as Report | null;
    for await (const line of createInterface({ input: child.stdout })) {
        const message = JSON.parse(line) as Message;
        switch (message.kind) {
            case "connect": {
                const error = await connect(page, message.start, message.origins);
                send(error ? { kind: "connected", error } : { kind: "connected" });
                break;
            }
            case "fetch":
                send({ kind: "fetched", ticket: message.ticket, ...(await pageFetch(page, message)) });
                break;
            case "report":
                report = message;
                child.stdin.end();
                break;
        }
    }
    const code = await exited;
    if (code !== 0 || report === null) {
        throw new Error(`archives-live-bridge ${archive} exited with ${code} without a report`);
    }
    return { collections: report.collections, native: report.native };
}
