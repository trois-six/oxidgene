// Arkothèque portal driver: searches the civil-status registers of one
// locality, act category and year, opens the only matching register and
// selects the cited view. Injected before every page load of the archive
// window; the Rust side replaces the request placeholder with JSON.
(() => {
    const request = __OXIDGENE_ARCHIVE_REQUEST__;
    const { portal, act, citation, messages } = request;
    if (location.origin !== portal.origin || location.pathname !== portal.searchPath) return;

    const group = field => `${portal.root}--filtreGroupes[groupes][0][${field}]`;
    const localityQuery = `${group(portal.localityField)}[q][]`;
    const actQuery = `${group(portal.actField)}[q][]`;
    const periodQuery = `${group(portal.periodField)}[q][]`;
    const period = `${citation.year}|${citation.year}`;
    const params = new URLSearchParams(location.search);
    const fold = value => value.normalize("NFD").replace(/[̀-ͯ]/g, "")
        .toLocaleLowerCase("fr").trim();
    const filtered = params.getAll(localityQuery)
            .some(value => value.startsWith(`${citation.locality}[[arko_fiche_`))
        && params.get(actQuery) === act.category
        && params.get(periodQuery) === period;
    const pause = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));

    // Each step runs once per window, so the reader can then change the
    // filters or go back without being sent to the cited search again.
    const firstTime = step => {
        const key = `oxidgene-archive-${step}`;
        try {
            if (sessionStorage.getItem(key)) return false;
            sessionStorage.setItem(key, "done");
        } catch (_) {}
        return true;
    };

    // A banner over the portal page: the page is not ours to translate.
    function status(text, settled) {
        const show = () => {
            let banner = document.getElementById("oxidgene-archive-status");
            if (!banner) {
                banner = document.createElement("div");
                banner.id = "oxidgene-archive-status";
                banner.setAttribute("role", "status");
                banner.style.cssText = "position:fixed;z-index:2147483647;top:12px;left:50%;"
                    + "transform:translateX(-50%);max-width:min(640px,90vw);display:flex;gap:12px;"
                    + "align-items:center;padding:10px 14px;border-radius:8px;background:#1e1a14;"
                    + "color:#f6f0e4;font:14px/1.4 system-ui,sans-serif;"
                    + "box-shadow:0 4px 16px #0000004d";
                const label = document.createElement("span");
                const close = document.createElement("button");
                close.type = "button";
                close.textContent = "×";
                close.setAttribute("aria-label", messages.close);
                close.style.cssText = "border:0;background:none;color:inherit;font-size:18px;"
                    + "cursor:pointer;line-height:1";
                close.addEventListener("click", () => banner.remove());
                banner.append(label, close);
                document.documentElement.append(banner);
            }
            banner.firstChild.textContent = text;
            if (settled === "done") setTimeout(() => banner.remove(), 1500);
        };
        if (document.documentElement) show();
        else document.addEventListener("DOMContentLoaded", show, { once: true });
    }

    async function waitFor(predicate, attempts = 150) {
        for (let attempt = 0; attempt < attempts; attempt += 1) {
            const value = predicate();
            if (value) return value;
            await pause(100);
        }
        throw new Error("archive page control timeout");
    }

    // Results arrive in batches: wait until the row count stops changing.
    async function settledRows(table) {
        let previous = -1;
        let stable = 0;
        await waitFor(() => {
            const count = table.querySelectorAll("tr").length;
            stable = count === previous ? stable + 1 : 0;
            previous = count;
            return stable >= 3;
        });
        return Array.from(table.querySelectorAll("tr"));
    }

    async function selectLocalityAndSearch() {
        status(messages.searching);
        const input = await waitFor(() => document.querySelector(portal.localityInput));
        input.parentElement
            .querySelector(`button[aria-label='${portal.localityListLabel}']`).click();
        const popup = await waitFor(() => document.querySelector(".filtre_liste_popup_container"));
        const firstLetter = fold(citation.locality)[0].toLocaleUpperCase("fr");
        const letter = Array.from(popup.querySelectorAll("nav button"))
            .find(button => button.textContent.trim() === firstLetter);
        if (!letter) return status(messages.not_found, "final");
        letter.click();

        const locality = await waitFor(() => Array.from(
            document.querySelectorAll(".filtre_liste_popup_container [role='checkbox']")
        ).find(item => fold(item.title) === fold(citation.locality)));
        locality.click();
        const chip = await waitFor(() => document.querySelector(
            `#aria-filtre-${portal.localityField} .filtres_texte_actifs button`
        ));
        const localityKey = chip.getAttribute("aria-label")
            ?.match(/\[\[arko_fiche_[^\]]+\]\]/)?.[0];
        if (!localityKey) return status(messages.failed, "final");

        const query = new URLSearchParams();
        query.set(`${portal.root}--ficheFocus`, "");
        query.set(`${portal.root}--filtreGroupes[mode]`, "simple");
        query.set(`${portal.root}--filtreGroupes[op]`, "AND");
        query.set(`${group(portal.localityField)}[op]`, "AND");
        query.append(localityQuery, "");
        query.append(localityQuery, `${citation.locality}${localityKey}`);
        query.set(`${group(portal.localityField)}[extras][mode]`, "popup");
        query.set(`${group(portal.actField)}[op]`, "AND");
        query.set(actQuery, act.category);
        query.set(`${group(portal.actField)}[extras][mode]`, "select");
        query.set(`${group(portal.periodField)}[op]`, "AND");
        query.set(periodQuery, period);
        query.set(`${group(portal.periodField)}[extras][mode]`, "slider");
        query.set(`${portal.root}--from`, "0");
        query.set(`${portal.root}--resultSize`, "25");
        for (const id of portal.contentIds) query.append(`${portal.root}--contenuIds[]`, id);
        query.set(`${portal.root}--modeRestit`, portal.displayMode);
        location.href = `${portal.origin}${portal.searchPath}?${query}`;
    }

    async function openRegister() {
        status(messages.searching);
        const table = await waitFor(() => document.querySelector("table tbody"));
        const rows = (await settledRows(table)).filter(row => {
            const text = fold(row.innerText);
            return text.includes(fold(citation.locality))
                && text.includes(String(citation.year))
                && text.includes(fold(act.rowLabel));
        });
        if (rows.length === 0) return status(messages.not_found, "final");
        if (rows.length > 1) return status(messages.ambiguous, "final");

        const open = rows[0].querySelector(`button[aria-label='${portal.openImagesLabel}']`);
        if (!open) return status(messages.failed, "final");
        open.click();
        const view = citation.view;
        await waitFor(() => {
            const total = Number(document.querySelector(".nb_total")?.textContent.match(/\d+/)?.[0]);
            const input = document.querySelector("input.nb_actuel");
            return input && !input.disabled && total && (!view || total === view.count);
        });
        if (!view) return status("", "done");
        if (await selectView(String(view.index))) return status("", "done");
        status(messages.view_not_selected, "final");
    }

    // The reader's page field is a React-controlled input: set its value
    // through the native setter so React sees the change, then confirm it
    // held once the reader has re-rendered.
    async function selectView(expected) {
        const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
        if (!setValue) return false;
        for (let attempt = 0; attempt < 8; attempt += 1) {
            const input = document.querySelector("input.nb_actuel");
            if (!input) return false;
            if (input.value === expected) {
                await pause(600);
                if (document.querySelector("input.nb_actuel")?.value === expected) return true;
                continue;
            }
            input.focus();
            input.select();
            setValue.call(input, expected);
            input.dispatchEvent(new InputEvent("input", {
                bubbles: true,
                inputType: "insertText",
                data: expected,
            }));
            await pause(300);
            input.blur();
            await pause(500);
        }
        return document.querySelector("input.nb_actuel")?.value === expected;
    }

    (async () => {
        try {
            if (filtered) {
                if (firstTime("register")) await openRegister();
            } else if (firstTime("search")) {
                await selectLocalityAndSearch();
            }
        } catch (_) {
            status(messages.failed, "final");
        }
    })();
})();
