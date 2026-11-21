// The native side of the shared `Modal` (components/modal.rs): the `dialog`
// is opened with `showModal()`, so the engine itself traps the focus in it,
// makes the page behind inert and stacks it over everything else. Rust keeps
// deciding when a dialog closes: the element never closes on its own.
//
// Evaluated before each call; the object is built once per page.
window.oxidgeneModal ??= (() => {
    // What had the focus when each dialog was asked for, by dialog id.
    const openers = new Map();
    // Dialogs Rust is closing: their `close` event is expected.
    const closing = new Set();
    // How long after an Escape key press a `cancel` event is taken to be
    // that press's own, already seen by the Rust key handler.
    const ESCAPE_WINDOW_MS = 250;

    const focus = element => element.focus({ preventScroll: true });

    function show(dialog) {
        // A field of the dialog that took the focus as it mounted keeps it.
        const held = dialog.contains(document.activeElement) ? document.activeElement : null;
        dialog.showModal();
        focus(held ?? dialog);
        // Fields mounted since had their chance; the focus must end inside.
        requestAnimationFrame(() => {
            if (dialog.isConnected && dialog.open && !dialog.contains(document.activeElement)) {
                focus(dialog);
            }
        });
    }

    return {
        // Called while the dialog is rendered, before any of its fields can
        // take the focus: the element to give it back to.
        remember(id) {
            openers.set(id, document.activeElement);
        },

        open(id) {
            const dialog = document.getElementById(id);
            if (!dialog || dialog.open) return;
            let lastEscape = -Infinity;
            dialog.addEventListener("keydown", event => {
                if (event.key === "Escape" && event.isTrusted) lastEscape = performance.now();
            }, true);
            // A close request the engine raises: never obeyed here. One
            // that no key press announced (a back gesture) is handed to
            // Rust as the Escape it stands for.
            dialog.addEventListener("cancel", event => {
                if (event.target !== dialog) return;
                event.preventDefault();
                if (performance.now() - lastEscape > ESCAPE_WINDOW_MS) {
                    dialog.dispatchEvent(new KeyboardEvent("keydown", {
                        key: "Escape", bubbles: true, cancelable: true,
                    }));
                }
            });
            // An engine may close a dialog whose cancel was refused twice
            // in a row; a dialog Rust still draws opens again.
            dialog.addEventListener("close", event => {
                if (event.target === dialog && !closing.has(id) && dialog.isConnected) show(dialog);
            });
            show(dialog);
        },

        // Called when Rust removes the dialog: the focus returns to what
        // opened it, unless closing already moved it somewhere of its own.
        close(id) {
            const opener = openers.get(id);
            openers.delete(id);
            const dialog = document.getElementById(id);
            const active = document.activeElement;
            const lost = !active || active === document.body || Boolean(dialog?.contains(active));
            // A dialog drawn in this one's place (a loading frame giving way
            // to its content) remembered an element that leaves with it: it
            // inherits this dialog's opener.
            for (const [other, element] of openers) {
                if (!element?.isConnected || dialog?.contains(element)) openers.set(other, opener);
            }
            if (dialog?.open) {
                closing.add(id);
                dialog.close();
                closing.delete(id);
            }
            if (lost && opener?.isConnected) focus(opener);
        },
    };
})();
