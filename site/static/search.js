// The header's search box: loads Zola's elasticlunr index of the page's
// language on first use, then lists the best matches as the reader types.
(function () {
    "use strict";
    const box = document.querySelector(".search");
    if (!box) return;
    const input = box.querySelector("input");
    const list = box.querySelector(".search-results");
    let index = null;
    let loading = null;

    function script(src) {
        return new Promise((resolve, reject) => {
            const element = document.createElement("script");
            element.src = src;
            element.onload = resolve;
            element.onerror = reject;
            document.head.appendChild(element);
        });
    }

    // The index of a language other than English names stemmers and
    // filters the bundled elasticlunr does not ship: they are registered as
    // identities, so the search still matches words and their prefixes.
    function load() {
        loading ??= script(box.dataset.lunr)
            .then(() => script(box.dataset.index))
            .then(() => {
                const identity = (token) => token;
                for (const name of window.searchIndex.pipeline || []) {
                    if (!elasticlunr.Pipeline.registeredFunctions[name]) {
                        elasticlunr.Pipeline.registerFunction(identity, name);
                    }
                }
                index = elasticlunr.Index.load(window.searchIndex);
            });
        return loading;
    }

    function excerpt(body, terms) {
        // The heading anchors' marks are not text.
        const text = (body || "").replaceAll("#", "").replace(/\s+/g, " ");
        const lower = text.toLowerCase();
        let at = -1;
        for (const term of terms) {
            at = lower.indexOf(term);
            if (at >= 0) break;
        }
        const start = Math.max(0, at - 60);
        return (start > 0 ? "…" : "") + text.slice(start, start + 160).trim() + "…";
    }

    function show(query) {
        list.replaceChildren();
        if (!index || query.trim().length < 2) {
            list.hidden = true;
            return;
        }
        const results = index.search(query, {
            bool: "AND",
            expand: true,
            fields: { title: { boost: 3 }, body: { boost: 1 } },
        }).slice(0, 8);
        const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
        if (results.length === 0) {
            const item = document.createElement("li");
            item.innerHTML = "<small></small>";
            item.firstChild.textContent = box.dataset.empty;
            list.appendChild(item);
        }
        for (const result of results) {
            const doc = index.documentStore.getDoc(result.ref);
            const item = document.createElement("li");
            const link = document.createElement("a");
            link.href = result.ref;
            const title = document.createElement("strong");
            title.textContent = doc.title;
            const snippet = document.createElement("small");
            snippet.textContent = excerpt(doc.body, terms);
            link.append(title, snippet);
            item.appendChild(link);
            list.appendChild(item);
        }
        list.hidden = false;
    }

    input.addEventListener("focus", load, { once: true });
    input.addEventListener("input", () => load().then(() => show(input.value)));
    input.addEventListener("keydown", (event) => {
        if (event.key === "Escape") {
            input.value = "";
            show("");
        } else if (event.key === "Enter") {
            const first = list.querySelector("a");
            if (first) window.location.href = first.href;
        }
    });
    document.addEventListener("click", (event) => {
        if (!box.contains(event.target)) list.hidden = true;
    });
})();
