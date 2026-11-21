// Keyboard travel between the elements of a chart (docs/ui-common.md §5).
//
// A chart's cards and segments are one stop of the Tab order: the selected
// person's element. From it the arrow keys move the focus to the nearest
// element drawn in that direction, and Enter or Space — handled by Rust, as
// a click is — makes the focused person the chart's focus.
//
// Evaluated before each call; the object is built once per page.
window.oxidgeneChartKeys ??= (() => {
    const ITEM = "[data-chart-item]";
    const DIRECTIONS = {
        ArrowLeft: [-1, 0],
        ArrowRight: [1, 0],
        ArrowUp: [0, -1],
        ArrowDown: [0, 1],
    };
    // How long a chart redrawn by an activation is watched for a lost focus.
    const WATCH_TICKS = 30;
    const WATCH_INTERVAL_MS = 100;

    const centre = rect => [rect.left + rect.width / 2, rect.top + rect.height / 2];
    const intersects = (a, b) =>
        a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
    const focus = element => element.focus({ preventScroll: true });

    // The index of the rectangle nearest to `origin` in a direction, or -1.
    // Only rectangles whose centre lies further that way count; a sideways
    // offset costs twice the same distance ahead, and a rectangle within
    // the cone opening that way is preferred to any outside it.
    function nearest(origin, rects, [dx, dy]) {
        const [ox, oy] = centre(origin);
        let best = -1;
        let bestScore = Infinity;
        rects.forEach((rect, index) => {
            const [cx, cy] = centre(rect);
            const ahead = (cx - ox) * dx + (cy - oy) * dy;
            const aside = Math.abs((cx - ox) * dy) + Math.abs((cy - oy) * dx);
            if (ahead < 1) return;
            const outside = aside > ahead * 2 ? 1e6 : 0;
            const score = ahead + aside * 2 + outside;
            if (score < bestScore) {
                best = index;
                bestScore = score;
            }
        });
        return best;
    }

    // The watch an activation started on each viewport, by viewport.
    const watches = new Map();

    function stopWatch(viewport) {
        clearInterval(watches.get(viewport));
        watches.delete(viewport);
    }

    // After an activation the chart is drawn again around another person.
    // Its elements are reused by position, so the one that has the focus
    // may then draw somebody else, or be gone: the focus follows the
    // selected person to their new element rather than staying on another
    // person or going back to the top of the page. The watch ends when the
    // reader moves the focus themselves, by a key in the chart or to
    // another part of the page.
    function keepFocus(viewport) {
        stopWatch(viewport);
        let ticks = WATCH_TICKS;
        const timer = setInterval(() => {
            const active = document.activeElement;
            const lost = !active || active === document.body || !active.isConnected;
            const elsewhere = !lost && !viewport.contains(active);
            ticks -= 1;
            if (!viewport.isConnected || elsewhere || ticks < 0) {
                stopWatch(viewport);
                return;
            }
            const selected = viewport.querySelector(`${ITEM}[tabindex="0"]`);
            if (lost) {
                focus(selected ?? viewport);
            } else if (selected && active !== selected && active.matches?.(ITEM)) {
                focus(selected);
            }
        }, WATCH_INTERVAL_MS);
        watches.set(viewport, timer);
    }

    function onKey(viewport, event) {
        if (event.altKey || event.ctrlKey || event.metaKey) return;
        const current = event.target.closest?.(ITEM) ?? null;
        const direction = DIRECTIONS[event.key];
        if (!direction) {
            if (current && (event.key === "Enter" || event.key === " ")) keepFocus(viewport);
            return;
        }
        // The reader travels on: the focus is theirs again.
        stopWatch(viewport);
        // An arrow pressed in a control of the chart keeps its own meaning.
        if (!current && event.target !== viewport) return;
        let next;
        if (current) {
            const bounds = viewport.getBoundingClientRect();
            const others = [...viewport.querySelectorAll(ITEM)].filter(item =>
                item !== current && intersects(item.getBoundingClientRect(), bounds));
            const rects = others.map(item => item.getBoundingClientRect());
            next = others[nearest(current.getBoundingClientRect(), rects, direction)];
        } else {
            next = viewport.querySelector(`${ITEM}[tabindex="0"]`) ?? viewport.querySelector(ITEM);
        }
        if (next) {
            event.preventDefault();
            focus(next);
        }
    }

    return {
        nearest,
        attach(id) {
            const viewport = document.getElementById(id);
            if (!viewport || viewport.dataset.chartKeys) return;
            viewport.dataset.chartKeys = "1";
            viewport.addEventListener("keydown", event => onKey(viewport, event));
        },
    };
})();
