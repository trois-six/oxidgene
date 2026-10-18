import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("../src/components/pdf_viewer.js", import.meta.url), "utf8");
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const run = new AsyncFunction("window", "document", "dioxus", source);

// A channel of the eval bridge: what the script sends, and what it receives.
function channel() {
    const sent = [];
    const inbox = [];
    const waiting = [];
    return {
        sent,
        push(message) {
            const next = waiting.shift();
            if (next) next(message);
            else inbox.push(message);
        },
        dioxus: {
            send: message => sent.push(message),
            recv: () => (inbox.length ? Promise.resolve(inbox.shift()) : new Promise(resolve => waiting.push(resolve))),
        },
    };
}

const document = {
    baseURI: "https://app.example.invalid/trees/1",
    getElementById: id => ({ id }),
};

// The script's library, installed once per window, without opening anything.
async function library(window = {}) {
    const bridge = channel();
    bridge.push({ mode: "none" });
    await run(window, document, bridge.dioxus);
    return window.__oxPdf;
}

test("a file sent in chunks is put back together byte for byte", async () => {
    const pdf = await library();
    const bytes = Uint8Array.from({ length: 10 }, (_, index) => index * 25);
    const bridge = channel();
    bridge.push(Buffer.from(bytes.subarray(0, 6)).toString("base64"));
    bridge.push(Buffer.from(bytes.subarray(6)).toString("base64"));
    assert.deepEqual(await pdf.receive({ size: 10 }, bridge.dioxus), bytes);
});

test("a cancel or a missing file ends the transfer", async () => {
    const pdf = await library();
    assert.equal(await pdf.receive({ cancel: true }, channel().dioxus), null);
    const bridge = channel();
    bridge.push({ cancel: true });
    assert.equal(await pdf.receive({ size: 4 }, bridge.dioxus), null);
    await assert.rejects(pdf.receive({ missing: true }, channel().dioxus));
    const overflowing = channel();
    overflowing.push(Buffer.from([1, 2, 3, 4]).toString("base64"));
    await assert.rejects(pdf.receive({ size: 2 }, overflowing.dioxus));
});

test("two previews run at a time, the others wait their turn", async () => {
    const pdf = await library();
    const tickets = [pdf.queue(), pdf.queue(), pdf.queue(), pdf.queue()];
    await Promise.resolve();
    assert.deepEqual(tickets.map(ticket => ticket.started), [true, true, false, false]);
    // A cancelled place is skipped when a turn comes.
    tickets[2].cancel();
    tickets[0].done();
    await tickets[3].ready;
    assert.equal(tickets[3].started, true);
    for (const ticket of tickets) ticket.done();
});

test("a preview drawn once is answered from the session's cache", async () => {
    const pdf = await library();
    pdf.previews.set("page-1", "blob:https://app.example.invalid/1");
    pdf.previews.set("page-2", null);
    for (const [key, answer] of [
        ["page-1", { event: "preview", url: "blob:https://app.example.invalid/1" }],
        ["page-2", { event: "failed" }],
    ]) {
        const bridge = channel();
        bridge.push({ mode: "preview", id: "tile", key });
        await run({ __oxPdf: pdf }, document, bridge.dioxus);
        // Nothing asked for: no file is read for a page already drawn.
        assert.deepEqual(bridge.sent, [answer]);
    }
});

test("each page is named for assistive technology in the interface's language", async () => {
    const pdf = await library({});
    const attributes = new Map([
        ["data-l10n-id", "pdfjs-page-landmark"],
        ["data-l10n-args", JSON.stringify({ page: 3 })],
    ]);
    const page = {
        getAttribute: name => attributes.get(name) ?? null,
        setAttribute: (name, value) => attributes.set(name, value),
        querySelectorAll: () => [],
    };
    await pdf.labels("Seite {page}").translateOnce(page);
    assert.equal(attributes.get("aria-label"), "Seite 3");
});
