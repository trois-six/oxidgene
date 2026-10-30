// The home page's tour: one screenshot at a time, with previous and next
// buttons, a dot per screenshot, the arrow keys, a swipe on a touch screen,
// and a rotation that pauses under the pointer or the keyboard focus, stops
// once the reader picks a screenshot, and starts off when the system asks
// for reduced motion. The address names the screenshot shown, `#tour-N`
// counting from 1, so a link can name one: the page opens at it,
// and the hash follows the reader without adding to the history. The markup
// is the template's (templates/index.html): without this script its first
// screenshot shows alone.
(function () {
    "use strict";
    const tour = document.querySelector(".tour");
    if (!tour) return;
    const slides = [...tour.querySelectorAll(".tour-slide")];
    const dots = [...tour.querySelectorAll(".tour-dots button")];
    const track = tour.querySelector(".tour-slides");
    const play = tour.querySelector(".tour-play");
    const count = tour.querySelector(".tour-count");
    const reduced = matchMedia("(prefers-reduced-motion: reduce)");
    const DELAY = 5000;
    const SWIPE = 40;
    const HASH = /^#tour(?:-(\d+))?$/;
    let current = 0;
    let timer = 0;
    // Whether the tour rotates when nothing pauses it: the reader's choice.
    let rotating = !reduced.matches;
    let hovered = false;
    let focused = false;
    let visible = false;

    // The images are lazy and hidden ones never load: the neighbours of the
    // one shown load ahead, so the next step shows at once.
    function preload(index) {
        const image = slides[(index + slides.length) % slides.length].querySelector("img");
        image.loading = "eager";
    }

    // The screenshot a `#tour` or `#tour-N` address names, as an index;
    // null for any other address.
    function named() {
        const match = HASH.exec(location.hash);
        if (!match) return null;
        const number = Number(match[1] || 1);
        return number >= 1 && number <= slides.length ? number - 1 : 0;
    }

    function record() {
        history.replaceState(history.state, "", `#tour-${current + 1}`);
    }

    function show(index) {
        const next = (index + slides.length) % slides.length;
        if (next === current) return;
        slides[current].hidden = true;
        slides[current].classList.remove("shown");
        dots[current].removeAttribute("aria-current");
        current = next;
        slides[current].hidden = false;
        slides[current].classList.add("shown");
        dots[current].setAttribute("aria-current", "true");
        count.textContent = `${current + 1} / ${slides.length}`;
        preload(current + 1);
        preload(current - 1);
    }

    function schedule() {
        clearTimeout(timer);
        tour.classList.toggle("stopped", !rotating);
        play.setAttribute("aria-label", rotating ? tour.dataset.pause : tour.dataset.play);
        // A screen reader announces a screenshot the reader brings, not
        // every step of the rotation.
        track.setAttribute("aria-live", rotating ? "off" : "polite");
        if (rotating && visible && !hovered && !focused && !document.hidden) {
            timer = setTimeout(() => {
                show(current + 1);
                // An address that names a screenshot keeps naming the one
                // shown; the rotation writes none otherwise.
                if (named() !== null) record();
                schedule();
            }, DELAY);
        }
    }

    // A screenshot the reader picks stops the rotation.
    function go(index) {
        rotating = false;
        show(index);
        record();
        schedule();
    }

    // Opened at `#tour-N`, or sent there by a link of the page: that
    // screenshot, the tour scrolled into view, as a pick of the reader.
    // `#tour` alone is the first, rotating, where the browser scrolls.
    function follow() {
        const index = named();
        if (index === null) return;
        if (/^#tour-/.test(location.hash)) {
            rotating = false;
            show(index);
            tour.scrollIntoView({ block: "start" });
        } else {
            show(index);
        }
        schedule();
    }
    addEventListener("hashchange", follow);

    play.addEventListener("click", () => {
        rotating = !rotating;
        schedule();
    });
    tour.querySelector(".tour-prev").addEventListener("click", () => go(current - 1));
    tour.querySelector(".tour-next").addEventListener("click", () => go(current + 1));
    dots.forEach((dot, index) => dot.addEventListener("click", () => go(index)));

    tour.addEventListener("keydown", (event) => {
        const target = { ArrowLeft: current - 1, ArrowRight: current + 1, Home: 0, End: slides.length - 1 }[event.key];
        if (target === undefined || event.altKey || event.ctrlKey || event.metaKey) return;
        event.preventDefault();
        go(target);
    });

    // A horizontal swipe; the page still scrolls vertically (touch-action).
    let start = null;
    track.addEventListener("pointerdown", (event) => {
        start = event.pointerType === "mouse" ? null : { x: event.clientX, y: event.clientY };
    });
    track.addEventListener("pointerup", (event) => {
        if (!start) return;
        const dx = event.clientX - start.x;
        const dy = event.clientY - start.y;
        start = null;
        if (Math.abs(dx) > SWIPE && Math.abs(dx) > Math.abs(dy)) go(dx < 0 ? current + 1 : current - 1);
    });
    track.addEventListener("pointercancel", () => {
        start = null;
    });

    // Only a mouse hovers: a tap would otherwise pause until the next one
    // elsewhere.
    tour.addEventListener("pointerenter", (event) => {
        hovered = event.pointerType === "mouse";
        schedule();
    });
    tour.addEventListener("pointerleave", () => {
        hovered = false;
        schedule();
    });
    // Keyboard focus pauses, except on the play button, which would
    // otherwise never start what it starts. A click focuses without
    // showing it; the pointer over the tour pauses then.
    tour.addEventListener("focusin", (event) => {
        focused = event.target !== play && event.target.matches(":focus-visible");
        schedule();
    });
    tour.addEventListener("focusout", () => {
        focused = false;
        schedule();
    });
    document.addEventListener("visibilitychange", schedule);
    reduced.addEventListener("change", () => {
        if (reduced.matches) rotating = false;
        schedule();
    });

    // Rotate only while the tour is on screen, and load the next
    // screenshot only once it is.
    new IntersectionObserver((entries) => {
        visible = entries[entries.length - 1].isIntersecting;
        if (visible) {
            preload(current + 1);
            preload(current - 1);
        }
        schedule();
    }, { threshold: 0.3 }).observe(tour);
    follow();
    schedule();
})();
