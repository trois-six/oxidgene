// Says when each main-frame document of the archive window starts, posting
// `{"kind": "document"}` — the window covers it with its progress overlay
// while it resolves —, and what it is once it is parsed: the portal's own
// page, an anti-bot check, a block, or a server's error page. Posts
// `{"kind": "page", "state", "vendor", "interactive", "status"}`, `state`
// being `portal`, `challenge`, `blocked` or `error`.
//
// A page is a check or a block when its markup, lower-cased, bears one of
// the adapters' anti-bot signatures (oxidgene-archives'
// `platform/challenges.json`, matched as `markup::anti_bot` matches it):
// every `markers` fragment and no `unless` one, the first match winning. A
// check is `interactive` when it also shows one of the `widgets`. A page is
// a server's error page — a gateway's `504 Gateway Time-out` in place of a
// search the portal took too long to answer — when the navigation's timing
// gives its 5xx `status` (WebViews that give it), or else when its title or
// first heading starts with a 5xx status followed by its reason phrase
// (`504 Gateway Time-out`, `Error 503 Backend fetch failed`): a portal's own
// heading (`500 ans d'histoire`) is not. Any other page is the portal once
// it renders something: text, or a frameset with a frame, whose body has no
// text of its own.
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
const REASON = /^\s*(?:error\s+)?(5\d\d)\b[\s:.\-–|]*(internal server error|not implemented|bad gateway|service (?:temporarily )?unavailable|gateway time-?out|backend fetch failed|web server is down|origin is unreachable|a timeout occurred)/i;
const serverError = () => {
    const timing = typeof performance !== "undefined" && performance.getEntriesByType
        ? performance.getEntriesByType("navigation")[0]
        : null;
    const status = timing && timing.responseStatus;
    if (status) return status >= 500 && status < 600 ? status : null;
    const heading = document.querySelector("h1");
    for (const text of [document.title, heading && heading.textContent]) {
        const found = REASON.exec(text || "");
        if (found) return Number(found[1]);
    }
    return null;
};
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
    const status = serverError();
    if (status) return { state: "error", status };
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
