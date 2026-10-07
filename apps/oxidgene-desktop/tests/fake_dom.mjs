// A tiny DOM for the archive window's page scripts, so that their tests run
// on Node.js without a dependency: a parser for well-formed fixture markup,
// the selectors the scripts use (type, `#id`, `.class`, `[attr]`,
// `[attr="value"]`, compounds of those, the descendant combinator and
// selector lists), visibility, clicks, values and dispatched events, a
// mutation observer the test fires, open shadow roots, and a manual clock.

const VOID = new Set(["br", "hr", "img", "input", "link", "meta"]);

const decode = text =>
    text.replace(/&(amp|lt|gt|quot|#39);/g, (_, entity) => ({ amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'" })[entity]);

class Text {
    constructor(text) {
        this.text = text;
        this.parent = null;
    }

    get textContent() {
        return this.text;
    }
}

class Element {
    constructor(document, localName) {
        this.ownerDocument = document;
        this.localName = localName;
        this.attributes = new Map();
        this.childNodes = [];
        this.parent = null;
        this.listeners = new Map();
        this.style = { cssText: "" };
        this.shadowRoot = null;
        this.host = null;
    }

    get children() {
        return this.childNodes.filter(node => node instanceof Element);
    }

    get id() {
        return this.attributes.get("id") ?? "";
    }

    set id(value) {
        this.attributes.set("id", value);
    }

    get value() {
        return this.attributes.get("value") ?? "";
    }

    set value(text) {
        this.attributes.set("value", String(text));
    }

    get textContent() {
        return this.childNodes.map(node => node.textContent).join("");
    }

    set textContent(text) {
        this.replaceChildren(new Text(String(text)));
    }

    get isConnected() {
        let node = this;
        while (node.parent || node.host) node = node.parent ?? node.host;
        return node === this.ownerDocument;
    }

    getAttribute(name) {
        return this.attributes.has(name) ? this.attributes.get(name) : null;
    }

    setAttribute(name, value) {
        this.attributes.set(name, String(value));
    }

    removeAttribute(name) {
        this.attributes.delete(name);
    }

    hasAttribute(name) {
        return this.attributes.has(name);
    }

    append(...nodes) {
        for (const node of nodes) {
            const child = typeof node === "string" ? new Text(node) : node;
            child.parent?.removeChild(child);
            child.parent = this;
            this.childNodes.push(child);
        }
    }

    removeChild(child) {
        this.childNodes = this.childNodes.filter(node => node !== child);
        child.parent = null;
    }

    replaceChildren(...nodes) {
        for (const node of this.childNodes) node.parent = null;
        this.childNodes = [];
        this.append(...nodes);
    }

    remove() {
        this.parent?.removeChild(this);
    }

    attachShadow() {
        this.shadowRoot = new Element(this.ownerDocument, "#shadow-root");
        this.shadowRoot.host = this;
        return this.shadowRoot;
    }

    addEventListener(type, listener) {
        const listeners = this.listeners.get(type) ?? [];
        listeners.push(listener);
        this.listeners.set(type, listeners);
    }

    click() {
        this.ownerDocument.clicked.push(this);
        for (const listener of this.listeners.get("click") ?? []) listener({ type: "click", target: this });
    }

    focus() {
        this.ownerDocument.activeElement = this;
    }

    blur() {
        if (this.ownerDocument.activeElement === this) this.ownerDocument.activeElement = null;
    }

    // Records the event on the document, then runs this element's
    // listeners of its type (no propagation).
    dispatchEvent(event) {
        this.ownerDocument.dispatched.push({ target: this, event });
        for (const listener of this.listeners.get(event.type) ?? []) listener(event);
        return true;
    }

    // Laid out unless detached, `hidden`, or `display: none` inline, on
    // itself or an ancestor.
    getClientRects() {
        if (!this.isConnected) return [];
        for (let node = this; node instanceof Element; node = node.parent ?? node.host) {
            const style = (node.getAttribute("style") ?? "").replace(/\s+/g, "");
            if (node.hasAttribute("hidden") || style.includes("display:none")) return [];
        }
        return [{}];
    }

    // The element's descendants in document order, shadow trees aside.
    descendants() {
        return this.children.flatMap(child => [child, ...child.descendants()]);
    }

    querySelectorAll(selector) {
        const list = parseSelectorList(selector);
        return this.descendants().filter(element => list.some(complex => matchesComplex(element, complex)));
    }

    querySelector(selector) {
        return this.querySelectorAll(selector)[0] ?? null;
    }

    matches(selector) {
        return parseSelectorList(selector).some(complex => matchesComplex(this, complex));
    }
}

// One compound selector: `input[type="button"].primary#go`.
function parseCompound(text) {
    const compound = { tag: null, ids: [], classes: [], attributes: [] };
    const token = /^(?:([a-zA-Z][\w-]*)|\*|#([\w-]+)|\.([\w-]+)|\[([\w-]+)(?:=(?:"([^"]*)"|'([^']*)'|([^\]]*)))?\])/;
    let rest = text;
    while (rest) {
        const found = token.exec(rest);
        if (!found) throw new Error(`unsupported selector: ${text}`);
        const [whole, tag, id, className, attribute, double, single, bare] = found;
        if (tag) compound.tag = tag.toLowerCase();
        if (id) compound.ids.push(id);
        if (className) compound.classes.push(className);
        if (attribute) compound.attributes.push({ name: attribute, value: double ?? single ?? bare ?? null });
        rest = rest.slice(whole.length);
    }
    return compound;
}

function parseSelectorList(selector) {
    return selector.split(/,(?![^[]*\])/).map(complex => complex.trim().split(/\s+(?![^[]*\])/).map(parseCompound));
}

function matchesCompound(element, compound) {
    const classes = (element.getAttribute("class") ?? "").split(/\s+/);
    return (
        (compound.tag === null || element.localName === compound.tag)
        && compound.ids.every(id => element.id === id)
        && compound.classes.every(name => classes.includes(name))
        && compound.attributes.every(
            ({ name, value }) => element.hasAttribute(name) && (value === null || element.getAttribute(name) === value),
        )
    );
}

function matchesComplex(element, compounds) {
    if (!matchesCompound(element, compounds[compounds.length - 1])) return false;
    let index = compounds.length - 2;
    for (let node = element.parent; node instanceof Element && index >= 0; node = node.parent) {
        if (matchesCompound(node, compounds[index])) index -= 1;
    }
    return index < 0;
}

class Document extends Element {
    constructor() {
        super(null, "#document");
        this.ownerDocument = this;
        this.readyState = "complete";
        this.clicked = [];
        this.dispatched = [];
        this.activeElement = null;
        this.observers = new Set();
        const document = this;
        this.MutationObserver = class {
            constructor(callback) {
                this.callback = callback;
            }

            observe() {
                document.observers.add(this);
            }

            disconnect() {
                document.observers.delete(this);
            }
        };
    }

    get documentElement() {
        return this.children[0] ?? null;
    }

    get body() {
        return this.documentElement?.children.find(child => child.localName === "body") ?? null;
    }

    get isConnected() {
        return true;
    }

    createElement(localName) {
        return new Element(this, localName.toLowerCase());
    }

    getElementById(id) {
        return this.descendants().find(element => element.id === id) ?? null;
    }

    // What a mutation observer hears of a change the test made.
    notify() {
        for (const observer of [...this.observers]) observer.callback([], observer);
    }
}

// The document of `body`'s markup, which must be well formed.
export function parse(body) {
    const document = new Document();
    const html = document.createElement("html");
    document.append(html);
    html.append(document.createElement("head"));
    const root = document.createElement("body");
    html.append(root);
    const stack = [root];
    const tag = /<!--[\s\S]*?-->|<\/([\w-]+)\s*>|<([\w-]+)((?:\s+[^\s=/>]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+))?)*)\s*(\/?)>|([^<]+)/g;
    const attribute = /([^\s=/>]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/g;
    for (const [, closing, opening, attributes, selfClosing, text] of body.matchAll(tag)) {
        const parent = stack[stack.length - 1];
        if (text !== undefined) {
            parent.append(decode(text));
        } else if (closing) {
            if (stack.length > 1) stack.pop();
        } else if (opening) {
            const element = document.createElement(opening);
            for (const [, name, double, single, bare] of attributes.matchAll(attribute)) {
                element.setAttribute(name.toLowerCase(), decode(double ?? single ?? bare ?? ""));
            }
            parent.append(element);
            if (!selfClosing && !VOID.has(element.localName)) stack.push(element);
        }
    }
    return document;
}

// A manual clock: `setTimeout`, `setInterval` and `now` that only move when
// the test calls `advance`.
export function clock() {
    let now = 0;
    let next = 1;
    const timers = new Map();
    const add = (callback, delay, every) => {
        const id = next++;
        timers.set(id, { callback, at: now + delay, every });
        return id;
    };
    return {
        now: () => now,
        setTimeout: (callback, delay = 0) => add(callback, delay, null),
        setInterval: (callback, delay) => add(callback, delay, delay),
        clearTimeout: id => timers.delete(id),
        clearInterval: id => timers.delete(id),
        get pending() {
            return timers.size;
        },
        advance(ms) {
            const end = now + ms;
            for (;;) {
                const due = [...timers.entries()].filter(([, timer]) => timer.at <= end).sort((a, b) => a[1].at - b[1].at)[0];
                if (!due) break;
                const [id, timer] = due;
                now = timer.at;
                if (timer.every) timer.at += timer.every;
                else timers.delete(id);
                timer.callback();
            }
            now = end;
        },
    };
}
