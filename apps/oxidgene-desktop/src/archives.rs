use std::sync::{Arc, Mutex};

use dioxus::desktop::tao::dpi::LogicalSize;
use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::tao::event_loop::EventLoopWindowTarget;
use dioxus::desktop::tao::window::{Window, WindowBuilder};
use dioxus::desktop::wry::{WebView, WebViewBuilder};
use oxidgene_ui::archive_viewer::{ArchiveViewerBridge, ArchiveViewerOpener, ArchiveViewerRequest};
use tracing::warn;

const ARCHIVE_URL: &str =
    "https://archives-numerisees.loire-atlantique.fr/chercher/etat-civil-et-registres-paroissiaux";
type Pending = Arc<Mutex<Vec<ArchiveViewerRequest>>>;

struct QueueingArchiveViewer(Pending);

impl ArchiveViewerOpener for QueueingArchiveViewer {
    fn open(&self, request: ArchiveViewerRequest) {
        if let Ok(mut pending) = self.0.lock() {
            pending.push(request);
        }
    }
}

struct ArchiveWindow {
    window: Window,
    _webview: WebView,
}

pub fn install<T: 'static>() -> (
    ArchiveViewerBridge,
    impl FnMut(&Event<'_, T>, &EventLoopWindowTarget<T>) + 'static,
) {
    let pending: Pending = Arc::new(Mutex::new(Vec::new()));
    let bridge = ArchiveViewerBridge::new(Arc::new(QueueingArchiveViewer(Arc::clone(&pending))));
    let handler_pending = Arc::clone(&pending);
    let mut windows = Vec::<ArchiveWindow>::new();

    let handler = move |event: &Event<'_, T>, target: &EventLoopWindowTarget<T>| {
        let requests: Vec<_> = handler_pending
            .lock()
            .map(|mut pending| pending.drain(..).collect())
            .unwrap_or_default();

        for request in requests {
            if let Some(window) = open(target, &request) {
                windows.push(window);
            }
        }

        if let Event::WindowEvent {
            window_id,
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            windows.retain(|open| open.window.id() != *window_id);
        }
    };

    (bridge, handler)
}

fn open<T>(
    target: &EventLoopWindowTarget<T>,
    request: &ArchiveViewerRequest,
) -> Option<ArchiveWindow> {
    let window = WindowBuilder::new()
        .with_title(request.title.clone())
        .with_inner_size(LogicalSize::new(1280.0, 900.0))
        .build(target)
        .inspect_err(|_| {
            warn!(
                error = "archive_window_creation",
                "could not create the AD44 archive window"
            );
        })
        .ok()?;

    let script = initialization_script(request);
    let builder = WebViewBuilder::new()
        .with_url(ARCHIVE_URL)
        .with_incognito(true)
        .with_initialization_script(&script);

    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    ))]
    let built = builder.build(&window);

    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    )))]
    let built = {
        use dioxus::desktop::tao::platform::unix::WindowExtUnix;
        use dioxus::desktop::wry::WebViewBuilderExtUnix;

        match window.default_vbox() {
            Some(vbox) => builder.build_gtk(vbox),
            None => builder.build_gtk(window.gtk_window()),
        }
    };

    let webview = built
        .inspect_err(|_| {
            warn!(
                error = "archive_webview_creation",
                "could not create the AD44 WebView"
            );
        })
        .ok()?;

    Some(ArchiveWindow {
        window,
        _webview: webview,
    })
}

fn initialization_script(request: &ArchiveViewerRequest) -> String {
    let payload = serde_json::json!({
        "commune": request.commune,
        "actCode": request.act_code,
        "year": request.year,
        "pageIndex": request.page_index,
        "pageCount": request.page_count,
    });
    let payload = serde_json::to_string(&payload).expect("archive request serializes");
    ARCHIVE_SCRIPT.replace("__OXIDGENE_ARCHIVE_REQUEST__", &payload)
}

const ARCHIVE_SCRIPT: &str = r##"
(() => {
    const request = __OXIDGENE_ARCHIVE_REQUEST__;
    const origin = "https://archives-numerisees.loire-atlantique.fr";
    const route = "/chercher/etat-civil-et-registres-paroissiaux";
    if (location.origin !== origin || location.pathname !== route) return;

    const root = "arko_default_6a6b4ac0309a5";
    const communeField = "arko_default_6a6b4ba5532ce";
    const actField = "arko_default_6a6b4ba578da0";
    const periodField = "arko_default_6a6b4ba58e64b";
    const communeQuery = `${root}--filtreGroupes[groupes][0][${communeField}][q][]`;
    const actQuery = `${root}--filtreGroupes[groupes][0][${actField}][q][]`;
    const periodQuery = `${root}--filtreGroupes[groupes][0][${periodField}][q][]`;
    const category = "Baptêmes et naissances[[arko_fiche_6a6b3d70f0fdf]]";
    const params = new URLSearchParams(location.search);
    const fold = value => value.normalize("NFD").replace(/[\u0300-\u036f]/g, "")
        .toLocaleLowerCase("fr").trim();
    const requestedTown = `${request.commune}[[arko_fiche_`;
    const filtered = params.getAll(communeQuery).some(value => value.startsWith(requestedTown))
        && params.get(actQuery) === category
        && params.get(periodQuery) === `${request.year}|${request.year}`;
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

    async function waitFor(predicate, attempts = 120) {
        for (let attempt = 0; attempt < attempts; attempt += 1) {
            const value = predicate();
            if (value) return value;
            await pause(100);
        }
        throw new Error("archive page control timeout");
    }

    async function selectTownAndSearch() {
        const communeInput = await waitFor(() => document.querySelector("#commune"));
        communeInput.parentElement.querySelector("button[aria-label='Consulter la liste']").click();
        let popup = await waitFor(() => document.querySelector(".filtre_liste_popup_container"));
        const firstLetter = fold(request.commune)[0].toLocaleUpperCase("fr");
        const letter = Array.from(popup.querySelectorAll("nav button"))
            .find(button => button.textContent.trim() === firstLetter);
        if (!letter) return;
        letter.click();

        const town = await waitFor(() => Array.from(
            document.querySelectorAll(".filtre_liste_popup_container [role='checkbox']")
        ).find(item => fold(item.title) === fold(request.commune)));
        town.click();
        const chip = await waitFor(() => document.querySelector(
            `#aria-filtre-${communeField} .filtres_texte_actifs button`
        ));
        const townKey = chip.getAttribute("aria-label")
            .match(/\[\[arko_fiche_[^\]]+\]\]/)?.[0];
        if (!townKey) return;

        const query = new URLSearchParams();
        query.set(`${root}--ficheFocus`, "");
        query.set(`${root}--filtreGroupes[mode]`, "simple");
        query.set(`${root}--filtreGroupes[op]`, "AND");
        query.set(`${root}--filtreGroupes[groupes][0][${communeField}][op]`, "AND");
        query.append(communeQuery, "");
        query.append(communeQuery, `${request.commune}${townKey}`);
        query.set(`${root}--filtreGroupes[groupes][0][${communeField}][extras][mode]`, "popup");
        query.set(`${root}--filtreGroupes[groupes][0][${actField}][op]`, "AND");
        query.set(actQuery, category);
        query.set(`${root}--filtreGroupes[groupes][0][${actField}][extras][mode]`, "select");
        query.set(`${root}--filtreGroupes[groupes][0][${periodField}][op]`, "AND");
        query.set(periodQuery, `${request.year}|${request.year}`);
        query.set(`${root}--filtreGroupes[groupes][0][${periodField}][extras][mode]`, "slider");
        query.set(`${root}--from`, "0");
        query.set(`${root}--resultSize`, "25");
        query.append(`${root}--contenuIds[]`, "1289790");
        query.set(`${root}--modeRestit`, "arko_default_6a6b4d95b8dc8");
        location.href = `${origin}${route}?${query}`;
    }

    async function openRegister() {
        const table = await waitFor(() => document.querySelector("table tbody"));
        await pause(500);
        const expectedType = request.actCode === "B" ? "Baptêmes" : "Naissances";
        const rows = Array.from(table.querySelectorAll("tr")).filter(row => {
            const text = fold(row.innerText);
            return text.includes(fold(request.commune))
                && text.includes(String(request.year))
                && text.includes(fold(expectedType));
        });
        if (rows.length !== 1) return;

        const openButton = rows[0].querySelector("button[aria-label='Visualiser les images']");
        if (!openButton) return;
        openButton.click();
        await waitFor(() => {
            const totalText = document.querySelector(".nb_total")?.textContent ?? "";
            const total = Number(totalText.match(/\d+/)?.[0]);
            return document.querySelector("input.nb_actuel")
                && total
                && (!request.pageCount || total === request.pageCount);
        });
        if (!request.pageIndex) return;

        await pause(1400);
        const expectedIndex = String(request.pageIndex);
        const nativeValueSetter = Object.getOwnPropertyDescriptor(
            HTMLInputElement.prototype,
            "value",
        )?.set;
        if (!nativeValueSetter) return;

        for (let attempt = 0; attempt < 6; attempt += 1) {
            const input = document.querySelector("input.nb_actuel");
            if (!input) return;
            if (input.value === expectedIndex) {
                await pause(800);
                if (document.querySelector("input.nb_actuel")?.value === expectedIndex) return;
            }

            input.focus();
            input.select();
            nativeValueSetter.call(input, expectedIndex);
            input.dispatchEvent(new InputEvent("input", {
                bubbles: true,
                inputType: "insertText",
                data: expectedIndex,
            }));
            await pause(400);
            input.blur();
            await pause(600);
            if (document.querySelector("input.nb_actuel")?.value === expectedIndex) return;
        }
        console.warn("OxidGene archive view selection did not stick");
    }

    (async () => {
        try {
            if (filtered) {
                if (firstTime("register")) await openRegister();
            } else if (firstTime("search")) {
                await selectTownAndSearch();
            }
        } catch (_) {
            console.warn("OxidGene archive lookup did not complete");
        }
    })();
})();
"##;
