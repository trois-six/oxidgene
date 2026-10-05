// Covers the archive window's page with OxidGene's progress while a citation
// is resolved, or removes that cover. The window runs it on each change of
// step and on each new document of the resolution.
//
// `overlay` is defined before this script runs: `null` removes the cover;
// otherwise `{style, heading, archive, citation, step, elapsed, seconds,
// cancel}`, texts in the interface language but `style`, the application's
// theme and spinner rules, and `elapsed`, the milliseconds the step has
// lasted. From 3 seconds on, the time the step has lasted shows beside it,
// `seconds` with its `{seconds}` placeholder filled, updated every second.
// `cancel` labels the button stopping the lookup, which posts
// `{"kind": "cancel"}` and removes the cover; there is none when `cancel` is
// null.
//
// The cover is an opaque host element hiding the whole page, whose content
// lives in a shadow root, out of reach of the portal's style sheets: the
// theme's custom properties are declared on the host, so that the cover and
// OxidGene's one spinner (`.spinner`) look as they do in the application.
// The steps are a polite status region; the elapsed time is left out of it,
// so that it is not read every second.
const ID = "oxidgene-archive-progress";
const SHOW_ELAPSED_MS = 3000;

// The cover's own rules, after the application's.
const CSS = `
.card {
    box-sizing: border-box;
    max-width: min(560px, 100%);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    text-align: center;
    color: var(--text-primary);
    font: 15px/1.5 var(--font-sans, system-ui, sans-serif);
}
.spinner { margin-bottom: 6px; }
.status { display: flex; flex-direction: column; align-items: center; gap: 6px; }
.heading { font-family: var(--font-heading, inherit); font-size: 17px; font-weight: 600; }
.muted { color: var(--text-secondary); font-size: 13px; overflow-wrap: anywhere; }
.step { margin-top: 6px; }
.elapsed { color: var(--text-secondary); font-size: 13px; min-height: 1.5em; }
.cancel {
    margin-top: 8px;
    padding: 6px 16px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-card);
    color: var(--text-primary);
    font: inherit;
    cursor: pointer;
}
.cancel[hidden] { display: none; }
`;

const element = (tag, className, name) => {
    const created = document.createElement(tag);
    created.setAttribute("class", className);
    if (name) created.setAttribute("data-part", name);
    return created;
};

// The cover's content, its parts named by `data-part`.
const build = root => {
    const card = element("div", "card");
    const spinner = element("div", "spinner");
    spinner.setAttribute("aria-hidden", "true");
    const status = element("div", "status");
    status.setAttribute("role", "status");
    status.setAttribute("aria-live", "polite");
    status.append(
        element("strong", "heading", "heading"),
        element("span", "archive", "archive"),
        element("span", "muted", "citation"),
        element("span", "step", "step"),
    );
    const elapsed = element("span", "elapsed", "elapsed");
    elapsed.setAttribute("aria-hidden", "true");
    const cancel = element("button", "cancel", "cancel");
    cancel.setAttribute("type", "button");
    card.append(spinner, status, elapsed, cancel);
    root.append(element("style", "", "style"), card);
};

const stop = host => {
    clearInterval(host.oxidgeneTimer);
    host.remove();
};

const render = () => {
    let host = document.getElementById(ID);
    if (host) clearInterval(host.oxidgeneTimer);
    if (!overlay) {
        if (host) stop(host);
        return;
    }
    if (!host) {
        host = document.createElement("div");
        host.id = ID;
        build(host.attachShadow({ mode: "open" }));
        host.style.cssText = "all:initial;position:fixed;inset:0;z-index:2147483647;display:flex;"
            + "align-items:center;justify-content:center;padding:16px;background:var(--bg-deep)";
        const cancel = host.shadowRoot.querySelector('[data-part="cancel"]');
        cancel.addEventListener("click", () => {
            window.ipc.postMessage(JSON.stringify({ kind: "cancel" }));
            stop(host);
        });
        document.documentElement.append(host);
    }
    const root = host.shadowRoot;
    const find = name => root.querySelector(`[data-part="${name}"]`);
    const style = `${overlay.style || ""}\n${CSS}`;
    if (find("style").textContent !== style) find("style").textContent = style;
    for (const name of ["heading", "archive", "citation", "step"]) {
        const text = overlay[name] || "";
        // Unchanged text is left alone, so that it is not read again.
        if (find(name).textContent !== text) find(name).textContent = text;
    }
    const cancel = find("cancel");
    cancel.textContent = overlay.cancel || "";
    if (overlay.cancel) cancel.removeAttribute("hidden");
    else cancel.setAttribute("hidden", "");
    const started = Date.now() - overlay.elapsed;
    const tick = () => {
        const lasted = Date.now() - started;
        find("elapsed").textContent =
            lasted >= SHOW_ELAPSED_MS ? overlay.seconds.replace("{seconds}", String(Math.floor(lasted / 1000))) : "";
    };
    tick();
    host.oxidgeneTimer = setInterval(tick, 1000);
};

// A document the window runs this on as it starts may have no root yet.
if (document.documentElement) render();
else document.addEventListener("DOMContentLoaded", render, { once: true });
