// Covers the archive window's page with OxidGene's progress while a citation
// is resolved, or removes that cover. The window runs it on each change of
// step and on each new document of the resolution.
//
// `overlay` is defined before this script runs: `null` removes the cover;
// otherwise `{heading, archive, citation, step, elapsed, seconds, cancel}`,
// texts in the interface language but `elapsed`, the milliseconds the step
// has lasted. From 3 seconds on, the time the step has lasted shows beside
// it, `seconds` with its `{seconds}` placeholder filled, updated every
// second. `cancel` labels the button stopping the lookup, which posts
// `{"kind": "cancel"}` and removes the cover; there is none when `cancel` is
// null.
//
// The cover is a host element whose content lives in a shadow root, out of
// reach of the portal's style sheets, styled inline with neutral colours of
// the reader's light or dark scheme. The steps are a polite status region;
// the elapsed time is left out of it, so that it is not read every second.
const ID = "oxidgene-archive-progress";
const SHOW_ELAPSED_MS = 3000;

const prefers = query => !!window.matchMedia?.(query)?.matches;

const styled = (tag, css) => {
    const element = document.createElement(tag);
    element.style.cssText = css;
    return element;
};

// The cover's content, its parts named by `data-part`.
const build = root => {
    const dark = prefers("(prefers-color-scheme: dark)");
    const [background, text, muted, line] = dark
        ? ["#1c1c1e", "#f2f2f3", "#a8a8ad", "#3a3a3d"]
        : ["#f4f4f5", "#1c1c1e", "#5a5a60", "#d0d0d4"];
    const card = styled(
        "div",
        `box-sizing:border-box;max-width:min(560px,100%);display:flex;flex-direction:column;`
            + `align-items:center;gap:10px;text-align:center;color:${text};`
            + "font:15px/1.5 system-ui,-apple-system,'Segoe UI',sans-serif",
    );
    const status = styled("div", "display:flex;flex-direction:column;align-items:center;gap:6px");
    status.setAttribute("role", "status");
    status.setAttribute("aria-live", "polite");
    const part = (name, element) => {
        element.setAttribute("data-part", name);
        return element;
    };
    const spinner = styled(
        "div",
        `width:28px;height:28px;margin-bottom:6px;border-radius:50%;border:3px solid ${line};`
            + `border-top-color:${text};box-sizing:border-box`,
    );
    spinner.setAttribute("aria-hidden", "true");
    if (!prefers("(prefers-reduced-motion: reduce)")) {
        spinner.animate?.([{ transform: "rotate(0turn)" }, { transform: "rotate(1turn)" }], {
            duration: 1000,
            iterations: Infinity,
        });
    }
    status.append(
        part("heading", styled("strong", "font-size:17px;font-weight:600")),
        part("archive", styled("span", "")),
        part("citation", styled("span", `color:${muted};font-size:13px;overflow-wrap:anywhere`)),
        part("step", styled("span", "margin-top:6px")),
    );
    const elapsed = part("elapsed", styled("span", `color:${muted};font-size:13px;min-height:1.5em`));
    elapsed.setAttribute("aria-hidden", "true");
    const cancel = part(
        "cancel",
        styled(
            "button",
            `margin-top:8px;padding:6px 16px;border:1px solid ${line};border-radius:6px;`
                + `background:${background};color:${text};font:inherit;cursor:pointer`,
        ),
    );
    cancel.setAttribute("type", "button");
    card.append(spinner, status, elapsed, cancel);
    root.append(card);
    return background;
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
        const background = build(host.attachShadow({ mode: "open" }));
        host.style.cssText = "all:initial;position:fixed;inset:0;z-index:2147483647;display:flex;"
            + `align-items:center;justify-content:center;padding:16px;background:${background}`;
        const cancel = host.shadowRoot.querySelector('[data-part="cancel"]');
        cancel.addEventListener("click", () => {
            window.ipc.postMessage(JSON.stringify({ kind: "cancel" }));
            stop(host);
        });
        document.documentElement.append(host);
    }
    const root = host.shadowRoot;
    const find = name => root.querySelector(`[data-part="${name}"]`);
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
