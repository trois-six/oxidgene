// Back and forward from the keyboard and the mouse, for the desktop window,
// which has no browser to handle them. Alt+Left and Alt+Right, and the
// mouse's back and forward buttons, report "back" or "forward"; a shortcut
// typed in a field is the field's (Alt+Left moves by word on some systems).
// The listeners are installed once per window; a later install only takes
// over where they report to.
window.__oxHistoryReport = direction => dioxus.send(direction);
if (window.__oxHistoryKeys) return;
window.__oxHistoryKeys = true;

const editable = target =>
    !!target && (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName));

document.addEventListener("keydown", event => {
    if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (event.defaultPrevented || editable(event.target)) return;
    const direction = { ArrowLeft: "back", ArrowRight: "forward" }[event.key];
    if (!direction) return;
    event.preventDefault();
    window.__oxHistoryReport(direction);
});

// Buttons 3 and 4 are the mouse's back and forward. The press is swallowed
// too, so the window never acts on it itself.
const mouseDirection = event => ({ 3: "back", 4: "forward" })[event.button];
document.addEventListener("mousedown", event => {
    if (mouseDirection(event)) event.preventDefault();
});
document.addEventListener("mouseup", event => {
    const direction = mouseDirection(event);
    if (!direction) return;
    event.preventDefault();
    window.__oxHistoryReport(direction);
});
