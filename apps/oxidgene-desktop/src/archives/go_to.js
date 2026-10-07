// Brings a portal's viewer that has no address per view — it opens every
// register on its first view — to the cited view, as a reader would with
// the viewer's own page-number control: in the archive window once the
// target has shown (docs/archives.md §6.1), and in the live checks' browser
// (§9.1).
//
// `viewer` and `view` are defined before this script runs: the viewer of
// the target's platform (oxidgene-archives' `platform/viewers.json`), with
// its `view` and `view_count` elements and its `go_to`, and the one-based
// view to show. The script's value is a promise of the outcome:
// `{state: "shown"}`, or `{state: "failed", reason, shown}`, `shown` being
// the view the viewer shows, `reason` one of `not_ready` (the viewer showed
// no view number in time), `beyond` (the view lies beyond the register),
// `no_control`, `not_shown` and `error`.
//
// The viewer is given 30 seconds to show its view number, and its count
// where it shows one. The view number is then written in the control (the
// `go_to.input` element, the `view` element by default) as a reader types
// it — its value set, an `input` event —, then submitted: a `change` event
// (`change`), followed by the Enter key (`enter`), a `blur` event, the
// control applying the number once left (`blur`), or a click of the
// `go_to.button` (`click`). Events are dispatched rather than the control
// focused: a window in the background focuses nothing, and leaving a
// control it did not focus would tell the viewer nothing. The viewer then
// has 5 seconds to show the view, and the whole is tried three times. The view counts as shown when the `view` element shows it and the
// count is the one shown before: where one element shows both
// (`Page 5 de 40`), both numbers must show, so that the number typed in it
// is not taken for the viewer's answer. Nothing else is filled or clicked.
const STEP_MS = 250;
const READY_MS = 30000;
const SHOW_MS = 5000;
const ATTEMPTS = 3;

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

// The numbers an element shows: an input's or a list's value, or its text.
const numbers = selector => {
    const element = selector ? document.querySelector(selector) : null;
    if (!element) return [];
    const text = element.localName === "input" || element.localName === "select" ? element.value : element.textContent;
    return (String(text ?? "").match(/\d+/g) ?? []).map(Number);
};

// What the viewer shows: its view, its count, and whether an element
// showing both does.
const shown = () => {
    const views = numbers(viewer.view);
    const counts = viewer.view_count ? numbers(viewer.view_count) : [];
    return {
        view: views.length > 0 ? views[0] : null,
        count: counts.length > 0 ? counts[counts.length - 1] : null,
        complete: viewer.view_count !== viewer.view || views.length >= 2,
    };
};

const until = async (test, ms) => {
    for (let waited = 0; ; waited += STEP_MS) {
        if (test()) return true;
        if (waited >= ms) return false;
        await sleep(STEP_MS);
    }
};

// Sets the control's value through its prototype's setter, which the
// frameworks that track a control's value observe.
const write = (control, text) => {
    for (let prototype = Object.getPrototypeOf(control); prototype; prototype = Object.getPrototypeOf(prototype)) {
        const setter = Object.getOwnPropertyDescriptor(prototype, "value")?.set;
        if (setter) return setter.call(control, text);
    }
    control.value = text;
};

const fire = (control, type, bubbles = true) => control.dispatchEvent(new window.Event(type, { bubbles }));

// The Enter key, with the legacy key codes the viewers' scripts read.
const press = control => {
    for (const type of ["keydown", "keypress", "keyup"]) {
        const code = type === "keypress" ? { charCode: 13 } : {};
        const event = new window.KeyboardEvent(type, Object.assign({ key: "Enter", code: "Enter", keyCode: 13, which: 13, bubbles: true, cancelable: true }, code));
        for (const name of ["keyCode", "which"]) {
            if (event[name] !== 13) Object.defineProperty(event, name, { get: () => 13 });
        }
        control.dispatchEvent(event);
    }
};

// Types the view in the control and submits it: whether it could.
const submit = (go, control) => {
    const button = go.submit === "click" ? document.querySelector(go.button) : null;
    if (go.submit === "click" && !button) return false;
    write(control, String(view));
    fire(control, "input");
    if (button) {
        button.click();
    } else if (go.submit === "blur") {
        fire(control, "blur", false);
    } else {
        fire(control, "change");
        if (go.submit === "enter") press(control);
    }
    return true;
};

return (async () => {
    try {
        const go = viewer.go_to;
        const input = go.input || viewer.view;
        const ready = await until(() => {
            const now = shown();
            return now.view !== null && (!viewer.view_count || now.count !== null) && now.complete;
        }, READY_MS);
        if (!ready) return { state: "failed", reason: "not_ready", shown: shown().view };
        const total = shown().count;
        if (total !== null && view > total) return { state: "failed", reason: "beyond", shown: shown().view };
        const shows = () => {
            const now = shown();
            return now.view === view && now.count === total && now.complete;
        };
        if (shows()) return { state: "shown" };
        for (let attempt = 0; attempt < ATTEMPTS; attempt += 1) {
            const control = document.querySelector(input);
            if (!control || !submit(go, control)) return { state: "failed", reason: "no_control", shown: shown().view };
            if (await until(shows, SHOW_MS)) return { state: "shown" };
        }
        return { state: "failed", reason: "not_shown", shown: shown().view };
    } catch {
        return { state: "failed", reason: "error", shown: null };
    }
})();
