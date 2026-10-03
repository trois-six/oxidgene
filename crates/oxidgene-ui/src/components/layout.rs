//! Application shell and layout.
//!
//! [`AppShell`] holds the state and styles shared by the whole application
//! above the router; [`Layout`] wraps all routed pages with a consistent
//! header/nav and renders the active route via [`Outlet`].

use dioxus::prelude::*;
use std::sync::LazyLock;

use crate::components::tree_cache;
use crate::i18n;
use crate::router::Route;

/// The logo (a 64×64 resize), embedded as the PNG itself.
const LOGO_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/brand/logo_64.png"
));

/// The logo as a data URL, encoded the first time a page draws it.
pub fn logo_data_url() -> &'static str {
    static URL: LazyLock<String> = LazyLock::new(|| crate::utils::png_data_url(LOGO_PNG));
    &URL
}

/// Stop a resized `<textarea>` from stranding its own text.
///
/// A textarea keeps the scroll offset it had while the user drags its grip
/// taller. Once the note is shorter than the new box there is nothing left to
/// scroll back with — no scrollbar, no wheel travel — so the offset can never
/// be undone and the first lines stay clipped above the top edge. That reads
/// as lost text. Re-clamping the offset on every size change puts it back.
///
/// The observer is attached the first time the user focuses a given textarea
/// rather than to all of them up front: the modals mount their fields lazily,
/// and re-scanning the DOM on every Dioxus mutation would cost far more than
/// the handful of fields anyone actually edits.
pub fn use_init_textarea_resize_clamp() {
    use_effect(move || {
        document::eval(
            r#"
            if (!window.__oxTextareaClamp) {
                window.__oxTextareaClamp = true;
                document.addEventListener('focusin', function (e) {
                    var t = e.target;
                    if (!t || t.tagName !== 'TEXTAREA' || t.dataset.oxClamp) return;
                    t.dataset.oxClamp = '1';
                    try {
                        new ResizeObserver(function () {
                            var max = Math.max(0, t.scrollHeight - t.clientHeight);
                            if (t.scrollTop > max) t.scrollTop = max;
                        }).observe(t);
                    } catch (err) {}
                });
            }
            "#,
        );
    });
}

/// Everything the whole application shares, rendered once above the router.
///
/// The router draws each route's layout from that route's own template, so
/// [`Layout`] is torn down and mounted again on every navigation between
/// routes. State provided there started over each time: the language fell
/// back to English until it was detected again, the theme to the default
/// one, and the tree and pedigree view caches emptied, so the pedigree came
/// back on the tree's root. Held here, it lives as long as the window, and
/// the stylesheet is parsed once instead of on every navigation.
#[component]
pub fn AppShell() -> Element {
    let _lang_signal = i18n::use_init_language();
    let _sort_particles = crate::prefs::use_init_sort_particles();
    let _pedigree_defaults = crate::prefs::use_init_pedigree_defaults();
    let _pedigree_theme = crate::prefs::use_init_pedigree_theme();
    let _pedigree_view = crate::prefs::use_init_pedigree_view();
    let theme = crate::theme::use_init_theme();
    use_init_textarea_resize_clamp();
    let _tree_cache = tree_cache::use_init_tree_cache();
    let _view_cache = tree_cache::use_init_view_state_cache();
    let _current_person = tree_cache::use_init_current_person();

    // The palette is a block of custom properties, so it can be recomputed
    // and swapped on its own: the stylesheet below never changes, and
    // switching theme repaints without reparsing five thousand rules.
    let palette = use_memo(move || theme.read().active().css());
    // Paper gets the light palette whatever the screen shows; it follows the
    // active one so that it overrides it for print media only.
    let print_palette = use_hook(crate::components::print::print_palette_css);
    crate::components::print::use_init_print();

    rsx! {
        style { {palette()} }
        style { {print_palette} }
        style { {FONT_FACES.as_str()} }
        style { {LAYOUT_STYLES} }
        Router::<Route> {}
    }
}

/// Shared layout rendered around every page.
///
/// Contains a navigation bar (shown only on Home / AppSettings) and an
/// [`Outlet`] for the matched child route. It is remounted on every
/// navigation between routes, so it holds no state: see [`AppShell`].
#[component]
pub fn Layout() -> Element {
    let route = use_route::<Route>();
    let show_nav = matches!(route, Route::Home {} | Route::AppSettings {});

    rsx! {
        if show_nav {
            nav { class: "app-nav",
                Link { to: Route::Home {}, class: "nav-logo",
                    img {
                        src: logo_data_url(),
                        alt: "OxidGene",
                        class: "nav-logo-img",
                    }
                }
            }
        }

        main { class: "app-main",
            Outlet::<Route> {}
        }
    }
}

/// The `@font-face` rules of the application's two typefaces, Cinzel and Lato.
///
/// The fonts are bundled rather than fetched from a font service, so neither
/// the web build nor the desktop makes a request to a third party when it
/// opens. Each file is inlined as a `data:` URL: the stylesheet needs no
/// address of its own on either build, and the browser downloads a subset
/// only when a page uses a character in its range. Both typefaces are under
/// the SIL Open Font License 1.1, whose text ships beside them in
/// `assets/fonts/` at the repository root; the files are the Latin and Latin Extended subsets Google
/// Fonts serves, which cover every interface language.
pub static FONT_FACES: LazyLock<String> = LazyLock::new(|| {
    use base64::Engine as _;

    /// Code points of the Latin subset.
    const LATIN: &str = "U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, \
        U+02DC, U+0304, U+0308, U+0329, U+2000-206F, U+20AC, U+2122, U+2191, U+2193, U+2212, \
        U+2215, U+FEFF, U+FFFD";
    /// Code points of the Latin Extended subset.
    const LATIN_EXT: &str = "U+0100-02BA, U+02BD-02C5, U+02C7-02CC, U+02CE-02D7, U+02DD-02FF, \
        U+0304, U+0308, U+0329, U+1D00-1DBF, U+1E00-1E9F, U+1EF2-1EFF, U+2020, U+20A0-20AB, \
        U+20AD-20C0, U+2113, U+2C60-2C7F, U+A720-A7FF";
    /// (family, weight, unicode range, WOFF2 file). Cinzel is a variable
    /// font: one file per subset spans every weight the stylesheet uses.
    const FACES: [(&str, &str, &str, &[u8]); 8] = [
        (
            "Cinzel",
            "400 700",
            LATIN,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/fonts/cinzel-latin.woff2"
            )),
        ),
        (
            "Cinzel",
            "400 700",
            LATIN_EXT,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/fonts/cinzel-latin-ext.woff2"
            )),
        ),
        (
            "Lato",
            "300",
            LATIN,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/fonts/lato-300-latin.woff2"
            )),
        ),
        (
            "Lato",
            "300",
            LATIN_EXT,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/fonts/lato-300-latin-ext.woff2"
            )),
        ),
        (
            "Lato",
            "400",
            LATIN,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/fonts/lato-400-latin.woff2"
            )),
        ),
        (
            "Lato",
            "400",
            LATIN_EXT,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/fonts/lato-400-latin-ext.woff2"
            )),
        ),
        (
            "Lato",
            "700",
            LATIN,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/fonts/lato-700-latin.woff2"
            )),
        ),
        (
            "Lato",
            "700",
            LATIN_EXT,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/fonts/lato-700-latin-ext.woff2"
            )),
        ),
    ];
    FACES
        .iter()
        .map(|(family, weight, range, woff2)| {
            format!(
                "@font-face{{font-family:'{family}';font-style:normal;font-weight:{weight};\
                 font-display:swap;src:url(data:font/woff2;base64,{}) format('woff2');\
                 unicode-range:{range};}}\n",
                base64::engine::general_purpose::STANDARD.encode(woff2),
            )
        })
        .collect()
});

/// CSS for the layout shell.
pub const LAYOUT_STYLES: &str = r#"
    /* The theme, which `AppShell` emits as its own `:root` block ahead of
       this stylesheet (see `crate::theme`), sets every colour, the
       typefaces (`--font-sans`, `--font-heading`), the radius scale
       (`--radius-xs` to `--radius-lg`), the shadow scale (`--shadow-sm` to
       `--shadow-lg`) and the density (`--space-unit`, `--text-scale`). What
       follows derives the scales the rules use from them, and the geometry
       a theme does not change. Rules use these tokens rather than literal
       sizes, radii, shadows or families. */
    :root {
        /* ── Component dimensions ──────────────────────────────────── */
        --sb:   46px;   /* icon sidebar width */
        --evw:  275px;  /* event panel width */

        /* ── Semantic aliases (used by shared components) ─────────── */
        --white:              var(--on-accent);
        --shadow-black:       var(--shadow);
        --nav-bg:     color-mix(in srgb, var(--nav-surface) 92%, transparent);

        --radius-pill: 999px;
        --font-mono: ui-monospace, "SFMono-Regular", "Cascadia Code", Menlo, Consolas, monospace;

        /* ── Spacing: --space-N is N steps of the theme's unit, 2px at
           regular density, so --space-6 is 12px. ──────────────────── */
        --space-1:  var(--space-unit);
        --space-2:  calc(2 * var(--space-unit));
        --space-3:  calc(3 * var(--space-unit));
        --space-4:  calc(4 * var(--space-unit));
        --space-5:  calc(5 * var(--space-unit));
        --space-6:  calc(6 * var(--space-unit));
        --space-7:  calc(7 * var(--space-unit));
        --space-8:  calc(8 * var(--space-unit));
        --space-9:  calc(9 * var(--space-unit));
        --space-10: calc(10 * var(--space-unit));
        --space-11: calc(11 * var(--space-unit));
        --space-12: calc(12 * var(--space-unit));
        --space-13: calc(13 * var(--space-unit));
        --space-14: calc(14 * var(--space-unit));
        --space-15: calc(15 * var(--space-unit));
        --space-16: calc(16 * var(--space-unit));
        --space-17: calc(17 * var(--space-unit));
        --space-18: calc(18 * var(--space-unit));
        --space-20: calc(20 * var(--space-unit));
        --space-23: calc(23 * var(--space-unit));
        --space-24: calc(24 * var(--space-unit));
        --space-26: calc(26 * var(--space-unit));
        --space-32: calc(32 * var(--space-unit));
        --space-40: calc(40 * var(--space-unit));

        /* ── Type scale: --text-N is N hundredths of a rem at regular
           density, scaled by the theme. ───────────────────────────── */
        --text-65:  calc(0.65rem * var(--text-scale));
        --text-70:  calc(0.7rem * var(--text-scale));
        --text-75:  calc(0.75rem * var(--text-scale));
        --text-80:  calc(0.8rem * var(--text-scale));
        --text-85:  calc(0.85rem * var(--text-scale));
        --text-90:  calc(0.9rem * var(--text-scale));
        --text-95:  calc(0.95rem * var(--text-scale));
        --text-100: calc(1rem * var(--text-scale));
        --text-110: calc(1.1rem * var(--text-scale));
        --text-120: calc(1.2rem * var(--text-scale));
        --text-130: calc(1.3rem * var(--text-scale));
        --text-150: calc(1.5rem * var(--text-scale));
        --text-200: calc(2rem * var(--text-scale));
        --text-300: calc(3rem * var(--text-scale));
    }

    html { height: 100%; }

    *, *::before, *::after {
        box-sizing: border-box;
        margin: 0;
        padding: 0;
    }

    body {
        height: 100%;
        display: flex;
        flex-direction: column;
        font-family: var(--font-sans);
        background: var(--bg-deep);
        color: var(--text-primary);
        line-height: 1.6;
        overflow-x: hidden;
    }

    /* Subtle radial light leaks on the page background. The rule is
       unconditional and the two colours carry their own alpha, so a theme
       that wants no leaks — as the light one does — sets them fully
       transparent rather than needing a selector of its own. */
    body::before {
        content: '';
        position: fixed;
        inset: 0;
        background:
            radial-gradient(ellipse at 20% 50%, var(--page-glow-warm) 0%, transparent 60%),
            radial-gradient(ellipse at 80% 20%, var(--page-glow-cool) 0%, transparent 50%);
        pointer-events: none;
        z-index: 0;
    }

    /* Dioxus desktop mounts into <div id="main"> */
    #main {
        flex: 1;
        min-height: 0;
        display: flex;
        flex-direction: column;
    }

    /* ── Navigation bar ─────────────────────────────────────────── */

    .app-nav {
        display: flex;
        align-items: center;
        justify-content: space-between;
        background: var(--nav-bg);
        backdrop-filter: blur(12px);
        -webkit-backdrop-filter: blur(12px);
        color: var(--text-primary);
        padding: 0 var(--space-20);
        height: 64px;
        border-bottom: 1px solid var(--border);
        box-shadow: var(--shadow-md);
        position: sticky;
        top: 0;
        z-index: 100;
    }

    .nav-logo {
        display: flex;
        align-items: center;
        text-decoration: none;
        gap: var(--space-4);
    }

    .nav-logo-img {
        height: 36px;
        width: auto;
    }

    /* ── Page layout containers ──────────────────────────────────── */

    /* Full-height flex host for all page content */
    .app-main {
        flex: 1;
        min-height: 0;
        display: flex;
        flex-direction: column;
        overflow: hidden;
        position: relative;
        z-index: 1;
    }

    /* Sub-page: full-height flex container with topbar + scrollable content */
    .sub-page {
        flex: 1;
        min-height: 0;
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }
    .sub-page-content {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
        padding: var(--space-12);
        max-width: 1200px;
        width: 100%;
        margin: 0 auto;
    }

    /* Native scrollbars are composited above fixed descendants in WebViews. */
    .sub-page-content:has(.cropper-backdrop) { overflow: hidden; }

    /* Tree-detail page: fills app-main, stacks header + pedigree vertically */
    .tree-detail-page {
        flex: 1;
        min-height: 0;
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    /* Pedigree card: grows to fill remaining height inside tree-detail-page */
    .pedigree-card {
        flex: 1;
        min-height: 0;
        display: flex;
        flex-direction: column;
        overflow: hidden;
        padding: 0;
    }

    /* ── Shared utility classes ──────────────────────────────────── */

    .card {
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: var(--space-10);
        box-shadow: var(--shadow-sm);
    }

    .btn {
        display: inline-flex;
        align-items: center;
        gap: var(--space-3);
        padding: var(--space-4) var(--space-8);
        border: none;
        border-radius: var(--radius);
        font-size: var(--text-90);
        font-weight: 500;
        cursor: pointer;
        transition: background 0.15s, box-shadow 0.15s, opacity 0.15s;
        font-family: var(--font-sans);
    }

    .btn-primary {
        background: linear-gradient(135deg, var(--orange), var(--orange-light));
        color: var(--on-accent);
        box-shadow: 0 2px 8px color-mix(in srgb, var(--orange) 30%, transparent);
    }

    .btn-primary:hover {
        opacity: 0.9;
        box-shadow: 0 4px 16px color-mix(in srgb, var(--orange) 40%, transparent);
    }

    .btn-danger {
        background: var(--danger);
        color: var(--on-accent);
    }

    .btn-danger:hover {
        opacity: 0.9;
    }

    .btn-outline {
        background: transparent;
        border: 1px solid var(--border);
        color: var(--text-secondary);
    }

    .btn-outline:hover {
        background: var(--bg-card-hover);
        color: var(--text-primary);
        border-color: var(--text-secondary);
    }

    .page-header {
        display: flex;
        align-items: stretch;
        justify-content: space-between;
        gap: var(--space-9);
        margin-bottom: var(--space-12);
    }

    .page-header h1 {
        font-size: var(--text-150);
        font-weight: 600;
        font-family: var(--font-heading);
        color: var(--text-primary);
    }

    .pd-avatar {
        flex: none;
        width: 76px;
        height: 76px;
        border-radius: 50%;
        object-fit: cover;
        border: 1px solid var(--border);
    }

    .pd-header-left {
        display: flex;
        gap: var(--space-9);
        align-items: flex-start;
        min-width: 0;
        flex: 1;
    }

    .pd-header-main {
        flex: 1;
        min-width: 0;
    }

    .pd-header-top {
        display: flex;
        align-items: baseline;
        justify-content: space-between;
        gap: var(--space-6);
    }

    .pd-header-actions {
        display: flex;
        flex-direction: column;
        align-items: flex-end;
        justify-content: space-between;
        gap: var(--space-6);
        flex-shrink: 0;
    }

    .pd-header-sosa {
        min-height: 24px;
        display: flex;
        justify-content: flex-end;
        gap: var(--space-3);
    }

    /* Icon buttons at every width: the tooltip and accessible name carry
       the label, so the actions stay compact at the right of the header. */
    .pd-header-buttons {
        display: flex;
        gap: var(--space-3);
        justify-content: flex-end;
    }

    .btn.pd-header-action-btn {
        width: 34px;
        height: 34px;
        justify-content: center;
        padding: 0;
    }

    .badge.pd-sosa-badge {
        background: var(--green);
        color: var(--on-accent);
        border-color: var(--green);
        font-size: var(--text-80);
    }

    .badge.pd-self-badge {
        background: var(--pn-self);
        color: var(--on-accent);
        border-color: var(--pn-self);
        cursor: pointer;
        font-size: var(--text-80);
        font-family: var(--font-sans);
    }
    .badge.pd-self-badge:hover {
        filter: brightness(1.1);
    }

    .pd-sex-mark {
        color: var(--orange);
        font-weight: 600;
        margin-right: var(--space-2);
    }

    .pd-vitals b {
        color: var(--text-primary);
        font-weight: 600;
    }

    .text-muted {
        color: var(--text-secondary);
    }

    .loading {
        text-align: center;
        padding: var(--space-24);
        color: var(--text-secondary);
    }

    .error-msg {
        background: color-mix(in srgb, var(--danger) 12%, transparent);
        border: 1px solid color-mix(in srgb, var(--danger) 40%, transparent);
        color: var(--danger-text);
        padding: var(--space-6) var(--space-8);
        border-radius: var(--radius);
        margin-bottom: var(--space-8);
    }

    .success-msg {
        background: color-mix(in srgb, var(--green-accent) 10%, transparent);
        border: 1px solid color-mix(in srgb, var(--green-accent) 35%, transparent);
        color: var(--green-light);
        padding: var(--space-6) var(--space-8);
        border-radius: var(--radius);
        margin-bottom: var(--space-8);
    }

    .warning-msg {
        background: color-mix(in srgb, var(--orange) 10%, transparent);
        border: 1px solid color-mix(in srgb, var(--orange) 35%, transparent);
        color: var(--orange-light);
        padding: var(--space-6) var(--space-8);
        border-radius: var(--radius);
        margin-bottom: var(--space-8);
    }

    /* Text fields, and only those. A bare `input` selector here reaches the
       checkbox and radio too, and full width plus padding, a border and a
       panel background turn one into a large empty box with the label it
       belongs to squeezed beside it — a fault every such control then had to
       undo for itself, and rediscover when it forgot to. */
    input:not([type="checkbox"]):not([type="radio"]), select, textarea {
        font-family: var(--font-sans);
        font-size: var(--text-90);
        padding: var(--space-4) var(--space-6);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        width: 100%;
        transition: border-color 0.15s, box-shadow 0.15s;
        background: var(--bg-panel);
        color: var(--text-primary);
    }

    /* What they are instead, in one place. Sized rather than left to the
       engine's default so a row can reserve a column for one. */
    input[type="checkbox"],
    input[type="radio"] {
        width: 16px;
        height: 16px;
        margin: 0;
        accent-color: var(--orange);
        cursor: pointer;
    }

    input::placeholder,
    textarea::placeholder {
        color: var(--text-muted);
    }

    input:not([type="checkbox"]):not([type="radio"]):focus, select:focus, textarea:focus {
        outline: none;
        border-color: var(--orange);
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--orange) 15%, transparent);
    }

    select option {
        background: var(--bg-panel);
        color: var(--text-primary);
    }

    label {
        display: block;
        font-size: var(--text-80);
        font-weight: 500;
        margin-bottom: var(--space-2);
        color: var(--text-secondary);
    }

    .form-group {
        margin-bottom: var(--space-8);
    }

    .form-row {
        display: flex;
        gap: var(--space-8);
        flex-wrap: wrap;
    }

    .form-row .form-group {
        flex: 1;
        min-width: 140px;
    }

    /* Secondary text under an input: what the app derived from what was
       typed, subordinate to the field itself. */
    .field-hint {
        display: block;
        margin-top: var(--space-2);
        font-size: var(--text-80);
        color: var(--text-secondary);
    }

    /* Surname particle: the detected split plus the affordance to correct it.
       Wraps rather than overflowing, since the summary text is translated and
       its length varies. */
    .particle-row {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: var(--space-4);
        margin-top: var(--space-2);
    }

    .particle-row .field-hint {
        margin-top: 0;
    }

    /* A particle that could not be applied: informational, not an error —
       nothing was lost, the cut simply did not happen. */
    .field-hint-warn {
        color: var(--orange);
    }

    .particle-label {
        font-size: var(--text-80);
        color: var(--text-secondary);
    }

    .particle-input {
        width: 8rem;
        flex: 0 0 auto;
        padding: var(--space-2) var(--space-4);
        font-size: var(--text-85);
    }

    .particle-btn {
        padding: var(--space-1) var(--space-4);
        font-size: var(--text-80);
        line-height: 1.6;
        color: var(--orange);
        background: none;
        border: 1px solid currentColor;
        border-radius: var(--radius);
        cursor: pointer;
    }

    .particle-btn:hover {
        background: color-mix(in srgb, var(--orange) 12%, transparent);
    }

    /* ── Note bodies ──────────────────────────────────────────────
       Notes render the sanitized HTML they were imported with (see
       oxidgene_db::html). The author of that markup is a GEDCOM or .gw file,
       not this app, so it gets bounded here: anything wide scrolls inside its
       own box rather than stretching the page. */

    .note-html {
        overflow-wrap: anywhere;
    }

    .note-html > *:first-child { margin-top: 0; }
    .note-html > *:last-child  { margin-bottom: 0; }

    .note-html p,
    .note-html ul,
    .note-html ol,
    .note-html blockquote,
    .note-html table {
        margin: 0 0 0.6em;
    }

    .note-html ul, .note-html ol { padding-left: 1.4em; }

    .note-html h1, .note-html h2, .note-html h3,
    .note-html h4, .note-html h5, .note-html h6 {
        font-size: 1em;
        font-weight: 600;
        margin: 0.8em 0 0.3em;
    }

    .note-html a {
        color: var(--orange);
        text-decoration: underline;
    }

    .note-html img {
        max-width: 100%;
        height: auto;
    }

    .note-html blockquote {
        border-left: 3px solid var(--border);
        padding-left: 0.8em;
        color: var(--text-secondary);
    }

    .note-html pre {
        overflow-x: auto;
        white-space: pre-wrap;
    }

    .note-html table {
        border-collapse: collapse;
        display: block;
        overflow-x: auto;
        max-width: 100%;
    }

    .note-html td, .note-html th {
        border: 1px solid var(--border);
        padding: var(--space-2) var(--space-4);
    }

    /* The one person picker (components/person_picker.rs): the chosen
       person's search row and its buttons, in a frame of their own. */
    .person-picker-display,
    .person-picker-empty {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--space-6);
        padding: var(--space-5) var(--space-6);
        background: var(--bg-deep);
        border: 1px solid var(--border);
        border-radius: var(--radius);
    }
    .person-picker-empty { border-style: dashed; }
    .person-picker-empty p { margin: 0; }
    .person-picker-person {
        display: flex;
        align-items: center;
        gap: var(--space-5);
        min-width: 0;
        flex: 1;
    }
    /* A row passed in as a link keeps none of a result list's frame. */
    .person-picker-person .search-person-result {
        border: none;
        padding: 0;
        background: none;
    }
    .person-picker-actions {
        display: flex;
        gap: var(--space-3);
        flex-shrink: 0;
    }
    .btn-danger-outline {
        color: var(--red) !important;
        border-color: var(--red) !important;
    }
    .btn-danger-outline:hover {
        background: color-mix(in srgb, var(--red) 10%, transparent) !important;
    }
    @media (max-width: 768px) {
        .person-picker-display {
            flex-direction: column;
            align-items: stretch;
        }
        .person-picker-person { align-items: flex-start; }
        .person-picker-person .sp-result-name {
            white-space: normal;
            overflow: visible;
            text-overflow: clip;
            overflow-wrap: anywhere;
        }
        .person-picker-person .sp-result-meta { overflow-wrap: anywhere; }
        .person-picker-actions { justify-content: flex-end; }
    }

    /* The one empty state (components/empty_state.rs). */
    .empty-state {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: var(--space-4);
        text-align: center;
        padding: var(--space-24) var(--space-12);
        color: var(--text-secondary);
    }

    .empty-state h3 {
        font-weight: 500;
        color: var(--text-primary);
    }

    .empty-state p { margin: 0; }

    .empty-state-icon {
        font-size: 3.5rem;
        line-height: 1;
    }

    .empty-state-action { margin-top: var(--space-2); }

    .empty-tree-container {
        display: flex;
        align-items: center;
        justify-content: center;
        flex: 1;
        min-height: 400px;
    }

    .empty-tree-slot {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: var(--space-6);
        width: 160px;
        height: 160px;
        border: 2px dashed var(--border);
        border-radius: var(--radius-lg);
        background: transparent;
        color: var(--text-muted);
        font-size: var(--text-85);
        font-family: var(--font-sans);
        cursor: pointer;
        transition: color 0.2s, border-color 0.2s;
    }

    .empty-tree-slot:hover {
        color: var(--orange);
        border-color: var(--orange);
    }

    .badge {
        display: inline-block;
        padding: var(--space-1) var(--space-4);
        font-size: var(--text-75);
        font-weight: 500;
        border-radius: var(--radius-lg);
        background: var(--bg-panel);
        color: var(--text-secondary);
        border: 1px solid var(--border);
    }

    /* ── Section header ─────────────────────────────────────────── */

    .section-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: var(--space-8);
    }

    .btn-sm {
        padding: var(--space-2) var(--space-5);
        font-size: var(--text-80);
    }

    /* The title of a card section, and the same with room below it when
       no .section-header row sets the spacing. */
    .section-title { font-size: var(--text-110); }
    .section-title-spaced { margin-bottom: var(--space-6); }

    /* A row of controls set side by side. */
    .inline-row { display: flex; gap: var(--space-4); }

    .note-entry {
        margin-bottom: var(--space-6);
        padding: var(--space-6);
        border: 1px solid var(--border);
        border-radius: var(--radius);
    }

    .error-msg-inset { margin: 0 var(--space-8); }
    .search-person-empty { padding: var(--space-4); }
    .confirm-message { margin: var(--space-6) 0; }
    .not-found-action { margin-top: var(--space-8); }

    /* ── Person detail page shell ────────────────────────────────── */

    .pd-page-shell {
        flex: 1;
        min-height: 0;
        display: flex;
        overflow: hidden;
    }

    .tree-icon-sidebar {
        align-self: stretch;
    }

    .tree-icon-sidebar .isb-btn {
        text-decoration: none;
        flex-shrink: 0;
    }

    .pd-content {
        margin: 0 auto;
    }

    .pd-section { margin-bottom: var(--space-12); }
    .pd-section:last-child { margin-bottom: 0; }

    /* ── Couple page ─────────────────────────────────────────────── */

    .sub-page-content.cp-content { max-width: 1600px; }

    /* The couple's actions, on their own borderless row above the bar. */
    .cp-actions {
        display: flex;
        justify-content: flex-end;
        gap: var(--space-4);
        margin-bottom: var(--space-6);
    }

    /* One selector per spouse, split like the columns below, with the
       ring centred in the gutter between them. */
    .cp-bar {
        position: relative;
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: var(--space-12);
        margin-bottom: var(--space-12);
    }
    .cp-select { min-width: 0; }
    .cp-ring {
        position: absolute;
        top: 50%;
        left: 50%;
        transform: translate(-50%, -50%);
        color: var(--orange);
        font-size: var(--text-120);
        line-height: 1;
    }

    /* Two spouse columns. A section shared by the couple spans both; each
       row pairs the same section of the two spouses, so their cards line
       up and stretch to the taller one. */
    .cp-grid {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: var(--space-12);
    }
    .cp-span { grid-column: 1 / -1; }
    .cp-cell {
        display: flex;
        flex-direction: column;
        min-width: 0;
    }
    .cp-cell > .card,
    .cp-span > .card { margin-bottom: 0; }
    .cp-cell > .card { flex: 1; }
    .cp-cell:empty { display: none; }

    /* A header shares its column with its spouse's: actions go below the
       identity, as on a narrow screen. */
    .cp-grid .page-header { flex-direction: column; gap: var(--space-7); }
    .cp-grid .pd-header-actions {
        width: 100%;
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
    }
    .cp-grid .pd-header-sosa { min-height: 0; justify-content: flex-start; }
    .cp-unknown {
        align-items: center;
        justify-content: center;
    }

    .pd-media-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--space-6);
        margin-bottom: var(--space-6);
    }

    /* ── Family connections ──────────────────────────────────────── */

    .pd-fc-section {
        margin-bottom: var(--space-6);
    }
    .pd-fc-section:last-child { margin-bottom: 0; }

    .pd-fc-label {
        font-size: var(--text-70);
        font-weight: 700;
        color: var(--orange);
        text-transform: uppercase;
        letter-spacing: 0.5px;
        margin-bottom: var(--space-3);
    }

    /* ── Alternate names sub-line, under the header name ─────────── */

    .pd-alt-names {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-1) var(--space-5);
        font-size: var(--text-85);
        color: var(--text-secondary);
        margin: var(--space-2) 0 0;
    }

    .pd-vitals {
        font-size: var(--text-90);
        color: var(--text-secondary);
        margin: var(--space-3) 0 0;
    }

    /* ── Family narrative (parents / unions / siblings) ──────────── */

    .pd-family-prose {
        font-size: var(--text-95);
        margin-bottom: var(--space-7);
    }

    .pd-person-chip {
        display: inline-flex;
        align-items: center;
        gap: 3px;
        white-space: nowrap;
    }

    .pd-person-identity {
        display: inline-flex;
        align-items: center;
        gap: 3px;
        min-width: 0;
    }

    .pd-sosa-mark {
        flex: none;
    }

    .pd-sex-glyph {
        flex: none;
        font-size: 0.85em;
        color: var(--text-muted);
    }
    .pd-sex-glyph.male {
        color: var(--pn-male-line);
    }
    .pd-sex-glyph.female {
        color: var(--pn-female-line);
    }

    .pd-person-link {
        color: var(--text-primary);
        font-weight: 600;
        text-decoration: none;
        border-bottom: 1px solid var(--orange-light);
    }
    .pd-person-link:hover {
        color: var(--orange);
    }

    .pd-person-years {
        font-size: 0.85em;
        color: var(--text-muted);
    }

    .pd-union {
        margin-bottom: var(--space-7);
    }
    .pd-union:last-child {
        margin-bottom: 0;
    }
    .pd-union-line {
        font-size: var(--text-95);
    }

    .pd-children {
        list-style: none;
        margin: var(--space-3) 0 0;
        padding: 0 0 0 var(--space-2);
    }
    .pd-children li {
        font-size: var(--text-90);
        padding: 3px 0 3px var(--space-7);
        position: relative;
    }
    .pd-children li::before {
        content: '';
        position: absolute;
        left: 0;
        top: 12px;
        width: 6px;
        height: 6px;
        border-radius: 50%;
        background: var(--border);
    }

    .pd-sib-group {
        margin-bottom: var(--space-6);
    }
    .pd-sib-group:last-child {
        margin-bottom: 0;
    }
    .pd-sib-group-head {
        font-size: var(--text-85);
        color: var(--text-secondary);
        margin-bottom: var(--space-1);
    }

    /* ── Events timeline (replaces the events table) ──────────────── */

    .pd-timeline {
        list-style: none;
        margin: 0;
        padding: 0;
    }
    .pd-timeline li {
        display: flex;
        gap: var(--space-7);
        padding: 9px 0;
        border-top: 1px solid var(--border);
        font-size: var(--text-90);
        /* A documented ancestor can have dozens of rows, several carrying a
           full media gallery of their own — skip layout/paint for the ones
           currently scrolled out of view instead of keeping the whole
           timeline "hot". `auto` remembers each row's real size once it has
           been rendered, so the placeholder guess only matters the first
           time a row comes into view. */
        content-visibility: auto;
        contain-intrinsic-size: auto 44px;
    }
    .pd-timeline li:first-child {
        border-top: none;
        padding-top: var(--space-1);
    }
    /* Events directly on the individual or their conjugal family stand out
       from narrative-context events (children, parents, siblings). */
    .pd-timeline li.pd-ev-direct {
        background: color-mix(in srgb, var(--orange) 8%, transparent);
        margin: 0 -14px;
        padding-left: var(--space-7);
        padding-right: var(--space-7);
        border-radius: var(--radius-sm);
    }
    .pd-ev-date {
        flex: none;
        width: 108px;
        font-variant-numeric: tabular-nums;
        color: var(--text-secondary);
        font-size: var(--text-80);
        padding-top: 1px;
    }
    .pd-ev-body {
        flex: 1;
        min-width: 0;
    }
    .pd-ev-row {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: var(--space-5);
    }
    .pd-ev-origin {
        font-size: var(--text-75);
        color: var(--text-muted);
        font-style: italic;
    }
    .pd-ev-sources {
        font-size: var(--text-75);
        color: var(--text-muted);
        font-style: italic;
        margin-top: var(--space-1);
    }
    .pd-ev-source-link {
        padding: 0;
        border: 0;
        background: none;
        color: inherit;
        font: inherit;
        text-decoration: underline dotted;
        cursor: pointer;
    }
    .pd-ev-source-link:focus-visible {
        outline: 2px solid currentColor;
        outline-offset: 2px;
    }
    /* ── Modal / confirmation dialog ─────────────────────────────── */

    .modal-backdrop {
        position: fixed;
        inset: 0;
        background: color-mix(in srgb, var(--scrim) 65%, transparent);
        display: flex;
        align-items: center;
        justify-content: center;
        /* Above every other overlay (the media viewer's .cropper-backdrop
           included, at 1200) so a confirmation raised from within one is
           never stacked underneath it and left unclickable. */
        z-index: 1300;
        backdrop-filter: blur(4px);
    }

    /* The dialog takes the focus when it opens so Escape reaches it; the
       card itself is not a control and draws no ring. */
    .modal-backdrop > [role="dialog"]:focus { outline: none; }

    .modal-card {
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: var(--space-12);
        min-width: 360px;
        max-width: 480px;
        box-shadow: var(--shadow-lg);
    }

    .modal-card h3 {
        color: var(--text-primary);
        margin-bottom: var(--space-6);
    }

    .modal-card p {
        color: var(--text-secondary);
    }

    .modal-actions {
        display: flex;
        justify-content: flex-end;
        gap: var(--space-4);
        margin-top: var(--space-8);
    }

    /* ── Tree detail topbar ──────────────────────────────────────── */

    .td-topbar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--space-6);
        height: 48px;
        padding: 0 var(--space-6);
        background: var(--bg-panel);
        border-bottom: 1px solid var(--border);
        flex-shrink: 0;
        min-width: 0;
        overflow: hidden;
    }

    .td-bc {
        display: flex;
        align-items: center;
        gap: var(--space-3);
        font-size: var(--text-90);
        min-width: 0;
        flex: 1 1 auto;
        overflow: hidden;
        white-space: nowrap;
    }

    .td-bc a {
        color: var(--text-secondary);
        text-decoration: none;
        transition: color 0.15s;
        min-width: 0;
    }

    .td-bc a:hover { color: var(--orange); }

    .td-bc-sep {
        color: var(--text-muted);
        margin: 0 var(--space-1);
        flex: 0 0 auto;
    }

    .td-bc-link {
        color: var(--text-secondary);
        font-size: var(--text-90);
        min-width: 0;
        max-width: clamp(48px, 34vw, 420px);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .td-bc-current {
        color: var(--text-primary);
        font-weight: 600;
        min-width: 0;
        max-width: clamp(42px, 24vw, 260px);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .td-bc-logo {
        display: inline-flex;
        align-items: center;
        flex-shrink: 0;
        margin-right: var(--space-1);
    }

    .td-bc-logo-img {
        height: 22px;
        width: auto;
    }

    .td-search-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 28px;
        height: 28px;
        border-radius: var(--radius);
        color: var(--text-muted);
        background: var(--bg-card);
        border: 1px solid var(--border);
        cursor: pointer;
        transition: color 0.15s, border-color 0.15s;
        flex-shrink: 0;
        padding: 0;
    }

    .td-search-btn:hover {
        color: var(--orange);
        border-color: var(--orange);
    }

    /* ── Tree view search ─────────────────────────────────────────── */

    .td-search-group {
        display: flex;
        align-items: center;
        gap: var(--space-3);
        margin-left: auto;
        flex: 0 0 auto;
        min-width: 0;
    }

    /* Scoped with the element so it outranks the app-wide
       `input:not(…):not(…)` field rule (0-2-1), declared earlier: without it
       the fields took a form field's full width and padding, and pushed the
       button out of a phone's topbar. */
    .td-search-group input.td-search-input {
        padding: var(--space-2) var(--space-4);
        font-size: var(--text-80);
        border: 1px solid var(--border);
        border-radius: var(--radius-sm);
        background: var(--bg-card);
        color: var(--text-primary);
        width: 140px;
        font-family: var(--font-sans);
        transition: border-color 0.2s;
    }

    .td-search-input:focus {
        outline: none;
        border-color: var(--orange);
    }

    .td-search-input::placeholder {
        color: var(--text-muted);
    }

    /* ── Topbar search suggestions ─────────────────────────────────
       A fixed overlay rather than a panel inside `.td-search-group`:
       `.td-topbar` clips its overflow, so anything positioned within it would
       be cut off. The surface chrome (background, shadow, border, radius,
       z-index) comes from `.context-menu`; only the sizing is here.
       One width whatever it lists, so the panel does not jump as the names
       and persons change under the typing; rows truncate their long lines.
       Doubled class to override `.context-menu`'s `max-content`, declared
       further down. */

    .context-menu.td-suggest {
        width: min(520px, calc(100vw - 32px));
        max-height: 60vh;
        overflow-y: auto;
        padding: 0;
    }

    .td-suggest-row {
        border-radius: 0;
    }

    /* Keyboard and pointer converge on one highlight, so arrowing through the
       list looks the same as hovering it. */
    .td-suggest-row.is-active {
        background: var(--bg-card-hover);
    }

    /* The names completing the field being typed, above the persons. */
    .td-suggest-names {
        padding: var(--space-2) 0;
        border-bottom: 1px solid var(--border);
    }

    .td-suggest-names .td-suggest-row {
        padding: 5px var(--space-5);
    }

    .td-suggest-sosa {
        flex-shrink: 0;
        align-self: center;
        padding: 1px var(--space-3);
        border-radius: var(--radius);
        background: var(--green);
        color: var(--bg-deep);
        font-size: var(--text-70);
        font-weight: 700;
        letter-spacing: 0.03em;
    }

    .td-suggest-more {
        display: block;
        width: 100%;
        padding: 7px var(--space-5);
        border: none;
        border-top: 1px solid var(--border);
        background: none;
        color: var(--orange);
        font-family: var(--font-sans);
        font-size: var(--text-80);
        text-align: center;
        cursor: pointer;
        transition: background 0.1s;
    }

    .td-suggest-more:hover {
        background: var(--bg-card-hover);
    }

    /* ── Homonym picker ────────────────────────────────────────────
       A drop-down of quick-search rows (`.search-person-result`,
       `.td-suggest-row`) under a trigger drawn as one of them. It opens in
       place rather than as a `.context-menu`: it lives in dialogs, which sit
       above every context-menu layer. */

    .homonym-card {
        max-width: min(520px, calc(100vw - 32px));
    }

    .homonym-picker {
        text-align: left;
    }

    .homonym-select {
        position: relative;
    }

    /* Doubled class: `.search-person-result` is declared further down and
       clears the border this trigger needs to read as a control. */
    .search-person-result.homonym-select-trigger {
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: var(--space-3) var(--space-14) var(--space-3) var(--space-4);
        position: relative;
    }

    .homonym-select-caret {
        position: absolute;
        right: 10px;
        top: 50%;
        transform: translateY(-50%);
        color: var(--text-muted);
    }

    /* ── Field with suggestions ────────────────────────────────────
       Its suggestions open in place for the same reason as the homonym
       picker's: these fields live in dialogs, above every context-menu layer.
       The list shares the picker's drop-down; rows are `.td-suggest-row`s. */

    .suggest-input {
        position: relative;
    }

    /* The name, its count and the sheet badge, spaced on one line; the
       badge keeps to the right edge. Doubled class to outrank
       `.context-menu-item`'s `display: block`, declared further down. */
    .context-menu-item.suggest-input-row {
        display: flex;
        align-items: baseline;
        gap: var(--space-3);
    }

    .suggest-input-name {
        font-weight: 600;
    }

    .suggest-input-detail {
        color: var(--text-muted);
        font-size: var(--text-80);
    }

    .suggest-input-sheet {
        flex-shrink: 0;
        align-self: center;
        margin-left: auto;
        padding: 0 var(--space-3);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        color: var(--text-muted);
        font-size: var(--text-70);
        cursor: help;
    }

    .homonym-select-list,
    .suggest-input-list {
        position: absolute;
        left: 0;
        right: 0;
        top: calc(100% + 4px);
        z-index: 1;
        max-height: 50vh;
        overflow-y: auto;
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        box-shadow: var(--shadow-md);
    }

    .homonym-separate-icon {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 36px;
        height: 36px;
        border-radius: 50%;
        border: 1px dashed var(--border);
        color: var(--text-muted);
        font-size: var(--text-110);
    }

    .homonym-warning {
        margin: var(--space-5) 0 0;
        padding: var(--space-4) var(--space-6);
        border-radius: var(--radius);
        background: color-mix(in srgb, var(--orange) 10%, transparent);
        border: 1px solid color-mix(in srgb, var(--orange) 35%, transparent);
        color: var(--text-primary);
        font-size: var(--text-80);
        line-height: 1.5;
    }

    /* ── Merge wizard (`docs/ui-merge.md`) ─────────────────────── */
    .modal-card.merge-card {
        width: 100%;
        min-width: 0;
        max-width: min(760px, calc(100vw - 32px));
        max-height: 90vh;
        overflow-y: auto;
    }

    .merge-step {
        margin: -4px 0 var(--space-6);
        color: var(--text-muted);
        font-size: var(--text-80);
    }

    .merge-persons {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: var(--space-4);
        margin-bottom: var(--space-6);
    }

    .merge-person {
        display: flex;
        flex-direction: column;
        gap: var(--space-3);
        cursor: pointer;
    }

    .merge-person > input { position: absolute; opacity: 0; pointer-events: none; }

    .merge-person-keep {
        display: inline-flex;
        align-items: center;
        gap: var(--space-3);
        font-size: var(--text-80);
        color: var(--text-muted);
    }

    .merge-person-keep::before {
        content: "";
        width: 12px;
        height: 12px;
        border-radius: 50%;
        border: 2px solid var(--border);
    }

    .merge-person > input:checked ~ .merge-person-keep { color: var(--orange); font-weight: 600; }
    .merge-person > input:checked ~ .merge-person-keep::before {
        border-color: var(--orange);
        background: radial-gradient(var(--orange) 40%, transparent 45%);
    }
    .merge-person > input:focus-visible ~ .merge-person-keep { outline: 2px solid var(--orange); outline-offset: 2px; }

    /* Nested: `.search-person-result:last-child`, declared further down,
       drops the bottom border a list's last row does not need. */
    .merge-person .search-person-result.merge-person-card {
        border: 1px solid var(--border);
        border-radius: var(--radius);
    }

    .merge-person > input:checked ~ .search-person-result.merge-person-card {
        border-color: var(--orange);
    }

    .merge-compare-table th { width: 26%; }
    .merge-compare-table td { width: 37%; overflow-wrap: anywhere; }
    .merge-compare-table tr.merge-differs td { color: var(--orange); }

    .merge-pick {
        display: flex;
        align-items: flex-start;
        gap: var(--space-3);
        cursor: pointer;
    }

    .merge-pick > input { flex: none; margin: 3px 0 0; }
    .merge-compare-table tr.merge-differs .merge-pick { color: var(--orange); }

    .merge-list-title { margin: var(--space-8) 0 var(--space-2); font-size: var(--text-90); }

    .merge-list {
        display: flex;
        flex-direction: column;
        gap: var(--space-2);
    }

    .merge-list-item {
        display: flex;
        align-items: baseline;
        flex-wrap: wrap;
        gap: var(--space-4);
        padding: var(--space-2) 0;
        cursor: pointer;
        overflow-wrap: anywhere;
    }

    .merge-already {
        color: var(--text-muted);
        font-size: var(--text-75);
        font-style: italic;
    }

    .merge-summary {
        margin: 0;
        padding-left: var(--space-9);
        line-height: 1.7;
    }

    @media (max-width: 640px) {
        .merge-persons { grid-template-columns: minmax(0, 1fr); }
    }

    /* ── Pedigree outer container ────────────────────────────────── */

    .pedigree-outer {
        position: relative;
        flex: 1;
        min-height: 0;
        display: flex;
        flex-direction: row;
        overflow: hidden;
    }

    /* ── Icon sidebar ────────────────────────────────────────────── */

    .isb {
        width: var(--sb);
        min-width: var(--sb);
        background: var(--bg-panel);
        border-right: 1px solid var(--border);
        display: flex;
        flex-direction: column;
        align-items: center;
        padding: var(--space-3) 0;
        gap: var(--space-1);
        flex-shrink: 0;
        z-index: 5;
    }

    .isb-btn {
        width: 34px;
        height: 34px;
        display: flex;
        align-items: center;
        justify-content: center;
        background: none;
        border: none;
        border-radius: var(--radius);
        cursor: pointer;
        font-size: var(--text-110);
        color: var(--text-secondary);
        transition: background 0.12s, color 0.12s;
        line-height: 1;
        padding: 0;
    }

    .isb-btn:hover { background: var(--bg-card-hover); color: var(--orange); }
    .isb-btn:active { background: color-mix(in srgb, var(--orange) 12%, transparent); }
    .isb-btn:disabled {
        color: var(--text-muted);
        cursor: default;
        opacity: 0.45;
    }
    .isb-btn:disabled:hover { background: none; color: var(--text-muted); }

    .isb-hr { width: 28px; height: 1px; background: var(--border); margin: var(--space-2) 0; }

    .isb-zoom-val {
        font-size: var(--text-65);
        color: var(--text-muted);
        text-align: center;
        line-height: 1;
        width: 100%;
        padding: 0 var(--space-1);
    }

    .pedigree-resize-fit-trigger {
        display: none;
    }

    /* ── Pedigree canvas viewport ────────────────────────────────── */

    .pedigree-viewport {
        position: relative;
        overflow: hidden;
        flex: 1;
        min-height: 0;
        cursor: grab;
        background: var(--bg-deep);
        -webkit-user-select: none;
        user-select: none;
    }

    .pedigree-viewport:active { cursor: grabbing; }

    .ped-card:hover .ped-card-rect { fill: var(--pn-hover-bg) !important; stroke: var(--pn-root-bg) !important; }
    .ped-card-focus:hover .ped-card-name-text, .ped-card-focus:hover .ped-card-name-text tspan { fill: var(--pn-text) !important; }

    /* ── Wheel and fan charts, ancestor and descendant ───────────────
       A segment is tinted by sex from the same tokens as a card's rule, a
       direct ancestor's inner edge carries the SOSA colour, and a missing
       parent is a dashed outline, as the empty card is. */

    .fan-seg { cursor: pointer; }
    .fan-seg-shape {
        fill: var(--pn-bg);
        stroke: var(--pn-border);
        stroke-width: 1;
    }
    .fan-seg-male .fan-seg-shape { fill: color-mix(in srgb, var(--pn-male-line) 16%, var(--pn-bg)); }
    .fan-seg-female .fan-seg-shape { fill: color-mix(in srgb, var(--pn-female-line) 16%, var(--pn-bg)); }
    .fan-seg:hover .fan-seg-shape { fill: var(--pn-hover-bg); stroke: var(--pn-root-bg); }
    .fan-root .fan-seg-shape { fill: var(--pn-root-bg); }
    .fan-root:hover .fan-seg-shape { fill: var(--pn-root-bg); stroke: var(--pn-border); }
    .fan-seg-band { fill: none; stroke-width: 3; pointer-events: none; }
    .fan-root-ring { fill: none; stroke-width: 3; pointer-events: none; }
    .fan-seg-label { pointer-events: none; }
    .fan-slot { cursor: pointer; }
    .fan-slot-shape {
        fill: var(--pn-bg);
        fill-opacity: 0.35;
        stroke: var(--pn-border);
        stroke-width: 1;
        stroke-dasharray: 4 4;
    }
    .fan-slot:hover .fan-slot-shape { fill-opacity: 1; fill: var(--pn-hover-bg); }
    .fan-slot-plus {
        fill: var(--pn-root-bg);
        font: 700 16px sans-serif;
        text-anchor: middle;
        pointer-events: none;
    }
    /* A union of a descendant chart: a neutral band between two
       generations, naming the spouse. */
    .fan-union { cursor: pointer; }
    .fan-union-shape {
        fill: color-mix(in srgb, var(--pn-border) 18%, var(--pn-bg));
        stroke: var(--pn-border);
        stroke-width: 1;
    }
    .fan-union:hover .fan-union-shape { fill: var(--pn-hover-bg); stroke: var(--pn-root-bg); }

    /* ── Lineage view (Gramps' Pedigree) ─────────────────────────────
       A non-birth link is dashed, as Gramps draws it; the button left of
       the root lists its children, those with children of their own in
       bold. */

    .lineage-link-non-birth { stroke-dasharray: 5 4; }
    /* A union's spouse under a person, in the descendant lineage and the
       hourglass: a slim box lighter than a card. */
    .lineage-spouse { cursor: pointer; }
    .lineage-spouse-unknown { cursor: default; }
    .lineage-spouse-rect {
        fill: color-mix(in srgb, var(--pn-border) 12%, var(--pn-bg));
        stroke: var(--pn-border);
        stroke-width: 1;
        stroke-dasharray: none;
    }
    .lineage-spouse-unknown .lineage-spouse-rect { stroke-dasharray: 3 3; }
    .lineage-spouse:hover:not(.lineage-spouse-unknown) .lineage-spouse-rect { fill: var(--pn-hover-bg); }
    .lineage-children { cursor: pointer; }
    .lineage-children circle {
        fill: var(--pn-bg);
        stroke: var(--pn-border);
        stroke-width: 1;
    }
    .lineage-children:hover circle { fill: var(--pn-hover-bg); stroke: var(--pn-root-bg); }
    .lineage-children text {
        fill: var(--pn-text);
        font: 700 18px sans-serif;
        text-anchor: middle;
        pointer-events: none;
    }
    .context-menu-item.lineage-child-with-children { font-weight: 700; font-style: italic; }

    .pedigree-inner {
        position: absolute;
        top: 0;
        left: 0;
        width: 100%;
        height: 100%;
        transform-origin: 0 0;
    }

    .pedigree-tree {
        position: relative;
        display: flex;
        flex-direction: column;
        align-items: stretch;
        min-width: 320px;
        padding: 0;
    }

    /* ── Depth popover (from isb) ────────────────────────────────── */

    .pedigree-depth-popover {
        position: absolute;
        top: 0;
        left: calc(100% + 4px);
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        box-shadow: var(--shadow-md);
        padding: var(--space-6) var(--space-7);
        z-index: 20;
        min-width: 170px;
        pointer-events: all;
    }

    .pedigree-depth-row { display: flex; align-items: center; gap: var(--space-3); margin-bottom: var(--space-4); }
    .pedigree-depth-row:last-child { margin-bottom: 0; }

    .pedigree-depth-btn {
        width: 24px;
        height: 24px;
        display: flex;
        align-items: center;
        justify-content: center;
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius-sm);
        cursor: pointer;
        font-size: var(--text-100);
        font-weight: 600;
        color: var(--text-primary);
        padding: 0;
        line-height: 1;
        transition: background 0.1s;
    }

    .pedigree-depth-btn:hover { background: var(--orange); color: white; border-color: var(--orange); }

    .pedigree-depth-val { width: 20px; text-align: center; font-size: var(--text-90); font-weight: 600; }

    /* ── Event panel ─────────────────────────────────────────────── */

    .ev-panel {
        /* --evw is a fixed width until the reader drags the handle, after which
           the pedigree stores it as a ratio and it tracks the window. The clamp
           keeps that ratio within the same bounds the drag enforced. */
        width: clamp(220px, var(--evw), 640px);
        min-width: 0;
        background: var(--bg-panel);
        border-left: 1px solid var(--border);
        display: flex;
        flex-direction: column;
        overflow: hidden;
        flex-shrink: 0;
        position: relative;
        transition: width 0.2s, min-width 0.2s;
    }

    .evp-resize-handle {
        position: relative;
        z-index: 12;
        width: 8px;
        flex: 0 0 8px;
        margin-left: -4px;
        margin-right: -4px;
        cursor: col-resize;
        touch-action: none;
        outline: none;
    }

    .evp-resize-handle::after {
        content: "";
        position: absolute;
        inset: 0 3px;
        background: transparent;
        transition: background 0.12s;
    }

    .evp-resize-handle:hover::after,
    .evp-resize-handle:focus-visible::after,
    .pedigree-is-resizing .evp-resize-handle::after {
        background: var(--orange);
    }

    .pedigree-is-resizing .ev-panel {
        transition: none;
    }

    .ev-panel-collapsed {
        width: 28px;
        min-width: 28px;
    }

    .evp-toggle {
        position: absolute;
        top: 19px;
        left: 4px;
        width: 20px;
        height: 28px;
        background: none;
        border: 1px solid var(--border);
        border-radius: var(--radius-sm);
        color: var(--text-muted);
        font-size: var(--text-100);
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 0;
        line-height: 1;
        z-index: 10;
        transform: translateY(-50%);
        transition: background 0.15s, color 0.15s;
    }

    .evp-toggle:hover {
        background: var(--bg-card-hover);
        color: var(--text-primary);
    }

    .ev-panel:not(.ev-panel-collapsed) .evp-toggle {
        left: -1px;
        top: 19px;
    }

    .evp-hd {
        min-height: 38px;
        padding: 0 var(--space-7) 0 var(--space-17);
        border-bottom: 1px solid var(--border);
        display: flex;
        align-items: center;
        font-size: var(--text-70);
        font-weight: 700;
        color: var(--text-secondary);
        text-transform: uppercase;
        letter-spacing: 0.5px;
        flex-shrink: 0;
    }

    .evp-person {
        display: flex;
        align-items: center;
        gap: var(--space-5);
        padding: var(--space-5) var(--space-7);
        border-bottom: 1px solid var(--border);
        flex-shrink: 0;
    }

    .evp-av {
        width: 36px;
        height: 36px;
        border-radius: 50%;
        background: var(--bg-card);
        border: 1px solid var(--border);
        display: flex;
        align-items: center;
        justify-content: center;
        overflow: hidden;
        flex-shrink: 0;
    }

    /* An `svg` where an `img` would be is a crop the browser is cutting for
       itself — a region of a picture we hold no copy of. It fills the frame
       the same way; `preserveAspectRatio` does there what `object-fit` does
       here. Every portrait and crop below reads the same way. */
    .evp-av img,
    .evp-av svg {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }

    .evp-name { display: flex; flex-direction: column; min-width: 0; }

    .evp-name strong {
        font-size: var(--text-90);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        color: var(--text-primary);
    }

    .evp-name span { font-size: var(--text-75); color: var(--text-secondary); }

    .evp-list { flex: 1; overflow-y: auto; padding: var(--space-3) 0; }

    .evp-empty { padding: var(--space-12) var(--space-7); text-align: center; color: var(--text-muted); font-size: var(--text-80); }

    .ev-item {
        display: flex;
        align-items: flex-start;
        gap: var(--space-4);
        padding: 7px var(--space-7);
        border-bottom: 1px solid var(--border);
        transition: background 0.1s;
    }

    .ev-item:last-child { border-bottom: none; }
    .ev-item:hover { background: var(--bg-card-hover); }

    /* Events directly on the selected person or their conjugal family stand
       out from narrative-context events (children, parents, siblings). */
    .ev-item.ev-item-direct { background: color-mix(in srgb, var(--orange) 8%, transparent); }
    .ev-item.ev-item-direct:hover { background: color-mix(in srgb, var(--orange) 14%, transparent); }

    .ev-ic {
        width: 24px;
        height: 24px;
        border-radius: var(--radius-sm);
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: var(--text-75);
        flex-shrink: 0;
        margin-top: 1px;
    }

    .ev-ic-birth { background: color-mix(in srgb, var(--green) 18%, transparent);  color: var(--green);  }
    .ev-ic-death { background: color-mix(in srgb, var(--blue) 15%, transparent); color: var(--blue);   }
    .ev-ic-marry { background: color-mix(in srgb, var(--orange) 15%, transparent); color: var(--orange); }
    .ev-ic-other { background: var(--bg-card-hover); color: var(--text-secondary); }

    .ev-info { display: flex; flex-direction: column; min-width: 0; flex: 1; }

    .ev-type { font-size: var(--text-80); font-weight: 600; color: var(--text-primary); line-height: 1.3; }
    .ev-date { font-size: var(--text-70); color: var(--text-secondary); line-height: 1.3; }
    .ev-place {
        font-size: var(--text-70); color: var(--text-muted); line-height: 1.3;
        white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    }

    /* ── Context menu ─────────────────────────────────────────────── */

    .context-menu-backdrop {
        position: fixed;
        inset: 0;
        z-index: 300;
    }

    .context-menu {
        position: fixed;
        z-index: 310;
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        box-shadow: var(--shadow-md);
        width: max-content;
        max-width: calc(100vw - 16px);
        padding: var(--space-2) 0;
    }

    .context-menu-header {
        padding: var(--space-4) var(--space-7);
        font-size: var(--text-80);
        font-weight: 600;
        color: var(--text-secondary);
        border-bottom: 1px solid var(--border);
    }

    /* A heading inside a menu's list, as "Spouses" above the spouses. */
    .context-menu-subheader {
        padding: var(--space-3) var(--space-7) var(--space-1);
        font-size: var(--text-70);
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.04em;
        color: var(--text-muted);
    }

    .context-menu-item {
        display: block;
        width: 100%;
        padding: var(--space-4) var(--space-7);
        text-align: left;
        background: none;
        border: none;
        font-size: var(--text-85);
        cursor: pointer;
        transition: background 0.1s;
        font-family: var(--font-sans);
        color: var(--text-primary);
        text-decoration: none;
    }

    .context-menu-item:hover {
        background: var(--bg-card-hover);
    }

    .context-menu-item.context-menu-danger {
        color: var(--danger-text);
    }

    .context-menu-item.context-menu-danger:hover {
        background: color-mix(in srgb, var(--danger) 10%, transparent);
    }

    .context-menu-divider {
        border: none;
        border-top: 1px solid var(--border);
        margin: var(--space-2) 0;
    }

    .context-menu-back {
        font-weight: 600;
        color: var(--text-secondary);
    }

    /* Grow leftwards from the anchor point instead of rightwards, so a menu
       opened from a control on the right edge stays aligned with it whatever
       its own width is. */
    .context-menu-anchor-right { transform: translateX(-100%); }

    .context-menu-events { width: 250px; }

    .context-menu-event-picker,
    .context-menu-event-list {
        display: flex;
        min-width: 0;
        flex-direction: column;
    }

    .context-menu-event-item {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .context-menu-event-scroll {
        display: grid;
        width: 100%;
        height: 22px;
        padding: 0;
        place-items: center;
        border: 0;
        background: none;
        color: var(--text-muted);
        cursor: pointer;
        font-size: var(--text-70);
        line-height: 1;
    }

    .context-menu-event-scroll:hover,
    .context-menu-event-scroll:focus-visible {
        background: var(--bg-card-hover);
        color: var(--orange);
    }

    /* ── Reference tooltip (occupation sheets, given-name meanings) ──── */

    .ref-hover-target {
        cursor: help;
    }

    .ref-tooltip {
        position: fixed;
        z-index: 320;
        max-width: 320px;
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        box-shadow: var(--shadow-md);
        padding: var(--space-5) var(--space-7);
        pointer-events: none;
    }

    .ref-tooltip-label {
        font-family: var(--font-heading);
        font-weight: 700;
        color: var(--orange);
        margin-bottom: var(--space-2);
    }

    .ref-tooltip-meta {
        font-size: var(--text-80);
        font-style: italic;
        color: var(--text-secondary);
        margin-bottom: var(--space-3);
    }

    .ref-tooltip-text {
        font-size: var(--text-85);
        line-height: 1.4;
        color: var(--text-primary);
    }

    /* ── SVG pedigree connector paths ─────────────────────────────── */

    .pedigree-connector-path {
        fill: none;
        stroke: var(--pn-border);
        stroke-width: 1;
    }

     /* ── Medieval pedigree theme ──────────────────────────────────────
         The cards and connectors keep their engraved palette while the
         canvas continues to use the application's own background. */

    .ped-theme-medieval {
        --parchment:      #efe2c4;
        --parchment-deep: #e3d2ac;
        --ink:            #43331f;
        --ink-soft:       #6d5838;
        --gilt:           #9c7b32;

        --pn-bg:          #f6ecd5;
        --pn-spouse-bg:   #f1e4c8;
        --pn-root-bg:     #7c3f2a;
        --pn-border:      var(--ink-soft);
        --pn-male-line:   #4a5f7a;
        --pn-female-line: #8c4a52;
        --pn-text:        var(--ink);
        --pn-text-muted:  var(--ink-soft);
        --pn-hover-bg:    #e8d7ae;
        --pn-sosa:        #6b7c3a;
        --pn-sosa-root:   var(--gilt);
        --pn-self:        #7c3f2a;
        /* The mat behind a portrait: aged paper, never a white chip. */
        --pn-mat:         var(--parchment-deep);

    }

    /* Ruled connectors are drawn with a pen, so they carry the ink colour
       and a little more weight than the hairline the classic theme uses. */
    .ped-theme-medieval .pedigree-connector-path {
        stroke: var(--ink);
        stroke-width: 4;
        stroke-linejoin: miter;
        stroke-linecap: square;
    }

    /* The lighter core that turns the band above into a double rule. */
    .ped-theme-medieval .pedigree-connector-core {
        stroke: var(--parchment);
        stroke-width: 1.6;
        stroke-linejoin: miter;
        stroke-linecap: square;
        fill: none;
    }

    /* The cartouche's second rule sits inside the frame in a lighter ink,
       the way an engraver would cut it. */
    .ped-theme-medieval .ped-card-inner-rule {
        stroke: var(--gilt);
        stroke-opacity: 0.75;
    }

    .ped-theme-medieval .ped-card-rect {
        stroke-linejoin: miter;
    }

    /* The portrait medallion gets its own ring rather than sitting flush. */
    .ped-theme-medieval .ped-card-mat {
        stroke: var(--gilt);
        stroke-width: 1.2;
    }

    .ped-theme-medieval .ped-card:hover .ped-card-rect {
        fill: var(--pn-hover-bg) !important;
        stroke: var(--gilt) !important;
    }

     /* ── Mini pedigree (person detail: ancestors/descendants) ────────
         Static viewport with an automatically fitted scale. ───────────── */

    .mini-pedigree {
        position: relative;
        overflow: hidden;
        height: 280px;
        border-radius: var(--radius);
        background: var(--bg-deep);
        -webkit-user-select: none;
        user-select: none;
    }

    .mini-pedigree-inner {
        position: absolute;
        top: 0;
        left: 0;
        transform-origin: 0 0;
    }
    .mini-pedigree-pending .mini-pedigree-inner { visibility: hidden; }

    .mini-pedigree-tooltip {
        position: absolute;
        left: 50%;
        top: 8px;
        z-index: 3;
        max-width: calc(100% - 16px);
        transform: translateX(-50%);
        padding: 6px 10px;
        border: 1px solid var(--border);
        border-radius: 6px;
        background: var(--bg-panel);
        color: var(--text-primary);
        box-shadow: var(--shadow);
        pointer-events: none;
        text-align: center;
        white-space: nowrap;
    }

    .mini-pedigree-tooltip-pointer {
        position: fixed;
        left: var(--mini-tooltip-x);
        top: var(--mini-tooltip-y);
        overflow: hidden;
    }

    .mini-pedigree-tooltip-right {
        max-width: calc(100% - var(--mini-tooltip-x) - 18px);
        transform: translateX(10px);
    }

    .mini-pedigree-tooltip-left {
        max-width: calc(var(--mini-tooltip-x) - 18px);
        transform: translateX(calc(-100% - 10px));
    }

    .mini-pedigree-tooltip-above {
        margin-top: -10px;
        translate: 0 -100%;
    }

    .mini-pedigree-tooltip-below {
        margin-top: 10px;
    }

    .mini-pedigree-tooltip-name {
        overflow: hidden;
        text-overflow: ellipsis;
        font-size: 0.86rem;
        font-weight: 700;
    }

    .mini-pedigree-tooltip-dates {
        margin-top: 1px;
        overflow: hidden;
        text-overflow: ellipsis;
        color: var(--text-secondary);
        font-size: 0.74rem;
    }

    /* ── Animated transitions ──────────────────────────────────── */

    .pedigree-animated .pedigree-inner {
        transition: transform 0.3s ease;
    }

    /* ── Active sidebar button ─────────────────────────────────── */

    .isb-btn-active {
        color: var(--orange) !important;
        background: color-mix(in srgb, var(--orange) 12%, transparent);
    }

    .isb-depth-wrap {
        position: relative;
    }

    .pedigree-depth-arrow {
        font-size: var(--text-100);
        width: 16px;
        text-align: center;
        color: var(--text-muted);
    }

    /* ── Event panel year groups ────────────────────────────────── */

    .ev-year-group {
        border-bottom: 1px solid var(--border);
    }

    .ev-year-group:last-child { border-bottom: none; }

    .ev-year-header {
        padding: var(--space-3) var(--space-7) var(--space-1);
        font-size: var(--text-75);
        font-weight: 700;
        color: var(--text-secondary);
        position: sticky;
        top: 0;
        background: var(--bg-panel);
        z-index: 1;
    }

    .ev-item-clickable {
        cursor: pointer;
    }

    /* ── Responsive: event panel below 900px ────────────────────── */

    @media (max-width: 900px) {
        /* Event panel as drawer on mobile — the collapsed width is the same
           as at any other size, so it is not restated here. */
        .ev-panel {
            position: absolute;
            right: 0;
            top: 0;
            bottom: 0;
            z-index: 50;
            box-shadow: var(--shadow-md);
        }

        .evp-resize-handle {
            display: none;
        }
    }

    /* ── Responsive: no event panel at all below 400px ───────────── */

    @media (max-width: 400px) {
        .ev-panel,
        .evp-resize-handle {
            display: none;
        }
    }

    /* ── Search person (typeahead) ────────────────────────────────── */

    .search-person {
        margin-top: var(--space-4);
    }

    .search-person-input-row {
        display: flex;
        gap: var(--space-4);
        align-items: center;
        margin-bottom: var(--space-4);
    }

    .search-person-input-row input {
        flex: 1;
    }

    .search-person-results {
        max-height: 300px;
        overflow-y: auto;
        border: 1px solid var(--border);
        border-radius: var(--radius);
    }

    .search-person-result {
        display: flex;
        align-items: center;
        gap: var(--space-4);
        width: 100%;
        padding: var(--space-2) var(--space-4);
        background: none;
        border: none;
        border-bottom: 1px solid var(--border);
        cursor: pointer;
        font-family: var(--font-sans);
        font-size: var(--text-85);
        text-align: left;
        transition: background 0.1s;
        color: var(--text-primary);
    }

    .search-person-result:last-child {
        border-bottom: none;
    }

    .search-person-result:hover {
        background: var(--bg-card-hover);
    }

    .sp-result-photo {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 36px;
        height: 36px;
        flex-shrink: 0;
    }

    .sp-result-portrait {
        display: block;
        width: 36px;
        height: 36px;
        object-fit: cover;
        border-radius: 50%;
    }

    .sp-result-info {
        flex: 1;
        min-width: 0;
    }

    .sp-result-name {
        font-weight: 600;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .sp-surname { text-transform: uppercase; font-size: var(--text-80); }
    .sp-given { font-weight: 400; font-size: var(--text-80); }

    .sp-result-dates {
        display: flex;
        gap: var(--space-4);
        font-size: var(--text-75);
        color: var(--text-secondary);
        margin-top: 1px;
    }
    .sp-birth { color: var(--green); }
    .sp-death { color: var(--blue); }

    /* Who the person is, not just what they are called: the spouse, or the
       parents when there is no spouse. One line, truncated — it identifies a
       result, it is not the result. */
    .sp-result-rel {
        font-size: var(--text-75);
        color: var(--text-secondary);
        margin-top: 1px;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .sp-result-meta {
        font-size: var(--text-75);
        color: var(--text-muted);
        margin-top: 1px;
    }

    .search-person-result.male { border-left: 3px solid color-mix(in srgb, var(--blue) 40%, transparent); }
    .search-person-result.female { border-left: 3px solid color-mix(in srgb, var(--pink) 40%, transparent); }

    /* ── Edit modals (person, couple) ──────────────────────────────────
       Both are the same object — a panel that fills its own height, a fixed
       header, a scrolling body, a fixed footer — so the chrome is described
       once and each modal only states where it differs (its width and how
       tall it is allowed to grow). They had a copy each, which is how the
       couple modal's fields ended up missing the control sizing below. */

    .person-form-modal,
    .union-form-modal {
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        box-shadow: var(--shadow-lg);
        max-width: 95vw;
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    .person-form-modal { width: 700px; max-height: 85vh; }
    .union-form-modal  { width: 720px; max-height: 90vh; }

    .person-form-header,
    .union-form-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: var(--space-8) var(--space-10);
        border-bottom: 1px solid var(--border);
    }

    .person-form-header h2,
    .union-form-header h2 {
        margin: 0;
        font-size: var(--text-110);
        color: var(--text-primary);
    }

    .uf-header-actions {
        display: flex;
        align-items: center;
        gap: var(--space-4);
        flex-shrink: 0;
    }

    .person-form-body,
    .union-form-body {
        flex: 1;
        overflow-y: auto;
        padding: var(--space-8) var(--space-10);
    }

    .pf-footer,
    .uf-footer {
        padding: var(--space-7) var(--space-10);
        border-top: 1px solid var(--border);
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--space-5);
        flex-shrink: 0;
    }

    .pf-footer-right,
    .uf-footer-right {
        display: flex;
        gap: var(--space-4);
        margin-left: auto;
    }

    .pf-footer .error-msg {
        flex: 1;
        margin: 0;
        font-size: var(--text-80);
    }

    .person-form-close {
        background: none;
        border: none;
        font-size: var(--text-120);
        cursor: pointer;
        color: var(--text-secondary);
        padding: var(--space-2) var(--space-4);
        border-radius: var(--radius-sm);
        transition: background 0.15s, color 0.15s;
    }

    .person-form-close:hover {
        background: var(--bg-card-hover);
        color: var(--text-primary);
    }

    .person-form-item {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: var(--space-5) var(--space-6);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        margin-bottom: var(--space-4);
        gap: var(--space-6);
        background: var(--bg-card);
    }

    /* Match every editable field in the modal (birth name, given names,
       dates, notes, ...) to the same background used by a saved
       .person-form-item row (e.g. a created profession), instead of the
       app-wide input background.

       The `:not()` chain is repeated from the app-wide rule, and repeating it
       is the whole point. `:not()` carries the specificity of its argument, so
       `input:not([type=…]):not([type=…])` scores 0-2-1 — above a bare
       `.person-form-modal input` at 0-1-1. Without it this rule reached the
       <select> and the <textarea> and silently missed every <input>, which is
       why a title field sat on the panel background while the description box
       beside it was white. */
    .person-form-modal input:not([type="checkbox"]):not([type="radio"]),
    .person-form-modal select,
    .person-form-modal textarea,
    .union-form-modal input:not([type="checkbox"]):not([type="radio"]),
    .union-form-modal select,
    .union-form-modal textarea,
    .pf-embedded input:not([type="checkbox"]):not([type="radio"]),
    .pf-embedded select,
    .pf-embedded textarea,
    /* The document form is one modal reached from three places, only two of
       which are a person or couple form. Listed here so its fields look the
       same wherever it was opened from, rather than inheriting the panel
       background — which is its own background — when opened from a profile.
       The media edit panel, inline in a gallery or as the viewer's column, is
       the same kind of form and follows the same rules. */
    .document-form-modal input:not([type="checkbox"]):not([type="radio"]),
    .media-panel input:not([type="checkbox"]):not([type="radio"]),
    .document-form-modal select,
    .media-panel select,
    .document-form-modal textarea,
    .media-panel textarea,
    /* The date input carries its own box wherever it is placed — a form, a
       filter panel, a tool — rather than inheriting whatever its host
       gives an input and a select, which is how its selects ended up taller
       than its fields outside the forms. */
    .pf-date-widget input:not([type="checkbox"]):not([type="radio"]),
    .pf-date-widget select {
        background: var(--bg-card);
    }

    /* A note copied off a parish register is routinely longer than the three
       rows it lands in, so the grip stays — but vertically only. Widening a
       textarea past its form column breaks the layout, and narrowing it just
       re-wraps the very text the user is trying to read.

       The scrollbar is spelled out and widened past the app-wide 6px: with a
       thumb the same colour as the field's own border, an overflowing note
       looked like it had simply lost its first lines. */
    .person-form-modal textarea,
    .union-form-modal textarea,
    .pf-embedded textarea,
    .document-form-modal textarea,
    .media-panel textarea {
        resize: vertical;
        min-height: 76px;
        overflow-y: auto;
    }

    .person-form-modal textarea::-webkit-scrollbar,
    .union-form-modal textarea::-webkit-scrollbar,
    .pf-embedded textarea::-webkit-scrollbar,
    .document-form-modal textarea::-webkit-scrollbar,
    .media-panel textarea::-webkit-scrollbar {
        width: 10px;
    }
    .person-form-modal textarea::-webkit-scrollbar-track,
    .union-form-modal textarea::-webkit-scrollbar-track,
    .pf-embedded textarea::-webkit-scrollbar-track,
    .document-form-modal textarea::-webkit-scrollbar-track,
    .media-panel textarea::-webkit-scrollbar-track {
        background: transparent;
    }
    .person-form-modal textarea::-webkit-scrollbar-thumb,
    .union-form-modal textarea::-webkit-scrollbar-thumb,
    .pf-embedded textarea::-webkit-scrollbar-thumb,
    .document-form-modal textarea::-webkit-scrollbar-thumb,
    .media-panel textarea::-webkit-scrollbar-thumb {
        background: var(--text-muted);
        border-radius: var(--radius-sm);
        border: 2px solid var(--bg-card);
    }

    /* <select> reserves extra native chrome height beyond its padding in
       some engines (e.g. WebKitGTK), rendering taller than a same-padded
       <input> — force both to the same box height so a "Date"/"Lieu" row
       lines up with a plain text field like "Note".

       The explicit line-height matters just as much: an <input> centres its
       single line of text in the content box whatever the line-height is,
       while a <select> lays the selected option out in a line box sized by
       the inherited one (1.6 from <body> = ~23px, taller than the 20px
       content box) and top-aligns it. Left alone the two texts sit a couple
       of pixels apart, which is what made the "Exact" qualifier look off
       next to the date field. 20px = 38px − 2×8px padding − 2×1px border. */
    .person-form-modal input,
    .person-form-modal select,
    .union-form-modal input,
    .union-form-modal select,
    .pf-embedded input,
    .pf-embedded select,
    .document-form-modal input,
    /* The media panel lists events to link with checkboxes, which a bare
       `input` here would stretch to a field's height. */
    .media-panel input:not([type="checkbox"]):not([type="radio"]),
    .document-form-modal select,
    .media-panel select,
    .pf-date-widget input:not([type="checkbox"]):not([type="radio"]),
    .pf-date-widget select {
        height: 38px;
        line-height: 20px;
    }

    /* Dropping the native appearance is what actually settles the text:
       while the engine draws the control, it positions the selected option
       with its own metrics — the padding and line-height above are advisory
       at best, which is why "Exact" kept sitting off-centre next to the date
       field. With appearance:none the select is an ordinary box that obeys
       both, and the arrow becomes ours to place. */
    .person-form-modal select,
    .union-form-modal select,
    .pf-embedded select,
    .document-form-modal select,
    .media-panel select,
    .pf-date-widget select {
        appearance: none;
        -webkit-appearance: none;
        padding-right: var(--space-15);
        background-color: var(--bg-card);
        /* The arrow is baked into a data URI, which cannot read a custom
           property, so the theme emits the whole URL with its own secondary
           text colour already substituted in. */
        background-image: var(--select-arrow);
        background-repeat: no-repeat;
        background-position: right 11px center;
        background-size: 10px 6px;
    }

    .person-form-item.editing {
        display: block;
        padding: var(--space-6);
        background: var(--bg-card);
    }

    .person-form-item-info {
        display: flex;
        align-items: center;
        gap: var(--space-4);
        flex-wrap: wrap;
        flex: 1;
        min-width: 0;
    }

    .person-form-item-actions {
        display: flex;
        gap: var(--space-2);
        flex-shrink: 0;
    }

    /* The persons a parent being created may already be
       (components/parent_suggestions.rs). */
    .pf-parent-suggestions {
        margin-bottom: var(--space-6);
        padding: var(--space-5) var(--space-6);
        border: 1px solid var(--orange);
        border-radius: var(--radius);
        background: color-mix(in srgb, var(--orange) 6%, transparent);
    }
    .pf-parent-suggestions-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--space-4);
        margin-bottom: var(--space-3);
        font-size: var(--text-85);
        color: var(--text-primary);
    }
    .pf-parent-suggestion {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--space-6);
        padding: var(--space-3) 0;
        border-top: 1px solid var(--border);
    }
    .pf-parent-suggestions-foot {
        margin: var(--space-3) 0 0;
        font-size: var(--text-80);
    }

    /* Empty-state placeholder sized like a .person-form-item row instead of
       the much taller generic .empty-state, so an empty list doesn't jump
       in height once its first entry is added. */
    .pf-empty-item {
        padding: var(--space-4) var(--space-6);
        border: 1px dashed var(--border);
        border-radius: var(--radius);
        background: var(--bg-panel);
        color: var(--text-secondary);
        text-align: center;
        margin-bottom: var(--space-4);
    }
    .pf-empty-item p { margin: 0; }

    /* Profession(s) / additional-information rows — same height as a plain
       input (8px vertical padding) instead of the slightly taller default
       .person-form-item used for events/notes. */
    .person-form-item.pf-compact-item { padding: var(--space-4) var(--space-6); }

    /* ── Person form — section redesign ────────────────────────────── */

    .pf-subtitle {
        font-size: var(--text-75);
        color: var(--text-secondary);
        display: block;
        margin-top: var(--space-1);
    }

    /* Section headings keep the orange: uppercase, letterspaced and 0.68rem,
       they read as chapter markers rather than as something clickable, and
       they are what makes the form's spine scannable. What was actually
       competing with the save CTA was the orange spent on *buttons* — the
       add actions are now monochrome (see the button-hierarchy block below),
       so orange-on-a-control means "press this" and nothing else. */
    /* ── Collapsible sections ──────────────────────────────────────────
       Every block in the modal is built the same way: a header row that
       toggles it, then a body. The header carries the section's own rule
       (the line trailing the title), which is why the standalone <hr>
       separators are gone — two lines for one boundary read as a gap in
       the form rather than as a division of it.

       Spacing is owned here and nowhere else: sections are separated by
       one rhythm (--pf-gap-section), sub-blocks inside a section by
       another (--pf-gap-block), so no block carries an inline margin of
       its own. */

    .pf-section { --pf-gap-section: 22px; --pf-gap-block: 16px; }
    .pf-section + .pf-section { margin-top: var(--pf-gap-section); }

    .pf-section-head {
        display: flex;
        align-items: center;
        gap: var(--space-5);
    }

    /* The toggle is the section's title, and a real button, so the heading is
       reachable by keyboard rather than being a div you must click. */
    .pf-section-toggle {
        flex: 1;
        display: flex;
        align-items: center;
        gap: var(--space-4);
        min-width: 0;
        padding: 0;
        background: none;
        border: none;
        cursor: pointer;
        text-align: left;
        font-family: var(--font-sans);
        font-size: var(--text-70);
        font-weight: 700;
        letter-spacing: 0.12em;
        text-transform: uppercase;
        color: var(--orange);
    }

    .pf-section-toggle::after {
        content: "";
        flex: 1;
        height: 1px;
        background: var(--border);
    }

    /* Two borders of a square, rotated: points right when the section is
       closed, down when it is open. */
    .pf-chevron {
        flex: none;
        width: 6px;
        height: 6px;
        border-right: 1.5px solid currentColor;
        border-bottom: 1.5px solid currentColor;
        transform: rotate(-45deg);
        transition: transform 0.15s;
    }

    .pf-chevron.is-open { transform: rotate(45deg); }

    /* "More details": a quiet text link revealing a form's rare fields. */
    .pf-more { margin-top: var(--space-2); }

    .pf-more-toggle {
        display: inline-flex;
        align-items: center;
        gap: var(--space-4);
        padding: var(--space-1) 0;
        border: none;
        background: none;
        color: var(--text-secondary);
        font-size: var(--text-80);
        font-family: var(--font-sans);
        cursor: pointer;
    }

    .pf-more-toggle:hover { color: var(--orange); }

    .pf-more-body { margin-top: var(--space-5); }

    .pf-section-body { margin-top: var(--space-7); }

    /* Sub-blocks within a section (Profession(s), Autres informations,
       Notes) — one rhythm, replacing the inline margins these carried. */
    .pf-subblock { margin-top: var(--pf-gap-block); }

    /* Heading for a sub-block inside a section (Profession(s), Autres
       informations, Notes). Deliberately the same weight, size and colour as
       a field <label> such as "Sexe" — these are peers of the fields around
       them, not sections of their own. The optional trailing button (add a
       profession, add a note, ...) rides on the right of the same line. */
    .pf-block-label {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--space-5);
        font-size: var(--text-80);
        font-weight: 500;
        margin-bottom: var(--space-3);
        color: var(--text-secondary);
    }

    /* ── Person form — button hierarchy ────────────────────────────────
       Three tiers, and only three, so a glance answers "what do I press?":

         1. the modal CTA (footer Save) — the only filled orange gradient;
         2. a sub-form confirm (.pf-confirm-btn) — orange outline on a tint,
            clearly the action *inside* the open box without competing with
            the CTA. At most one sub-form is open at a time;
         3. everything else (.pf-add-btn to open a sub-form, .pf-row-btn for
            per-row edit/delete) — monochrome until hovered.

       Filled red (.btn-danger) is reserved for a delete that has already
       been confirmed. A row's own "Supprimer" is tier 3: it turns red on
       hover, so the modal no longer reads as a column of red blocks.

       Section headings stay orange — they are typographically unmistakable
       as headings, so they don't compete with a control for the same
       meaning. The rule is about *buttons*: an orange button is one you are
       meant to press. */

    .pf-add-btn {
        display: inline-flex;
        align-items: center;
        gap: 5px;
        padding: var(--space-2) var(--space-5);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        background: transparent;
        color: var(--text-secondary);
        font-size: var(--text-80);
        font-weight: 500;
        font-family: var(--font-sans);
        cursor: pointer;
        transition: color 0.15s, border-color 0.15s, background 0.15s;
    }

    .pf-add-btn::before {
        content: "+";
        font-size: var(--text-95);
        line-height: 1;
    }

    .pf-add-btn.is-open::before { content: "\00d7"; }

    .pf-add-btn:hover {
        color: var(--orange);
        border-color: var(--orange);
        background: color-mix(in srgb, var(--orange) 7%, transparent);
    }

    .pf-confirm-btn {
        padding: var(--space-3) var(--space-7);
        border-radius: var(--radius);
        border: 1px solid var(--orange);
        background: color-mix(in srgb, var(--orange) 10%, transparent);
        color: var(--orange);
        font-size: var(--text-80);
        font-weight: 600;
        font-family: var(--font-sans);
        cursor: pointer;
        transition: background 0.15s;
    }

    .pf-confirm-btn:hover { background: color-mix(in srgb, var(--orange) 20%, transparent); }
    .pf-confirm-btn:disabled { opacity: 0.5; cursor: default; }

    /* Row actions stay legible when idle (muted label, no border) rather
       than disappearing until hover — hidden-on-hover controls are
       unreachable on touch — but they only gain a box once pointed at. */
    .pf-row-btn {
        background: none;
        border: 1px solid transparent;
        border-radius: var(--radius-sm);
        padding: 3px 9px;
        font-size: var(--text-80);
        font-family: var(--font-sans);
        color: var(--text-muted);
        cursor: pointer;
        white-space: nowrap;
        transition: color 0.15s, border-color 0.15s, background 0.15s;
    }

    .pf-row-btn:hover {
        color: var(--text-primary);
        border-color: var(--border);
        background: var(--bg-card-hover);
    }

    .pf-row-btn.is-active {
        color: var(--orange);
        border-color: var(--orange);
    }

    .pf-row-btn.is-danger:hover {
        color: var(--danger-text);
        border-color: var(--danger-text);
        background: color-mix(in srgb, var(--red) 8%, transparent);
    }

    /* An open sub-form ("add a profession", "add a note", ...) sat on
       --bg-deep, which in the light palette is a hair off the modal's own
       --bg-panel — the box had no edge and its fields read as part of the
       surrounding form. Card background + border makes it a distinct
       container you can see the boundaries of. */
    .pf-subform,
    .pf-section .pf-embedded {
        padding: var(--space-7);
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
    }

    .pf-subform { margin-bottom: var(--space-6); }

    .badge-primary {
        background: color-mix(in srgb, var(--orange) 12%, transparent);
        border-color: var(--orange);
        color: var(--orange);
    }

    .pf-gender-group {
        display: flex;
        gap: var(--space-3);
        flex-wrap: wrap;
    }

    .pf-gender-btn {
        padding: 7px var(--space-9);
        border-radius: var(--radius);
        border: 1px solid var(--border);
        background: transparent;
        color: var(--text-secondary);
        cursor: pointer;
        font-size: var(--text-85);
        font-family: var(--font-sans);
        transition: border-color 0.15s, color 0.15s, background 0.15s;
    }

    .pf-gender-btn:hover:not(.active) {
        border-color: var(--text-muted);
        color: var(--text-primary);
    }

    .pf-gender-btn.active {
        border-color: var(--orange);
        color: var(--orange);
        background: color-mix(in srgb, var(--orange) 10%, transparent);
    }

    /* ── Date qualifier row ────────────────────────────────────────── */

    .pf-date-row { display: flex; gap: var(--space-4); align-items: center; flex-wrap: wrap; }
    .pf-date-qualifier-select { flex: 0 0 130px; }
    /* Only sizing here: the box itself (background, border, height, radius,
       and the select chevron) comes from the shared .person-form-modal
       input/select rules above, so these fields cannot drift from the rest
       of the modal. */
    .pf-date-widget {
        display: flex;
        flex-direction: column;
        gap: var(--space-2);
    }
    .pf-date-calendar { flex: 0 0 150px; }
    .pf-date-part { text-align: center; }
    .pf-date-dd,
    .pf-date-mm { flex: 0 0 56px; }
    /* Wide enough for the longest month a calendar names — « vendémiaire »,
       « jour compl. » — since these are read, not typed. */
    .pf-date-month-select { flex: 0 0 140px; }
    .pf-date-yyyy { flex: 0 0 72px; }
    /* "From an age" mode: an age, then the year it was observed in. */
    .pf-date-age { flex: 0 0 64px; }
    .pf-date-literal {
        font-size: var(--text-70);
        color: var(--text-secondary);
        padding-left: var(--space-1);
    }
    /* Sits exactly where the literal preview would, so the row never jumps. */
    .pf-date-error {
        font-size: var(--text-70);
        color: var(--red);
        padding-left: var(--space-1);
    }
    /* Centred with the 38px controls it sits between, rather than pinned to
       the top of the row by a hand-tuned line-height. */
    .pf-date-separator {
        line-height: 1;
        font-size: var(--text-80);
        color: var(--text-secondary);
        padding: 0 var(--space-2);
        white-space: nowrap;
    }

    /* ── Per-event notes & source ──────────────────────────────────── */

    /* Rendered as a sibling right under its .person-form-item row: the row
       loses its bottom rounding and the panel picks it up, so the two read
       as one card. */
    .person-form-item.pf-ns-open { margin-bottom: 0; border-radius: var(--radius) var(--radius) 0 0; }
    .pf-ns-body {
        padding: var(--space-6);
        margin-bottom: var(--space-4);
        border: 1px solid var(--border);
        border-top: none;
        border-radius: 0 0 var(--radius) var(--radius);
        background: var(--bg-card);
    }
    .pf-ns-actions { display: flex; gap: var(--space-4); align-items: center; }

    /* An event's evidence block. Separated by a rule rather than by a heading:
       it sits below the event's own Save button and writes on its own, so the
       line is there to say "past this point, changes are already saved". */
    .pf-ns-block {
        margin-top: var(--space-7);
        padding-top: var(--space-6);
        border-top: 1px solid var(--border);
    }

    .pf-ns-label {
        display: block;
        font-size: var(--text-70);
        text-transform: uppercase;
        letter-spacing: 0.07em;
        color: var(--orange);
        margin-bottom: var(--space-1);
    }

    .pf-ns-hint {
        font-size: var(--text-70);
        color: var(--text-muted);
        margin: 0 0 var(--space-4);
    }

    /* ── Witnesses list ────────────────────────────────────────────── */

    .pf-witness-list { margin-bottom: var(--space-3); }
    .pf-witness-row { display: flex; gap: var(--space-3); align-items: center; margin-bottom: var(--space-3); }
    .pf-witness-row input { flex: 1; }
    .pf-witness-name { font-weight: 500; }
    .pf-witness-relation { color: var(--text-secondary); font-size: var(--text-90); }
    .pf-witness-add { display: flex; flex-direction: column; gap: var(--space-3); margin-top: var(--space-3); }
    .pf-witness-remove {
        flex: 0 0 auto;
        background: none;
        border: 1px solid var(--border);
        border-radius: var(--radius-sm);
        color: var(--text-secondary);
        cursor: pointer;
        padding: var(--space-1) var(--space-4);
        font-size: var(--text-80);
        line-height: 1.6;
        transition: border-color 0.15s, color 0.15s;
    }
    .pf-witness-remove:hover { border-color: var(--red); color: var(--red); }


    /* ── Delete person section ─────────────────────────────────────── */

    .pf-delete-section { margin-top: var(--space-4); }
    /* Same line as .person-form-header's border-bottom, with the same
       ~16px breathing room on each side as the header/body padding gives
       it above "État civil". */
    .pf-delete-person-btn {
        margin-top: var(--space-6);
        background: none;
        border: 1px solid color-mix(in srgb, var(--red) 35%, transparent);
        border-radius: var(--radius-sm);
        color: var(--red);
        cursor: pointer;
        font-size: var(--text-85);
        padding: var(--space-3) var(--space-7);
        transition: border-color 0.15s, background 0.15s;
        width: 100%;
        text-align: center;
    }
    .pf-delete-person-btn:hover { border-color: var(--red); background: color-mix(in srgb, var(--red) 8%, transparent); }
    .pf-delete-confirm,
    .uf-child-detach-confirm {
        background: color-mix(in srgb, var(--red) 7%, transparent);
        border: 1px solid color-mix(in srgb, var(--red) 30%, transparent);
    }

    .pf-delete-confirm {
        border-radius: var(--radius);
        padding: var(--space-8);
        margin-top: var(--space-4);
    }
    .pf-delete-confirm-name {
        font-weight: 600;
        font-size: var(--text-95);
        margin: 0 0 var(--space-4);
        color: var(--text-primary);
    }
    .pf-delete-confirm-message {
        font-size: var(--text-85);
        color: var(--text-secondary);
        margin: 0 0 var(--space-7);
        line-height: 1.5;
    }
    .pf-delete-confirm-actions { display: flex; gap: var(--space-4); justify-content: flex-end; }

    /* ── Linking panel ─────────────────────────────────────────────── */

    .linking-card {
        margin-bottom: var(--space-12);
        border: 2px solid var(--orange);
    }

    .linking-panel {
        padding: var(--space-8);
        background: var(--bg-card);
        border-radius: var(--radius);
        margin-top: var(--space-6);
    }

    .linking-panel-title {
        font-size: var(--text-85);
        color: var(--text-secondary);
        margin-bottom: var(--space-6);
    }

    .linking-shortcuts {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-4);
        margin-bottom: var(--space-5);
    }
    .linking-panel-or {
        text-align: center;
        color: var(--text-secondary);
        font-size: var(--text-85);
        margin: var(--space-6) 0;
    }

    /* ── Couple modal — person blocks, children ───────────────────── */

    .uf-child-row {
        display: flex;
        align-items: center;
        gap: var(--space-5);
        padding: var(--space-4) var(--space-6);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        margin-bottom: var(--space-3);
        background: var(--bg-card);
        transition: opacity 0.15s;
    }

    .uf-child-row.pending-detach {
        opacity: 0.45;
    }

    .uf-child-avatar {
        width: 26px;
        height: 26px;
        border-radius: 50%;
        background: var(--bg-card-hover);
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: var(--text-75);
        color: var(--text-secondary);
        flex-shrink: 0;
    }

    .uf-child-info {
        flex: 1;
        display: flex;
        align-items: center;
        gap: var(--space-5);
        min-width: 0;
        flex-wrap: wrap;
        font-size: var(--text-85);
    }

    .uf-child-detach-confirm {
        border-radius: var(--radius);
        padding: var(--space-5) var(--space-6);
        margin-bottom: var(--space-3);
        font-size: var(--text-85);
    }

    .uf-child-detach-confirm p {
        margin: 0 0 var(--space-4);
        color: var(--text-secondary);
    }

    .uf-child-detach-confirm .pf-delete-confirm-actions {
        margin: 0;
    }

    /* ── Responsive: modals become full-screen drawer below 600px ── */

    @media (max-width: 600px) {
        .person-form-modal, .union-form-modal {
            width: 100vw;
            max-width: 100vw;
            max-height: 100dvh;
            height: 100dvh;
            border-radius: 0;
            position: fixed;
            bottom: 0;
            left: 0;
            right: 0;
            top: 0;
            animation: slideUpModal 0.22s ease-out;
        }

        .modal-backdrop {
            align-items: flex-end;
        }

        .union-form-header > div:first-child { min-width: 0; }
        .union-form-header h2 { overflow-wrap: anywhere; }
    }

    @keyframes slideUpModal {
        from { transform: translateY(60px); opacity: 0.6; }
        to   { transform: translateY(0);    opacity: 1; }
    }

    /* ── Search results page ─────────────────────────────────────── */

    .search-results-page {
        display: flex;
        flex-direction: column;
        height: 100%;
        overflow: hidden;
    }

    .search-results-page .sub-page-content {
        flex: 1;
        overflow-y: auto;
        max-width: 1200px;
        margin: 0 auto;
        width: 100%;
        padding: var(--space-8) var(--space-12);
    }

    .sr-count {
        font-size: var(--text-85);
        color: var(--text-muted);
        margin: 0;
    }

    /* Filters */
    .sr-filters-toggle {
        margin-bottom: var(--space-4);
    }

    .sr-chevron {
        display: inline-block;
        font-size: var(--text-65);
        margin-left: var(--space-2);
        transition: transform 0.2s;
    }

    .sr-chevron.open {
        transform: rotate(180deg);
    }

    .sr-filters {
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: var(--space-9);
        margin-bottom: var(--space-6);
    }

    .sr-filters .pf-section {
        --pf-gap-section: 18px;
    }

    .sr-filter-grid,
    .sr-relations-grid {
        display: grid;
        gap: var(--space-7) var(--space-8);
        align-items: start;
    }

    .sr-filter-grid-person {
        grid-template-columns: repeat(4, minmax(0, 1fr));
    }

    .sr-filter-grid-event,
    .sr-relations-grid {
        grid-template-columns: repeat(3, minmax(0, 1fr));
    }

    .sr-filter-group {
        min-width: 0;
        margin: 0;
    }

    .sr-filter-group input,
    .sr-filter-group select {
        width: 100%;
        min-width: 0;
    }

    .sr-filter-sex {
        grid-column: span 2;
    }

    .sr-filter-sex .pf-gender-group {
        min-height: 38px;
        align-items: stretch;
    }

    .sr-filter-sex .pf-gender-btn {
        flex: 1 1 auto;
        padding-inline: var(--space-6);
    }

    .sr-date-range {
        display: grid;
        grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
        align-items: center;
        gap: var(--space-4);
    }

    .sr-date-range input {
        width: 100%;
        min-width: 0;
        text-align: center;
    }

    .sr-date-range span {
        color: var(--text-muted);
    }

    .sr-media-filter {
        min-height: 38px;
        display: flex;
        align-items: center;
        align-self: end;
        gap: var(--space-4);
        margin: 0;
        padding: 0 var(--space-1);
        cursor: pointer;
    }

    /* Size and accent colour come from the shared checkbox rule; the flex
       basis is this row's own. */
    .pf-embedded .sr-media-filter input {
        flex: 0 0 16px;
    }

    .sr-relation-group.pf-subform {
        display: grid;
        gap: var(--space-4);
        margin: 0;
    }

    .sr-relation-group .pf-block-label {
        margin-bottom: 0;
    }

    .sr-relation-group input {
        width: 100%;
        min-width: 0;
    }

    .sr-filter-actions {
        display: flex;
        justify-content: flex-end;
        margin-top: var(--space-7);
    }

    /* The filters in force, each a chip that removes itself. */
    .sr-active-filters {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: var(--space-3);
        margin-bottom: var(--space-6);
    }

    .sr-active-filters:empty {
        display: none;
    }

    .sr-filter-chip {
        display: inline-flex;
        align-items: center;
        /* A flex item's leading space collapses: the gap keeps the removal
           cross apart from the label. */
        gap: var(--space-2);
        max-width: 100%;
        padding: 3px var(--space-5);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        background: var(--bg-card);
        color: var(--text-primary);
        font-family: inherit;
        font-size: var(--text-80);
        overflow-wrap: anywhere;
        cursor: pointer;
    }

    .sr-filter-chip:hover {
        border-color: var(--orange);
    }

    @media (max-width: 900px) {
        .sr-filter-grid-person,
        .sr-filter-grid-event,
        .sr-relations-grid {
            grid-template-columns: repeat(2, minmax(0, 1fr));
        }
    }

    @media (max-width: 640px) {
        .sr-filters { padding: var(--space-7); }
        .sr-filter-grid-person,
        .sr-filter-grid-event,
        .sr-relations-grid {
            grid-template-columns: minmax(0, 1fr);
        }
        .sr-filter-sex { grid-column: auto; }
    }

    /* Toolbar */
    .sr-toolbar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: var(--space-6);
        padding: var(--space-4) var(--space-6);
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
    }

    .sr-sort {
        display: flex;
        align-items: center;
        gap: var(--space-4);
    }

    .sr-sort label {
        font-size: var(--text-80);
        color: var(--text-muted);
        margin-bottom: 0;
    }

    .sr-sort select {
        padding: var(--space-2) var(--space-4);
        font-size: var(--text-80);
        border: 1px solid var(--border);
        border-radius: var(--radius-sm);
        background: var(--bg-deep);
        color: var(--text-primary);
    }

    /* The list / grid switch (components/view_toggle.rs). */
    .view-toggle {
        display: flex;
        gap: var(--space-2);
        flex-shrink: 0;
    }

    .view-toggle-btn {
        background: none;
        border: 1px solid var(--border);
        border-radius: var(--radius-sm);
        color: var(--text-muted);
        cursor: pointer;
        padding: var(--space-2) var(--space-4);
        font-size: var(--text-100);
    }

    .view-toggle-btn.active {
        background: var(--orange);
        color: var(--on-accent);
        border-color: var(--orange);
    }

    .view-toggle-btn:hover:not(.active) {
        background: var(--bg-card-hover);
    }

    /* ── Pager ─────────────────────────────────────────────────────
       One control for every paged list and document (components/pager.rs). */
    .pager {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: var(--space-2);
        margin-top: var(--space-10);
        padding: var(--space-6) 0;
    }

    .pager-btn,
    .pager-num {
        flex: 0 0 auto;
        min-width: 32px;
        min-height: 32px;
        padding: 5px var(--space-4);
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius-sm);
        color: var(--text-primary);
        font-size: var(--text-80);
        line-height: 1;
        text-align: center;
        cursor: pointer;
    }

    .pager-btn:hover:not(:disabled),
    .pager-num:hover:not(:disabled):not(.is-current) {
        background: var(--bg-card-hover);
    }

    .pager-btn:disabled,
    .pager-num:disabled {
        opacity: 0.4;
        cursor: not-allowed;
    }

    .pager-num.is-current {
        background: var(--orange);
        color: var(--on-accent);
        border-color: var(--orange);
    }

    /* Scrolls rather than wraps: a forty-page register must not push the
       image out of the panel to make room for its own page numbers. */
    .pager-numbers {
        display: flex;
        gap: var(--space-2);
        overflow-x: auto;
        max-width: min(520px, 60vw);
        padding: var(--space-1);
    }

    .pager-gap {
        flex: 0 0 auto;
        padding: var(--space-2) var(--space-1);
        color: var(--text-muted);
        font-size: var(--text-80);
        user-select: none;
    }

    .pager-status {
        padding: 0 var(--space-4);
        color: var(--text-muted);
        font-size: var(--text-80);
        font-variant-numeric: tabular-nums;
    }

    /* Under the media viewer's image, and under its list of relations. */
    .pager.media-pager {
        gap: var(--space-3);
        margin: 0;
        padding: var(--space-4) var(--space-6);
        border-top: 1px solid var(--border);
    }

    .pager.media-relation-pager {
        justify-content: space-between;
        gap: var(--space-4);
        margin: 0;
        padding: 0;
    }

    /* Full-page search results: override typeahead dropdown constraints */
    .search-person-results.sr-results-page {
        max-height: none;
        overflow-y: visible;
        border: none;
        border-radius: 0;
        background: transparent;
    }
    .search-person-results.sr-results-page .search-person-result {
        gap: var(--space-5);
        padding: var(--space-4) var(--space-6);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        margin-bottom: var(--space-2);
    }
    .search-person-results.sr-results-page .sp-result-photo,
    .search-person-results.sr-results-page .sp-result-portrait {
        width: 44px;
        height: 44px;
    }
    a.search-person-result {
        text-decoration: none;
        color: inherit;
        cursor: pointer;
    }

    /* ── Kinship page ──────────────────────────────────────────────
       Each path is a grid: a narrow generation column, then the first
       person's line and the other's side by side under the ancestors they
       share. Persons are the shared search row. */
    .kin-content { max-width: 1000px; }
    .kin-ends {
        display: flex;
        align-items: flex-start;
        gap: var(--space-8);
        margin-bottom: var(--space-8);
    }
    .kin-end-slot { flex: 1; min-width: 0; }
    .kin-end-label {
        margin-bottom: var(--space-3);
        font-size: var(--text-75);
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--text-muted);
    }
    .kin-swap { align-self: center; margin-top: var(--space-10); font-size: var(--text-100); }
    .kin-person {
        border: 1px solid var(--border);
        border-radius: var(--radius);
        background: var(--bg-card);
    }
    .kin-person.kin-end { border-color: var(--orange); }
    .kin-status { margin: 0 0 var(--space-6); }
    /* The summary: one row per path, the chosen one drawn below it. */
    .kin-summary {
        display: flex;
        flex-direction: column;
        gap: var(--space-1);
        padding: var(--space-3);
        margin-bottom: var(--space-8);
    }
    .kin-summary-row {
        display: grid;
        grid-template-columns: 24px minmax(0, 1fr) auto;
        column-gap: var(--space-5);
        align-items: baseline;
        padding: var(--space-4) var(--space-5);
        border: none;
        border-left: 3px solid transparent;
        border-radius: var(--radius);
        background: none;
        color: var(--text-primary);
        font-family: var(--font-sans);
        font-size: var(--text-90);
        text-align: left;
        cursor: pointer;
    }
    .kin-summary-row:hover { background: var(--bg-card-hover); }
    .kin-summary-row.active {
        border-left-color: var(--orange);
        background: color-mix(in srgb, var(--orange) 12%, transparent);
    }
    .kin-summary-title { font-weight: 600; }
    .kin-summary-gen {
        font-size: var(--text-80);
        font-variant-numeric: tabular-nums;
        color: var(--text-secondary);
        white-space: nowrap;
    }
    .kin-summary-via {
        grid-column: 2 / -1;
        font-size: var(--text-80);
        color: var(--text-muted);
    }
    .kin-path { margin-bottom: var(--space-8); }
    .kin-path-hd {
        display: flex;
        align-items: baseline;
        gap: var(--space-5);
        flex-wrap: wrap;
        margin-bottom: var(--space-6);
    }
    .kin-path-num {
        font-size: var(--text-80);
        font-weight: 600;
        color: var(--orange);
    }
    .kin-path-title {
        margin: 0;
        font-family: var(--font-heading);
        font-size: var(--text-110);
    }
    .kin-path-note, .kin-chain-hint {
        font-size: var(--text-80);
        color: var(--text-muted);
    }
    .kin-chain { margin: 0 0 var(--space-1); font-weight: 600; }
    .kin-chain-hint { margin: 0 0 var(--space-6); }
    .kin-seg {
        display: grid;
        grid-template-columns: 36px minmax(0, 1fr) minmax(0, 1fr);
        gap: var(--space-7) var(--space-8);
        align-items: center;
    }
    .kin-seg-single { grid-template-columns: 36px minmax(0, 1fr); }
    .kin-gen {
        font-size: var(--text-75);
        font-variant-numeric: tabular-nums;
        text-align: right;
        color: var(--text-muted);
    }
    .kin-top {
        grid-column: 2 / -1;
        display: flex;
        justify-content: center;
        gap: var(--space-4);
    }
    .kin-top > .kin-person { flex: 0 1 280px; min-width: 0; }
    /* The line down from the generation above. */
    .kin-cell { position: relative; }
    .kin-cell:not(:empty)::before {
        content: '';
        position: absolute;
        left: 50%;
        top: -14px;
        height: 14px;
        border-left: 2px solid var(--border);
    }
    .kin-union {
        margin: var(--space-7) 0 var(--space-7) var(--space-26);
        font-size: var(--text-80);
        color: var(--text-secondary);
    }
    @media (max-width: 640px) {
        .kin-ends { flex-direction: column; align-items: stretch; }
        .kin-swap { align-self: center; margin-top: 0; }
        .kin-seg { grid-template-columns: 28px minmax(0, 1fr) minmax(0, 1fr); gap: var(--space-6) var(--space-4); }
        .kin-seg-single { grid-template-columns: 28px minmax(0, 1fr); }
        .kin-union { margin-left: var(--space-18); }
        .kin-summary-row { grid-template-columns: 24px minmax(0, 1fr); }
        .kin-summary-gen { grid-column: 2; white-space: normal; }
        /* Two rows side by side leave ~150px each on a phone: keep the name
           and the years, which is what places a person in the chain. */
        .kin-seg .sp-result-photo,
        .kin-seg .sp-result-rel,
        .kin-seg .sp-result-meta { display: none; }
        .kin-seg .sp-result-dates { flex-wrap: wrap; gap: 0 var(--space-3); }
        .kin-seg .search-person-result { padding: var(--space-2) var(--space-3); }
    }

    /* Grid (card) view: one mini-pedigree per result */
    .sr-grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(min(340px, 100%), 1fr));
        gap: var(--space-7);
    }

    .sr-grid-card {
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        overflow: hidden;
        display: flex;
        flex-direction: column;
    }
    .sr-grid-card.male   { border-top: 3px solid color-mix(in srgb, var(--blue) 40%, transparent); }
    .sr-grid-card.female { border-top: 3px solid color-mix(in srgb, var(--pink) 40%, transparent); }

    a.sr-grid-card-hd {
        display: flex;
        align-items: baseline;
        justify-content: space-between;
        gap: var(--space-5);
        padding: var(--space-5) var(--space-7);
        text-decoration: none;
        color: inherit;
        border-bottom: 1px solid var(--border);
    }
    a.sr-grid-card-hd:hover .sp-surname,
    a.sr-grid-card-hd:hover .sp-given {
        color: var(--orange);
    }

    .sr-grid-ped {
        flex: 1;
    }
    .sr-grid-ped .mini-pedigree {
        height: 210px;
        border-radius: 0;
    }

    .sr-grid-ped-msg {
        display: flex;
        align-items: center;
        justify-content: center;
        height: 210px;
        color: var(--text-muted);
        font-size: var(--text-80);
    }

    /* ── Dictionary page ──────────────────────────────────────────── */

    .dict-tabs {
        display: flex;
        gap: var(--space-2);
        margin-bottom: var(--space-6);
        border-bottom: 1px solid var(--border);
    }

    .dict-tab {
        background: none;
        border: none;
        font-family: inherit;
        padding: var(--space-4) var(--space-7);
        font-size: var(--text-90);
        color: var(--text-muted);
        cursor: pointer;
        border-bottom: 2px solid transparent;
    }

    .dict-tab.active {
        color: var(--orange);
        border-bottom-color: var(--orange);
        font-weight: 600;
    }

    .dict-tab:hover:not(.active) {
        color: var(--text-primary);
    }

    .dict-alphabet {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--space-2);
        margin-bottom: var(--space-4);
    }

    .dict-letter-strip {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-2);
    }

    .dict-total-count {
        margin-left: auto;
        text-align: right;
        white-space: nowrap;
    }

    .dict-letter-btn {
        min-width: 26px;
        padding: var(--space-2) var(--space-3);
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius-sm);
        color: var(--text-primary);
        font-size: var(--text-80);
        cursor: pointer;
    }

    .dict-letter-btn.active {
        background: var(--orange);
        color: var(--on-accent);
        border-color: var(--orange);
    }

    .dict-letter-btn:disabled {
        opacity: 0.3;
        cursor: not-allowed;
    }

    .dict-letter-btn:hover:not(.active):not(:disabled) {
        background: var(--bg-card-hover);
    }

    .dict-src-breadcrumb {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: var(--space-2);
        margin-bottom: var(--space-5);
    }

    .dict-src-crumb {
        background: none;
        border: none;
        color: var(--text-muted);
        font-size: var(--text-80);
        cursor: pointer;
        padding: var(--space-1) var(--space-1);
    }

    .dict-src-crumb:hover {
        color: var(--orange);
    }

    .dict-src-crumb.active {
        color: var(--text-primary);
        font-weight: 600;
        cursor: default;
    }

    .dict-src-crumb-sep {
        color: var(--text-muted);
        font-size: var(--text-80);
    }

    .dict-src-summary {
        font-size: var(--text-85);
        color: var(--text-primary);
        margin-bottom: var(--space-4);
    }

    .dict-src-groups-label {
        font-size: var(--text-80);
        color: var(--text-muted);
        margin: var(--space-4) 0 var(--space-3);
    }

    .dict-filter-row {
        display: flex;
        align-items: center;
        gap: var(--space-6);
        margin-bottom: var(--space-6);
        flex-wrap: wrap;
    }

    .dict-filter-input {
        flex: 1;
        min-width: 160px;
        padding: var(--space-3) var(--space-5);
        font-size: var(--text-85);
        border: 1px solid var(--border);
        border-radius: var(--radius-sm);
        background: var(--bg-deep);
        color: var(--text-primary);
    }

    .dict-page-size {
        display: flex;
        align-items: center;
        gap: var(--space-3);
        font-size: var(--text-80);
        color: var(--text-muted);
    }

    .dict-page-size select {
        height: 32px;
        padding: 0 var(--space-4);
        font-size: var(--text-80);
        border: 1px solid var(--border);
        border-radius: var(--radius-sm);
        background: var(--bg-deep);
        color: var(--text-primary);
    }

    .dict-warning {
        background: color-mix(in srgb, var(--orange) 12%, transparent);
        border: 1px solid var(--orange);
        color: var(--orange);
        border-radius: var(--radius);
        padding: var(--space-4) var(--space-6);
        font-size: var(--text-80);
        margin-bottom: var(--space-6);
    }

    /* Media tab: the tag cloud. Each tag's size and weight are set inline
       from its count; a large vocabulary scrolls inside the cloud rather
       than pushing the grid off the screen. */
    .dict-media-cloud {
        display: flex;
        flex-wrap: wrap;
        align-items: baseline;
        gap: var(--space-3) var(--space-5);
        max-height: 14rem;
        overflow-y: auto;
        margin-bottom: var(--space-6);
    }

    .dict-media-tag {
        background: none;
        border: 1px solid transparent;
        border-radius: var(--radius-sm);
        padding: var(--space-1) var(--space-3);
        font-family: inherit;
        line-height: 1.3;
        color: var(--text-primary);
        overflow-wrap: anywhere;
        cursor: pointer;
    }

    .dict-media-tag:hover:not(.active) {
        background: var(--bg-card-hover);
    }

    .dict-media-tag.active {
        background: var(--orange);
        border-color: var(--orange);
        color: var(--on-accent);
    }

    .dict-media-tag-count {
        margin-left: var(--space-2);
        font-size: var(--text-70);
        font-weight: 400;
        vertical-align: super;
        color: var(--text-muted);
    }

    .dict-media-tag.active .dict-media-tag-count {
        color: inherit;
    }

    .dict-media-count {
        margin-left: auto;
        white-space: nowrap;
    }

    .dict-group-header {
        font-family: var(--font-heading);
        font-size: var(--text-80);
        letter-spacing: 0.05em;
        color: var(--orange);
        padding: var(--space-5) var(--space-2) var(--space-2);
    }

    .dict-list {
        display: flex;
        flex-direction: column;
        gap: var(--space-2);
    }

    .dict-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--space-6);
        padding: var(--space-4) var(--space-6);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        background: var(--bg-card);
        cursor: pointer;
    }

    .dict-row:hover .dict-row-value {
        color: var(--orange);
    }

    .dict-row-main {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: var(--space-1);
    }

    .dict-row-value {
        font-size: var(--text-90);
        color: var(--text-primary);
    }

    .dict-row-meta {
        font-size: var(--text-75);
        color: var(--text-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .dict-row-count {
        font-size: var(--text-80);
        color: var(--text-muted);
        white-space: nowrap;
    }

    .dict-row-action {
        background: none;
        border: none;
        color: var(--text-muted);
        cursor: pointer;
        font-size: var(--text-90);
        padding: var(--space-2);
    }

    .dict-row-action:hover {
        color: var(--orange);
    }

    /* Bulk surname-particle editor, opened from a family-name row. */
    /* The Dictionary's editors: a family name, a source, a repository. */
    .dict-edit-modal {
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: var(--space-10) var(--space-12) var(--space-8);
        width: min(460px, calc(100vw - 32px));
        max-height: calc(100vh - 32px);
        overflow-y: auto;
        box-shadow: var(--shadow-lg);
    }

    .dict-edit-modal.is-wide { width: min(600px, calc(100vw - 32px)); }

    .dict-edit-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--space-6);
        margin-bottom: var(--space-6);
    }

    .dict-edit-header h2 {
        font-size: var(--text-110);
        color: var(--text-primary);
    }

    .dict-particle-intro {
        color: var(--text-primary);
        font-size: var(--text-90);
    }

    .dict-particle-scope {
        color: var(--text-muted);
        font-size: var(--text-80);
        margin: var(--space-2) 0 var(--space-7);
    }

    .dict-particle-hint {
        color: var(--text-muted);
        font-size: var(--text-80);
        margin-top: var(--space-3);
    }

    .dict-particle-preview {
        margin-top: var(--space-7);
        padding: var(--space-5) var(--space-6);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        background: var(--bg-deep);
    }

    .dict-particle-preview-row {
        display: flex;
        justify-content: space-between;
        gap: var(--space-6);
        font-size: var(--text-85);
        padding: var(--space-1) 0;
    }

    .dict-particle-preview-key {
        color: var(--text-muted);
    }

    .dict-particle-preview-val {
        color: var(--text-primary);
        font-weight: 600;
    }

    .dict-pin {
        color: var(--green);
    }

    .dict-accordion {
        border: 1px solid var(--border);
        border-top: none;
        border-radius: 0 0 var(--radius) var(--radius);
        background: var(--bg-deep);
        padding: var(--space-5) var(--space-7);
        margin-top: -6px;
    }

    a.dict-accordion-item {
        display: flex;
        align-items: baseline;
        gap: var(--space-4);
        padding: var(--space-2) 0;
        color: var(--text-primary);
        text-decoration: none;
        font-size: var(--text-85);
    }

    a.dict-accordion-item:hover {
        color: var(--orange);
    }

    .dict-accordion-name {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .dict-accordion-dates {
        flex-shrink: 0;
        color: var(--text-muted);
        font-size: var(--text-80);
    }

    .dict-accordion-empty {
        color: var(--text-muted);
        font-size: var(--text-80);
        padding: var(--space-2) 0;
    }

    /* ── Import overlay (blocking spinner) ───────────────────────── */

    .import-overlay {
        position: fixed;
        inset: 0;
        z-index: 9999;
        background: color-mix(in srgb, var(--scrim) 75%, transparent);
        backdrop-filter: blur(6px);
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: var(--space-12);
    }

    .import-spinner {
        width: 48px;
        height: 48px;
        border: 4px solid var(--border);
        border-top-color: var(--orange);
        border-radius: 50%;
        animation: spin 0.8s linear infinite;
    }

    @keyframes spin {
        to { transform: rotate(360deg); }
    }

    /* ── In-button spinner (ConfirmDialog `busy`) ─────────────────── */

    .btn-spinner {
        display: inline-block;
        width: 0.85em;
        height: 0.85em;
        margin-right: 0.5em;
        vertical-align: -0.1em;
        border: 2px solid currentColor;
        /* Transparent top edge is what makes the ring read as spinning. */
        border-top-color: transparent;
        border-radius: 50%;
        animation: spin 0.7s linear infinite;
    }

    .modal-actions .btn:disabled {
        opacity: 0.6;
        cursor: not-allowed;
    }

    .import-overlay-text {
        font-family: var(--font-heading);
        font-size: var(--text-110);
        color: var(--text-primary);
        letter-spacing: 0.04em;
    }

    /* ── Media gallery (Sprint F.2) ───────────────────────────────── */

    /* One row of tiles that reflows rather than scrolls: a person with
       twenty scans should read as a contact sheet, not as a filmstrip the
       user has to drag through to know how much is there. */
    /* The tiles are the way into the documents, and at 112px a scan of a
       register was a grey rectangle — you could not tell two apart without
       opening both. The generated thumbnail is 400px on its long edge, so
       this stays well inside what the server already produces. */
    .media-grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
        gap: var(--space-7);
    }

    .media-tile {
        display: flex;
        flex-direction: column;
        gap: var(--space-3);
        min-width: 0;
    }

    .media-thumb {
        position: relative;
        aspect-ratio: 1;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        overflow: hidden;
        background: var(--bg-deep);
    }

    .media-tile.is-open .media-thumb { border-color: var(--orange); }

    /* A crop is a region of something larger, and reads as one: dashed, so it
       is not mistaken for a second photograph of the same scene. */
    .media-thumb.is-vignette { border-style: dashed; }

    .media-vignette-badge {
        position: absolute;
        left: 4px;
        bottom: 4px;
        background: color-mix(in srgb, var(--scrim) 55%, transparent);
        border-radius: var(--radius-xs);
        color: var(--text-primary);
        font-size: var(--text-70);
        line-height: 1;
        padding: 3px var(--space-2);
    }

    .media-thumb img,
    .media-thumb svg {
        width: 100%;
        height: 100%;
        /* Cover, not contain: a grid of letterboxed scans is mostly
           background, and the tile is a way in, not the document. */
        object-fit: cover;
        display: block;
    }

    .media-document-mosaic {
        display: grid;
        width: 100%;
        height: 100%;
        gap: var(--space-1);
        padding: 3px;
        box-sizing: border-box;
        background: var(--bg-deep);
    }

    /* One page is a photograph, not a mosaic of one: no gutter, so a document
       holding a single scan looks exactly like the scan. */
    .media-document-mosaic.is-1 { gap: 0; padding: 0; }
    .media-document-mosaic.is-2 { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .media-document-mosaic.is-3,
    .media-document-mosaic.is-4 {
        grid-template-columns: repeat(2, minmax(0, 1fr));
        grid-template-rows: repeat(2, minmax(0, 1fr));
    }
    .media-document-mosaic.is-3 .media-document-mosaic-cell:first-child {
        grid-row: 1 / span 2;
    }

    .media-document-mosaic-cell {
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        background: var(--bg-card);
    }

    .media-thumb .media-document-mosaic-page {
        min-width: 0;
        min-height: 0;
        object-position: top center;
    }

    .media-thumb-icon {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 100%;
        height: 100%;
    }

    /* What a PDF gets instead of a thumbnail — the server could not
       rasterise it, so the file type is the picture. */
    .media-kind {
        font-family: var(--font-heading);
        font-size: var(--text-80);
        letter-spacing: 0.08em;
        color: var(--text-muted);
        border: 1px solid var(--border);
        border-radius: var(--radius-xs);
        padding: 3px 7px;
    }

    .media-star {
        position: absolute;
        top: 4px;
        right: 5px;
        color: var(--orange);
        font-size: var(--text-95);
        text-shadow: 0 1px 3px color-mix(in srgb, var(--scrim) 60%, transparent);
        pointer-events: none;
    }

    .media-pages {
        position: absolute;
        bottom: 4px;
        left: 4px;
        font-size: var(--text-65);
        letter-spacing: 0.03em;
        background: color-mix(in srgb, var(--scrim) 62%, transparent);
        color: var(--on-accent);
        border-radius: var(--radius-xs);
        padding: 1px 5px;
        pointer-events: none;
    }

    /* Controls appear on hover and, crucially, on keyboard focus — a row of
       buttons reachable only by pointer is unreachable on a touch screen and
       invisible to a keyboard. */
    .media-tile-actions {
        position: absolute;
        inset: auto 0 0 0;
        display: flex;
        justify-content: center;
        gap: var(--space-1);
        padding: var(--space-2);
        background: linear-gradient(transparent, color-mix(in srgb, var(--scrim) 72%, transparent));
        opacity: 0;
        transition: opacity 0.15s ease;
    }

    .media-thumb:hover .media-tile-actions,
    .media-tile-actions:focus-within { opacity: 1; }

    .media-act {
        background: none;
        border: none;
        color: var(--media-caption);
        font-size: var(--text-80);
        line-height: 1;
        padding: var(--space-2) 5px;
        border-radius: var(--radius-xs);
        cursor: pointer;
        text-decoration: none;
    }

    .media-act:hover { background: color-mix(in srgb, var(--on-accent) 16%, transparent); }
    .media-act.is-on { color: var(--orange); }
    .media-act.is-danger:hover { background: var(--red); color: var(--on-accent); }
    .media-act:disabled { opacity: 0.45; cursor: default; }

    .media-confirm {
        position: absolute;
        inset: 0;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: var(--space-4);
        padding: var(--space-4);
        text-align: center;
        font-size: var(--text-70);
        background: color-mix(in srgb, var(--media-bg) 90%, transparent);
        color: var(--text-primary);
    }

    .media-confirm-actions { display: flex; gap: var(--space-3); }

    /* Under the caption in a listing: what the record is, how often used. */
    .media-footnote {
        margin-top: -4px;
        font-size: var(--text-70);
        color: var(--text-muted);
        text-align: center;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .media-caption {
        font-size: var(--text-70);
        color: var(--text-muted);
        text-align: center;
        /* One line, ellipsised: file names run long and a two-line caption
           makes neighbouring tiles sit at different heights. */
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .media-event-link {
        font-size: var(--text-70);
        font-style: italic;
        line-height: 1.35;
        text-align: center;
    }

    .media-empty {
        font-size: var(--text-80);
        color: var(--text-muted);
        padding: var(--space-4) 0;
    }

    /* ── Upload cell ──────────────────────────────────────────────── */

    .media-drop { aspect-ratio: 1; }

    .media-drop.media-upload-icon {
        width: 28px;
        height: 28px;
        aspect-ratio: auto;
        flex: 0 0 auto;
    }

    .media-upload-icon-btn {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 100%;
        height: 100%;
        padding: 0;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        background: none;
        color: var(--text-muted);
        cursor: pointer;
    }

    .media-upload-icon-btn:hover:not(:disabled) {
        border-color: var(--orange);
        color: var(--orange);
    }

    .media-drop-btn {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: var(--space-2);
        width: 100%;
        height: 100%;
        padding: var(--space-4);
        text-align: center;
        background: none;
        border: 1px dashed var(--border);
        border-radius: var(--radius);
        color: var(--text-muted);
        cursor: pointer;
        transition: border-color 0.15s ease, color 0.15s ease;
    }

    .media-drop-btn:hover:not(:disabled) {
        border-color: var(--orange);
        color: var(--orange);
    }

    .media-drop.is-dragging .media-drop-btn {
        border-color: var(--orange);
        border-style: solid;
        color: var(--orange);
        background: color-mix(in srgb, var(--orange) 8%, transparent);
    }

    .media-drop-btn:disabled { cursor: default; }
    .media-drop-icon { font-size: var(--text-130); line-height: 1; }
    .media-drop-label { font-size: var(--text-75); }
    .media-drop-hint {
        font-size: var(--text-65);
        opacity: 0.75;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        max-width: 100%;
    }

    /* ── Inline media edit panel ──────────────────────────────────── */

    .media-panel {
        margin-top: var(--space-7);
        padding: var(--space-7);
        border: 1px solid var(--orange);
        border-radius: var(--radius);
        background: var(--bg-panel);
    }

    .media-panel-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--space-5);
        margin-bottom: var(--space-2);
    }

    .media-panel-title {
        font-family: var(--font-heading);
        font-size: var(--text-90);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .media-panel-meta {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-5);
        margin-bottom: var(--space-6);
        font-size: var(--text-70);
        color: var(--text-muted);
    }

    .media-panel-section { margin-top: var(--space-6); }
    .media-panel-section > label {
        display: block;
        margin-bottom: var(--space-3);
        font-size: var(--text-70);
        color: var(--text-muted);
    }

    .media-panel-actions {
        display: flex;
        justify-content: flex-end;
        gap: var(--space-4);
        margin-top: var(--space-6);
    }

    /* The media viewer's information column leaves 272px inside its padding.
       The date widget is designed for the wider person forms, where its two
       selectors and three date fields naturally share a line. Here, make the
       first pair fill one row and leave the day/month/year triplet together
       on the next one instead of spilling it over three rows. */
    .media-panel .pf-date-calendar,
    .media-panel .pf-date-qualifier-select {
        flex: 1 1 calc(50% - 4px);
        min-width: 0;
    }

    .media-panel .pf-date-month-select {
        flex: 1 1 0;
        min-width: 0;
    }

    /* A phone is as narrow as the media panel: the same two rows, wherever
       the date input is. */
    @media (max-width: 640px) {
        .pf-date-calendar,
        .pf-date-qualifier-select {
            flex: 1 1 calc(50% - 4px);
            min-width: 0;
        }

        .pf-date-month-select {
            flex: 1 1 0;
            min-width: 0;
        }
    }

    /* ── Image cropper ────────────────────────────────────────────── */

    .cropper-backdrop {
        position: fixed;
        inset: 0;
        z-index: 1200;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: var(--space-12);
        background: color-mix(in srgb, var(--media-panel) 78%, transparent);
    }

    .cropper-panel {
        display: flex;
        flex-direction: column;
        max-width: min(1000px, 100%);
        max-height: 100%;
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        overflow: hidden;
    }

    .document-form-modal {
        display: flex;
        flex-direction: column;
        width: min(1100px, 100%);
        max-height: 100%;
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        overflow: hidden;
    }

    .document-form-body {
        min-height: 0;
        padding: var(--space-8);
        overflow-y: auto;
    }

    .cropper-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--space-6);
        padding: var(--space-5) var(--space-7);
        border-bottom: 1px solid var(--border);
    }

    .cropper-title {
        font-family: var(--font-heading);
        font-size: var(--text-90);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .cropper-close {
        background: none;
        border: none;
        color: var(--text-muted);
        font-size: var(--text-130);
        line-height: 1;
        cursor: pointer;
        padding: 0 var(--space-2);
    }
    .cropper-close:hover { color: var(--text-primary); }

    /* The drag surface. `position: relative` is load-bearing: every overlay
       rectangle is positioned against this box, and the coordinates the
       handlers read are relative to it. */
    .cropper-stage {
        position: relative;
        flex: 1;
        min-height: 0;
        overflow: auto;
        background: var(--bg-deep);
        cursor: crosshair;
        /* Without this the engine starts its own text/image drag partway
           through, which cancels the crop. */
        user-select: none;
    }

    .cropper-image {
        display: block;
        max-width: 100%;
        max-height: 62vh;
        margin: 0 auto;
    }

    .cropper-selection {
        position: absolute;
        border: 2px solid var(--orange);
        background: color-mix(in srgb, var(--orange) 14%, transparent);
        pointer-events: none;
    }

    /* Crops already recorded, so the user can see what is covered while
       drawing the next one. Dashed and muted so the live selection stays
       the thing the eye goes to. */
    .cropper-existing {
        position: absolute;
        border: 1px dashed color-mix(in srgb, var(--media-frame) 65%, transparent);
        background: color-mix(in srgb, var(--media-frame) 6%, transparent);
        pointer-events: none;
    }

    .cropper-foot {
        display: flex;
        flex-direction: column;
        gap: var(--space-5);
        padding: var(--space-6) var(--space-7);
        border-top: 1px solid var(--border);
    }

    .cropper-status {
        font-size: var(--text-75);
        color: var(--text-muted);
    }

    .cropper-fields {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-6);
    }
    .cropper-fields .form-group { flex: 1 1 200px; margin: 0; }

    .cropper-actions {
        display: flex;
        justify-content: flex-end;
        gap: var(--space-4);
    }

    .cropper-empty {
        padding: var(--space-12);
        font-size: var(--text-85);
        color: var(--text-muted);
    }

    /* ── Media: kinds, remote, viewer (Sprint F.3) ────────────────── */

    .media-thumb[role="button"] { cursor: pointer; }

    .media-thumb-icon {
        flex-direction: column;
        gap: 5px;
    }

    .media-glyph { font-size: var(--text-150); line-height: 1; }
    .media-glyph-large { font-size: var(--text-300); line-height: 1; }

    /* A media whose bytes are somebody else's. Marked, because a broken tile
       on a remote file means their server, not ours, and the reader needs to
       be able to tell. */
    .media-remote {
        position: absolute;
        top: 4px;
        left: 5px;
        font-size: var(--text-70);
        opacity: 0.85;
        text-shadow: 0 1px 3px color-mix(in srgb, var(--scrim) 60%, transparent);
        pointer-events: none;
    }

    .media-events {
        display: flex;
        flex-direction: column;
        gap: var(--space-2);
        max-height: 180px;
        overflow-y: auto;
    }

    .media-event-row {
        display: flex;
        align-items: center;
        gap: var(--space-4);
        font-size: var(--text-75);
        cursor: pointer;
    }

    .media-event-row input { flex: 0 0 auto; }

    .media-viewer {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        overflow: hidden;
        font-family: var(--font-sans);
    }

    /* Image and facts side by side: what is written about a scan is most of
       why it is worth opening, and it used to live only inside an edit form
       a reader on a profile page never sees. */
    .media-viewer-body {
        display: flex;
        flex: 1;
        min-height: 0;
    }

    .media-viewer-main {
        display: flex;
        flex-direction: column;
        flex: 1;
        min-width: 0;
        min-height: 0;
        position: relative;
    }

    .media-viewer-aside {
        flex: 0 0 300px;
        min-width: 0;
        border-right: 1px solid var(--border);
        overflow-y: auto;
        padding: var(--space-6);
        background: var(--bg-card);
    }

    .media-viewer-aside label,
    .media-viewer-aside input,
    .media-viewer-aside textarea,
    .media-viewer-aside select,
    .media-viewer-aside button {
        font-family: var(--font-sans);
    }

    /* Below the fold on a narrow screen: the document comes first, and a
       300px column beside a phone-width image leaves neither readable. */
    @media (max-width: 900px) {
        /* Stacked, the document leads and the facts follow it: on a phone the
           scan is what the reader opened, and a column above it would push it
           off the screen. */
        .media-viewer-body { flex-direction: column-reverse; }
        .media-viewer-aside {
            flex: 0 0 auto;
            max-height: 40vh;
            border-right: none;
            border-top: 1px solid var(--border);
        }
    }

    .media-facts {
        display: flex;
        flex-direction: column;
        font-family: var(--font-sans);
    }

    .media-facts > .media-fact {
        display: grid;
        grid-template-columns: minmax(76px, 30%) minmax(0, 1fr);
        align-items: start;
        gap: var(--space-4);
        margin: 0;
        padding: var(--space-2) 0;
    }

    .media-facts > .media-fact > label {
        margin: 0;
        color: var(--text-secondary);
        font-family: var(--font-sans);
        font-size: var(--text-70);
        font-weight: 600;
        line-height: 1.4;
        overflow-wrap: anywhere;
        text-transform: none;
        letter-spacing: normal;
    }

    .media-facts > .media-fact.is-relations {
        grid-template-columns: minmax(0, 1fr);
        gap: var(--space-4);
        margin-top: var(--space-4);
        padding-top: var(--space-4);
        border-top: 1px solid var(--border);
    }

    .media-fact-value {
        font-family: var(--font-sans);
        color: var(--text-primary);
        font-size: var(--text-80);
        line-height: 1.4;
        overflow-wrap: anywhere;
        white-space: pre-wrap;
    }

    /* An unset field is shown, not hidden: it says the fact can be recorded
       and has not been, which an absent field cannot say. */
    .media-fact-value.is-empty { color: var(--text-muted); }

    .media-fact.is-prose .media-fact-value { line-height: 1.5; }

    .media-relations {
        display: grid;
        grid-template-columns: minmax(0, 1fr);
        gap: var(--space-4);
        min-width: 0;
    }

    .media-relation-list {
        display: flex;
        max-height: 192px;
        min-width: 0;
        flex-direction: column;
        gap: var(--space-4);
        overflow: hidden;
    }

    .media-relation-list > .media-vignette-item,
    .media-relation-list > .media-identification,
    .media-relation-list > .media-identification > .media-vignette-item {
        height: 32px;
        min-height: 32px;
    }

    .media-relations.is-paged .media-relation-list { height: 192px; }

    .media-viewer :is(button, a):focus-visible {
        outline: 2px solid var(--orange);
        outline-offset: 2px;
    }

    .media-attachment-couple {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--text-secondary);
    }

    .media-relation-scope {
        flex: 0 0 auto;
        padding: 1px var(--space-2);
        border: 1px solid var(--border);
        border-radius: var(--radius-sm);
        color: var(--text-secondary);
        font-size: var(--text-65);
        line-height: 1.2;
    }

    .media-fact-tags {
        display: flex;
        flex-wrap: wrap;
        align-items: flex-start;
        gap: var(--space-2);
    }

    .media-fact-tag {
        font-family: var(--font-sans);
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius-lg);
        color: var(--text-secondary);
        font-size: var(--text-75);
        padding: var(--space-1) var(--space-4);
    }

    .media-tags-editor { margin-bottom: var(--space-8); }
    .media-tag-form { margin-bottom: var(--space-5); }
    .media-tag-form .form-group { margin-bottom: var(--space-5); }
    .media-edit-tags { min-height: 24px; }

    .media-fact-tag.is-editable {
        display: inline-flex;
        align-items: center;
        gap: var(--space-2);
        padding-right: 3px;
    }

    .media-tag-remove {
        width: 18px;
        height: 18px;
        padding: 0;
        border: none;
        border-radius: 50%;
        background: transparent;
        color: var(--text-muted);
        font-family: var(--font-sans);
        font-size: var(--text-95);
        line-height: 1;
        cursor: pointer;
    }
    .media-tag-remove:hover { background: var(--bg-card-hover); color: var(--danger-text); }

    .media-fact-tech {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-3);
        border-top: 1px solid var(--border);
        padding-top: var(--space-3);
        color: var(--text-secondary);
        font-size: var(--text-70);
    }

    .media-vignette-item {
        display: flex;
        align-items: center;
        gap: 7px;
        min-width: 0;
        color: var(--text-primary);
        font-size: var(--text-80);
    }

    .media-identification-target {
        display: flex;
        align-items: center;
        min-width: 0;
        flex: 1;
        gap: 7px;
        padding: 0;
        border: 0;
        background: none;
        color: inherit;
        cursor: default;
        font: inherit;
        text-align: left;
    }

    .media-identification { min-width: 0; }

    .media-identification-person {
        min-width: 0;
        overflow: hidden;
        color: inherit;
        text-decoration: none;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .media-identification-person:hover,
    .media-identification-person:focus-visible { color: var(--orange); }

    .media-vignette-thumbnail {
        display: block;
        width: 36px;
        height: 28px;
        flex: 0 0 auto;
        border: 1px solid var(--border);
        object-fit: cover;
    }

    .media-identification-delete {
        width: 32px;
        height: 32px;
        flex: 0 0 auto;
        margin-left: auto;
        padding: 0;
        border: none;
        background: none;
        color: var(--text-muted);
        cursor: pointer;
        font: inherit;
        font-size: var(--text-130);
        line-height: 1;
    }

    .media-identification-delete:hover,
    .media-identification-delete:focus-visible { color: var(--orange-light); }

    .media-identification-delete:disabled { cursor: default; opacity: 0.55; }

    .media-facts-actions {
        display: flex;
        align-items: center;
        justify-content: center;
        flex-wrap: wrap;
        gap: var(--space-4);
        margin-top: var(--space-4);
    }

    .media-facts-actions .media-facts-edit,
    .media-facts-actions .media-facts-delete {
        width: auto;
        margin: 0;
        white-space: nowrap;
    }

    /* Inside the viewer the panel is the column, not a card floating in one. */
    .media-panel.is-embedded {
        background: none;
        border: none;
        border-radius: 0;
        margin: 0;
        padding: 0;
    }

    .media-panel.is-embedded .media-panel-actions { justify-content: center; }

    .media-viewer-stage {
        flex: 1;
        min-height: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: var(--space-6);
        overflow: auto;
        background: var(--bg-deep);
    }

    /* The viewer borrows the pedigree's compact controls but keeps them in a
       horizontal row above the document, where they never obscure a scan. */
    .media-viewer-controls {
        display: flex;
        align-items: center;
        justify-content: center;
        flex-wrap: wrap;
        gap: var(--space-2);
        padding: var(--space-3) var(--space-6) 0;
    }

    .media-viewer-controls .isb-btn { width: 30px; height: 30px; }

    .media-relation-menu-button { margin-left: var(--space-4); }

    .context-menu-media-relations { transform: translateX(-50%); }

    .media-attachment-picker {
        width: min(620px, calc(100% - 24px));
        max-height: 45%;
        margin: var(--space-4) auto 0;
        padding: var(--space-5);
        overflow-y: auto;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        background: var(--bg-card);
        box-shadow: var(--shadow-md);
        z-index: 2;
    }

    .media-attachment-picker-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: var(--space-4);
    }

    .media-family-choices {
        display: grid;
        gap: var(--space-3);
    }

    .media-family-choices .btn {
        justify-content: flex-start;
        width: 100%;
    }

    .media-attachment-notice {
        margin: var(--space-4) var(--space-6) 0;
        color: var(--green);
        text-align: center;
        font-size: var(--text-85);
    }

    .media-viewer-stage.is-image { cursor: grab; }
    .media-viewer-stage.is-image.is-dragging { cursor: grabbing; user-select: none; }
    .media-viewer-stage .media-viewer-static-image {
        pointer-events: none;
        user-select: none;
        -webkit-user-drag: none;
    }

     /* Stop centring only on an axis that actually overflows. Otherwise a
         portrait that grows beyond the stage height needlessly jumps left. */
     .media-viewer-stage.is-overflow-x { justify-content: flex-start; }
     .media-viewer-stage.is-overflow-y { align-items: flex-start; }

    /* Contain, not cover: this is the view where the document is the point,
       so nothing may be cropped out of it. The inline `width` a zoom level
       sets overrides both maxima. */
    .media-viewer-image {
        max-width: 100%;
        max-height: 100%;
        object-fit: contain;
    }

    .media-viewer-stage.is-zoomed .media-viewer-image { flex: 0 0 auto; }

    .media-viewer-image-frame {
        position: relative;
        display: inline-block;
        line-height: 0;
    }

    .media-viewer-vignette {
        position: absolute;
        box-sizing: border-box;
        border: 2px solid color-mix(in srgb, var(--media-frame) 95%, transparent);
        background: color-mix(in srgb, var(--media-tint) 10%, transparent);
        box-shadow: 0 0 0 1px color-mix(in srgb, var(--scrim) 45%, transparent);
        opacity: 0;
        pointer-events: auto;
    }

    .media-viewer-vignette:hover,
    .media-viewer-vignette.is-active { opacity: 1; }

    .media-viewer-vignette-label {
        position: absolute;
        top: 100%;
        left: 0;
        max-width: 180px;
        overflow: hidden;
        padding: var(--space-1) 5px;
        background: color-mix(in srgb, var(--scrim) 72%, transparent);
        color: var(--on-accent);
        font-family: var(--font-sans);
        font-size: var(--text-70);
        line-height: 1.2;
        text-overflow: ellipsis;
        white-space: normal;
    }

    .media-viewer-vignette-surname,
    .media-viewer-vignette-given { display: block; }

    .media-viewer-audio { width: min(520px, 100%); }

    .media-viewer-fallback {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: var(--space-5);
        padding: var(--space-16);
        text-align: center;
        color: var(--text-muted);
        font-size: var(--text-85);
    }

    .media-viewer-path {
        font-family: var(--font-sans);
        font-size: var(--text-70);
        word-break: break-all;
        max-width: 40ch;
    }

    .media-viewer .cropper-foot {
        padding: var(--space-4) var(--space-6);
        max-height: 30vh;
        overflow-y: auto;
        flex-shrink: 0;
    }

    .media-viewer .cropper-actions {
        flex-wrap: wrap;
        align-items: center;
    }

    .media-viewer .cropper-actions .btn {
        min-height: 32px;
        max-width: 100%;
        font: 600 0.78rem/1.4 var(--font-sans);
        white-space: normal;
        text-align: center;
    }

    .media-download {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: var(--space-4);
        text-decoration: none;
    }

    .media-download svg { flex: 0 0 auto; }

    .media-viewer .cropper-actions .error-msg {
        flex-basis: 100%;
        margin: 0;
    }

    /* ── Multi-page documents ─────────────────────────────────────── */

    .doc-pages {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(88px, 1fr));
        gap: var(--space-4);
    }

    .doc-page {
        position: relative;
        display: flex;
        flex-direction: column;
        gap: var(--space-2);
    }

    .doc-page-thumb {
        aspect-ratio: 3 / 4;
        display: flex;
        align-items: center;
        justify-content: center;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        overflow: hidden;
        background: var(--bg-deep);
    }

    .doc-page-thumb img { width: 100%; height: 100%; object-fit: cover; }

    .doc-page-number {
        position: absolute;
        top: 3px;
        left: 4px;
        z-index: 1;
        font-size: var(--text-65);
        padding: 0 5px;
        border-radius: var(--radius-xs);
        background: color-mix(in srgb, var(--scrim) 66%, transparent);
        color: var(--on-accent);
    }

    .doc-page-actions { display: flex; justify-content: center; gap: var(--space-1); }

    /* A page that has not been written yet has no thumbnail to recognise it
       by, so the name is shown. Clipped to the cell rather than wrapped: a
       scanner's file names are long and alike, and three wrapped lines of
       "IMG_20240712_14" push every following page out of the grid row. */
    .doc-page-name {
        font-size: var(--text-70);
        color: var(--text-secondary);
        text-align: center;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    /* The document's own upload cell sits in the same grid as its pages, so
       "add a page" is the cell after the last one. */
    .doc-pages .media-drop { aspect-ratio: 3 / 4; }

    /* The address field under that grid, shown while a link cell or a page's
       pencil has it open. */
    .doc-page-url {
        display: flex;
        gap: var(--space-4);
        align-items: center;
        margin-top: var(--space-4);
    }

    .doc-page-url input { flex: 1; min-width: 0; }

    /* The link cell stays marked while the field below belongs to it, so the
       two read as one control rather than as a button and an unrelated row. */
    .media-drop.is-open { border-color: var(--orange); }

    .media-pager-count { font-size: var(--text-75); color: var(--text-muted); }

    /* ── Event evidence on the profile timeline ───────────────────── */

    .pd-ev-evidence {
        margin-top: var(--space-3);
    }

    .pd-ev-evidence .media-grid-compact {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-3);
    }

    .pd-ev-evidence .media-grid-compact .media-tile {
        width: 44px;
        flex: 0 0 44px;
        gap: 0;
    }

    .pd-ev-evidence .media-grid-compact .media-thumb {
        width: 44px;
        height: 44px;
        border-radius: var(--radius-xs);
    }

    .media-grid-compact .media-caption,
    .media-grid-compact .media-event-link {
        display: none;
    }

    /* ── Scrollbar ──────────────────────────────────────────────────
       Drawn from the text colours, not from --border. A 6px thumb in the
       border colour on the page background is a pale line on a pale field:
       on the light palette it was invisible, and a scrollbar nobody can see
       reads as a page that does not scroll. Wide enough to grab, and given
       a track so the trough itself shows there is somewhere to go. */

    ::-webkit-scrollbar { width: 10px; height: 10px; }
    ::-webkit-scrollbar-track { background: var(--bg-panel); }
    ::-webkit-scrollbar-thumb {
        background: var(--text-muted);
        border-radius: var(--radius-sm);
        border: 2px solid var(--bg-panel);
        background-clip: padding-box;
    }
    ::-webkit-scrollbar-thumb:hover { background: var(--text-secondary); }
    ::-webkit-scrollbar-corner { background: var(--bg-panel); }

    /* The standard properties, for engines that ignore the prefixed ones. */
    * { scrollbar-width: thin; scrollbar-color: var(--text-muted) var(--bg-panel); }

    /* ── Responsive ───────────────────────────────────────────────── */

    @media (max-width: 1080px) {
        .cp-grid,
        .cp-bar { grid-template-columns: minmax(0, 1fr); }
        .cp-bar { gap: var(--space-6); }
        .cp-ring { display: none; }
    }

    @media (max-width: 900px) {
        .page-header {
            flex-direction: column;
            gap: var(--space-7);
        }

        .pd-header-actions {
            width: 100%;
            flex-direction: row;
            align-items: center;
            justify-content: space-between;
            gap: var(--space-6);
        }

        .pd-header-sosa {
            min-height: 0;
            align-items: center;
            justify-content: flex-start;
        }

    }

    @media (max-width: 640px) {
        .app-nav { padding: 0 var(--space-8); }
        .sub-page-content { padding: var(--space-8) var(--space-6); }
        .td-topbar { padding: var(--space-5) var(--space-6); }
        .td-bc { gap: var(--space-2); }
        .td-bc-link { max-width: clamp(36px, 22vw, 140px); }
        .td-bc-current { max-width: clamp(76px, 24vw, 96px); }
        .td-search-group input.td-search-input { width: clamp(72px, 22vw, 110px); }
        .page-header {
            padding: var(--space-7);
            margin-bottom: var(--space-8);
        }
        .pd-header-left {
            display: grid;
            grid-template-columns: 64px minmax(0, 1fr);
            gap: var(--space-4) var(--space-6);
            align-items: start;
        }
        .pd-avatar {
            grid-column: 1;
            grid-row: 1;
            width: 64px;
            height: 64px;
        }
        .pd-header-main { display: contents; }
        .pd-header-top {
            grid-column: 2;
            min-width: 0;
            align-self: center;
        }
        .page-header h1 {
            font-size: var(--text-120);
            line-height: 1.3;
            overflow-wrap: break-word;
        }
        .pd-alt-names {
            grid-column: 2;
            margin-top: 0;
        }
        .pd-vitals {
            grid-column: 1 / -1;
            width: 100%;
            margin-top: var(--space-1);
            font-size: var(--text-85);
            line-height: 1.45;
        }
        .pd-header-actions {
            padding-top: var(--space-5);
            border-top: 1px solid var(--border);
        }
        .pd-family-card { padding: var(--space-7); }
        .pd-family-prose,
        .pd-union-line,
        .pd-sib-group-head {
            font-size: var(--text-90);
            line-height: 1.5;
        }
        .pd-family-card .pd-person-chip {
            max-width: 100%;
            align-items: baseline;
            white-space: normal;
            vertical-align: middle;
        }
        .pd-family-card .pd-person-identity {
            max-width: 100%;
            align-items: baseline;
        }
        .pd-family-card .pd-person-link {
            min-width: 0;
            overflow-wrap: anywhere;
            border-bottom: none;
            text-decoration: underline;
            text-decoration-color: var(--orange-light);
            text-underline-offset: 2px;
        }
        .pd-family-card .pd-children .pd-person-chip {
            display: flex;
            flex-direction: column;
            align-items: flex-start;
            gap: 1px;
        }
        .pd-family-card .pd-children .pd-person-years {
            margin-left: var(--space-8);
        }
        .pd-family-card .pd-children { padding-left: 0; }
        .pd-family-card .pd-children li { padding-left: var(--space-6); }
        /* Tabs that do not fit scroll sideways rather than squeeze into
           each other's labels. */
        .dict-tabs {
            gap: 0;
            overflow-x: auto;
            scrollbar-width: none;
        }
        .dict-tab {
            flex: 1 0 auto;
            padding: var(--space-4) 5px;
            font-size: var(--text-80);
            white-space: nowrap;
        }
        .dict-alphabet {
            flex-direction: column;
            align-items: stretch;
            gap: var(--space-4);
        }
        .dict-letter-strip {
            flex: 0 0 auto;
            width: 100%;
            min-width: 0;
            flex-wrap: wrap;
        }
        .dict-letter-btn { flex: 0 0 auto; }
        .dict-total-count {
            order: -1;
            align-self: flex-end;
            flex: 0 0 auto;
            margin-left: 0;
        }
        /* A list's pager keeps its step buttons; the document viewer keeps
           its numbers, which scroll, since registers are cited by page. */
        .pager:not(.media-pager) .pager-numbers { display: none; }
        .media-grid { grid-template-columns: repeat(auto-fill, minmax(120px, 1fr)); }
        .cropper-backdrop { padding: 0; }
        .cropper-panel { max-height: 100vh; border-radius: 0; }
        /* Controls that only appear on hover are unreachable by touch. */
        .media-tile-actions { opacity: 1; }
    }

    /* ── Import modal (file + Geneanet wizard) ───────────────────── */

    .import-modal {
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        box-shadow: var(--shadow-lg);
        width: min(820px, 94vw);
        max-height: 90vh;
        display: flex;
        flex-direction: column;
    }

    .import-modal-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--space-6);
        padding: var(--space-9) var(--space-11) var(--space-7);
        border-bottom: 1px solid var(--border);
    }

    .import-modal-header h2 {
        font-family: var(--font-heading);
        font-size: var(--text-110);
        color: var(--text-primary);
        margin: 0;
    }

    .import-tabs {
        display: flex;
        gap: var(--space-2);
        padding: var(--space-5) var(--space-11) 0;
        border-bottom: 1px solid var(--border);
    }

    .import-tab {
        background: none;
        border: none;
        border-bottom: 2px solid transparent;
        color: var(--text-muted);
        font-family: var(--font-sans);
        font-size: var(--text-85);
        padding: var(--space-4) var(--space-7);
        cursor: pointer;
        transition: color 0.15s, border-color 0.15s;
    }

    .import-tab:hover { color: var(--text-secondary); }

    .import-tab.is-active {
        color: var(--orange);
        border-bottom-color: var(--orange);
    }

    /* The one scroll container: the header and tabs stay put while five
       steps of instructions move under them. */
    .import-modal-body {
        padding: var(--space-10) var(--space-11) var(--space-11);
        overflow-y: auto;
    }

    /* ── File tab ─────────────────────────────────────────────────── */

    .import-drop {
        position: relative;
        border: 2px dashed var(--border);
        border-radius: var(--radius);
        padding: var(--space-17) var(--space-10);
        text-align: center;
        cursor: pointer;
        transition: border-color 0.15s, background 0.15s;
    }

    .import-file-input {
        position: absolute;
        inset: 0;
        z-index: 1;
        width: 100%;
        height: 100%;
        opacity: 0;
        cursor: pointer;
    }

    .import-file-input:disabled { cursor: default; }

    .import-drop:hover,
    .import-drop.is-dragging {
        border-color: var(--orange);
        background: color-mix(in srgb, var(--orange) 6%, transparent);
    }

    .import-drop-icon { font-size: 1.9rem; line-height: 1; margin-bottom: var(--space-5); }
    .import-drop-label { color: var(--text-primary); font-size: var(--text-90); }
    .import-drop-name {
        color: var(--text-primary);
        font-size: var(--text-90);
        font-weight: 600;
        /* A long export name truncates from the middle in the summary lines,
           but here it has the width to sit whole. */
        word-break: break-all;
    }
    .import-drop-hint {
        color: var(--text-muted);
        font-size: var(--text-80);
        margin-top: var(--space-3);
    }

    /* ── Shared: stat row, result, warnings ───────────────────────── */

    .import-stats {
        display: grid;
        grid-template-columns: repeat(4, 1fr);
        gap: var(--space-5);
        margin: var(--space-8) 0;
    }

    .import-stat {
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: var(--space-6) var(--space-5);
        text-align: center;
    }

    .import-stat-value {
        font-family: var(--font-heading);
        font-size: var(--text-130);
        color: var(--orange);
        line-height: 1.1;
    }

    .import-stat-label {
        color: var(--text-muted);
        font-size: var(--text-70);
        margin-top: var(--space-2);
    }

    .import-done { text-align: center; padding: var(--space-4) 0; }
    .import-done-icon {
        font-size: var(--text-200);
        color: var(--green-light);
        line-height: 1;
    }
    .import-done h3 {
        font-family: var(--font-heading);
        color: var(--text-primary);
        margin: var(--space-4) 0 var(--space-2);
    }

    .import-warnings {
        text-align: left;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: var(--space-5) var(--space-7);
        margin-top: var(--space-6);
    }
    .import-warnings summary {
        cursor: pointer;
        color: var(--text-secondary);
        font-size: var(--text-80);
    }
    .import-warnings ul {
        margin: var(--space-5) 0 0 var(--space-9);
        max-height: 220px;
        overflow-y: auto;
        color: var(--text-muted);
        font-size: var(--text-80);
    }
    .import-warnings li { margin-bottom: var(--space-2); }

    /* ── Geneanet steps ───────────────────────────────────────────── */

    .gn-steps { display: flex; flex-direction: column; gap: var(--space-4); }

    .gn-step {
        border: 1px solid var(--border);
        border-radius: var(--radius);
        overflow: hidden;
    }

    .gn-step.is-open { border-color: var(--orange); }

    /* Not-yet-reachable steps stay visible so the whole journey is legible
       from the first second — dimmed rather than hidden. */
    .gn-step.is-dim { opacity: 0.45; }

    /* A collapsed step is a button: Enter and Space reopen it. */
    .gn-step-head {
        display: flex;
        align-items: center;
        gap: var(--space-5);
        width: 100%;
        background: none;
        border: none;
        padding: var(--space-6) var(--space-7);
        cursor: pointer;
        text-align: left;
        font-family: var(--font-sans);
    }

    .gn-step-head:disabled { cursor: default; }

    .gn-step-mark {
        flex: 0 0 auto;
        width: 22px;
        height: 22px;
        border-radius: 50%;
        border: 1px solid var(--border);
        color: var(--text-muted);
        font-size: var(--text-75);
        display: flex;
        align-items: center;
        justify-content: center;
    }

    .gn-step-mark.is-done {
        background: var(--green-accent);
        border-color: var(--green-accent);
        color: var(--on-accent);
    }

    .gn-step-title {
        color: var(--text-primary);
        font-size: var(--text-90);
        flex: 0 0 auto;
    }

    /* The receipt of a settled step. Truncates from the end of the line, but
       the counts sit last so they survive — the file name is the part with
       room to spare. */
    .gn-step-summary {
        color: var(--text-muted);
        font-size: var(--text-80);
        flex: 1 1 auto;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gn-step-edit {
        margin-left: auto;
        color: var(--orange);
        font-size: var(--text-75);
        flex: 0 0 auto;
    }

    .gn-step-body {
        padding: var(--space-2) var(--space-7) var(--space-8) var(--space-23);
        border-top: 1px solid var(--border);
    }

    .gn-lead {
        color: var(--text-secondary);
        font-size: var(--text-85);
        line-height: 1.55;
        margin: var(--space-6) 0;
    }

    .gn-note {
        color: var(--text-muted);
        font-size: var(--text-80);
        line-height: 1.5;
        margin: var(--space-4) 0;
    }

    .gn-howto {
        margin: var(--space-5) 0 var(--space-6) var(--space-9);
        color: var(--text-secondary);
        font-size: var(--text-80);
        line-height: 1.7;
    }

    .gn-aside {
        border-top: 1px solid var(--border);
        padding-top: var(--space-5);
        margin: var(--space-6) 0;
    }
    .gn-aside summary {
        cursor: pointer;
        color: var(--orange);
        font-size: var(--text-80);
    }
    .gn-aside p {
        color: var(--text-muted);
        font-size: var(--text-80);
        line-height: 1.55;
        margin-top: var(--space-4);
    }

    /* The fieldset stays a block and the options get their own flex box: a
       `legend` is lifted out of its parent's layout by the engine, so a
       fieldset that is itself the flex container lays out one child fewer
       than it appears to. */
    .gn-choice {
        border: 0;
        margin: var(--space-7) 0 var(--space-2);
        padding: 0;
    }

    .gn-choice-opts {
        display: flex;
        flex-direction: column;
        gap: var(--space-4);
    }

    .gn-choice-legend {
        color: var(--text-primary);
        font-size: var(--text-85);
        font-weight: 600;
        padding: 0;
        margin-bottom: var(--space-1);
    }

    /* Grid rather than flex: the radio gets a column of its own width, so a
       long description cannot push it about. */
    .gn-choice-opt {
        display: grid;
        grid-template-columns: 16px 1fr;
        column-gap: var(--space-5);
        align-items: start;
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: var(--space-5) var(--space-6);
        margin: 0;
        cursor: pointer;
    }

    .gn-choice-opt:hover {
        border-color: var(--orange);
    }

    .gn-choice-opt.is-on {
        border-color: var(--orange);
        background: color-mix(in srgb, var(--orange) 8%, transparent);
    }

    /* Level with the first line of the option's name, not with the top of the
       box it sits in. */
    .gn-choice-opt input {
        margin-top: var(--space-1);
    }

    .gn-choice-text {
        display: flex;
        flex-direction: column;
        gap: 3px;
    }

    .gn-choice-name {
        color: var(--text-primary);
        font-size: var(--text-85);
        font-weight: 600;
    }

    .gn-choice-why {
        color: var(--text-muted);
        font-size: var(--text-80);
        line-height: 1.5;
    }

    .gn-warn-box {
        background: color-mix(in srgb, var(--orange) 8%, transparent);
        border-left: 3px solid var(--orange);
        border-radius: var(--radius-sm);
        padding: var(--space-5) var(--space-7);
        color: var(--text-secondary);
        font-size: var(--text-80);
        line-height: 1.5;
        margin: var(--space-6) 0;
    }

    .gn-desktop-only {
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: var(--space-7) var(--space-8);
        margin: var(--space-6) 0;
    }
    .gn-desktop-only strong {
        color: var(--text-primary);
        font-size: var(--text-85);
        display: block;
        margin-bottom: var(--space-3);
    }
    .gn-desktop-only p {
        color: var(--text-muted);
        font-size: var(--text-80);
        line-height: 1.55;
        margin: 0;
    }

    /* ── Archive list ─────────────────────────────────────────────── */

    .gn-archive-list {
        list-style: none;
        margin: var(--space-6) 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: var(--space-3);
    }

    .gn-archive-list li {
        display: flex;
        align-items: center;
        gap: var(--space-5);
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: var(--space-4) var(--space-6);
    }

    .gn-archive-name {
        color: var(--text-primary);
        font-size: var(--text-80);
        flex: 1 1 auto;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gn-archive-count {
        color: var(--text-muted);
        font-size: var(--text-75);
        flex: 0 0 auto;
    }

    .gn-archive-remove {
        background: none;
        border: none;
        color: var(--text-muted);
        cursor: pointer;
        font-size: var(--text-85);
        padding: var(--space-1) var(--space-2);
        flex: 0 0 auto;
    }
    .gn-archive-remove:hover { color: var(--red); }

    /* ── Progress ─────────────────────────────────────────────────── */

    .gn-progress-block { margin: var(--space-7) 0; }

    .gn-progress-label {
        color: var(--text-secondary);
        font-size: var(--text-80);
        margin-bottom: var(--space-4);
    }

    .gn-progress {
        height: 8px;
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius-pill);
        overflow: hidden;
    }

    .gn-progress-fill {
        height: 100%;
        background: linear-gradient(90deg, var(--orange), var(--orange-light));
        transition: width 0.25s ease;
    }

    /* Neither bulk endpoint reports a total, so a bar that cannot know how
       far along it is says so instead of inventing a percentage. */
    .gn-progress-fill.is-indeterminate {
        width: 35%;
        animation: gn-slide 1.3s ease-in-out infinite;
    }

    @keyframes gn-slide {
        0%   { margin-left: -35%; }
        100% { margin-left: 100%; }
    }

    .gn-progress-count {
        color: var(--text-muted);
        font-size: var(--text-75);
        margin-top: var(--space-3);
    }

    /* ── Findings list: step 4's preview, and the Geneanet result ──── */

    .gn-findings {
        list-style: none;
        margin: var(--space-7) 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: var(--space-4);
        /* The result screen centres its text, and a centred line whose
           marker is pinned to the left reads as detached from it. Same
           reset `.import-warnings` makes for the same reason. */
        text-align: left;
    }

    .gn-findings li {
        font-size: var(--text-80);
        line-height: 1.5;
        padding-left: var(--space-11);
        position: relative;
        color: var(--text-secondary);
    }

    .gn-findings li::before {
        position: absolute;
        left: 0;
        top: 0;
    }

    .gn-findings li.is-good::before { content: "\2713"; color: var(--green-light); }
    .gn-findings li.is-info::before { content: "\24D8"; color: var(--text-muted); }
    .gn-findings li.is-warn::before { content: "\26A0"; color: var(--orange); }

    .gn-findings summary {
        cursor: pointer;
        color: inherit;
    }

    .gn-findings details ul {
        margin: var(--space-4) 0 0 var(--space-8);
        max-height: 180px;
        overflow-y: auto;
        color: var(--text-muted);
        font-size: var(--text-80);
    }

    .gn-mismatch {
        background: color-mix(in srgb, var(--orange) 10%, transparent);
        border: 1px solid color-mix(in srgb, var(--orange) 35%, transparent);
        border-radius: var(--radius);
        padding: var(--space-7) var(--space-8);
        margin-top: var(--space-7);
    }

    .gn-mismatch p {
        color: var(--text-primary);
        font-size: var(--text-85);
        line-height: 1.55;
        margin: 0;
    }

    /* People created outside the tree who share a name with someone in it. */
    .gn-homonyms {
        text-align: left;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: var(--space-6) var(--space-7);
        margin-top: var(--space-7);
        display: flex;
        flex-direction: column;
        gap: var(--space-6);
    }

    .gn-homonyms h4 {
        margin: 0;
        color: var(--text-primary);
        font-size: var(--text-90);
    }

    .gn-homonyms-intro {
        margin: 0;
        color: var(--text-secondary);
        font-size: var(--text-80);
        line-height: 1.5;
    }

    .gn-homonym {
        display: flex;
        flex-direction: column;
        gap: var(--space-3);
        padding-top: var(--space-5);
        border-top: 1px solid var(--border);
    }

    .gn-homonym-name {
        color: var(--text-primary);
        font-weight: 600;
    }

    .gn-homonym-done {
        margin: 0;
        color: var(--text-secondary);
        font-size: var(--text-80);
    }

    /* ── Responsive ───────────────────────────────────────────────── */

    @media (max-width: 900px) {
        .import-stats { grid-template-columns: repeat(2, 1fr); }
        /* The step body loses the indent that lined it up under the title:
           at this width the indent costs more than the alignment buys. */
        .gn-step-body { padding-left: var(--space-7); }
    }

    @media (max-width: 600px) {
        .import-modal-backdrop {
            top: 64px;
            align-items: center;
            padding: var(--space-6);
        }
        .import-modal {
            width: 100%;
            max-height: calc(100dvh - 88px);
        }
        .import-modal-header {
            align-items: flex-start;
            padding: var(--space-7) var(--space-8) var(--space-6);
        }
        .import-modal-header h2 {
            font-size: var(--text-100);
            line-height: 1.45;
            overflow-wrap: anywhere;
        }
        .import-tabs { padding: var(--space-4) var(--space-8) 0; }
        .import-modal-body { padding: var(--space-8); }
    }

    @media (max-width: 560px) {
        .import-stats { grid-template-columns: 1fr; }
        /* The summary drops below the title rather than competing with it
           for a line that no longer fits both. */
        .gn-step-head { flex-wrap: wrap; }
        .gn-step-summary { flex-basis: 100%; padding-left: var(--space-16); }
        .gn-step-edit { margin-left: 0; }
    }


    /* ── Saving and reloading a Geneanet session ──────────────────── */

    /* Deliberately quiet. These sit inside a step whose own action is the
       thing to do next, so they read as a footnote to it rather than as a
       second choice competing for the same attention. */
    .gn-session {
        border-top: 1px solid var(--border);
        margin-top: var(--space-7);
        padding-top: var(--space-5);
    }

    .gn-session-why summary {
        cursor: pointer;
        color: var(--text-muted);
        font-size: var(--text-75);
        list-style: none;
    }
    .gn-session-why summary::-webkit-details-marker { display: none; }
    .gn-session-why summary::before {
        content: "\203A";
        display: inline-block;
        width: 12px;
        transition: transform 0.15s;
    }
    .gn-session-why[open] summary::before { transform: rotate(90deg); }
    .gn-session-why summary:hover { color: var(--text-secondary); }
    .gn-session-why p {
        color: var(--text-muted);
        font-size: var(--text-75);
        line-height: 1.5;
        margin: var(--space-3) 0 0 var(--space-6);
    }

    .gn-session-actions {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-3);
        margin-top: var(--space-4);
    }

    .gn-session-btn {
        display: inline-flex;
        align-items: center;
        gap: var(--space-3);
        background: none;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        color: var(--text-secondary);
        font-family: var(--font-sans);
        font-size: var(--text-75);
        padding: 5px var(--space-5);
        cursor: pointer;
        transition: border-color 0.15s, color 0.15s;
    }
    .gn-session-btn:hover:not(:disabled) {
        border-color: var(--orange);
        color: var(--text-primary);
    }
    .gn-session-btn:disabled { opacity: 0.5; cursor: default; }
    .gn-session-icon { font-size: var(--text-85); line-height: 1; }

    /* ── Statistics page ───────────────────────────────────────────────
       Charts draw with the theme's own tokens: the palette below maps the
       series onto accents every theme defines, so no chart color is a
       literal (docs/ui-statistics.md §6). */

    .stats-page {
        --chart-1: var(--orange);
        --chart-2: var(--green);
        --chart-3: var(--blue);
        --chart-4: var(--pink);
        --chart-5: var(--red);
        --chart-6: var(--green-accent);
        --chart-7: var(--orange-light);
        --chart-8: var(--pn-male-line);
        --chart-9: var(--pn-female-line);
        --chart-10: var(--danger);
        --chart-11: var(--connector);
        --chart-12: var(--text-secondary);
    }

    .stats-interval {
        display: flex;
        align-items: center;
        gap: var(--space-4);
        font-size: var(--text-80);
        color: var(--text-secondary);
    }

    /* Compact in the ruler bar: the app-wide form field is sized for forms. */
    .stats-interval select {
        width: auto;
        padding: 3px var(--space-4);
        font-size: var(--text-80);
    }

    .stats-tabs { margin-bottom: var(--space-10); }

    /* The ruler choosing the years of the period charts stays in view while
       they scroll by, and leaves with them. */
    .stats-timeline {
        position: sticky;
        top: 0;
        z-index: 5;
        display: grid;
        grid-template-columns: auto auto minmax(0, 1fr) auto auto;
        align-items: center;
        gap: var(--space-4) var(--space-7);
        margin-bottom: var(--space-10);
        padding: var(--space-5) var(--space-7) var(--space-11);
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        font-size: var(--text-80);
        color: var(--text-secondary);
    }

    .stats-timeline-years {
        color: var(--text-primary);
        font-weight: 600;
        font-variant-numeric: tabular-nums;
    }

    .stats-ruler {
        position: relative;
        height: 20px;
        margin: 0 var(--space-4);
    }

    .stats-ruler-track,
    .stats-ruler-range {
        position: absolute;
        top: 7px;
        height: 6px;
        border-radius: var(--radius-xs);
    }

    .stats-ruler-track { left: 8px; right: 8px; background: var(--border); }
    /* The chosen years stand out dark against the rest of the rule. */
    .stats-ruler-range { background: var(--text-secondary); }

    .stats-ruler-tick {
        position: absolute;
        top: 14px;
        width: 1px;
        height: 4px;
        background: var(--text-muted);
    }

    .stats-ruler-tick.major { height: 7px; }

    .stats-ruler-label {
        position: absolute;
        top: 8px;
        transform: translateX(-50%);
        font-size: var(--text-65);
        color: var(--text-muted);
        white-space: nowrap;
    }

    /* Two native sliders share the track: each draws only its handle, and
       only the handles take the pointer, so either can be dragged. The
       selector outranks the app-wide `input:not(…):not(…)` rule (0-2-1),
       whose background and border would otherwise make the upper slider
       hide the other handle and the chosen range. */
    .stats-ruler input[type="range"].stats-ruler-input {
        position: absolute;
        inset: 0;
        width: 100%;
        height: 20px;
        margin: 0;
        padding: 0;
        border: none;
        border-radius: 0;
        background: transparent;
        box-shadow: none;
        pointer-events: none;
        -webkit-appearance: none;
        appearance: none;
    }

    .stats-ruler-input::-webkit-slider-runnable-track {
        height: 20px;
        background: transparent;
        border: none;
    }

    .stats-ruler-input::-moz-range-track {
        height: 20px;
        background: transparent;
        border: none;
    }

    .stats-ruler-input::-webkit-slider-thumb {
        -webkit-appearance: none;
        width: 16px;
        height: 16px;
        margin-top: var(--space-1);
        border-radius: 50%;
        background: var(--orange);
        border: 2px solid var(--bg-card);
        box-shadow: 0 0 0 1px var(--text-secondary);
        cursor: ew-resize;
        pointer-events: auto;
    }

    .stats-ruler-input::-moz-range-thumb {
        width: 16px;
        height: 16px;
        border-radius: 50%;
        background: var(--orange);
        border: 2px solid var(--bg-card);
        box-shadow: 0 0 0 1px var(--text-secondary);
        cursor: ew-resize;
        pointer-events: auto;
    }

    .stats-ruler-input:focus-visible::-webkit-slider-thumb { outline: 2px solid var(--text-primary); }
    .stats-ruler-input:focus-visible::-moz-range-thumb { outline: 2px solid var(--text-primary); }

    .stats-option {
        display: flex;
        align-items: center;
        gap: var(--space-3);
        margin-left: auto;
        font-size: var(--text-80);
        color: var(--text-secondary);
        cursor: pointer;
    }

    .stats-tiles {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
        gap: var(--space-5);
        margin-bottom: var(--space-5);
    }

    .stats-tile {
        display: flex;
        flex-direction: column;
        gap: var(--space-1);
        min-width: 0;
        padding: var(--space-5) var(--space-6);
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
    }

    .stats-tile-value {
        font-size: var(--text-130);
        font-weight: 600;
        color: var(--text-primary);
        font-variant-numeric: tabular-nums;
    }

    .stats-tile-label { font-size: var(--text-80); color: var(--text-secondary); }
    .stats-tile-detail { font-size: var(--text-70); color: var(--text-muted); }

    .stats-grid-3 {
        grid-template-columns: repeat(3, minmax(0, 1fr));
        margin-top: var(--space-8);
    }

    .stats-bars { display: grid; gap: 3px; font-size: var(--text-75); }

    .stats-bar-row {
        display: grid;
        grid-template-columns: 2.5em minmax(0, 1fr) 4em;
        align-items: center;
        gap: var(--space-3);
    }

    .stats-bar-label { text-align: right; color: var(--text-muted); }
    .stats-bar-track { display: block; height: 12px; }

    .stats-bar {
        display: block;
        height: 100%;
        border-radius: var(--radius-xs);
        background: var(--chart-1);
    }

    .stats-feats {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
        gap: var(--space-5);
    }

    .stats-feat {
        display: flex;
        flex-direction: column;
        gap: 3px;
        min-width: 0;
        padding: var(--space-5) var(--space-6);
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        font-size: var(--text-80);
    }

    .stats-feat-title { font-weight: 600; font-size: var(--text-75); color: var(--text-secondary); }
    .stats-feat-who a { color: var(--text-primary); }
    .stats-feat-value { color: var(--orange); font-weight: 600; }
    .stats-feat-date { color: var(--text-muted); font-size: var(--text-70); }

    .stats-section { margin-bottom: var(--space-14); }

    .stats-section-title {
        display: flex;
        align-items: center;
        gap: var(--space-3);
        font-family: var(--font-heading);
        font-size: var(--text-110);
        margin: 0 0 var(--space-6);
        padding-bottom: var(--space-3);
        border-bottom: 1px solid var(--border);
    }

    .stats-grid {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: var(--space-8);
    }

    .stats-card {
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: var(--space-6) var(--space-7);
        min-width: 0;
    }

    .stats-card-title {
        display: flex;
        align-items: center;
        gap: var(--space-3);
        font-size: var(--text-85);
        font-weight: 600;
        margin: 0 0 var(--space-4);
    }

    .stats-hint {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 16px;
        height: 16px;
        border-radius: 50%;
        border: 1px solid var(--border);
        color: var(--text-muted);
        font-size: var(--text-65);
        cursor: help;
    }

    .stats-empty,
    .stats-loading,
    .stats-note {
        color: var(--text-muted);
        font-size: var(--text-80);
    }

    .stats-donut {
        display: flex;
        align-items: center;
        gap: var(--space-8);
    }

    .stats-donut-svg { width: 150px; height: 150px; flex-shrink: 0; }

    /* The column may shrink below its rows' content, so long labels are
       truncated instead of pushing the counts out of the card. */
    .stats-legend {
        list-style: none;
        margin: 0;
        padding: 0;
        display: grid;
        grid-template-columns: minmax(0, 1fr);
        gap: 3px;
        font-size: var(--text-75);
        min-width: 0;
        flex: 1;
    }

    .stats-legend li {
        display: flex;
        align-items: center;
        gap: var(--space-3);
    }

    .stats-legend-inline {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-2) var(--space-6);
    }

    .stats-swatch {
        width: 10px;
        height: 10px;
        border-radius: var(--radius-xs);
        flex-shrink: 0;
    }

    .stats-legend-label {
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .stats-legend-count { color: var(--text-muted); }

    .stats-lines-svg { width: 100%; height: auto; display: block; }

    .stats-grid-line { stroke: var(--border); stroke-width: 1; }

    .stats-axis {
        fill: var(--text-muted);
        font-size: 10px;
        font-family: var(--font-sans);
    }

    .stats-line {
        fill: none;
        stroke-width: 2;
        stroke-linejoin: round;
    }

    .stats-area {
        fill-opacity: 0.18;
        stroke: none;
    }

    .stats-point { cursor: pointer; }

    /* A marked moment of a line chart (an import): a dashed line under a
       numbered badge, drawn like the map's numbered markers. */
    .stats-marker {
        stroke: var(--text-muted);
        stroke-width: 1;
        stroke-dasharray: 3 3;
    }

    .stats-marker-badge { cursor: pointer; }

    .stats-marker-badge circle {
        fill: var(--bg-card);
        stroke: var(--text-primary);
        stroke-width: 1;
    }

    .stats-marker-text {
        fill: var(--text-primary);
        font-family: var(--font-sans);
        font-size: 9px;
        font-weight: 700;
        dominant-baseline: central;
    }

    .stats-markers {
        list-style: none;
        margin: var(--space-2) 0 0;
        padding: 0;
        display: grid;
        gap: 3px;
        font-size: var(--text-75);
        color: var(--text-secondary);
    }

    .stats-markers li {
        display: flex;
        gap: var(--space-3);
        min-width: 0;
    }

    .stats-markers-number { font-weight: 700; color: var(--text-primary); flex-shrink: 0; }

    .stats-markers-file {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--text-muted);
    }

    .stats-hover {
        min-height: 1.2em;
        margin: var(--space-1) 0 var(--space-2);
        font-size: var(--text-75);
        color: var(--text-secondary);
    }

    .stats-places {
        display: grid;
        grid-template-columns: minmax(0, 2fr) minmax(0, 1fr);
        gap: var(--space-8);
    }

    .stats-map {
        position: relative;
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        overflow: hidden;
    }

    .stats-map-svg {
        width: 100%;
        height: auto;
        display: block;
        aspect-ratio: 1 / 0.62;
        cursor: grab;
        user-select: none;
    }

    .stats-map-land {
        fill: var(--bg-panel);
        stroke: var(--border);
        stroke-width: 1;
        vector-effect: non-scaling-stroke;
    }

    /* Place names under the numbered markers, haloed to read over the heat. */
    .stats-map-cities { pointer-events: none; }
    .stats-map-city-dot { fill: var(--text-secondary); }

    .stats-map-city {
        fill: var(--text-secondary);
        font-family: var(--font-sans);
        paint-order: stroke;
        stroke: var(--bg-card);
        stroke-width: 3px;
        stroke-linejoin: round;
        vector-effect: non-scaling-stroke;
    }

    .stats-map-marker {
        fill: var(--bg-card);
        stroke: var(--text-primary);
        stroke-width: 1;
        vector-effect: non-scaling-stroke;
    }

    .stats-map-marker-text {
        fill: var(--text-primary);
        font-family: var(--font-sans);
        font-weight: 700;
    }

    .stats-map-controls {
        position: absolute;
        top: 8px;
        right: 8px;
        display: flex;
        flex-direction: column;
        gap: var(--space-2);
    }

    /* The list column is as tall as the map; the note of the places that
       could not be located sits at its bottom, level with the map's. */
    .stats-top-places {
        display: flex;
        flex-direction: column;
        min-width: 0;
    }

    .stats-top-places .stats-note { margin: auto 0 0; padding-top: var(--space-4); }

    .stats-top-places ol {
        margin: 0;
        padding-left: 1.4em;
        font-size: var(--text-85);
    }

    .stats-top-places li {
        display: flex;
        justify-content: space-between;
        gap: var(--space-4);
        padding: 3px 0;
    }

    .stats-top-places li::marker { color: var(--text-muted); }

    .stats-top-place-name {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    /* A located place of the list zooms the map onto it. */
    .stats-top-place-link {
        padding: 0;
        border: none;
        background: none;
        font: inherit;
        color: inherit;
        text-align: left;
        cursor: pointer;
    }

    .stats-top-place-link:hover { color: var(--orange); text-decoration: underline; }
    .stats-top-place-link.active { color: var(--orange); font-weight: 600; }

    .stats-map-marker-group { cursor: pointer; }

    .stats-table {
        width: 100%;
        border-collapse: collapse;
        font-size: var(--text-85);
        margin-top: var(--space-4);
    }

    .stats-table td {
        padding: 5px var(--space-4);
        border-bottom: 1px solid var(--border);
    }

    .stats-table a {
        color: var(--text-primary);
        text-decoration: none;
    }

    .stats-table a:hover {
        color: var(--orange);
    }

    .stats-age { text-align: right; white-space: nowrap; }

    .stats-pyramid { display: grid; gap: var(--space-1); margin-top: var(--space-4); font-size: var(--text-75); }

    .stats-pyramid-head,
    .stats-pyramid-row {
        display: grid;
        grid-template-columns: 1fr 64px 1fr;
        align-items: center;
        gap: var(--space-3);
    }

    .stats-pyramid-head { color: var(--text-secondary); font-weight: 600; }
    .stats-pyramid-head span:first-child { text-align: right; }

    .stats-pyramid-side { display: flex; align-items: center; gap: var(--space-2); }
    .stats-pyramid-men { justify-content: flex-end; }

    .stats-pyramid-bar { height: 12px; border-radius: var(--radius-xs); }

    .stats-pyramid-age { text-align: center; color: var(--text-muted); }
    .stats-pyramid-count { color: var(--text-muted); min-width: 2em; }
    .stats-pyramid-men .stats-pyramid-count { text-align: right; }

    @media (max-width: 900px) {
        .stats-grid,
        .stats-grid-3,
        .stats-places { grid-template-columns: minmax(0, 1fr); }
        /* The years, the interval and the button on one line, the ruler
           on its own below them. */
        .stats-timeline { grid-template-columns: minmax(0, 1fr) auto auto; }
        .stats-timeline-title { display: none; }
        .stats-timeline .stats-ruler { grid-column: 1 / -1; grid-row: 2; }
    }

    @media (max-width: 640px) {
        /* Two tiles a row. */
        .stats-tiles { grid-template-columns: repeat(auto-fill, minmax(130px, 1fr)); }
        /* The years and the button, then the interval, then the ruler, whose
           labels hang into the bar's bottom padding. */
        .stats-timeline { grid-template-columns: minmax(0, 1fr) auto; }
        .stats-timeline .stats-interval { grid-column: 1 / -1; grid-row: 2; }
        .stats-timeline .stats-ruler { grid-row: 3; }
        /* A chart is drawn at about half its size here: its axis text is
           drawn larger to stay legible, with half the labels to keep room. */
        .stats-axis { font-size: 15px; }
        .stats-marker-badge circle { r: 10px; }
        .stats-marker-text { font-size: 13px; }
        .stats-axis-alt,
        .stats-ruler-label-alt { display: none; }
        .stats-donut { flex-direction: column; align-items: stretch; }
        .stats-donut-svg { align-self: center; }
    }

    /* ── Copy field (components/copy_field.rs) ───────────────────── */

    .copy-field {
        display: flex;
        flex-direction: column;
        gap: var(--space-3);
    }

    .copy-field-row {
        display: flex;
        align-items: flex-start;
        gap: var(--space-5);
    }

    .copy-field-value {
        flex: 1;
        min-width: 0;
        font-family: var(--font-mono);
        font-size: var(--text-80);
        resize: none;
        background: var(--bg-deep);
        color: var(--text-secondary);
    }

    textarea.copy-field-value {
        line-height: 1.4;
    }

    .copy-field-btn {
        flex: none;
        /* Lines up with the first row of a multi-line field instead of
           stretching or centering across its full height. */
        align-self: flex-start;
    }

    @media (max-width: 640px) {
        .copy-field-row {
            flex-direction: column;
            align-items: stretch;
        }
    }

    /* ── Tools page ─────────────────────────────────────────────────
       Tabs, titled sections, tiles and tables are the statistics page's;
       what follows is only what a tool adds (docs/ui-tools.md). */

    .tools-intro {
        margin: -4px 0 var(--space-7);
        font-size: var(--text-85);
        color: var(--text-secondary);
        max-width: 72ch;
    }

    .tools-controls {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: var(--space-6);
        margin-bottom: var(--space-6);
    }

    /* Compact beside the tool, like the statistics interval. */
    .tools-controls select {
        width: auto;
        padding: 3px var(--space-4);
        font-size: var(--text-80);
    }

    .tools-empty { padding: var(--space-12); }

    .stats-table th {
        padding: 5px var(--space-4);
        text-align: left;
        font-size: var(--text-75);
        font-weight: 600;
        color: var(--text-secondary);
        border-bottom: 1px solid var(--border);
    }

    .tools-col-fact { text-align: right; white-space: nowrap; }

    .tools-completeness {
        display: grid;
        grid-template-columns: auto minmax(40px, 1fr) auto;
        align-items: center;
        gap: var(--space-4);
        white-space: nowrap;
    }

    .tools-bar-track {
        display: block;
        height: 8px;
        border-radius: var(--radius-sm);
        background: var(--border);
        overflow: hidden;
    }

    .tools-bar { display: block; height: 100%; }
    .tools-bar-good { background: var(--green); }
    .tools-bar-fair { background: var(--orange); }
    .tools-bar-poor { background: var(--red); }

    .tools-filter { margin: var(--space-6) 0 var(--space-4); }

    .tools-generation {
        margin-top: var(--space-5);
        padding: var(--space-4) var(--space-6);
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
    }

    .tools-generation summary {
        cursor: pointer;
        font-weight: 600;
        font-size: var(--text-90);
    }

    .tools-sosa {
        width: 1%;
        white-space: nowrap;
        color: var(--text-muted);
        font-variant-numeric: tabular-nums;
    }

    .tools-dates {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-1) var(--space-5);
        font-size: var(--text-75);
    }

    .tools-missing { color: var(--text-muted); font-style: italic; }

    .tools-facts {
        display: flex;
        flex-wrap: wrap;
        justify-content: flex-end;
        gap: var(--space-2);
    }

    .tools-fact {
        padding: 1px var(--space-3);
        border-radius: var(--radius-pill);
        border: 1px solid var(--border);
        font-size: var(--text-70);
        white-space: nowrap;
    }

    .tools-fact-yes { color: var(--green); border-color: var(--green); }
    .tools-fact-no { color: var(--red); border-color: var(--red); text-decoration: line-through; }

    /* A category tile filters the anomalies: a tile that is a button. */
    .tools-category {
        font: inherit;
        color: inherit;
        text-align: left;
        cursor: pointer;
    }

    .tools-category:hover { border-color: var(--text-muted); }
    .tools-category.active { border-color: var(--orange); }

    .tools-category-section { margin-top: var(--space-9); }

    .tools-rule summary {
        display: flex;
        align-items: center;
        gap: var(--space-4);
    }

    .tools-rule-title { flex: 1; min-width: 0; }
    .tools-rule-hint { margin: var(--space-4) 0 var(--space-2); }

    .tools-severity {
        padding: 1px var(--space-3);
        border-radius: var(--radius-pill);
        font-size: var(--text-70);
        font-weight: 600;
        text-transform: uppercase;
        white-space: nowrap;
    }

    .tools-severity-error { color: var(--red); border: 1px solid var(--red); }
    .tools-severity-warning { color: var(--orange); border: 1px solid var(--orange); }

    .tools-persons { overflow-wrap: anywhere; }

    .tools-person { white-space: nowrap; }

    .tools-person-pedigree,
    .tools-couple-link {
        margin-left: var(--space-2);
        font-size: var(--text-75);
        color: var(--text-muted);
    }

    .stats-table a.tools-person-pedigree:hover,
    .stats-table a.tools-couple-link:hover { color: var(--orange); }

    .tools-couple-link { margin-left: var(--space-5); white-space: nowrap; }

    .tools-detail { font-size: var(--text-80); }

    .tools-place-name { overflow-wrap: anywhere; }

    .tools-place-edit {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: var(--space-3);
    }

    .tools-place-edit > :first-child { flex: 1 1 220px; min-width: 0; }

    .tools-place-actions {
        display: flex;
        flex-wrap: wrap;
        justify-content: flex-end;
        gap: var(--space-2);
    }

    .tools-pairs { display: grid; gap: var(--space-6); }

    .tools-pair-head {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: var(--space-3);
        margin-bottom: var(--space-4);
        font-size: var(--text-80);
    }

    .tools-confidence {
        padding: 1px var(--space-4);
        border-radius: var(--radius-pill);
        font-weight: 600;
        border: 1px solid currentColor;
    }

    .tools-confidence-very_likely { color: var(--red); }
    .tools-confidence-likely { color: var(--orange); }
    .tools-confidence-possible { color: var(--text-secondary); }

    .tools-reasons { display: flex; flex-wrap: wrap; gap: var(--space-2); }

    .tools-reason {
        padding: 1px var(--space-3);
        border-radius: var(--radius-pill);
        background: var(--bg-deep);
        color: var(--text-secondary);
        font-size: var(--text-70);
    }

    .tools-pair-persons {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: var(--space-4);
    }

    /* Doubled class: `.search-person-result:last-child` drops the bottom
       border a list's last row does not need, which cut the second card of
       each pair open at the bottom. */
    .search-person-result.tools-pair-person {
        border: 1px solid var(--border);
        border-radius: var(--radius);
        color: var(--text-primary);
        text-decoration: none;
    }

    .tools-pair-actions {
        display: flex;
        flex-wrap: wrap;
        justify-content: flex-end;
        gap: var(--space-3);
        margin-top: var(--space-4);
    }

    @media (max-width: 640px) {
        .tools-pair-persons { grid-template-columns: minmax(0, 1fr); }
        /* The person and the detail of an anomaly one above the other. */
        .tools-rule .stats-table td { display: block; border-bottom: none; }
        .tools-rule .stats-table tr { display: block; border-bottom: 1px solid var(--border); }
        .tools-person { white-space: normal; }
    }

    .tools-words {
        display: flex;
        flex-direction: column;
        gap: var(--space-5);
        margin-bottom: var(--space-6);
    }

    .tools-words-parts caption {
        text-align: left;
        font-weight: 600;
        font-size: var(--text-85);
        padding-bottom: var(--space-2);
    }

    .tools-words-parts th { width: 1%; white-space: nowrap; }
    .tools-words-parts td { overflow-wrap: anywhere; }

    .tools-words-read-title { margin-top: var(--space-10); }

    .tools-words-read {
        display: flex;
        flex-wrap: wrap;
        gap: var(--space-4);
        align-items: center;
    }

    .tools-words-read input { flex: 1 1 240px; min-width: 0; }

    .tools-converter-input { margin-bottom: var(--space-6); }

    .tools-converter-result.active { border-color: var(--orange); }

    .tools-converter-date {
        font-size: var(--text-100);
        font-weight: 600;
        color: var(--text-primary);
        overflow-wrap: anywhere;
    }

    .tools-converter-weekday {
        font-size: var(--text-85);
        color: var(--text-secondary);
    }

    /* ── Printing ────────────────────────────────────────────────────
       `PrintAction` (components/print.rs) sits in the icon sidebar, above
       Settings; `PrintHeading` sits last in a page's topbar; the pedigree's snapshot is appended to
       `body` as `.print-chart`. On screen neither shows. On paper the page
       is recoloured with the light theme (see `print_palette_css`), the app
       chrome and every control disappear, scroll containers give way to the
       document flow, and the topbar shrinks to that header. */

    .print-header,
    .print-page-note,
    .print-chart { display: none; }

    /* The choice between printing what the screen shows and the whole chart
       over several sheets. */
    .modal-card.print-choice { max-width: min(460px, calc(100vw - 32px)); }
    .print-choice-options { display: flex; flex-direction: column; gap: 8px; margin: 12px 0; }
    .print-choice-option {
        display: flex;
        justify-content: space-between;
        align-items: baseline;
        gap: 12px;
        padding: 10px 14px;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        background: var(--bg-card);
        color: var(--text-primary);
        font: inherit;
        text-align: left;
        cursor: pointer;
    }
    .print-choice-option:hover:not(:disabled) { border-color: var(--orange); }
    .print-choice-option:disabled { opacity: 0.5; cursor: not-allowed; }
    .print-choice-name { font-weight: 600; }
    .print-choice-detail { color: var(--text-secondary); font-size: 0.85rem; white-space: nowrap; }

    @media print {
        @page { margin: 12mm; }
        /* Any chart view prints on a landscape sheet of the chosen paper;
           so do its tiles, whose 255 × 175 mm fit A4 and US Letter within
           these margins, clear of what printers cannot reach. */
        @page chart { size: landscape; margin: 10mm; }
        @page tiles { size: landscape; margin: 10mm; }

        html, body, #main, .app-main, .sub-page, .pd-page-shell,
        .sub-page-content, .tree-detail-page {
            display: block !important;
            height: auto !important;
            min-height: 0 !important;
            max-height: none !important;
            overflow: visible !important;
        }

        body {
            background: var(--bg-deep);
            -webkit-print-color-adjust: exact;
            print-color-adjust: exact;
        }

        body::before { display: none; }

        *, *::before, *::after {
            box-shadow: none !important;
            text-shadow: none !important;
            animation: none !important;
            transition: none !important;
        }

        .sub-page-content {
            max-width: none !important;
            width: auto !important;
            margin: 0 !important;
            padding: 0 !important;
        }

        /* The topbar becomes the printed header. */
        .td-topbar {
            display: block;
            height: auto;
            padding: 0;
            border: 0;
            background: none;
            overflow: visible;
        }
        .td-topbar > :not(.print-header) { display: none !important; }

        .print-header {
            display: flex;
            justify-content: space-between;
            align-items: flex-end;
            gap: 16px;
            padding-bottom: 6px;
            margin-bottom: 16px;
            border-bottom: 1px solid var(--border);
            break-after: avoid;
        }
        .print-header-main { min-width: 0; }
        .print-header-title {
            font-family: var(--font-heading);
            font-size: 1.3rem;
            font-weight: 600;
            line-height: 1.25;
            color: var(--text-primary);
        }
        .print-header-tree {
            font-size: 0.85rem;
            color: var(--text-secondary);
        }
        .print-header-date {
            flex-shrink: 0;
            font-size: 0.75rem;
            color: var(--text-muted);
            white-space: nowrap;
        }
        .print-page-note {
            display: block;
            margin-top: 12px;
            font-size: 0.75rem;
            color: var(--text-muted);
            text-align: center;
        }

        /* Chrome, overlays and controls: nothing to read on paper. */
        .app-nav, .tree-icon-sidebar, .isb, .ev-panel,
        .modal-backdrop, .context-menu, .context-menu-backdrop,
        .ref-tooltip, .mini-pedigree-tooltip, .pedigree-depth-popover,
        .import-overlay, .cropper-backdrop,
        .btn, .td-btn, .td-search-btn, .isb-btn, .pd-header-buttons, .cp-actions,
        .cp-bar, .pf-row-btn, .pf-add-btn, .pf-confirm-btn, .dict-row-action,
        .dict-letter-strip, .dict-filter-row, .dict-page-size,
        .media-act, .media-upload-icon-btn, .media-drop-btn,
        .media-tag-remove, .media-identification-delete,
        .sr-filters-toggle, .sr-filters, .sr-sort, .view-toggle,
        .sr-filter-actions, .sr-clear-filters, .pager, .stats-option,
        .stats-interval,
        .tools-controls, .tools-place-actions, .tools-pair-actions, .ph-toolbar,
        input, select, textarea, .no-print {
            display: none !important;
        }

        /* A choice among several prints only what was chosen: the active
           tab, as a heading, and the version a history page compares. */
        .dict-tabs { border: 0; margin-bottom: 8px; }
        .dict-tab:not(.active) { display: none !important; }
        .dict-tab.active {
            padding: 0;
            border: 0;
            background: none;
            color: var(--text-primary);
            font-weight: 600;
        }
        .ph-layout { grid-template-columns: minmax(0, 1fr) !important; }
        .ph-versions li:not(:has(.ph-version.active)) { display: none !important; }

        /* A search's filters print as their chips, without the removal cross. */
        .sr-filter-chip > span { display: none; }

        /* Two spouses side by side, as on a wide screen. */
        .cp-grid { grid-template-columns: repeat(2, minmax(0, 1fr)) !important; gap: 12px; }

        /* A mini pedigree fits its box: the sheet's width on a profile, the
           card's usual height on a search result. */
        .mini-pedigree { height: auto !important; overflow: visible; }
        .mini-pedigree-inner { position: static; transform: none !important; }
        .mini-pedigree-inner svg {
            width: 100% !important;
            height: auto !important;
            max-height: 110mm;
        }
        .sr-grid-ped .mini-pedigree-inner svg { max-height: 200px; }

        /* Links read as text; one leaving the application keeps its address. */
        a { color: inherit !important; text-decoration: none !important; }
        a[href^="http"]::after {
            content: " (" attr(href) ")";
            font-size: 0.75em;
            color: var(--text-muted);
            overflow-wrap: anywhere;
        }

        /* Nothing is cut in half by a page break. */
        tr, img, svg, figure,
        .pd-ev-row, .stats-card, .stats-tile, .stats-feat, .sr-grid-card-hd,
        .search-person-result, .dict-row, .dict-accordion-item, .kin-path-body,
        .tools-pair-head, .cp-cell {
            break-inside: avoid;
        }
        h1, h2, h3, h4, .stats-section-title, .stats-card-title,
        .dict-group-header {
            break-after: avoid;
        }
        thead { display: table-header-group; }

        /* The pedigree prints its snapshot, scaled to one landscape sheet. */
        body:has(.print-chart) { page: chart; }
        body:has(.print-chart) .pedigree-outer { display: none !important; }
        .print-chart { display: block; }
        /* The whole chart over several sheets: one tile a sheet, its caption
           above, within the sheet's margins; the header of the page gives
           way to the captions. */
        body:has(.print-tiles) { page: tiles; }
        body:has(.print-tiles) .print-header { display: none !important; }
        .print-tiles-source { position: absolute; width: 0; height: 0; }
        .print-tile { break-after: page; break-inside: avoid; }
        .print-tile:last-child { break-after: auto; }
        .print-tile-caption {
            font-size: 8pt;
            color: var(--text-muted);
            margin-bottom: 2mm;
        }
        .print-chart.print-tiles .print-tile svg {
            display: block;
            max-width: none;
            max-height: none;
            margin: 0;
        }
        .print-tile-guide {
            stroke: var(--text-muted);
            stroke-width: 0.6;
            stroke-dasharray: 4 3;
            vector-effect: non-scaling-stroke;
        }
        /* Both sizes auto: the box keeps the snapshot's own ratio within the
           two caps, so nothing beyond its viewBox shows at the sides. */
        .print-chart svg {
            display: block;
            margin: 0 auto;
            width: auto;
            height: auto;
            max-width: 100%;
            max-height: 165mm;
        }
    }

    /* ── Settings sections (app and tree settings) ─────────────────────────────── */

    /* ── Theme pickers ──────────────────────────────────────────────
       One grid for both the application palette and the pedigree chart
       style: they are the same control over different things, and a
       reader moving between the two sections should not have to work
       out that they are. Only the swatch inside differs — a painted
       miniature for a palette, an SVG card for a chart style. */
    /* `auto-fill`, not `auto-fit`: fit collapses the empty tracks and lets the
       few items that exist stretch across the row, which would give the two
       pedigree styles tiles three times the width of the five palettes. Fill
       keeps the tracks, so a tile is the same size in both pickers. */
    .theme-picker {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(132px, 1fr));
        gap: var(--space-6);
    }

    .theme-picker-option {
        display: grid;
        justify-items: center;
        align-content: start;
        gap: var(--space-3);
        padding: var(--space-6);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        background: none;
        cursor: pointer;
        transition: border-color 0.15s, background 0.15s;
        text-align: center;
        color: var(--text-primary);
        font-family: var(--font-sans);
    }

    .theme-picker-option:hover { border-color: var(--orange); }

    .theme-picker-option.active {
        border-color: var(--orange);
        background: var(--sel-bg);
    }

    .theme-picker-label {
        font-size: var(--text-90);
        font-weight: 600;
    }

    .theme-picker-hint {
        font-size: var(--text-80);
        color: var(--text-secondary);
        line-height: 1.35;
    }

    .theme-picker-tag {
        font-size: var(--text-70);
        letter-spacing: 0.04em;
        text-transform: uppercase;
        color: var(--text-secondary);
    }

    /* Both swatches occupy the same box, so the two pickers line up. */
    .app-theme-swatch,
    .ped-theme-swatch {
        width: 100%;
        height: 68px;
    }

    /* The miniature declares the custom properties of the theme it shows
       on itself (`Theme::declarations`), so these rules paint it in that
       theme rather than the active one. Its geometry stays fixed: only the
       corners, shadow and typeface follow the theme. */
    .app-theme-swatch {
        background: var(--bg-deep);
        border: 1px solid var(--border);
        border-radius: var(--radius-sm);
        overflow: hidden;
        display: flex;
        flex-direction: column;
    }

    .app-theme-swatch-bar {
        height: 13px;
        background: var(--nav-surface);
        border-bottom: 1px solid var(--border);
        display: flex;
        align-items: center;
        padding: 0 var(--space-2);
        flex: none;
    }

    .app-theme-swatch-dot {
        width: 6px;
        height: 6px;
        border-radius: 50%;
        background: var(--orange);
    }

    .app-theme-swatch-card {
        position: relative;
        margin: var(--space-3);
        padding: 5px 5px 5px var(--space-13);
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius-sm);
        box-shadow: var(--shadow-sm);
        flex: 1;
        display: flex;
        flex-direction: column;
        justify-content: center;
        gap: var(--space-2);
    }

    .app-theme-swatch-type {
        position: absolute;
        left: 6px;
        top: 50%;
        transform: translateY(-50%);
        font-family: var(--font-heading);
        font-size: 13px;
        line-height: 1;
        color: var(--text-primary);
    }

    .app-theme-swatch-line {
        height: 3px;
        border-radius: var(--radius-xs);
        width: 100%;
        background: var(--text-primary);
        opacity: 0.85;
    }

    .app-theme-swatch-line.is-short {
        width: 60%;
        background: var(--text-secondary);
    }

    .app-theme-swatch-accent {
        height: 5px;
        width: 34%;
        border-radius: var(--radius-xs);
        background: var(--green-accent);
    }

    .app-theme-source {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: var(--space-5);
    }

    .app-theme-folder {
        font-size: var(--text-80);
        padding: 3px 7px;
        border: 1px solid var(--border);
        border-radius: var(--radius-sm);
        background: var(--bg-deep);
        color: var(--text-secondary);
        word-break: break-all;
    }

    /* A theme file that failed to load is named here rather than only in the
       log: the person who wrote it is the one who can fix it. */
    .app-theme-errors {
        margin: 0;
        padding-left: var(--space-9);
        font-size: var(--text-80);
        line-height: 1.45;
        color: var(--danger-text);
    }

    /* The swatch carries the theme's own variables and uses the same ground
       as the pedigree canvas. */
    .ped-theme-swatch {
        border-radius: var(--radius-sm);
        border: 1px solid var(--border);
        /* The ground is painted by the rect inside. */
        --pn-swatch-bg: var(--bg-deep);
    }

    .settings-layout {
        display: flex;
        gap: var(--space-12);
        min-height: 0;
    }

    .settings-nav {
        width: 200px;
        min-width: 200px;
        flex-shrink: 0;
    }

    .settings-nav-group {
        margin-bottom: var(--space-10);
    }

    .settings-nav-group-label {
        font-size: var(--text-70);
        font-weight: 700;
        color: var(--orange);
        text-transform: uppercase;
        letter-spacing: 0.5px;
        margin-bottom: var(--space-3);
        padding: 0 var(--space-4);
    }

    .settings-nav-item {
        display: block;
        width: 100%;
        padding: var(--space-3) var(--space-4);
        text-align: left;
        background: none;
        border: none;
        border-radius: var(--radius-sm);
        font-size: var(--text-85);
        color: var(--text-secondary);
        cursor: pointer;
        transition: background 0.12s, color 0.12s;
        font-family: var(--font-sans);
    }

    .settings-nav-item:hover {
        background: var(--bg-card-hover);
        color: var(--text-primary);
    }

    .settings-nav-item.active {
        background: var(--sel-bg);
        color: var(--text-primary);
        font-weight: 600;
    }

    .settings-content {
        flex: 1;
        min-width: 0;
        max-width: 860px;
    }

    .settings-section-eyebrow {
        font-size: var(--text-70);
        font-weight: 700;
        color: var(--orange);
        text-transform: uppercase;
        letter-spacing: 0.5px;
        margin-bottom: var(--space-2);
    }

    .settings-section-title {
        font-family: var(--font-heading);
        font-size: var(--text-120);
        font-weight: 600;
        color: var(--text-primary);
        margin-bottom: var(--space-2);
    }

    .settings-section-subtitle {
        font-size: var(--text-85);
        color: var(--text-secondary);
    }

    .app-settings-card {
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: var(--space-10);
    }

    .app-settings-option {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--space-8);
    }

    /* Must stay after the rule above: both are single-class selectors, so the
       later one wins the tie. Declared first, `align-items: center` took it
       back and every picker grid collapsed to one centred column. */
    .app-settings-option-stacked {
        flex-direction: column;
        align-items: stretch;
        gap: var(--space-7);
    }

    .app-settings-option-info {
        display: flex;
        flex-direction: column;
        gap: var(--space-1);
    }

    .app-settings-option + .app-settings-option {
        margin-top: var(--space-8);
        padding-top: var(--space-8);
        border-top: 1px solid var(--border);
    }

    .settings-section + .settings-section {
        margin-top: var(--space-10);
    }

    .pedigree-depth-stepper {
        display: grid;
        grid-template-columns: 2rem 2.5rem 2rem;
        align-items: center;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        overflow: hidden;
        flex-shrink: 0;
    }

    .pedigree-depth-step {
        width: 2rem;
        height: 2rem;
        border: none;
        background: none;
        color: var(--text-primary);
        cursor: pointer;
        font-size: var(--text-100);
    }

    .pedigree-depth-step:hover:not(:disabled) {
        background: var(--bg-card-hover);
    }

    .pedigree-depth-step:disabled {
        color: var(--text-muted);
        cursor: default;
        opacity: 0.5;
    }

    .pedigree-depth-value {
        line-height: 2rem;
        text-align: center;
        border-right: 1px solid var(--border);
        border-left: 1px solid var(--border);
        color: var(--text-primary);
        font-variant-numeric: tabular-nums;
        font-weight: 600;
    }

    .app-settings-option-label {
        font-size: var(--text-95);
        font-weight: 600;
        color: var(--text-primary);
    }

    .app-settings-option-hint {
        font-size: var(--text-80);
        color: var(--text-muted);
    }

    .theme-toggle-group {
        display: flex;
        gap: 0;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        overflow: hidden;
    }

    .theme-toggle-btn {
        display: flex;
        align-items: center;
        gap: var(--space-3);
        padding: var(--space-4) var(--space-7);
        border: none;
        background: none;
        font-size: var(--text-85);
        color: var(--text-muted);
        cursor: pointer;
        transition: background 0.15s, color 0.15s;
    }

    .theme-toggle-btn:first-child {
        border-right: 1px solid var(--border);
    }

    .theme-toggle-btn:hover {
        background: var(--bg-card-hover);
        color: var(--text-primary);
    }

    .theme-toggle-btn.active {
        background: var(--orange);
        color: var(--white);
    }

    .lang-options {
        display: flex;
        flex-direction: column;
        gap: var(--space-4);
    }

    .lang-option {
        display: flex;
        align-items: center;
        gap: var(--space-6);
        padding: var(--space-6) var(--space-8);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        background: none;
        cursor: pointer;
        transition: border-color 0.15s, background 0.15s;
        width: 100%;
        text-align: left;
        font-size: var(--text-95);
        color: var(--text-primary);
    }

    .lang-option:hover {
        border-color: var(--orange);
        background: var(--bg-card-hover);
    }

    .lang-option.active {
        border-color: var(--orange);
        background: color-mix(in srgb, var(--orange) 8%, transparent);
    }

    .lang-option-flag {
        font-size: var(--text-130);
    }

    .lang-option-label {
        flex: 1;
        font-weight: 500;
    }

    .lang-option-check {
        color: var(--orange);
        font-weight: 700;
        font-size: var(--text-100);
    }

    @media (max-width: 768px) {
        .settings-layout {
            flex-direction: column;
        }
        .settings-nav {
            width: 100%;
            min-width: 0;
            display: flex;
            flex-wrap: nowrap;
            align-items: center;
            gap: var(--space-6);
            overflow-x: auto;
            padding-bottom: var(--space-2);
        }
        .settings-nav-group {
            display: flex;
            flex: none;
            align-items: center;
            flex-wrap: nowrap;
            gap: var(--space-2);
            margin-bottom: 0;
        }
        .settings-nav-group-label {
            width: auto;
            margin: 0 var(--space-2) 0 0;
            padding: 0 var(--space-4) 0 0;
            border-right: 1px solid var(--border);
            white-space: nowrap;
        }
        .settings-nav-item {
            width: auto;
            flex: none;
            white-space: nowrap;
        }
    }

    @media (max-width: 640px) {
        .app-settings-option {
            flex-direction: column;
            align-items: flex-start;
        }
        .app-settings-option-stacked {
            align-items: stretch;
        }
    }

    @media (min-width: 300px) and (max-width: 640px) {
        .theme-picker {
            grid-template-columns: repeat(2, minmax(0, 1fr));
        }
    }

    /* ── Change history and version comparison ─────────────────────────────── */

    .hd { display: flex; flex-direction: column; gap: var(--space-9); }
    .hd-banner {
        padding: var(--space-5) var(--space-7);
        border-radius: var(--radius);
        font-size: var(--text-90);
    }
    .hd-banner-removed {
        background: color-mix(in srgb, var(--danger) 14%, transparent);
        color: var(--danger-text);
        border: 1px solid color-mix(in srgb, var(--danger) 40%, transparent);
    }
    .hd-empty { color: var(--text-muted); font-style: italic; }
    .hd-section-title {
        font-family: var(--font-heading);
        font-size: var(--text-95);
        color: var(--orange);
        margin: 0 0 var(--space-4);
    }
    .hd-table {
        width: 100%;
        border-collapse: collapse;
        table-layout: fixed;
        font-size: var(--text-85);
    }
    .hd-table th, .hd-table td {
        padding: var(--space-3) var(--space-5);
        border-bottom: 1px solid var(--border);
        text-align: start;
        vertical-align: top;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
    }
    .hd-col-label { width: 22%; }
    .hd-col-value {
        width: 39%;
        color: var(--text-secondary);
        font-weight: 600;
        font-size: var(--text-80);
        text-transform: uppercase;
        letter-spacing: 0.04em;
    }
    .hd-label { color: var(--text-secondary); font-weight: 400; }
    .hd-group-title td {
        padding-top: var(--space-6);
        font-weight: 700;
        color: var(--text-primary);
    }
    .hd-kind {
        display: inline-block;
        margin-inline-end: var(--space-4);
        padding: 1px var(--space-4);
        border-radius: var(--radius-lg);
        font-size: var(--text-70);
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.04em;
        border: 1px solid var(--border);
        color: var(--text-secondary);
    }
    .hd-added .hd-kind { color: var(--green); border-color: var(--green); }
    .hd-removed .hd-kind { color: var(--danger-text); border-color: var(--danger); }
    .hd-changed .hd-kind { color: var(--orange); border-color: var(--orange); }
    .hd-row-changed .hd-before {
        background: color-mix(in srgb, var(--danger) 12%, transparent);
    }
    .hd-row-changed .hd-after {
        background: color-mix(in srgb, var(--green) 14%, transparent);
    }
    .hd-removed .hd-row-changed .hd-before { text-decoration: line-through; }
    .hd-row-changed .hd-none { background: none; color: var(--text-muted); }

    @media (max-width: 768px) {
        .hd-col-label { width: 30%; }
        .hd-col-value { width: 35%; }
        .hd-table th, .hd-table td { padding: 5px var(--space-3); }
    }

    /* ── Audit log ─────────────────────────────── */

    .al-filters { display: flex; flex-wrap: wrap; gap: var(--space-3); margin: var(--space-6) 0 var(--space-8); }
    .al-filter {
        padding: var(--space-2) var(--space-6);
        border-radius: var(--radius-lg);
        border: 1px solid var(--border);
        background: transparent;
        color: var(--text-secondary);
        font-size: var(--text-80);
        cursor: pointer;
        font-family: var(--font-sans);
    }
    .al-filter:hover { background: var(--bg-card-hover); color: var(--text-primary); }
    .al-filter.active { border-color: var(--orange); color: var(--orange); font-weight: 600; }
    .al-entries {
        list-style: none;
        margin: 0 0 var(--space-6);
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: var(--space-4);
    }
    .al-entry { padding: var(--space-6) var(--space-7); display: flex; flex-direction: column; gap: var(--space-3); }
    .al-entry-head {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: var(--space-4) var(--space-6);
    }
    .al-time { font-size: var(--text-80); color: var(--text-muted); min-width: 140px; }
    .al-what { font-size: var(--text-90); color: var(--text-primary); }
    .al-subject { font-size: var(--text-90); font-weight: 700; color: var(--orange); }
    .al-toggle { margin-inline-start: auto; }
    .al-details { font-size: var(--text-80); color: var(--text-secondary); }
    .al-category-data { color: var(--blue); border-color: var(--blue); }
    .al-category-settings { color: var(--text-primary); }
    .al-category-media { color: var(--pink); border-color: var(--pink); }
    .al-category-import, .al-category-export { color: var(--green); border-color: var(--green); }
    .al-category-history { color: var(--orange); border-color: var(--orange); }
    .al-changes {
        display: flex;
        flex-direction: column;
        gap: var(--space-9);
        margin-top: var(--space-4);
        padding-top: var(--space-6);
        border-top: 1px solid var(--border);
    }
    .al-change { display: flex; flex-direction: column; gap: var(--space-5); }
    .al-change-head { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-4); }
    .al-change-head a.btn { text-decoration: none; }
    .al-change-record { font-weight: 700; color: var(--text-primary); margin-inline-end: auto; }
"#;

#[cfg(test)]
mod font_tests {
    use super::*;

    #[test]
    fn the_fonts_are_bundled_and_nothing_is_fetched_from_a_third_party() {
        assert!(!LAYOUT_STYLES.contains("@import"));
        assert!(!LAYOUT_STYLES.contains("url(http"));
        assert_eq!(FONT_FACES.matches("@font-face").count(), 8);
        assert!(!FONT_FACES.contains("http"));
        for family in ["'Cinzel'", "'Lato'"] {
            assert!(FONT_FACES.contains(family), "{family}");
        }
    }
}
