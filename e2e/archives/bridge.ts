// The page end of `archives-live-bridge` (crates/oxidgene-archives/src/live/
// bridge.rs): the Rust check of one archive sends JSON lines asking to load
// a portal's start page or to run one request with the page's own `fetch`,
// and this answers each in turn, as the desktop's archive window does
// (docs/archives.md §4.2). No adapter logic lives here: the requests, their
// order and every verdict come from Rust.

import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

import type { Page } from "@playwright/test";

import { type CollectionReport, isChallenge } from "./report";

// The native transport's bound on one request (transport.rs, TIMEOUT).
const REQUEST_TIMEOUT_MS = 10_000;
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

// Whether the page is a loaded page of the portal rather than a challenge,
// which renders nothing but a script that navigates on.
async function rendersThePortal(page: Page, origins: string[]): Promise<boolean> {
    if (!origins.includes(originOf(page.url()) ?? "")) return false;
    try {
        return await page.evaluate(
            () => document.readyState === "complete" && !!document.body && document.body.innerText.trim().length > 0,
        );
    } catch {
        // The page is navigating, as a challenge does once passed.
        return false;
    }
}

// Loads the start page and waits until it is the portal's own page.
async function connect(page: Page, start: string, origins: string[]): Promise<string | null> {
    const current = new URL(page.url());
    const wanted = new URL(start);
    // Already on the start page with its cookies: the check does not load it
    // again for every collection it resolves in.
    if (current.origin === wanted.origin && current.pathname === wanted.pathname && (await rendersThePortal(page, origins))) {
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
    while (Date.now() < deadline) {
        if (await rendersThePortal(page, origins)) {
            // A challenge answering with its own refusal page.
            const refused = status === 403 || status === 429 || isChallenge(await page.content().catch(() => ""));
            return refused ? "challenged" : null;
        }
        await page.waitForTimeout(500);
    }
    return "challenged";
}

// Runs one request as the page's own `fetch`, with the portal's cookies.
async function pageFetch(page: Page, message: Fetch): Promise<Record<string, unknown>> {
    const options = {
        method: message.method,
        headers: message.headers,
        body: message.body,
        credentials: "include" as const,
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
