import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { clock, parse } from "./fake_dom.mjs";

const source = await readFile(new URL("../src/archives/consent.js", import.meta.url), "utf8");
const consent = JSON.parse(await readFile(new URL("../src/archives/consent.json", import.meta.url), "utf8"));
const script = new Function("consent", "window", "document", "MutationObserver", "setTimeout", source);

// Runs the script on a document of `body`, as the archive window's main
// frame, or as a frame within it.
function run(body, { frame = false } = {}) {
    const document = parse(body);
    const timers = clock();
    const posted = [];
    const window = { ipc: { postMessage: message => posted.push(JSON.parse(message)) } };
    window.top = frame ? {} : window;
    script(consent, window, document, document.MutationObserver, timers.setTimeout);
    return { document, timers, posted, clicked: () => document.clicked.map(describe) };
}

const describe = element => element.id || element.getAttribute("class") || element.textContent.trim();

// The consent managers of French archive portals, in fictitious markup.
const banners = {
    arkotheque: {
        markup: `<div id="analytics-container"><div><div class="cookie_enabled"><div class="intitule_cookie"><span>Ce site utilise des cookies.</span></div><div class="btn_cookie"><button type="button" class="btn_refuser" title="Refuser"><i class="icon"></i><span>Refuser</span></button><button type="button" class="btn_accepter" title="Accepter"><i class="icon"></i><span>Accepter</span></button></div></div></div></div>`,
        refuse: "btn_refuser",
    },
    tarteaucitron: {
        markup: `<div id="tarteaucitronRoot"><div id="tarteaucitronAlertBig"><span id="tarteaucitronDisclaimerAlert">Ce site utilise des cookies.</span><button type="button" class="tarteaucitronCTAButton tarteaucitronAllow" id="tarteaucitronPersonalize2">✓ Tout accepter</button><button type="button" class="tarteaucitronCTAButton tarteaucitronDeny" id="tarteaucitronAllDenied2">✗ Tout refuser</button><button type="button" id="tarteaucitronCloseAlert">Personnaliser</button></div></div>`,
        refuse: "tarteaucitronAllDenied2",
    },
    axeptio: {
        markup: `<div id="axeptio_overlay"><div class="axeptio_widget"><p>Ce site utilise des cookies.</p><button id="axeptio_btn_dismiss">Non merci</button><button id="axeptio_btn_configure">Je choisis</button><button id="axeptio_btn_acceptAll">OK pour moi</button></div></div>`,
        refuse: "axeptio_btn_dismiss",
    },
    didomi: {
        markup: `<div id="didomi-host"><div id="didomi-notice"><p>Vos choix concernant les cookies.</p><button id="didomi-notice-learn-more-button">Personnaliser</button><button id="didomi-notice-disagree-button">Continuer sans accepter →</button><button id="didomi-notice-agree-button">Accepter &amp; fermer</button></div></div>`,
        refuse: "didomi-notice-disagree-button",
    },
    onetrust: {
        markup: `<div id="onetrust-consent-sdk"><div id="onetrust-banner-sdk"><p>Nous utilisons des cookies.</p><button id="onetrust-pc-btn-handler">Paramètres</button><button id="onetrust-reject-all-handler">Tout refuser</button><button id="onetrust-accept-btn-handler">Tout accepter</button></div></div>`,
        refuse: "onetrust-reject-all-handler",
    },
    "cookieconsent v3": {
        markup: `<div id="cc-main"><div class="cm cm--box"><p class="cm__desc">Ce site utilise des cookies.</p><div class="cm__btns"><button class="cm__btn" data-role="all">Tout accepter</button><button class="cm__btn" data-role="necessary">Tout refuser</button><button class="cm__btn cm__btn--secondary" data-role="show">Gérer les préférences</button></div></div></div>`,
        refuse: "cm__btn",
        manager: "cookieconsent",
        control: element => element.getAttribute("data-role") === "necessary",
    },
    "cookieconsent v2": {
        markup: `<div role="dialog" class="cc-window cc-banner"><span class="cc-message">Ce site utilise des cookies.</span><div class="cc-compliance cc-highlight"><a role="button" class="cc-btn cc-deny">Refuser</a><a role="button" class="cc-btn cc-allow">Autoriser</a></div></div>`,
        refuse: "cc-btn cc-deny",
        manager: "cookieconsent",
    },
    osano: {
        markup: `<div class="osano-cm-window"><div class="osano-cm-dialog"><p>Ce site utilise des cookies.</p><button class="osano-cm-accept-all osano-cm-button">Tout accepter</button><button class="osano-cm-denyAll osano-cm-button">Tout refuser</button></div></div>`,
        refuse: "osano-cm-denyAll osano-cm-button",
    },
    klaro: {
        markup: `<div id="klaro" class="klaro"><div class="cookie-notice"><div class="cn-body"><p>Ce site utilise des cookies.</p><div class="cn-ok"><button class="cm-btn cm-btn-success">Accepter</button><button class="cm-btn cm-btn-danger cn-decline">Je refuse</button></div></div></div></div>`,
        refuse: "cm-btn cm-btn-danger cn-decline",
    },
    complianz: {
        markup: `<div id="cmplz-cookiebanner-container"><div class="cmplz-cookiebanner banner-1 optin"><p>Ce site utilise des cookies.</p><div class="cmplz-buttons"><button class="cmplz-btn cmplz-accept">Accepter</button><button class="cmplz-btn cmplz-deny">Refuser</button><button class="cmplz-btn cmplz-view-preferences">Préférences</button></div></div></div>`,
        refuse: "cmplz-btn cmplz-deny",
    },
    // Orejime (the Aveyron's Ligeo portal): "Accepter" is its save button.
    orejime: {
        markup: `<div class="orejime-Notice"><div class="orejime-Notice-body"><p class="orejime-Notice-description">En continuant votre navigation, vous acceptez l'utilisation de cookies.</p><ul class="orejime-Notice-actions"><li><button class="orejime-Button orejime-Button--save orejime-Notice-saveButton" type="button">Accepter</button></li><li><button class="orejime-Button orejime-Button--decline orejime-Notice-declineButton" type="button">Refuser</button></li><li><button class="orejime-Button orejime-Button--info orejime-Notice-learnMoreButton" type="button">En savoir plus</button></li></ul></div></div>`,
        refuse: "orejime-Button orejime-Button--decline orejime-Notice-declineButton",
    },
};

for (const [name, banner] of Object.entries(banners)) {
    test(`${name}: refuses with its refuse control, never an accept one`, () => {
        const page = run(`<main>Registres paroissiaux</main>${banner.markup}`);
        assert.equal(page.document.clicked.length, 1);
        const [control] = page.document.clicked;
        assert.equal(describe(control), banner.refuse);
        if (banner.control) assert.ok(banner.control(control));
        assert.deepEqual(page.posted, [{ kind: "consent", state: "refused", manager: banner.manager ?? name }]);
    });
}

test("a banner that appears later is refused once it shows", () => {
    const page = run(`<main>Registres</main><div hidden>${banners.arkotheque.markup}</div>`);
    assert.deepEqual(page.clicked(), []);
    page.timers.advance(2000);
    page.document.querySelector("div[hidden]").removeAttribute("hidden");
    page.document.notify();
    assert.deepEqual(page.clicked(), ["btn_refuser"]);
    // The banner goes: nothing more happens.
    page.document.getElementById("analytics-container").remove();
    page.document.notify();
    page.timers.advance(15000);
    assert.deepEqual(page.clicked(), ["btn_refuser"]);
    assert.deepEqual(page.posted, [{ kind: "consent", state: "refused", manager: "arkotheque" }]);
    assert.equal(page.document.observers.size, 0);
});

test("a banner with an accept control only is left to the reader", () => {
    const markup = `<div id="analytics-container"><div class="cookie_enabled"><span>Ce site utilise des cookies.</span><button type="button" class="btn_accepter">Accepter</button></div></div>`;
    const page = run(markup);
    assert.deepEqual(page.clicked(), []);
    assert.deepEqual(page.posted, [{ kind: "consent", state: "left", manager: "arkotheque" }]);
    // The watch ends; the banner stays the reader's until it is gone.
    page.timers.advance(15000);
    assert.equal(page.posted.length, 1);
    page.document.getElementById("analytics-container").remove();
    page.document.notify();
    assert.deepEqual(page.posted[1], { kind: "consent", state: "closed", manager: "arkotheque" });
    assert.equal(page.document.observers.size, 0);
    assert.deepEqual(page.clicked(), []);
});

test("an information banner whose only control acknowledges is left to the reader", () => {
    const page = run(`<div class="cc-window cc-banner"><span>Ce site utilise des cookies.</span><a role="button" class="cc-btn cc-dismiss">J'ai compris</a></div>`);
    assert.deepEqual(page.clicked(), []);
    assert.deepEqual(page.posted, [{ kind: "consent", state: "left", manager: "cookieconsent" }]);
});

test("a refuse control labelled as an accept is never clicked", () => {
    const page = run(`<div id="onetrust-banner-sdk"><button id="onetrust-reject-all-handler">Tout accepter</button><button id="onetrust-accept-btn-handler">Accepter</button></div>`);
    assert.deepEqual(page.clicked(), []);
    assert.deepEqual(page.posted, [{ kind: "consent", state: "left", manager: "onetrust" }]);
});

test("within a recognized banner, a control whose whole text refuses is clicked", () => {
    const markup = `<div id="analytics-container"><div class="cookie_enabled"><span>Ce site utilise des cookies.</span><button type="button" class="btn_accepter">Accepter</button><button type="button" class="btn_autre">  Tout   refuser </button></div></div>`;
    const page = run(markup);
    assert.deepEqual(page.clicked(), ["btn_autre"]);
});

test("a control merely mentioning a refusal is not clicked", () => {
    const markup = `<div id="analytics-container"><div class="cookie_enabled"><button type="button" class="btn_accepter">Accepter</button><a class="lien">Refuser les cookies publicitaires et accepter les autres</a></div></div>`;
    const page = run(markup);
    assert.deepEqual(page.clicked(), []);
    assert.equal(page.posted[0].state, "left");
});

test("a banner still on screen after a refusal is left to the reader", () => {
    const page = run(banners.osano.markup);
    assert.deepEqual(page.clicked(), ["osano-cm-denyAll osano-cm-button"]);
    // The banner is still there once the click has settled.
    page.timers.advance(1000);
    assert.deepEqual(page.clicked(), ["osano-cm-denyAll osano-cm-button"]);
    assert.deepEqual(page.posted.map(message => message.state), ["refused", "left"]);
});

test("a banner without any control yet is left to the reader once the watch ends", () => {
    const page = run(`<div class="cmplz-cookiebanner"><p>Ce site utilise des cookies.</p></div>`);
    assert.deepEqual(page.posted, []);
    page.timers.advance(15000);
    assert.deepEqual(page.posted, [{ kind: "consent", state: "left", manager: "complianz" }]);
    assert.deepEqual(page.clicked(), []);
});

test("a page without a banner, or with a hidden one, is left alone", () => {
    for (const body of [
        "<main>Registres paroissiaux</main><button>Refuser</button><button>Accepter</button>",
        `<div style="display: none">${banners.tarteaucitron.markup}</div>`,
    ]) {
        const page = run(body);
        page.document.notify();
        page.timers.advance(15000);
        assert.deepEqual(page.clicked(), [], body);
        assert.deepEqual(page.posted, [], body);
        assert.equal(page.document.observers.size, 0, body);
    }
});

test("a frame of the page is left alone", () => {
    const page = run(banners.arkotheque.markup, { frame: true });
    assert.deepEqual(page.clicked(), []);
    assert.equal(page.document.observers.size, 0);
});

test("every manager declares a banner, refuse and accept controls", () => {
    for (const manager of consent.managers) {
        assert.ok(manager.name && manager.banner, JSON.stringify(manager));
        assert.ok(manager.refuse.length > 0 && manager.accept.length > 0, manager.name);
    }
    const accept = new Set(consent.accept_phrases);
    assert.deepEqual(consent.refuse_phrases.filter(phrase => accept.has(phrase)), []);
});
