// Shows the archive window's banner over the portal's page, replacing any
// earlier one: OxidGene's message in the interface language, a close
// button, and the button of an action when there is one.
//
// `banner` is defined before this script runs: `{text, close, action,
// message, hold}` — the text, the close button's accessible label, the
// action button's label (or null) and the IPC message its click posts, and
// `hold`, CSS selectors of what the reader must answer first. Closing the
// banner posts `{"kind": "dismiss"}`. Everything is set as text, and a
// banner without text is not shown.
//
// The banner is a host element whose content lives in a shadow root, its
// styles reset (`all: initial`), so that no style sheet of the portal
// changes it — no portal rule may turn it into an empty strip, its light
// text on a light background.
//
// A banner with `hold` stays hidden while an element matching one of them
// is on screen — the portal's modal dialog, its viewer's reuse licence, a
// cookie banner — and shows once none is, looked at every half second: it
// never covers a dialog the reader has to answer. A banner with an action
// is compact, so that it covers little of the viewer.
const ID = "oxidgene-archive-status";
const HOLD_STEP_MS = 500;

const CSS = `
.banner {
    display: flex;
    align-items: center;
    box-sizing: border-box;
    border-radius: 8px;
    background: #1e1a14;
    color: #f6f0e4;
    font-family: system-ui, sans-serif;
    box-shadow: 0 4px 16px #0000004d;
    max-width: min(640px, 90vw);
    gap: 12px;
    padding: 10px 14px;
    font-size: 14px;
    line-height: 1.4;
}
.banner.compact { max-width: min(480px, 90vw); gap: 8px; padding: 6px 10px; font-size: 13px; line-height: 1.35; }
button { font: inherit; color: inherit; background: none; cursor: pointer; }
.action { border: 1px solid currentColor; border-radius: 6px; padding: 3px 8px; white-space: nowrap; }
.close { border: 0; font-size: 18px; line-height: 1; padding: 0; }
`;

document.getElementById(ID)?.remove();
if (String(banner.text || "").trim()) {
    const host = document.createElement("div");
    host.id = ID;
    host.style.cssText = "all:initial;position:fixed;z-index:2147483647;top:12px;left:50%;transform:translateX(-50%)";
    const root = host.attachShadow({ mode: "open" });
    const style = document.createElement("style");
    style.textContent = CSS;
    const element = document.createElement("div");
    element.setAttribute("class", banner.action ? "banner compact" : "banner");
    element.setAttribute("role", "status");
    const label = document.createElement("span");
    label.textContent = banner.text;
    element.append(label);
    if (banner.action) {
        const action = document.createElement("button");
        action.type = "button";
        action.setAttribute("class", "action");
        action.textContent = banner.action;
        action.addEventListener("click", () => window.ipc.postMessage(banner.message));
        element.append(action);
    }
    const close = document.createElement("button");
    close.type = "button";
    close.setAttribute("class", "close");
    close.textContent = "×";
    close.setAttribute("aria-label", banner.close);
    close.addEventListener("click", () => {
        host.remove();
        window.ipc.postMessage(JSON.stringify({ kind: "dismiss" }));
    });
    element.append(close);
    root.append(style, element);

    const hold = (banner.hold || []).join(", ");
    const answering = () =>
        !!hold && Array.from(document.querySelectorAll(hold)).some(found => found.getClientRects().length > 0);
    const reveal = () => {
        // Replaced by another banner, or closed: nothing more to do.
        if (!host.isConnected) return;
        if (answering()) {
            setTimeout(reveal, HOLD_STEP_MS);
            return;
        }
        host.style.display = "block";
    };
    // The host's inline display, which a `hidden` attribute would not
    // override.
    const held = answering();
    host.style.display = held ? "none" : "block";
    document.documentElement.append(host);
    if (held) setTimeout(reveal, HOLD_STEP_MS);
}
