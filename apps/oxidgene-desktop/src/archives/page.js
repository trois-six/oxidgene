// Says when each main-frame document of the archive window starts, posting
// `{"kind": "document"}` — the window covers it with its progress overlay
// while it resolves —, and what it is once it is parsed: the portal's own
// page, an anti-bot check, or a block. Posts `{"kind": "page", "state",
// "vendor", "interactive"}`, `state` being `portal`, `challenge` or
// `blocked`.
//
// A page is a check or a block when its markup, lower-cased, bears one of
// the adapters' anti-bot signatures (oxidgene-archives'
// `platform/challenges.json`, matched as `markup::anti_bot` matches it):
// every `markers` fragment and no `unless` one, the first match winning. A
// check is `interactive` when it also shows one of the `widgets`. Any other
// page is the portal once it renders something: text, or a frameset with a
// frame, whose body has no text of its own.
//
// The page is first checked once its markup is parsed (`DOMContentLoaded`),
// not once everything it loads has (`load`): a portal's own images and
// scripts may keep it loading for long, and neither the banner the window
// shows over the page nor the requests it sends from it wait for them. It
// is checked again every half second while it is a check or shows nothing
// yet, since a check's widget, or a portal's own content, may come later,
// and a message is posted only when the answer changes. A check that passes
// navigates, and the next document posts anew. Nothing is filled or clicked.
//
// `antiBot` is defined before this script runs.
window.ipc.postMessage(JSON.stringify({ kind: "document" }));
const classify = () => {
    const root = document.documentElement;
    if (!root) return null;
    const markup = root.outerHTML.toLowerCase();
    const found = antiBot.signatures.find(
        signature =>
            signature.markers.every(marker => markup.includes(marker))
            && !(signature.unless || []).some(marker => markup.includes(marker)),
    );
    if (found) {
        return {
            state: found.guard === "block" ? "blocked" : "challenge",
            vendor: found.vendor,
            interactive: found.guard === "challenge" && antiBot.widgets.some(widget => markup.includes(widget)),
        };
    }
    const body = document.body;
    if (!body) return null;
    const rendered = body.localName === "frameset" ? !!body.querySelector("frame") : !!body.innerText.trim();
    return rendered ? { state: "portal" } : null;
};
let posted = "";
const check = () => {
    const page = classify();
    if (page) {
        const message = JSON.stringify(Object.assign({ kind: "page" }, page));
        if (message !== posted) {
            posted = message;
            window.ipc.postMessage(message);
        }
        if (page.state !== "challenge") return;
    }
    setTimeout(check, 500);
};
if (document.readyState === "loading") addEventListener("DOMContentLoaded", check, { once: true });
else check();
