import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { clock, parse } from "./fake_dom.mjs";

const source = await readFile(new URL("../src/archives/banner.js", import.meta.url), "utf8");
const script = new Function("banner", "window", "document", "setTimeout", source);

const HOLD = ['dialog[open], [aria-modal="true"]', "#tarteaucitronAlertBig"];

// Runs the script over `body` with the banner `model`.
function show(body, model, document = parse(body), timers = clock()) {
    const posted = [];
    const window = { ipc: { postMessage: message => posted.push(JSON.parse(message)) } };
    script(Object.assign({ close: "Close", action: null, message: null, hold: [] }, model), window, document, timers.setTimeout);
    const banner = () => document.getElementById("oxidgene-archive-status");
    // The banner itself, in the host's shadow root: its label, its action
    // and its close button.
    const content = () => banner()?.shadowRoot.querySelector('[role="status"]') ?? null;
    return { document, timers, posted, banner, content, shown: () => !!banner() && banner().style.display !== "none" };
}

const attach = {
    text: "OxidGene can keep this view.",
    action: "Attach as a document",
    message: JSON.stringify({ kind: "attach" }),
    hold: HOLD,
};

// The reuse licence of a portal's viewer, as a modal dialog.
const licence = '<div class="overlay"><div role="dialog" aria-modal="true"><p>Réutilisation des informations publiques</p><button>Accepter</button><button>Refuser</button></div></div>';

test("a banner without anything to wait for shows at once, set as text", () => {
    const page = show("<main>Visionneuse</main>", { text: "Go to <b>view</b> 5." });
    assert.ok(page.shown());
    assert.equal(page.content().children[0].textContent, "Go to <b>view</b> 5.");
    assert.equal(page.timers.pending, 0);
    // Out of reach of the portal's style sheets.
    assert.match(page.banner().style.cssText, /^all:initial;/);
    assert.ok(page.banner().shadowRoot.querySelector("style").textContent.includes("background: #1e1a14"));
    // Closed: the window stops showing it.
    page.content().children.at(-1).click();
    assert.equal(page.banner(), null);
    assert.deepEqual(page.posted, [{ kind: "dismiss" }]);
});

test("the offer to attach waits for the portal's licence dialog to be answered", () => {
    const page = show(`<main>Visionneuse</main>${licence}`, attach);
    // On the page, but not on screen over the dialog.
    assert.ok(page.banner());
    assert.ok(!page.shown());
    page.timers.advance(2000);
    assert.ok(!page.shown());
    // The reader accepts: the dialog goes, and the offer shows.
    page.document.querySelector(".overlay").remove();
    page.timers.advance(500);
    assert.ok(page.shown());
    assert.equal(page.banner().style.display, "block");
    assert.equal(page.timers.pending, 0);
    // Its button sends the views.
    page.content().children[1].click();
    assert.deepEqual(page.posted, [{ kind: "attach" }]);
});

test("a cookie banner OxidGene recognizes holds the offer too", () => {
    const page = show('<div id="tarteaucitronAlertBig"><button>Refuser</button></div>', attach);
    assert.ok(!page.shown());
    page.document.getElementById("tarteaucitronAlertBig").setAttribute("hidden", "");
    page.timers.advance(500);
    assert.ok(page.shown());
});

test("a dialog not on screen holds nothing", () => {
    const page = show(`<div style="display: none">${licence}</div><dialog>Fermé</dialog>`, attach);
    assert.ok(page.shown());
    assert.equal(page.timers.pending, 0);
});

test("a banner replaced while it waits stops looking", () => {
    const document = parse(`<main>Visionneuse</main>${licence}`);
    const timers = clock();
    show(null, attach, document, timers);
    show(null, { text: "Go to view 5." }, document, timers);
    assert.equal(document.querySelectorAll("#oxidgene-archive-status").length, 1);
    timers.advance(1000);
    assert.equal(timers.pending, 0);
    const host = document.getElementById("oxidgene-archive-status");
    assert.equal(host.shadowRoot.querySelector('[role="status"]').children[0].textContent, "Go to view 5.");
});

test("the offer to attach is compact", () => {
    const kind = model => show("<main></main>", model).content().getAttribute("class");
    assert.equal(kind(attach), "banner compact");
    assert.equal(kind({ text: "Go to view 5." }), "banner");
});

test("a banner without text is never shown, and removes the one before", () => {
    const document = parse("<main>Visionneuse</main>");
    const timers = clock();
    show(null, { text: "Go to view 5." }, document, timers);
    for (const text of ["", "   ", null]) {
        const page = show(null, { text }, document, timers);
        assert.equal(page.banner(), null, String(text));
    }
});

test("a banner over a licence dialog the window recognizes waits for it", () => {
    // Arkothèque's « licence clic », a full-window overlay.
    const body = '<div class="licence"><p>Réutilisation d\'informations publiques</p><button data-cy="accept-license">J\'accepte</button></div>';
    const page = show(body, { text: "The register counts other views.", hold: ['button[data-cy="accept-license"]'] });
    assert.ok(!page.shown());
    page.document.querySelector(".licence").remove();
    page.timers.advance(500);
    assert.ok(page.shown());
});
