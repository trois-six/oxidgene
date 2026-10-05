import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("../src/archives/page.js", import.meta.url), "utf8");
const antiBot = JSON.parse(
    await readFile(new URL("../../../crates/oxidgene-archives/src/platform/challenges.json", import.meta.url), "utf8"),
);
const run = new Function("antiBot", "window", "document", "addEventListener", "setTimeout", source);

// A loaded document of `html`, whose body shows `text` or is a frameset.
function page({ html, text = "", frames = 0 }) {
    return {
        readyState: "complete",
        documentElement: { outerHTML: html },
        body: frames
            ? { localName: "frameset", innerText: "", querySelector: () => ({}) }
            : { localName: "body", innerText: text, querySelector: () => null },
    };
}

// Runs the script on `document`; `next` runs its pending re-check.
function classify(document) {
    const posted = [];
    let pending = null;
    const window = { ipc: { postMessage: message => posted.push(JSON.parse(message)) } };
    run(antiBot, window, document, () => assert.fail("loaded already"), callback => { pending = callback; });
    return {
        posted,
        next() {
            const callback = pending;
            pending = null;
            callback?.();
        },
        get waiting() {
            return pending !== null;
        },
    };
}

test("a portal page with text is the portal, and is not checked again", () => {
    const run = classify(page({ html: "<html><body><main>Registres</main></body></html>", text: "Registres" }));
    assert.deepEqual(run.posted, [{ kind: "page", state: "portal" }]);
    assert.equal(run.waiting, false);
});

test("a frameset with a frame is the portal, though its body shows no text", () => {
    const run = classify(page({ html: "<html><frameset><frame src=\"a.asp\"></frameset></html>", frames: 1 }));
    assert.deepEqual(run.posted, [{ kind: "page", state: "portal" }]);
});

test("a challenge page with text is not the portal", () => {
    const html = "<html><head><title>Vérification</title><script id=\"anubis_challenge\" type=\"application/json\">{}</script></head><body>Calcul en cours…</body></html>";
    const run = classify(page({ html, text: "Calcul en cours…" }));
    assert.deepEqual(run.posted, [{ kind: "page", state: "challenge", vendor: "anubis", interactive: false }]);
    // Still a check: it is looked at again, without posting the same answer.
    assert.equal(run.waiting, true);
    run.next();
    assert.equal(run.posted.length, 1);
});

test("a widget that appears makes the check interactive", () => {
    const document = page({ html: "<html><head><title>Just a moment...</title></head><body>Verifying</body></html>", text: "Verifying" });
    const run = classify(document);
    assert.equal(run.posted[0].interactive, false);
    document.documentElement.outerHTML = "<html><head><title>Just a moment...</title></head><body><div class=\"cf-turnstile\"></div></body></html>";
    run.next();
    assert.deepEqual(run.posted[1], { kind: "page", state: "challenge", vendor: "cloudflare", interactive: true });
});

test("a block is posted once and not checked again", () => {
    const html = "<html><head><title>Attention Required! | Cloudflare</title></head><body><div id=\"cf-error-details\">Sorry, you have been blocked</div></body></html>";
    const run = classify(page({ html, text: "Sorry, you have been blocked" }));
    assert.deepEqual(run.posted, [{ kind: "page", state: "blocked", vendor: "cloudflare", interactive: false }]);
    assert.equal(run.waiting, false);
});

test("a page that shows nothing yet waits for its content", () => {
    const document = page({ html: "<html><body><div id=\"app\"></div></body></html>" });
    const run = classify(document);
    assert.deepEqual(run.posted, []);
    document.body.innerText = "Résultats";
    run.next();
    assert.deepEqual(run.posted, [{ kind: "page", state: "portal" }]);
});
