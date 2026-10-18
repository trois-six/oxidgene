// The PDF viewer and the gallery's PDF previews, drawn by pdf.js
// (docs/ui-common.md, Media viewer). Run through `document::eval`: the body
// of an async function with `window`, `document` and `dioxus` in scope.
//
// The first message says what to draw:
//   { mode: "viewer", id, page_label }  the viewer, in the scrolling element
//                                       `id`, whose first child holds pages
//   { mode: "preview", id, key }        page 1 of medium `key`, once the
//                                       element `id` comes into view
// The file follows as one message — straight away for the viewer, once the
// preview asks for it with { event: "want" }:
//   { path }        a path of this origin to fetch: the desktop's media handler
//   { size }        then base64 chunks adding up to `size` bytes (the web
//                   build, whose bytes come through the typed client)
//   { cancel }      the caller is gone; anything else is a failure
// The viewer then takes commands until { cmd: "close" }. What the script
// reports is an object with an `event` field: opened, page, scale, found,
// want, preview, failed.
//
// pdf.js is loaded once per page, on first use, from /pdfjs/ (served by the
// web bundle and by the desktop's own handler, never from the network). It
// runs no script of the document, renders no XFA form and never evaluates
// code built from the file.

const pdf = (window.__oxPdf ??= library(window, document));
const request = await dioxus.recv();
if (request?.mode === "viewer") {
    await viewer(pdf, request, dioxus, document);
} else if (request?.mode === "preview") {
    await preview(pdf, request, dioxus, document);
}

function library(window, document) {
    const base = new URL("/pdfjs/", document.baseURI).href;
    // Two previews at a time: each one holds a whole file and a canvas.
    const LIMIT = 2;
    // Page 1 of a tile is drawn this wide, twice the tile on a dense screen.
    const PREVIEW_WIDTH = 320;
    let loading = null;
    let worker = null;
    let observer = null;
    const watched = new Map();
    const waiting = [];
    let running = 0;

    // The library, its viewer components and their stylesheet. A failed
    // load is forgotten, so the next PDF tries again.
    function load() {
        loading ??= (async () => {
            const lib = await import(base + "pdf.min.mjs");
            lib.GlobalWorkerOptions.workerSrc = base + "pdf.worker.min.mjs";
            // The viewer components read the library from the global object.
            window.pdfjsLib = lib;
            const components = await import(base + "pdf_viewer.mjs");
            const link = document.createElement("link");
            link.rel = "stylesheet";
            link.href = base + "pdf_viewer.css";
            const styled = new Promise(resolve => { link.onload = link.onerror = resolve; });
            document.head.append(link);
            await styled;
            return { lib, components };
        })();
        loading.catch(() => { loading = null; });
        return loading;
    }

    // One worker for every document of the page: it parses 1.3 MB of script
    // when it starts, which a tile per PDF would pay again and again.
    function open(lib, data) {
        worker ??= new lib.PDFWorker();
        return lib.getDocument({
            data,
            worker,
            wasmUrl: base + "wasm/",
            // Gone since pdf.js 4, which never evaluates code built from a
            // file (CVE-2024-4367); said anyway so no release can bring it back.
            isEvalSupported: false,
            enableXfa: false,
            // The whole file is already here: nothing to stream or to fetch
            // by range.
            disableRange: true,
            disableStream: true,
            disableAutoFetch: true,
        });
    }

    function decode(base64) {
        if (typeof Uint8Array.fromBase64 === "function") return Uint8Array.fromBase64(base64);
        const binary = atob(base64);
        const bytes = new Uint8Array(binary.length);
        for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
        return bytes;
    }

    // The file a message announces, or null when the caller gave up.
    async function receive(file, dioxus) {
        if (file?.cancel) return null;
        if (typeof file?.path === "string") {
            const response = await fetch(file.path);
            if (!response.ok) throw new Error(`the file answered ${response.status}`);
            return new Uint8Array(await response.arrayBuffer());
        }
        if (!Number.isSafeInteger(file?.size) || file.size < 0) throw new Error("no file");
        const bytes = new Uint8Array(file.size);
        let offset = 0;
        while (offset < file.size) {
            const chunk = await dioxus.recv();
            if (typeof chunk !== "string") return null;
            const part = decode(chunk);
            // A chunk past the announced size throws, which is a failure.
            bytes.set(part, offset);
            offset += part.length;
            if (part.length === 0) throw new Error("an empty chunk");
        }
        return bytes;
    }

    // Resolves once `element` comes within a screen of the viewport.
    function visible(element) {
        if (typeof window.IntersectionObserver !== "function") return Promise.resolve();
        observer ??= new window.IntersectionObserver(entries => {
            for (const entry of entries) {
                if (!entry.isIntersecting) continue;
                observer.unobserve(entry.target);
                const resolve = watched.get(entry.target);
                watched.delete(entry.target);
                resolve?.();
            }
        }, { rootMargin: "200px" });
        return new Promise(resolve => {
            watched.set(element, resolve);
            observer.observe(element);
        });
    }

    function forget(element) {
        observer?.unobserve(element);
        watched.delete(element);
    }

    // A place in the queue of previews: `ready` resolves when it may run,
    // `done` frees the place, `cancel` leaves the queue unserved.
    function queue() {
        const ticket = { cancelled: false, started: false };
        ticket.ready = new Promise(resolve => { ticket.start = resolve; });
        ticket.done = () => {
            if (!ticket.started) return;
            ticket.started = false;
            let next = waiting.shift();
            while (next?.cancelled) next = waiting.shift();
            if (next) begin(next);
            else running -= 1;
        };
        ticket.cancel = () => {
            ticket.cancelled = true;
            ticket.done();
        };
        if (running < LIMIT) {
            running += 1;
            begin(ticket);
        } else {
            waiting.push(ticket);
        }
        return ticket;
    }

    function begin(ticket) {
        ticket.started = true;
        ticket.start();
    }

    // Page 1 drawn on white, as an image the tile can show.
    async function render(lib, data) {
        const task = open(lib, data);
        try {
            const documentProxy = await task.promise;
            const page = await documentProxy.getPage(1);
            const natural = page.getViewport({ scale: 1 });
            const viewport = page.getViewport({ scale: PREVIEW_WIDTH / natural.width });
            const canvas = document.createElement("canvas");
            canvas.width = Math.ceil(viewport.width);
            canvas.height = Math.ceil(viewport.height);
            await page.render({
                canvasContext: canvas.getContext("2d"),
                viewport,
                background: "rgb(255, 255, 255)",
            }).promise;
            const blob = await new Promise(resolve => canvas.toBlob(resolve, "image/jpeg", 0.85));
            if (!blob) throw new Error("the page could not be encoded");
            return URL.createObjectURL(blob);
        } finally {
            await task.destroy();
        }
    }

    // pdf.js names each page for assistive technology through its own
    // localisation, which would say it in English: this one says it in the
    // interface's language, `template` being "Page {page}" translated. It
    // follows the pages the viewer adds and renumbers, as pdf.js's does.
    function labels(template) {
        const observers = [];
        const name = node => {
            if (node?.getAttribute?.("data-l10n-id") !== "pdfjs-page-landmark") return;
            const { page } = JSON.parse(node.getAttribute("data-l10n-args") || "{}");
            node.setAttribute("aria-label", String(template).replace("{page}", String(page)));
        };
        const nameAll = root => {
            name(root);
            for (const node of root?.querySelectorAll?.("[data-l10n-id]") ?? []) name(node);
        };
        const translate = async root => {
            nameAll(root);
            if (!root || typeof window.MutationObserver !== "function") return;
            const observer = new window.MutationObserver(records => {
                for (const record of records) {
                    if (record.type === "attributes") name(record.target);
                    for (const node of record.addedNodes) nameAll(node);
                }
            });
            observer.observe(root, {
                subtree: true,
                childList: true,
                attributes: true,
                attributeFilter: ["data-l10n-args"],
            });
            observers.push(observer);
        };
        return {
            getLanguage: () => document.documentElement?.lang || "en",
            getDirection: () => "ltr",
            get: async (_ids, _args, fallback) => fallback ?? "",
            translate,
            translateOnce: async root => nameAll(root),
            pause() {},
            resume() {},
            destroy: async () => {
                for (const observer of observers.splice(0)) observer.disconnect();
            },
        };
    }

    return {
        load, open, receive, decode, visible, forget, queue, render, labels,
        previews: new Map(),
    };
}

async function viewer(pdf, request, dioxus, document) {
    let shown = null;
    try {
        const loaded = pdf.load();
        loaded.catch(() => {});
        const data = await pdf.receive(await dioxus.recv(), dioxus);
        if (!data) return;
        const { lib, components } = await loaded;
        const container = document.getElementById(request.id);
        if (!container) return;
        shown = await show(pdf, lib, components, container, request, data, dioxus);
    } catch (error) {
        console.warn("pdf.js could not open the file", error);
        dioxus.send({ event: "failed" });
        return;
    }
    try {
        for (;;) {
            const command = await dioxus.recv();
            if (!command || command.cmd === "close") break;
            obey(shown, command);
        }
    } finally {
        shown.resize.disconnect();
        await shown.l10n.destroy();
        await shown.task.destroy();
    }
}

async function show(pdf, lib, components, container, request, data, dioxus) {
    const eventBus = new components.EventBus();
    const linkService = new components.PDFLinkService({
        eventBus,
        // A link out of the document opens apart from the application; the
        // desktop hands it to the system's browser.
        externalLinkTarget: components.LinkTarget.BLANK,
        externalLinkRel: "noopener noreferrer nofollow",
    });
    const findController = new components.PDFFindController({ eventBus, linkService });
    const l10n = pdf.labels(request.page_label ?? "{page}");
    const view = new components.PDFViewer({
        container,
        viewer: container.firstElementChild,
        eventBus,
        linkService,
        findController,
        l10n,
        // The text layer, which selection, copying and search work on.
        textLayerMode: 1,
        // Links and the annotations' appearance; forms are drawn, never
        // filled in, and no editing tool is offered.
        annotationMode: lib.AnnotationMode.ENABLE,
        annotationEditorMode: lib.AnnotationEditorType.DISABLE,
    });
    linkService.setViewer(view);
    // Given a localisation, the viewer leaves its container to the caller.
    await l10n.translate(container);
    const shown = { view, eventBus, l10n, fitted: true };
    eventBus.on("pagesinit", () => { view.currentScaleValue = "page-width"; });
    eventBus.on("pagechanging", ({ pageNumber }) => dioxus.send({ event: "page", page: pageNumber }));
    eventBus.on("scalechanging", ({ scale }) => {
        dioxus.send({ event: "scale", percent: Math.round(scale * 100) });
    });
    const found = matches => dioxus.send({
        event: "found",
        current: matches?.current ?? 0,
        total: matches?.total ?? 0,
    });
    eventBus.on("updatefindmatchescount", ({ matchesCount }) => found(matchesCount));
    eventBus.on("updatefindcontrolstate", ({ state, matchesCount }) => {
        // FindState: 0 found, 1 not found, 2 wrapped, 3 pending.
        if (state !== 3) found(state === 1 ? null : matchesCount);
    });
    // A fitted document stays fitted when its window is resized.
    shown.resize = new ResizeObserver(() => {
        if (shown.fitted && view.pagesCount) view.currentScaleValue = "page-width";
    });
    shown.resize.observe(container);
    shown.task = pdf.open(lib, data);
    const documentProxy = await shown.task.promise;
    view.setDocument(documentProxy);
    linkService.setDocument(documentProxy, null);
    dioxus.send({ event: "opened", pages: documentProxy.numPages });
    return shown;
}

function obey(shown, command) {
    const { view, eventBus } = shown;
    switch (command.cmd) {
        case "zoom":
            shown.fitted = false;
            view.currentScale = command.percent / 100;
            break;
        case "fit":
            shown.fitted = true;
            view.currentScaleValue = "page-width";
            break;
        case "page":
            view.currentPageNumber = command.page;
            break;
        case "find":
            eventBus.dispatch("find", {
                source: null,
                type: command.again ? "again" : "",
                query: command.query,
                caseSensitive: false,
                entireWord: false,
                highlightAll: true,
                findPrevious: !!command.previous,
                matchDiacritics: false,
            });
            break;
        default:
            break;
    }
}

async function preview(pdf, request, dioxus, document) {
    const { key, id } = request;
    if (pdf.previews.has(key)) {
        const url = pdf.previews.get(key);
        dioxus.send(url ? { event: "preview", url } : { event: "failed" });
        return;
    }
    const element = document.getElementById(id);
    if (!element) return;
    // Nothing is sent before the script asks for the file, but a cancel.
    const reply = dioxus.recv();
    const STOP = {};
    const stopped = reply.then(() => STOP, () => STOP);
    if ((await Promise.race([pdf.visible(element), stopped])) === STOP) {
        pdf.forget(element);
        return;
    }
    const ticket = pdf.queue();
    if ((await Promise.race([ticket.ready, stopped])) === STOP) {
        ticket.cancel();
        return;
    }
    try {
        dioxus.send({ event: "want" });
        const data = await pdf.receive(await reply, dioxus);
        if (!data) return;
        const { lib } = await pdf.load();
        const url = await pdf.render(lib, data);
        pdf.previews.set(key, url);
        dioxus.send({ event: "preview", url });
    } catch (error) {
        console.warn("pdf.js could not draw a preview", error);
        pdf.previews.set(key, null);
        dioxus.send({ event: "failed" });
    } finally {
        ticket.done();
    }
}
