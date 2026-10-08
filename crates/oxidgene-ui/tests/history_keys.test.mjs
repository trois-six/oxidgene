import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("../src/history_keys.js", import.meta.url), "utf8");
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const run = new AsyncFunction("window", "document", "dioxus", source);

async function fixture(window = {}) {
    const sent = [];
    const listeners = {};
    const document = {
        addEventListener(type, listener) {
            (listeners[type] ??= []).push(listener);
        },
    };
    await run(window, document, { send: message => sent.push(message) });
    const dispatch = (type, fields) => {
        const event = {
            altKey: false, ctrlKey: false, metaKey: false, shiftKey: false,
            defaultPrevented: false, target: { tagName: "DIV" }, ...fields,
            preventDefault() { this.defaultPrevented = true; },
        };
        for (const listener of listeners[type] ?? []) listener(event);
        return event;
    };
    return { sent, dispatch, listeners, window, document };
}

test("Alt+Left and Alt+Right go back and forward", async () => {
    const { sent, dispatch } = await fixture();
    const back = dispatch("keydown", { altKey: true, key: "ArrowLeft" });
    const forward = dispatch("keydown", { altKey: true, key: "ArrowRight" });
    assert.deepEqual(sent, ["back", "forward"]);
    assert.ok(back.defaultPrevented && forward.defaultPrevented);
});

test("other keys and other modifiers are left alone", async () => {
    const { sent, dispatch } = await fixture();
    dispatch("keydown", { key: "ArrowLeft" });
    dispatch("keydown", { altKey: true, key: "ArrowUp" });
    dispatch("keydown", { altKey: true, shiftKey: true, key: "ArrowLeft" });
    dispatch("keydown", { altKey: true, ctrlKey: true, key: "ArrowLeft" });
    dispatch("keydown", { altKey: true, key: "ArrowLeft", defaultPrevented: true });
    assert.deepEqual(sent, []);
});

test("a shortcut typed in a field stays the field's", async () => {
    const { sent, dispatch } = await fixture();
    for (const tagName of ["INPUT", "TEXTAREA", "SELECT"]) {
        const event = dispatch("keydown", { altKey: true, key: "ArrowLeft", target: { tagName } });
        assert.equal(event.defaultPrevented, false);
    }
    dispatch("keydown", {
        altKey: true, key: "ArrowRight", target: { tagName: "DIV", isContentEditable: true },
    });
    assert.deepEqual(sent, []);
});

test("the mouse's back and forward buttons move once, on release", async () => {
    const { sent, dispatch } = await fixture();
    const down = dispatch("mousedown", { button: 3 });
    assert.ok(down.defaultPrevented);
    assert.deepEqual(sent, []);
    dispatch("mouseup", { button: 3 });
    dispatch("mouseup", { button: 4 });
    dispatch("mouseup", { button: 0 });
    dispatch("mouseup", { button: 2 });
    assert.deepEqual(sent, ["back", "forward"]);
    assert.equal(dispatch("mousedown", { button: 0 }).defaultPrevented, false);
});

test("a second install reports through the new channel without listening twice", async () => {
    const first = await fixture();
    const sent = [];
    await run(first.window, first.document, { send: message => sent.push(message) });
    assert.equal(first.listeners.keydown.length, 1);
    first.dispatch("keydown", { altKey: true, key: "ArrowLeft" });
    assert.deepEqual(first.sent, []);
    assert.deepEqual(sent, ["back"]);
});
