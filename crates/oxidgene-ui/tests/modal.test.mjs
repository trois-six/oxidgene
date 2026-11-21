import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("../src/components/modal.js", import.meta.url), "utf8");
const run = new Function("window", "document", "performance", "requestAnimationFrame", "KeyboardEvent", source);

class KeyboardEvent {
    constructor(type, init) {
        Object.assign(this, { type, isTrusted: false, defaultPrevented: false }, init);
    }
    preventDefault() { this.defaultPrevented = true; }
}

function element(name, document) {
    const listeners = {};
    return {
        name,
        isConnected: true,
        open: false,
        children: [],
        shown: 0,
        closed: 0,
        contains(other) { return other === this || this.children.includes(other); },
        focus() { document.activeElement = this; },
        addEventListener(type, listener) { (listeners[type] ??= []).push(listener); },
        dispatchEvent(event) {
            event.target ??= this;
            (this.dispatched ??= []).push(event);
            for (const listener of listeners[event.type] ?? []) listener(event);
        },
        showModal() { this.open = true; this.shown += 1; },
        close() {
            this.open = false;
            this.closed += 1;
            this.dispatchEvent({ type: "close" });
        },
    };
}

function fixture() {
    const frames = [];
    const clock = { now: 0 };
    const document = { body: null, activeElement: null, elements: {} };
    document.getElementById = id => document.elements[id] ?? null;
    document.body = element("body", document);
    const opener = element("opener", document);
    const dialog = element("dialog", document);
    const field = element("field", document);
    dialog.children.push(field);
    document.elements["modal-0"] = dialog;
    document.activeElement = opener;
    const window = {};
    run(window, document, { now: () => clock.now }, frame => frames.push(frame), KeyboardEvent);
    return { modal: window.oxidgeneModal, document, opener, dialog, field, frames, clock, window };
}

test("the object is built once per page", () => {
    const { window, document } = fixture();
    const first = window.oxidgeneModal;
    run(window, document, { now: () => 0 }, () => {}, KeyboardEvent);
    assert.equal(window.oxidgeneModal, first);
});

test("opening shows the dialog modally and gives it the focus", () => {
    const { modal, document, dialog, frames } = fixture();
    modal.remember("modal-0");
    modal.open("modal-0");
    assert.equal(dialog.shown, 1);
    assert.equal(document.activeElement, dialog);
    // A dialog already open is not opened again.
    modal.open("modal-0");
    assert.equal(dialog.shown, 1);
    assert.equal(frames.length, 1);
});

test("a field that took the focus as it mounted keeps it", () => {
    const { modal, document, dialog, field, frames } = fixture();
    modal.remember("modal-0");
    document.activeElement = field;
    modal.open("modal-0");
    assert.equal(document.activeElement, field);
    frames[0]();
    assert.equal(document.activeElement, field);
    assert.equal(dialog.shown, 1);
});

test("a focus that left the dialog by the next frame returns to it", () => {
    const { modal, document, dialog, opener, frames } = fixture();
    modal.remember("modal-0");
    modal.open("modal-0");
    document.activeElement = opener;
    frames[0]();
    assert.equal(document.activeElement, dialog);
});

test("closing returns the focus to what had it before the dialog", () => {
    const { modal, document, dialog, opener, field } = fixture();
    modal.remember("modal-0");
    document.activeElement = field;
    modal.open("modal-0");
    modal.close("modal-0");
    assert.equal(dialog.closed, 1);
    // Rust closed it: the dialog is not shown again.
    assert.equal(dialog.shown, 1);
    assert.equal(document.activeElement, opener);
});

test("closing after the dialog left the page still returns the focus", () => {
    const { modal, document, opener } = fixture();
    modal.remember("modal-0");
    modal.open("modal-0");
    delete document.elements["modal-0"];
    document.activeElement = document.body;
    modal.close("modal-0");
    assert.equal(document.activeElement, opener);
});

test("a focus the closing action moved elsewhere is left alone", () => {
    const { modal, document, opener } = fixture();
    const editor = element("editor", document);
    modal.remember("modal-0");
    modal.open("modal-0");
    document.activeElement = editor;
    modal.close("modal-0");
    assert.equal(document.activeElement, editor);
    assert.notEqual(document.activeElement, opener);
});

test("a dialog replacing another inherits its opener", () => {
    const { modal, document, opener, dialog, field } = fixture();
    const next = element("next", document);
    document.elements["modal-1"] = next;
    modal.remember("modal-0");
    modal.open("modal-0");
    document.activeElement = field;
    // The replacement is rendered while the first still holds the focus.
    modal.remember("modal-1");
    modal.close("modal-0");
    assert.equal(document.activeElement, opener);
    modal.open("modal-1");
    assert.equal(document.activeElement, next);
    modal.close("modal-1");
    assert.equal(document.activeElement, opener);
    assert.equal(dialog.closed, 1);
});

test("an opener that left the page is not focused", () => {
    const { modal, document, opener, dialog } = fixture();
    modal.remember("modal-0");
    modal.open("modal-0");
    opener.isConnected = false;
    modal.close("modal-0");
    assert.equal(document.activeElement, dialog);
});

test("the engine's cancel is refused, and Escape is left to the key handler", () => {
    const { modal, dialog, clock } = fixture();
    modal.remember("modal-0");
    modal.open("modal-0");
    clock.now = 1000;
    dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", isTrusted: true }));
    clock.now = 1010;
    const cancel = { type: "cancel", preventDefault() { this.defaultPrevented = true; } };
    dialog.dispatchEvent(cancel);
    assert.ok(cancel.defaultPrevented);
    // Only the real key press reached the dialog: no second Escape.
    assert.equal(dialog.dispatched.filter(event => event.type === "keydown").length, 1);
});

test("a cancel no key press announced reaches Rust as an Escape", () => {
    const { modal, dialog, clock } = fixture();
    modal.remember("modal-0");
    modal.open("modal-0");
    clock.now = 5000;
    const cancel = { type: "cancel", preventDefault() { this.defaultPrevented = true; } };
    dialog.dispatchEvent(cancel);
    assert.ok(cancel.defaultPrevented);
    const keys = dialog.dispatched.filter(event => event.type === "keydown");
    assert.equal(keys.length, 1);
    assert.equal(keys[0].key, "Escape");
    assert.ok(keys[0].bubbles);
});

test("a dialog the engine closed on its own opens again", () => {
    const { modal, dialog } = fixture();
    modal.remember("modal-0");
    modal.open("modal-0");
    dialog.close();
    assert.equal(dialog.shown, 2);
    assert.ok(dialog.open);
});
