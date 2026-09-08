import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("../src/download.js", import.meta.url), "utf8");
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const run = new AsyncFunction("fileName", "window", "dioxus", "fetch", "URL", "document", "setTimeout", source);
const endpoint = "https://example.invalid/api/v1/artifact";

function fixture({ picker, response = new Response("file"), fetchError } = {}) {
    const messages = [];
    const requests = [];
    const links = [];
    const blobs = [];
    const timers = [];
    const revoked = [];
    let sendEndpoint;
    const channel = new Promise(resolve => { sendEndpoint = resolve; });
    const window = picker ? { showSaveFilePicker: picker } : {};
    const dioxus = { send: message => messages.push(message), recv: () => channel };
    const fetch = async url => {
        requests.push(url);
        if (fetchError) throw fetchError;
        return response;
    };
    const URL = {
        createObjectURL(blob) { blobs.push(blob); return "blob:local-download"; },
        revokeObjectURL(url) { revoked.push(url); },
    };
    const document = {
        body: { appendChild: link => links.push(link) },
        createElement: () => ({
            click() { this.clicked = true; },
            remove() { this.removed = true; },
        }),
    };
    const promise = run("neutral-file.zip", window, dioxus, fetch, URL, document,
        (callback, delay) => timers.push({ callback, delay }));
    return { promise, sendEndpoint, messages, requests, links, blobs, timers, revoked };
}

test("opens the picker synchronously and pipes chunks without buffering a Blob", async () => {
    let picked = false;
    let finish;
    const complete = new Promise(resolve => { finish = resolve; });
    let firstWritten;
    const written = new Promise(resolve => { firstWritten = resolve; });
    const chunks = [];
    let closed = false;
    const response = new Response(new ReadableStream({
        async start(controller) {
            controller.enqueue(new Uint8Array([1, 2]));
            await complete;
            controller.enqueue(new Uint8Array([3, 4]));
            controller.close();
        },
    }));
    response.blob = () => assert.fail("streaming must not buffer the response");
    const writable = new WritableStream({
        write(chunk) { chunks.push(...chunk); firstWritten(); },
        close() { closed = true; },
    });
    const f = fixture({
        picker: options => {
            picked = true;
            assert.equal(options.suggestedName, "neutral-file.zip");
            return { createWritable: async () => writable };
        },
        response,
    });
    assert.equal(picked, true);
    f.sendEndpoint(endpoint);
    await written;
    assert.deepEqual(chunks, [1, 2]);
    assert.equal(closed, false);
    finish();
    assert.equal(await f.promise, "saved");
    assert.deepEqual(chunks, [1, 2, 3, 4]);
    assert.equal(closed, true);
    assert.deepEqual(f.messages, ["ready"]);
    assert.deepEqual(f.requests, [endpoint]);
    assert.equal(f.blobs.length, 0);
    assert.equal(f.links.length, 0);
});

test("picker cancellation does not fetch or fall back to a Blob", async () => {
    const f = fixture({ picker: () => { throw new DOMException("cancel", "AbortError"); } });
    assert.equal(await f.promise, "cancelled");
    assert.deepEqual(f.messages, ["cancelled"]);
    assert.deepEqual(f.requests, []);
    assert.deepEqual(f.blobs, []);
});

test("picker failures are not mistaken for cancellation", async () => {
    const f = fixture({ picker: () => { throw new Error("permission denied"); } });
    assert.equal(await f.promise, "failed");
    assert.deepEqual(f.messages, ["failed"]);
    assert.deepEqual(f.requests, []);
});

test("failed export preparation releases the session without creating a writer", async () => {
    const f = fixture({ picker: () => ({ createWritable: () => assert.fail("no artifact") }) });
    f.sendEndpoint(null);
    assert.equal(await f.promise, "cancelled");
    assert.deepEqual(f.requests, []);
});

test("write permission failures do not fetch the artifact", async () => {
    const f = fixture({ picker: () => ({ createWritable: () => { throw new Error("denied"); } }) });
    f.sendEndpoint(endpoint);
    assert.equal(await f.promise, "failed");
    assert.deepEqual(f.requests, []);
});

for (const failure of ["http", "network", "truncated"]) {
    test(`${failure} failure aborts the writable without committing a file`, async () => {
        let aborted = false;
        let closed = false;
        const writable = new WritableStream({
            abort() { aborted = true; },
            close() { closed = true; },
        });
        const response = failure === "http"
            ? new Response("not found", { status: 404 })
            : new Response(new ReadableStream({ start(c) { c.error(new Error("truncated")); } }));
        const f = fixture({
            picker: () => ({ createWritable: () => writable }),
            response,
            fetchError: failure === "network" ? new Error("offline") : undefined,
        });
        f.sendEndpoint(endpoint);
        assert.equal(await f.promise, "failed");
        assert.equal(aborted, true);
        assert.equal(closed, false);
    });
}

test("unsupported browsers save a native Blob and navigate only to its local URL", async () => {
    const f = fixture();
    f.sendEndpoint(endpoint);
    assert.equal(await f.promise, "saved");
    assert.equal(await f.blobs[0].text(), "file");
    assert.equal(f.links[0].href, "blob:local-download");
    assert.equal(f.links[0].download, "neutral-file.zip");
    assert.equal(f.links[0].clicked, true);
    assert.equal(f.links[0].removed, true);
    assert.deepEqual(f.revoked, []);
    assert.equal(f.timers[0].delay, 60000);
    f.timers[0].callback();
    assert.deepEqual(f.revoked, ["blob:local-download"]);
});

test("fallback never downloads an HTTP error as a file", async () => {
    const f = fixture({ response: new Response("failure", { status: 500 }) });
    f.sendEndpoint(endpoint);
    assert.equal(await f.promise, "failed");
    assert.deepEqual(f.blobs, []);
    assert.deepEqual(f.links, []);
});
