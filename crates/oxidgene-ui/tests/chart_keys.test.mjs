import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(
    new URL("../src/components/pedigree_chart/chart_keys.js", import.meta.url),
    "utf8",
);
const run = new Function("window", "document", "setInterval", "clearInterval", source);

const rect = (left, top, width = 100, height = 40) => ({
    left, top, width, height, right: left + width, bottom: top + height,
});

function item(name, bounds, tabindex = "-1") {
    return {
        name,
        tabindex,
        isConnected: true,
        getBoundingClientRect: () => bounds,
        closest() { return this; },
        matches: () => true,
    };
}

function fixture(items, bounds = rect(0, 0, 1000, 800)) {
    const timers = [];
    const document = { body: { name: "body" }, activeElement: null, elements: {} };
    document.getElementById = id => document.elements[id] ?? null;
    const listeners = [];
    const viewport = {
        dataset: {},
        isConnected: true,
        getBoundingClientRect: () => bounds,
        contains: element => element === viewport || items.includes(element),
        closest: () => null,
        querySelectorAll: () => items,
        querySelector: selector => selector.includes('tabindex="0"')
            ? items.find(candidate => candidate.tabindex === "0") ?? null
            : items[0] ?? null,
        addEventListener(type, listener) { listeners.push(listener); },
        focus() { document.activeElement = viewport; },
    };
    for (const candidate of items) candidate.focus = () => { document.activeElement = candidate; };
    document.elements["pan-zoom-0"] = viewport;
    const window = {};
    run(window, document, tick => { timers.push(tick); return timers.length; }, () => { timers.length = 0; });
    window.oxidgeneChartKeys.attach("pan-zoom-0");
    const press = (target, key, fields = {}) => {
        const event = {
            key, target, defaultPrevented: false, ...fields,
            preventDefault() { this.defaultPrevented = true; },
        };
        for (const listener of listeners) listener(event);
        return event;
    };
    return { keys: window.oxidgeneChartKeys, document, viewport, press, timers, listeners };
}

test("the nearest rectangle ahead is chosen, a sideways one costing more", () => {
    const { keys } = fixture([]);
    const origin = rect(0, 100);
    const rects = [rect(200, 100), rect(150, 300), rect(-200, 100), rect(400, 100)];
    assert.equal(keys.nearest(origin, rects, [1, 0]), 0);
    assert.equal(keys.nearest(origin, rects, [-1, 0]), 2);
    assert.equal(keys.nearest(origin, rects, [0, 1]), 1);
    assert.equal(keys.nearest(origin, rects, [0, -1]), -1);
});

test("a rectangle within the cone is preferred to a nearer one outside it", () => {
    const { keys } = fixture([]);
    // Barely to the right and far below, against one straight to the right.
    assert.equal(keys.nearest(rect(0, 0), [rect(20, 400), rect(600, 0)], [1, 0]), 1);
    // With nothing in the cone, what lies that way is still reached.
    assert.equal(keys.nearest(rect(0, 0), [rect(20, 400)], [1, 0]), 0);
});

test("an arrow moves the focus to the neighbour that way", () => {
    const root = item("root", rect(0, 300), "0");
    const father = item("father", rect(200, 200));
    const mother = item("mother", rect(200, 400));
    const { document, press } = fixture([root, father, mother]);
    const right = press(root, "ArrowRight");
    assert.ok(right.defaultPrevented);
    assert.ok([father, mother].includes(document.activeElement));
    press(father, "ArrowDown");
    assert.equal(document.activeElement, mother);
    press(mother, "ArrowLeft");
    assert.equal(document.activeElement, root);
});

test("an arrow at an edge of the chart is left to the page", () => {
    const root = item("root", rect(0, 300), "0");
    const { document, press } = fixture([root]);
    const event = press(root, "ArrowUp");
    assert.ok(!event.defaultPrevented);
    assert.equal(document.activeElement, null);
});

test("elements outside the viewport are not reached", () => {
    const root = item("root", rect(0, 300), "0");
    const hidden = item("hidden", rect(2000, 300));
    const { press } = fixture([root, hidden]);
    assert.ok(!press(root, "ArrowRight").defaultPrevented);
});

test("an arrow on the canvas itself enters the chart at the selected element", () => {
    const root = item("root", rect(0, 300));
    const selected = item("selected", rect(200, 200), "0");
    const { document, viewport, press } = fixture([root, selected]);
    press(viewport, "ArrowDown");
    assert.equal(document.activeElement, selected);
});

test("arrows with a modifier, or pressed in a control of the chart, are left alone", () => {
    const root = item("root", rect(0, 300), "0");
    const father = item("father", rect(200, 300));
    const { document, press } = fixture([root, father]);
    assert.ok(!press(root, "ArrowRight", { altKey: true }).defaultPrevented);
    const control = { closest: () => null };
    assert.ok(!press(control, "ArrowRight").defaultPrevented);
    assert.equal(document.activeElement, null);
});

test("after an activation the focus follows the selected person to their new element", () => {
    // Enter on the father's card selects him: his element is the stop.
    const first = item("first", rect(0, 300));
    const second = item("second", rect(200, 300), "0");
    const { document, press, timers } = fixture([first, second]);
    document.activeElement = second;
    press(second, "Enter");
    assert.equal(timers.length, 1);
    // Nothing redrawn yet: the focus stays where it is.
    timers[0]();
    assert.equal(document.activeElement, second);
    // The chart is drawn again around him: the elements are reused by
    // position, and the one that has the focus now draws somebody else.
    first.tabindex = "0";
    second.tabindex = "-1";
    timers[0]();
    assert.equal(document.activeElement, first);
});

test("a focus lost when an activation redraws the chart goes to the selected element", () => {
    const gone = item("gone", rect(0, 300), "0");
    const selected = item("selected", rect(200, 300));
    const { document, viewport, press, timers } = fixture([gone, selected]);
    document.activeElement = gone;
    press(gone, "Enter");
    gone.isConnected = false;
    gone.tabindex = "-1";
    selected.tabindex = "0";
    timers[0]();
    assert.equal(document.activeElement, selected);
    // With no selected element drawn, the canvas itself takes the focus.
    selected.isConnected = false;
    selected.tabindex = "-1";
    timers[0]();
    assert.equal(document.activeElement, viewport);
});

test("a focus the reader moved elsewhere is not taken back", () => {
    const root = item("root", rect(0, 300), "0");
    const { document, press, timers } = fixture([root]);
    press(root, " ");
    document.activeElement = { name: "search", isConnected: true };
    timers[0]();
    assert.equal(timers.length, 0);
    assert.equal(document.activeElement.name, "search");
});

test("an arrow pressed after an activation ends the watch", () => {
    const root = item("root", rect(0, 300), "0");
    const father = item("father", rect(200, 300));
    const { document, press, timers } = fixture([root, father]);
    document.activeElement = root;
    press(root, "Enter");
    assert.equal(timers.length, 1);
    press(root, "ArrowRight");
    assert.equal(timers.length, 0);
    assert.equal(document.activeElement, father);
});

test("the watch ends by itself", () => {
    const root = item("root", rect(0, 300), "0");
    const { document, press, timers } = fixture([root]);
    document.activeElement = root;
    press(root, "Enter");
    const tick = timers[0];
    for (let ticks = 0; ticks < 31 && timers.length; ticks += 1) tick();
    assert.equal(timers.length, 0);
});

test("a viewport is listened to once", () => {
    const { keys, listeners } = fixture([]);
    keys.attach("pan-zoom-0");
    assert.equal(listeners.length, 1);
});
