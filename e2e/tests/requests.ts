// What the request-budget and trace-continuity specs share: seeding a tree
// with pictures, and recording the API requests a page makes.

import { createHash } from "node:crypto";
import { crc32, deflateSync } from "node:zlib";

import type { APIRequestContext, CDPSession, Page } from "@playwright/test";

import { apiUrl, expect, seedTree, type SeededTree } from "./fixtures";

/// A PNG of `width` × `height` with a gradient seeded by `seed`: fictitious
/// pictures, generated, so no binary fixture is committed.
export function png(width: number, height: number, seed: number): Buffer {
    const raw = Buffer.alloc((width * 3 + 1) * height);
    for (let y = 0; y < height; y++) {
        const row = y * (width * 3 + 1);
        for (let x = 0; x < width; x++) {
            raw[row + 1 + x * 3] = (x * 7 + seed * 31) % 256;
            raw[row + 2 + x * 3] = (y * 5 + seed * 17) % 256;
            raw[row + 3 + x * 3] = (x + y + seed * 13) % 256;
        }
    }
    const chunk = (type: string, data: Buffer) => {
        const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
        const length = Buffer.alloc(4);
        length.writeUInt32BE(data.length);
        const crc = Buffer.alloc(4);
        crc.writeUInt32BE(crc32(body) >>> 0);
        return Buffer.concat([length, body, crc]);
    };
    const header = Buffer.alloc(13);
    header.writeUInt32BE(width, 0);
    header.writeUInt32BE(height, 4);
    header.set([8, 2, 0, 0, 0], 8);
    return Buffer.concat([
        Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
        chunk("IHDR", header),
        chunk("IDAT", deflateSync(raw)),
        chunk("IEND", Buffer.alloc(0)),
    ]);
}

export interface PicturedTree extends SeededTree {
    /// The anchor's family as a spouse, the couple page's family.
    familyId: string;
    /// A grandparent of the anchor, for the kinship page.
    kinId: string;
}

async function ok(response: Awaited<ReturnType<APIRequestContext["get"]>>): Promise<any> {
    expect(response.ok(), await response.text()).toBeTruthy();
    const text = await response.text();
    return text ? JSON.parse(text) : null;
}

/// The fixture tree with what a real one shows: portraits on block 0's
/// three generations, a group photograph whose crops are portraits, a
/// three-page document on the anchor and its birth, a note and a citation.
export async function picturedTree(request: APIRequestContext, name: string): Promise<PicturedTree> {
    const tree = await seedTree(request, name);
    const api = (path: string) => `${apiUrl}/api/v1/trees/${tree.treeId}${path}`;
    const post = async (path: string, data: unknown) => ok(await request.post(api(path), { data }));
    const document = async (title: string) => (await post("/media/document", { title })).id as string;
    const page = async (documentId: string, bytes: Buffer, file: string) =>
        ok(
            await request.post(api("/media/upload"), {
                multipart: {
                    file: { name: file, mimeType: "image/png", buffer: bytes },
                    document_id: documentId,
                },
            }),
        );

    const profiles = (await ok(await request.get(api("/profiles?first=100")))).edges.map(
        (edge: { node: any }) => edge.node,
    );
    const anchor = profiles.find((p: any) => p.person_id === tree.anchorId);
    const familyId = anchor.families_as_spouse[0].family_id as string;
    const ancestors = await ok(await request.get(api(`/persons/${tree.anchorId}/ancestors`)));
    const kinId = ancestors.find((a: any) => a.depth === 2).person_id as string;
    const portrayed = [tree.anchorId, ...ancestors.map((a: any) => a.person_id)];

    for (const [index, personId] of portrayed.entries()) {
        const id = await document(`Portrait ${index}`);
        await page(id, png(96, 128, index), `portrait-${index}.png`);
        await post("/media-links", { media_id: id, person_id: personId });
        await request.put(api(`/persons/${personId}/portrait`), { data: { media_id: id } });
    }
    const others = profiles.map((p: any) => p.person_id).filter((id: string) => !portrayed.includes(id));
    const group = await document("Family group");
    const scan = await page(group, png(400, 240, 99), "group.png");
    for (const [index, personId] of others.slice(0, 3).entries()) {
        const crop = await post(`/media/${scan.id}/vignettes`, {
            x: 20 + index * 120,
            y: 40,
            width: 100,
            height: 140,
            person_id: personId,
        });
        await post("/media-links", { media_id: group, person_id: personId });
        await request.put(api(`/persons/${personId}/portrait`), { data: { vignette_id: crop.id } });
    }
    const register = await document("Fictitious register");
    for (let index = 0; index < 3; index++) await page(register, png(300, 420, 50 + index), `register-${index}.png`);
    await post("/media-links", { media_id: register, person_id: tree.anchorId });
    await post("/media-links", { media_id: register, event_id: anchor.birth.event_id });
    await post("/notes", { text: "A fictitious note", person_id: tree.anchorId });
    const source = await post("/sources", { title: "Fictitious parish register" });
    await post("/citations", { source_id: source.id, person_id: tree.anchorId, page: "f. 12" });
    return { ...tree, familyId, kinId };
}

/// One API request as the browser saw it, times in milliseconds from the
/// first request of the scenario.
export interface Recorded {
    method: string;
    url: string;
    body: string | null;
    start: number;
    end: number | null;
    status: number | string | null;
    bytes: number;
    traceparent: string | null;
}

/// Records the API requests of `page` through the DevTools protocol, with
/// `latency` milliseconds added to each, as a slow network would.
export class Recorder {
    private requests = new Map<string, Recorded>();
    private origin: number | null = null;
    private constructor(readonly cdp: CDPSession) {}

    static async attach(page: Page, latency: number): Promise<Recorder> {
        const cdp = await page.context().newCDPSession(page);
        const recorder = new Recorder(cdp);
        await cdp.send("Network.enable", { maxPostDataSize: 65536 });
        if (latency > 0) {
            await cdp.send("Network.emulateNetworkConditions", {
                offline: false,
                latency,
                downloadThroughput: -1,
                uploadThroughput: -1,
            });
        }
        cdp.on("Network.requestWillBeSent", (event) => {
            const url = event.request.url;
            if (!url.startsWith(apiUrl) || url.includes("/v1/traces")) return;
            const headers = event.request.headers as Record<string, string>;
            recorder.origin ??= event.timestamp * 1000;
            recorder.requests.set(event.requestId, {
                method: event.request.method,
                url: url.slice(apiUrl.length),
                body: event.request.postData
                    ? createHash("sha1").update(event.request.postData).digest("hex").slice(0, 12)
                    : null,
                start: event.timestamp * 1000 - recorder.origin,
                end: null,
                status: null,
                bytes: 0,
                traceparent: headers.traceparent ?? headers.Traceparent ?? null,
            });
        });
        cdp.on("Network.requestWillBeSentExtraInfo", (event) => {
            const recorded = recorder.requests.get(event.requestId);
            const headers = event.headers as Record<string, string>;
            if (recorded && !recorded.traceparent) recorded.traceparent = headers.traceparent ?? null;
        });
        cdp.on("Network.responseReceived", (event) => {
            const recorded = recorder.requests.get(event.requestId);
            if (recorded) recorded.status = event.response.status;
        });
        cdp.on("Network.dataReceived", (event) => {
            const recorded = recorder.requests.get(event.requestId);
            if (recorded) recorded.bytes += event.dataLength;
        });
        cdp.on("Network.loadingFinished", (event) => {
            const recorded = recorder.requests.get(event.requestId);
            if (recorded) recorded.end = event.timestamp * 1000 - (recorder.origin ?? 0);
        });
        cdp.on("Network.loadingFailed", (event) => {
            const recorded = recorder.requests.get(event.requestId);
            if (recorded) {
                recorded.end = event.timestamp * 1000 - (recorder.origin ?? 0);
                recorded.status = `failed: ${event.errorText}`;
            }
        });
        return recorder;
    }

    /// Forget what was recorded so far.
    reset(): void {
        this.requests.clear();
        this.origin = null;
    }

    all(): Recorded[] {
        return [...this.requests.values()].sort((a, b) => a.start - b.start);
    }

    inflight(): number {
        return this.all().filter((r) => r.end === null).length;
    }

    /// Wait until no API request has been in flight for `quietMs`.
    async settle(page: Page, quietMs = 800, maxMs = 20_000): Promise<void> {
        const deadline = Date.now() + maxMs;
        let quietSince: number | null = null;
        let count = -1;
        while (Date.now() < deadline) {
            await page.waitForTimeout(100);
            const now = this.all().length;
            if (this.inflight() === 0 && now === count) {
                quietSince ??= Date.now();
                if (Date.now() - quietSince >= quietMs) return;
            } else {
                quietSince = null;
            }
            count = now;
        }
    }
}

/// The structural metrics of a page's requests: deterministic, so a budget
/// can hold them.
export interface Metrics {
    requests: number;
    depth: number;
    duplicates: number;
    bytes: number;
}

/// The metrics of `recorded`. The depth is the longest chain of requests
/// each starting after the previous ended: the waterfall's levels.
export function metrics(recorded: Recorded[]): Metrics {
    const requests = recorded.filter((r) => r.method !== "OPTIONS");
    const level = new Map<Recorded, number>();
    for (const request of requests) {
        let best = 0;
        for (const before of requests) {
            if (before !== request && before.end !== null && before.end <= request.start + 0.5) {
                best = Math.max(best, level.get(before) ?? 1);
            }
        }
        level.set(request, best + 1);
    }
    const seen = new Map<string, number>();
    for (const request of requests) {
        if (typeof request.status === "string") continue;
        const key = `${request.method} ${request.url} ${request.body}`;
        seen.set(key, (seen.get(key) ?? 0) + 1);
    }
    return {
        requests: requests.length,
        depth: Math.max(0, ...level.values()),
        duplicates: [...seen.values()].reduce((sum, n) => sum + n - 1, 0),
        bytes: requests.reduce((sum, r) => sum + r.bytes, 0),
    };
}

/// A request's route with its ids erased, for messages.
export function template(request: Recorded): string {
    const [path] = request.url.split("?");
    return `${request.method} ${path.replace(/[0-9a-f]{8}-[0-9a-f-]{27}/g, "{id}")}`;
}
