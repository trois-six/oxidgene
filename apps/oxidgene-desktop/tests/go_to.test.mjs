import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { clock, parse } from "./fake_dom.mjs";

const source = await readFile(new URL("../src/archives/go_to.js", import.meta.url), "utf8");
const viewers = JSON.parse(
    await readFile(new URL("../../../crates/oxidgene-archives/src/platform/viewers.json", import.meta.url), "utf8"),
);
const script = new Function("viewer", "view", "window", "document", "setTimeout", source);

class Event {
    constructor(type, init = {}) {
        this.type = type;
        this.bubbles = !!init.bubbles;
    }
}

// Like WebKit's before it read the legacy codes: `keyCode` and `which`
// stay 0 whatever the init says.
class KeyboardEvent extends Event {
    constructor(type, init = {}) {
        super(type, init);
        this.key = init.key;
        Object.defineProperty(this, "keyCode", { value: 0, configurable: true });
        Object.defineProperty(this, "which", { value: 0, configurable: true });
    }
}

// Runs the script on `body` for `viewer`, toward `view`; `outcome()`
// advances the clock until the script's promise settles.
function drive(body, viewer, view, setup = () => {}) {
    const document = parse(body);
    const timers = clock();
    setup(document, timers);
    const window = { Event, KeyboardEvent };
    let settled = null;
    script(viewer, view, window, document, timers.setTimeout).then(result => {
        settled = result;
    });
    return {
        document,
        timers,
        async outcome(limit = 60000) {
            for (let elapsed = 0; settled === null && elapsed <= limit; elapsed += 250) {
                await new Promise(resolve => setImmediate(resolve));
                if (settled === null) timers.advance(250);
            }
            await new Promise(resolve => setImmediate(resolve));
            return { result: settled, at: timers.now() };
        },
        events: () => document.dispatched.map(({ event }) => event.type),
    };
}

// A GAIA-like page box: `Page n de total`, which the viewer (jqPagination)
// writes back once the box is left with a number typed in it, a moment
// later.
const gaiaBox = '<div id="pagination"><input type="text" value="Page 1 de 396"></div>';
function gaiaViewer({ on = "blur", total = 396 } = {}) {
    return (document, timers) => {
        const box = document.querySelector("#pagination input");
        box.addEventListener(on, event => {
            if (on === "keydown" && event.keyCode !== 13) return;
            const page = Number(box.value);
            if (page >= 1 && page <= total) timers.setTimeout(() => (box.value = `Page ${page} de ${total}`), 400);
        });
    };
}

test("GAIA: types the view in the page box, leaves it, and waits for the viewer's own writing", async () => {
    const page = drive(gaiaBox, viewers.gaia, 178, gaiaViewer());
    const { result } = await page.outcome();
    assert.deepEqual(result, { state: "shown" });
    const box = page.document.querySelector("#pagination input");
    assert.equal(box.value, "Page 178 de 396");
    // The value, then events: nothing is focused or clicked.
    assert.deepEqual(page.events(), ["input", "blur"]);
    const blur = page.document.dispatched.find(({ event }) => event.type === "blur").event;
    assert.equal(blur.bubbles, false);
    assert.equal(page.document.activeElement, null);
    assert.deepEqual(page.document.clicked, []);
});

test("the Enter key carries its legacy key codes", async () => {
    const viewer = { ...viewers.gaia, go_to: { submit: "enter" } };
    const page = drive(gaiaBox, viewer, 12, gaiaViewer({ on: "keydown" }));
    assert.deepEqual((await page.outcome()).result, { state: "shown" });
    const keydown = page.document.dispatched.find(({ event }) => event.type === "keydown").event;
    assert.equal(keydown.keyCode, 13);
    assert.equal(keydown.which, 13);
    assert.equal(keydown.key, "Enter");
    assert.deepEqual(page.events(), ["input", "change", "keydown", "keypress", "keyup"]);
});

test("the number typed in a box that also shows the count is not taken for the viewer's answer", async () => {
    // A viewer that ignores the box: it keeps the number typed.
    const page = drive(gaiaBox, viewers.gaia, 178);
    const { result, at } = await page.outcome();
    assert.deepEqual(result, { state: "failed", reason: "not_shown", shown: 178 });
    // Three tries of 5 seconds each.
    assert.ok(at >= 15000 && at < 16500, String(at));
    assert.equal(page.events().filter(type => type === "blur").length, 3);
});

test("waits for the viewer to show its view and count before typing", async () => {
    const page = drive('<div id="pagination"><input type="text" value=""></div>', viewers.gaia, 5, (document, timers) => {
        gaiaViewer({ total: 40 })(document, timers);
        timers.setTimeout(() => (document.querySelector("#pagination input").value = "Page 1 de 40"), 3000);
    });
    const { result, at } = await page.outcome();
    assert.deepEqual(result, { state: "shown" });
    assert.ok(at >= 3000 && at < 4500, String(at));
});

test("a viewer that never shows a view number fails within its 30 seconds", async () => {
    const page = drive("<main>Chargement…</main>", viewers.gaia, 5);
    const { result, at } = await page.outcome();
    assert.deepEqual(result, { state: "failed", reason: "not_ready", shown: null });
    assert.ok(at >= 30000 && at < 31000, String(at));
    assert.deepEqual(page.events(), []);
});

test("a view beyond the register's count is not typed", async () => {
    const page = drive(gaiaBox, viewers.gaia, 400, gaiaViewer());
    assert.deepEqual((await page.outcome()).result, { state: "failed", reason: "beyond", shown: 1 });
    assert.deepEqual(page.events(), []);
});

test("a viewer already on the view is left alone", async () => {
    const page = drive('<div id="pagination"><input type="text" value="Page 7 de 9"></div>', viewers.gaia, 7);
    assert.deepEqual((await page.outcome()).result, { state: "shown" });
    assert.deepEqual(page.events(), []);
});

test("THOT: changes the view input, whose count stands beside it", async () => {
    const body = '<div><input type="text" id="imageNum" value="1"><h3 id="imageNumMax"> / 62</h3></div>';
    const page = drive(body, viewers.thot, 31, document => {
        document.getElementById("imageNum").addEventListener("change", () => {});
    });
    assert.deepEqual((await page.outcome()).result, { state: "shown" });
    assert.equal(page.document.getElementById("imageNum").value, "31");
    // A change only: no key.
    assert.deepEqual(page.events(), ["input", "change"]);
});

test("a viewer submitting with a button is clicked there", async () => {
    const viewer = {
        view: "input.page",
        view_count: "span.total",
        go_to: { submit: "click", button: "button.go" },
    };
    const body = '<form><input class="page" value="1"><span class="total">/ 20</span><button class="go">OK</button></form>';
    const page = drive(body, viewer, 4);
    assert.deepEqual((await page.outcome()).result, { state: "shown" });
    assert.deepEqual(page.document.clicked.map(element => element.getAttribute("class")), ["go"]);
    assert.deepEqual(page.events(), ["input"]);

    const missing = drive('<form><input class="page" value="1"><span class="total">/ 20</span></form>', viewer, 4);
    assert.deepEqual((await missing.outcome()).result, { state: "failed", reason: "no_control", shown: 1 });
});

test("every viewer driven to a view names how", () => {
    for (const [id, viewer] of Object.entries(viewers)) {
        if (!viewer.go_to) continue;
        assert.ok(viewer.view, id);
        assert.ok(["change", "enter", "blur", "click"].includes(viewer.go_to.submit), id);
        assert.equal(viewer.go_to.submit === "click", !!viewer.go_to.button, id);
    }
});
