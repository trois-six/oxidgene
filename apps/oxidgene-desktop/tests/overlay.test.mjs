import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { clock, parse } from "./fake_dom.mjs";

const source = await readFile(new URL("../src/archives/overlay.js", import.meta.url), "utf8");
const script = new Function("overlay", "window", "document", "setInterval", "clearInterval", "Date", source);

const ID = "oxidgene-archive-progress";

// The overlay of a search step, as the window sends it.
const searching = {
    heading: "Looking for the cited register…",
    archive: "Archives of Example",
    citation: "AD00 - Exampleville - (aucun) - N - 1877",
    step: "Searching for the register…",
    elapsed: 0,
    seconds: "{seconds} s",
    cancel: "Cancel",
};

// A portal page the window renders overlays on, as it would evaluate them.
function portal({ dark = false } = {}) {
    const document = parse("<main>Registres paroissiaux</main>");
    const timers = clock();
    const posted = [];
    const window = {
        ipc: { postMessage: message => posted.push(JSON.parse(message)) },
        matchMedia: query => ({ matches: dark && query.includes("dark") }),
    };
    const render = overlay =>
        script(overlay, window, document, timers.setInterval, timers.clearInterval, { now: timers.now });
    const host = () => document.getElementById(ID);
    const part = name => host().shadowRoot.querySelector(`[data-part="${name}"]`);
    return { document, timers, posted, render, host, part };
}

test("covers the page with the archive, the citation and the step", () => {
    const page = portal();
    page.render(searching);
    const host = page.host();
    assert.ok(host);
    assert.match(host.style.cssText, /position:fixed;inset:0;z-index:2147483647/);
    assert.match(host.style.cssText, /background:#f4f4f5/);
    assert.equal(page.part("heading").textContent, searching.heading);
    assert.equal(page.part("archive").textContent, searching.archive);
    assert.equal(page.part("citation").textContent, searching.citation);
    assert.equal(page.part("step").textContent, searching.step);
    // The steps are a polite status; the elapsed time is kept out of it.
    const status = host.shadowRoot.querySelector('[role="status"]');
    assert.equal(status.getAttribute("aria-live"), "polite");
    assert.ok(status.querySelector('[data-part="step"]'));
    assert.equal(status.querySelector('[data-part="elapsed"]'), null);
    assert.equal(page.part("elapsed").getAttribute("aria-hidden"), "true");
    assert.equal(page.part("cancel").textContent, "Cancel");
    assert.equal(page.part("cancel").hasAttribute("hidden"), false);
    // The page itself is left as it is.
    assert.equal(page.document.querySelector("main").textContent, "Registres paroissiaux");
});

test("follows the reader's dark scheme", () => {
    const page = portal({ dark: true });
    page.render(searching);
    assert.match(page.host().style.cssText, /background:#1c1c1e/);
});

test("a new step updates the same overlay in place", () => {
    const page = portal();
    page.render({ ...searching, step: "Connecting to the archive portal…" });
    const heading = page.part("heading");
    page.render(searching);
    assert.equal(page.document.querySelectorAll(`#${ID}`).length, 1);
    assert.equal(page.part("heading"), heading);
    assert.equal(page.part("step").textContent, searching.step);
});

test("shows how long a step lasts from 3 seconds on, every second", () => {
    const page = portal();
    page.render(searching);
    assert.equal(page.part("elapsed").textContent, "");
    page.timers.advance(2999);
    assert.equal(page.part("elapsed").textContent, "");
    page.timers.advance(1);
    assert.equal(page.part("elapsed").textContent, "3 s");
    page.timers.advance(1000);
    assert.equal(page.part("elapsed").textContent, "4 s");
    // A new step starts its own count, with one timer only.
    page.render({ ...searching, elapsed: 0 });
    assert.equal(page.part("elapsed").textContent, "");
    assert.equal(page.timers.pending, 1);
    // A step already under way, on a document that just started.
    page.render({ ...searching, elapsed: 7400 });
    assert.equal(page.part("elapsed").textContent, "7 s");
});

test("gives way, or is gone on landing, when rendered with nothing", () => {
    const page = portal();
    page.render(searching);
    page.render(null);
    assert.equal(page.host(), null);
    assert.equal(page.timers.pending, 0);
    // Removing it again changes nothing; it may come back.
    page.render(null);
    page.render(searching);
    assert.ok(page.host());
});

test("the landing step has nothing left to cancel", () => {
    const page = portal();
    page.render(searching);
    page.render({ ...searching, step: "Opening view 5…", cancel: null });
    assert.equal(page.part("step").textContent, "Opening view 5…");
    assert.equal(page.part("cancel").hasAttribute("hidden"), true);
});

test("cancel asks the window to stop the lookup and removes the overlay", () => {
    const page = portal();
    page.render(searching);
    page.part("cancel").click();
    assert.deepEqual(page.posted, [{ kind: "cancel" }]);
    assert.equal(page.host(), null);
    assert.equal(page.timers.pending, 0);
});

test("texts are set as text", () => {
    const page = portal();
    page.render({ ...searching, citation: "AD00 - <b>Exampleville</b>" });
    assert.equal(page.part("citation").textContent, "AD00 - <b>Exampleville</b>");
    assert.equal(page.part("citation").children.length, 0);
});
