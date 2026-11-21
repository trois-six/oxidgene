import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("../src/crash_screen.js", import.meta.url), "utf8");
const run = new Function(
    "document",
    "location",
    "console",
    "getComputedStyle",
    "title",
    "message",
    "action",
    "report",
    source,
);

function element(name) {
    const listeners = {};
    return {
        name,
        id: "",
        textContent: "",
        style: { cssText: "" },
        attributes: {},
        children: [],
        setAttribute(key, value) { this.attributes[key] = value; },
        append(...nodes) { this.children.push(...nodes); },
        replaceChildren(...nodes) { this.children = nodes; },
        addEventListener(type, listener) { (listeners[type] ??= []).push(listener); },
        click() { for (const listener of listeners.click ?? []) listener(); },
        focus() { this.focused = true; },
    };
}

function fixture(tokens = {}) {
    const body = element("body");
    body.children.push(element("div"), element("dialog"));
    const document = {
        body,
        documentElement: element("html"),
        createElement: element,
        getElementById: id => body.children.find(child => child.id === id) ?? null,
    };
    const location = { reloads: 0, reload() { this.reloads += 1; } };
    const logged = [];
    const console = { error: text => logged.push(text) };
    const getComputedStyle = () => ({ getPropertyValue: name => tokens[name] ?? "" });
    const crash = report =>
        run(document, location, console, getComputedStyle, "Stopped", "It stopped on an error.", "Reload", report);
    return { document, location, logged, crash };
}

test("the crash screen replaces everything the page drew with the translated message", () => {
    const { document, logged, crash } = fixture();
    crash("panicked at somewhere");

    assert.equal(document.body.children.length, 1);
    const [screen] = document.body.children;
    assert.equal(screen.id, "crash-screen");
    assert.equal(screen.attributes.role, "alert");
    const [title, message, reload] = screen.children;
    assert.deepEqual(
        [title.name, title.textContent, message.name, message.textContent, reload.name, reload.textContent],
        ["h1", "Stopped", "p", "It stopped on an error.", "button", "Reload"],
    );
    // The report goes to the console and nowhere on the page.
    assert.deepEqual(logged, ["OxidGene stopped: panicked at somewhere"]);
    assert.ok(!screen.children.some(child => child.textContent.includes("panicked")));
});

test("the action reloads the page and holds the focus", () => {
    const { document, location, crash } = fixture();
    crash("x");
    const reload = document.body.children[0].children[2];
    assert.equal(reload.focused, true);
    assert.equal(reload.type, "button");
    reload.click();
    assert.equal(location.reloads, 1);
});

test("the screen keeps the theme's colours, and has its own before any theme", () => {
    const themed = fixture({ "--bg-deep": " #101418 ", "--text-primary": "#f0f0f0", "--orange": "#ff8800" });
    themed.crash("x");
    const screen = themed.document.body.children[0];
    assert.match(screen.style.cssText, /background: #101418; color: #f0f0f0;/);
    assert.match(screen.children[2].style.cssText, /background: #ff8800;/);

    const bare = fixture();
    bare.crash("x");
    assert.match(bare.document.body.children[0].style.cssText, /background: #ffffff; color: #1a1a1a;/);
});

test("a second panic leaves the first screen alone", () => {
    const { document, logged, crash } = fixture();
    crash("first");
    const screen = document.body.children[0];
    crash("second");
    assert.equal(document.body.children[0], screen);
    assert.deepEqual(logged, ["OxidGene stopped: first"]);
});
