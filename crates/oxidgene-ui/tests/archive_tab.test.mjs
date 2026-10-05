import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("../src/archive_viewer/archive_tab.js", import.meta.url), "utf8");
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const run = new AsyncFunction("searching", "window", "dioxus", source);
const portal = "https://archives.example.org/search?detail=1";

// A blank tab's document, recording what the script puts in it.
function blankDocument() {
    const appended = [];
    return {
        title: "",
        appended,
        body: { appendChild: element => appended.push(element) },
        createElement: tag => ({
            tag,
            click() { this.clicked = true; },
        }),
    };
}

function fixture({ blocked = false } = {}) {
    const opened = [];
    const document = blankDocument();
    const tab = { opener: "this page", closed: false, document, close() { this.closed = true; } };
    const window = {
        open(url, target, features) {
            opened.push({ url, target, features });
            return blocked || features ? null : tab;
        },
    };
    let send;
    const channel = new Promise(resolve => { send = resolve; });
    const dioxus = { recv: () => channel };
    const promise = run("Looking for the register…", window, dioxus);
    return { promise, send, opened, tab, document };
}

test("opens a blank tab during the click, cut from the page", () => {
    const { opened, tab, document } = fixture();
    assert.deepEqual(opened, [{ url: "", target: "_blank", features: undefined }]);
    assert.equal(tab.opener, null);
    assert.equal(document.title, "Looking for the register…");
    assert.equal(document.appended[0].textContent, "Looking for the register…");
});

test("sends the tab to the address through a link without referrer", async () => {
    const { promise, send, opened, document } = fixture();
    send(portal);
    await promise;
    assert.equal(opened.length, 1);
    const link = document.appended[1];
    assert.equal(link.tag, "a");
    assert.equal(link.href, portal);
    assert.equal(link.rel, "noopener noreferrer");
    assert.equal(link.clicked, true);
});

test("opens the address afterwards when the blank tab was refused", async () => {
    const { promise, send, opened } = fixture({ blocked: true });
    send(portal);
    await promise;
    assert.deepEqual(opened[1], { url: portal, target: "_blank", features: "noopener,noreferrer" });
});

test("leaves a tab the reader closed, and closes one with no address", async () => {
    const closed = fixture();
    closed.tab.closed = true;
    closed.send(portal);
    await closed.promise;
    assert.equal(closed.document.appended.length, 1);

    const unsafe = fixture();
    unsafe.send("javascript:alert(1)");
    await unsafe.promise;
    assert.equal(unsafe.tab.closed, true);
    assert.equal(unsafe.document.appended.length, 1);
});
