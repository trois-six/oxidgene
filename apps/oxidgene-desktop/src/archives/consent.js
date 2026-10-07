// Refuses a portal's cookie consent on the reader's behalf, in each
// main-frame document of the archive window: OxidGene never accepts. The
// WebView's persistent profile then keeps the choice, as a browser would.
//
// `consent` is defined before this script runs (`consent.json`): the consent
// managers recognized — the `banner` each shows, the `refuse` controls to
// click and the `accept` controls never to click, as CSS selectors —, the
// information notices recognized, and the exact `refuse_phrases` and
// `accept_phrases` of consent controls.
//
// The document is watched for 15 seconds (a MutationObserver: managers
// render after the page's own scripts). Once a recognized banner shows, its
// first visible refuse control is clicked, once: the manager's own, or else,
// within the banner, a button or a link whose whole text is a refuse phrase.
// Nothing in a consent banner whose text, title, label or value is an accept
// phrase is ever clicked. A banner that shows an accept control and no
// refuse control, or is still on screen after the 15 seconds, is left to
// the reader, and OxidGene clicks nothing more in the document.
//
// `consent.notices` are information notices that ask no consent — a
// portal's note that it sets only cookies exempt from consent —, each with
// the `banner` it shows and the `dismiss` control acknowledging it, listed
// once the notice's text has been read. Acknowledging such a notice grants
// nothing: while the document is watched, its dismiss control is clicked,
// once, unless the notice offers a control whose whole text is a refuse
// phrase, which would make it a consent request.
//
// Posts `{"kind": "consent", "state", "manager"}`: `refused` on a click,
// `left` for a banner left to the reader, `closed` once that banner is
// gone, and `dismissed` for a notice acknowledged.
const WATCH_MS = 15000;
// How long a refused banner may take to go away before it counts as stuck.
const SETTLE_MS = 1000;
const CLICKABLE = 'button, a, [role="button"], input[type="button"], input[type="submit"]';

const normalize = text =>
    (text || "")
        .replace(/[’`]/g, "'")
        .replace(/\s+/g, " ")
        .trim()
        .toLowerCase()
        .replace(/^[^\p{L}\p{N}']+|[^\p{L}\p{N}']+$/gu, "");
const refusePhrases = new Set(consent.refuse_phrases);
const acceptPhrases = new Set(consent.accept_phrases);
const shown = element => element.getClientRects().length > 0;
const visible = (root, selector) => Array.from(root.querySelectorAll(selector)).filter(shown);
const accepts = element =>
    [element.textContent, element.value, element.getAttribute("title"), element.getAttribute("aria-label")].some(
        text => acceptPhrases.has(normalize(text)),
    );
const report = message => window.ipc?.postMessage(JSON.stringify(Object.assign({ kind: "consent" }, message)));

const clicked = new Set();
// Whether the watch is still on, a click is settling, and the manager whose
// banner is left to the reader.
let watching = true;
let settling = false;
let left = null;

const banners = () =>
    consent.managers
        .map(manager => ({ manager, banner: visible(document, manager.banner)[0] }))
        .filter(found => found.banner);

const refuseControl = (manager, banner) => {
    const usable = control => !clicked.has(control) && !accepts(control);
    return (
        manager.refuse.flatMap(selector => visible(banner, selector)).find(usable)
        ?? visible(banner, CLICKABLE).find(
            control => usable(control) && refusePhrases.has(normalize(control.textContent || control.value)),
        )
    );
};

// Clicks the first refuse control of a banner on screen: whether it did.
const refuse = found => {
    for (const { manager, banner } of found) {
        const control = refuseControl(manager, banner);
        if (control) {
            clicked.add(control);
            settling = true;
            setTimeout(() => {
                settling = false;
                look();
            }, SETTLE_MS);
            control.click();
            report({ state: "refused", manager: manager.name });
            return true;
        }
    }
    return false;
};

// Acknowledges the information notices on screen, each once.
const dismissed = new Set();
const dismiss = () => {
    for (const notice of consent.notices) {
        const banner = visible(document, notice.banner)[0];
        const control = banner && visible(banner, notice.dismiss)[0];
        if (!control || dismissed.has(notice.name)) continue;
        const asks = visible(banner, CLICKABLE).some(other =>
            refusePhrases.has(normalize(other.textContent || other.value)),
        );
        if (asks) continue;
        dismissed.add(notice.name);
        control.click();
        report({ state: "dismissed", manager: notice.name });
    }
};

const observer = new MutationObserver(() => look());

function look() {
    if (watching) dismiss();
    if (settling) return;
    const found = banners();
    if (left === null && watching && refuse(found)) return;
    if (left === null) {
        const stuck = found.find(
            ({ manager, banner }) => !watching || manager.accept.some(selector => visible(banner, selector).length > 0),
        );
        if (stuck) {
            left = stuck.manager.name;
            report({ state: "left", manager: left });
        }
    } else if (found.length === 0) {
        report({ state: "closed", manager: left });
        left = null;
    }
    if (!watching && left === null) observer.disconnect();
}

if (window.top === window) {
    observer.observe(document, {
        childList: true,
        subtree: true,
        attributes: true,
        attributeFilter: ["class", "style", "hidden"],
    });
    setTimeout(() => {
        watching = false;
        look();
    }, WATCH_MS);
    look();
}
