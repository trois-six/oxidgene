//! Application layout with navigation bar.
//!
//! Wraps all routed pages with a consistent header/nav and renders the
//! active route via [`Outlet`].

use dioxus::prelude::*;

use crate::components::tree_cache;
use crate::i18n;
use crate::router::Route;

/// Logo PNG embedded at compile time (64×64 resize).
pub const LOGO_PNG_B64: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/logo_64.b64"));

/// Initialise the theme signal as a Dioxus context.
///
/// Reads persisted preference from `localStorage` (key `oxidgene-theme`),
/// falling back on first use to the OS-level `prefers-color-scheme` media
/// query, and to the light theme when neither can be read.
/// Returns the shared signal so the Layout can consume it if needed.
pub fn use_init_theme() -> Signal<bool> {
    let mut is_dark = use_context_provider(|| Signal::new(false));

    use_effect(move || {
        spawn(async move {
            // Storage and `matchMedia` are probed independently: blocked
            // storage (private browsing) must not skip the OS query, and a
            // webview without `matchMedia` must not throw. Light is the
            // fallback for both, matching the CSS default.
            let result = document::eval(
                r#"
                let stored = null;
                try { stored = localStorage.getItem('oxidgene-theme'); } catch (e) {}
                let dark = stored === 'dark';
                if (stored !== 'dark' && stored !== 'light') {
                    try {
                        dark = !!(window.matchMedia
                            && window.matchMedia('(prefers-color-scheme: dark)').matches);
                    } catch (e) { dark = false; }
                }
                document.documentElement.classList.toggle('dark', dark);
                return dark;
                "#,
            );
            if let Ok(val) = result.await {
                is_dark.set(val.as_bool().unwrap_or(false));
            }
        });
    });

    is_dark
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

/// Persist and apply a theme change.
pub fn set_theme(mut is_dark: Signal<bool>, dark: bool) {
    is_dark.set(dark);
    if dark {
        document::eval(
            "document.documentElement.classList.add('dark'); localStorage.setItem('oxidgene-theme','dark');",
        );
    } else {
        document::eval(
            "document.documentElement.classList.remove('dark'); localStorage.setItem('oxidgene-theme','light');",
        );
    }
}

/// Shared layout rendered around every page.
///
/// Contains a navigation bar (shown only on Home / AppSettings) and an
/// [`Outlet`] for the matched child route.
#[component]
pub fn Layout() -> Element {
    let _lang_signal = i18n::use_init_language();
    let _sort_particles = crate::prefs::use_init_sort_particles();
    let _theme_signal = use_init_theme();
    use_init_textarea_resize_clamp();
    let _tree_cache = tree_cache::use_init_tree_cache();
    let _view_cache = tree_cache::use_init_view_state_cache();

    let route = use_route::<Route>();
    let show_nav = matches!(route, Route::Home {} | Route::AppSettings {});

    rsx! {
        style { {LAYOUT_STYLES} }

        if show_nav {
            nav { class: "app-nav",
                Link { to: Route::Home {}, class: "nav-logo",
                    img {
                        src: LOGO_PNG_B64,
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

/// CSS for the layout shell.
pub const LAYOUT_STYLES: &str = r#"
    @import url('https://fonts.googleapis.com/css2?family=Cinzel:wght@400;600;700&family=Lato:wght@300;400;700&display=swap');

    :root {
        /* ── Light palette (default) ─────────────────────────────── */
        --bg-deep:        #ffffff;
        --bg-panel:       #ede9e2;
        --bg-card:        #ffffff;
        --bg-card-hover:  #f5f3ef;
        --border:         #d4ccc0;
        --border-glow:    #e07820;
        --orange:         #e07820;
        --orange-light:   #f5a03a;
        --green:          #4ea832;
        --green-light:    #7ec45f;
        --green-accent:   #5aab3c;
        --blue:           #4a90d9;
        --pink:           #c4587a;
        --sel-bg:         #e8e0d4;
        --text-primary:   #1e1a14;
        --text-secondary: #5c5447;
        --text-muted:     #9e9488;
        --connector:      #a0937f;
        --nav-bg:         rgba(244,242,238,0.92);
        --tree-visual-bg:     #e8e0d4;
        --tree-visual-branch: #b0a898;
        --color-danger-text:  #dc2626;
        --white:          #ffffff;
        --red:            #e05555;
        --shadow-black:   #000000;

        /* ── Component dimensions ──────────────────────────────────── */
        --sb:   46px;   /* icon sidebar width */
        --evw:  275px;  /* event panel width */

        /* ── Semantic aliases (used by shared components) ─────────── */
        --color-bg:           var(--bg-deep);
        --color-surface:      var(--bg-card);
        --color-primary:      var(--orange);
        --color-primary-hover:var(--orange-light);
        --color-text:         var(--text-primary);
        --color-text-muted:   var(--text-secondary);
        --color-border:       var(--border);
        --color-danger:       #e05252;
        --shadow-sm:  0 1px 3px rgba(0,0,0,0.08);
        --shadow-md:  0 4px 16px rgba(0,0,0,0.12);
        --radius: 8px;
        --font-sans:    'Lato', -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
        --font-heading: 'Cinzel', Georgia, serif;

        /* ── Person node (pedigree card) variables ─────────────────── */
        --pn-bg:          #efefef;
        --pn-root-bg:     #006AC4;
        --pn-spouse-bg:   #ffffff;
        --pn-border:      #888888;
        --pn-male-line:   #00A6C0;
        --pn-female-line: #FF6699;
        --pn-born:        #4ea832;
        --pn-died:        #4a90d9;
        --pn-sosa:        #95C417;
        --pn-sosa-root:   #6da118;
        --pn-text:        #111111;
        --pn-text-muted:  #555555;
        --pn-hover-bg:    #cfe3fa;
    }

    :root.dark {
        /* ── Dark pedigree node overrides ───────────────────────────── */
        --pn-bg:         #1e2330;
        --pn-spouse-bg:  #252d3d;
        --pn-text:       #e8dfc8;
        --pn-text-muted: #7a8da8;
        --pn-hover-bg:   #2b4364;
        /* ── Dark palette ─────────────────────────────────────────── */
        --bg-deep:        #0d0f14;
        --bg-panel:       #111318;
        --bg-card:        #16191f;
        --bg-card-hover:  #1c2030;
        --border:         #252d3d;
        --sel-bg:         #192038;
        --text-primary:   #ddd8cc;
        --text-secondary: #7a8da8;
        --text-muted:     #404f65;
        --connector:      #2e4a6a;
        --nav-bg:         rgba(10,11,13,0.92);
        --tree-visual-bg:     #0d1018;
        --tree-visual-branch: #3a4458;
        --color-danger-text:  #f87171;
        --shadow-sm:  0 1px 3px rgba(0,0,0,0.35);
        --shadow-md:  0 4px 16px rgba(0,0,0,0.55);
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

    /* Subtle radial light leaks on the page background (dark only) */
    :root.dark body::before {
        content: '';
        position: fixed;
        inset: 0;
        background:
            radial-gradient(ellipse at 20% 50%, rgba(224,120,32,0.04) 0%, transparent 60%),
            radial-gradient(ellipse at 80% 20%, rgba(90,171,60,0.03) 0%, transparent 50%);
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
        padding: 0 2.5rem;
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
        gap: 8px;
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
        padding: 24px;
        max-width: 1200px;
        width: 100%;
        margin: 0 auto;
    }

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
        padding: 20px;
        box-shadow: var(--shadow-sm);
    }

    .btn {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 8px 16px;
        border: none;
        border-radius: var(--radius);
        font-size: 0.875rem;
        font-weight: 500;
        cursor: pointer;
        transition: background 0.15s, box-shadow 0.15s, opacity 0.15s;
        font-family: var(--font-sans);
    }

    .btn-primary {
        background: linear-gradient(135deg, var(--orange), var(--orange-light));
        color: #fff;
        box-shadow: 0 2px 8px rgba(224,120,32,0.3);
    }

    .btn-primary:hover {
        opacity: 0.9;
        box-shadow: 0 4px 16px rgba(224,120,32,0.4);
    }

    .btn-danger {
        background: var(--color-danger);
        color: #fff;
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
        gap: 18px;
        margin-bottom: 24px;
    }

    .page-header h1 {
        font-size: 1.5rem;
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
        gap: 18px;
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
        gap: 12px;
    }

    .pd-header-actions {
        display: flex;
        flex-direction: column;
        align-items: flex-end;
        justify-content: space-between;
        gap: 12px;
        flex-shrink: 0;
    }

    .pd-header-sosa {
        min-height: 24px;
        display: flex;
        justify-content: flex-end;
    }

    .pd-header-buttons {
        display: flex;
        gap: 8px;
        justify-content: flex-end;
    }

    .badge.pd-sosa-badge {
        background: var(--green);
        color: #fff;
        border-color: var(--green);
        font-size: 0.8rem;
    }

    .pd-sex-mark {
        color: var(--orange);
        font-weight: 600;
        margin-right: 4px;
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
        padding: 48px;
        color: var(--text-secondary);
    }

    .error-msg {
        background: rgba(220, 82, 82, 0.12);
        border: 1px solid rgba(220, 82, 82, 0.4);
        color: var(--color-danger-text);
        padding: 12px 16px;
        border-radius: var(--radius);
        margin-bottom: 16px;
    }

    .success-msg {
        background: rgba(90, 171, 60, 0.1);
        border: 1px solid rgba(90, 171, 60, 0.35);
        color: var(--green-light);
        padding: 12px 16px;
        border-radius: var(--radius);
        margin-bottom: 16px;
    }

    .warning-msg {
        background: rgba(224, 120, 32, 0.10);
        border: 1px solid rgba(224, 120, 32, 0.35);
        color: var(--orange-light);
        padding: 12px 16px;
        border-radius: var(--radius);
        margin-bottom: 16px;
    }

    input, select, textarea {
        font-family: var(--font-sans);
        font-size: 0.9rem;
        padding: 8px 12px;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        width: 100%;
        transition: border-color 0.15s, box-shadow 0.15s;
        background: var(--bg-panel);
        color: var(--text-primary);
    }

    input::placeholder,
    textarea::placeholder {
        color: var(--text-muted);
    }

    input:focus, select:focus, textarea:focus {
        outline: none;
        border-color: var(--orange);
        box-shadow: 0 0 0 3px rgba(224, 120, 32, 0.15);
    }

    select option {
        background: var(--bg-panel);
        color: var(--text-primary);
    }

    label {
        display: block;
        font-size: 0.8rem;
        font-weight: 500;
        margin-bottom: 4px;
        color: var(--text-secondary);
    }

    .form-group {
        margin-bottom: 16px;
    }

    .form-row {
        display: flex;
        gap: 16px;
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
        margin-top: 4px;
        font-size: 0.8rem;
        color: var(--text-secondary, #9a9384);
    }

    /* Surname particle: the detected split plus the affordance to correct it.
       Wraps rather than overflowing, since the summary text is translated and
       its length varies. */
    .particle-row {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 8px;
        margin-top: 4px;
    }

    .particle-row .field-hint {
        margin-top: 0;
    }

    /* A particle that could not be applied: informational, not an error —
       nothing was lost, the cut simply did not happen. */
    .field-hint-warn {
        color: var(--orange, #e07820);
    }

    .particle-label {
        font-size: 0.8rem;
        color: var(--text-secondary, #9a9384);
    }

    .particle-input {
        width: 8rem;
        flex: 0 0 auto;
        padding: 4px 8px;
        font-size: 0.85rem;
    }

    .particle-btn {
        padding: 2px 8px;
        font-size: 0.78rem;
        line-height: 1.6;
        color: var(--orange, #e07820);
        background: none;
        border: 1px solid currentColor;
        border-radius: var(--radius, 6px);
        cursor: pointer;
    }

    .particle-btn:hover {
        background: color-mix(in srgb, var(--orange, #e07820) 12%, transparent);
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
        padding: 4px 8px;
    }

    .empty-state {
        text-align: center;
        padding: 48px 24px;
        color: var(--text-secondary);
    }

    .empty-state h3 {
        margin-bottom: 8px;
        font-weight: 500;
    }

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
        gap: 0.75rem;
        width: 160px;
        height: 160px;
        border: 2px dashed var(--border);
        border-radius: 16px;
        background: transparent;
        color: var(--text-muted);
        font-size: 0.85rem;
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
        padding: 2px 8px;
        font-size: 0.75rem;
        font-weight: 500;
        border-radius: 12px;
        background: var(--bg-panel);
        color: var(--text-secondary);
        border: 1px solid var(--border);
    }

    /* ── Section header ─────────────────────────────────────────── */

    .section-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: 16px;
    }

    .btn-sm {
        padding: 4px 10px;
        font-size: 0.8rem;
    }

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

    /* ── Family connections ──────────────────────────────────────── */

    .pd-fc-section {
        margin-bottom: 12px;
    }
    .pd-fc-section:last-child { margin-bottom: 0; }

    .pd-fc-label {
        font-size: 0.72rem;
        font-weight: 700;
        color: var(--orange);
        text-transform: uppercase;
        letter-spacing: 0.5px;
        margin-bottom: 6px;
    }

    /* ── Alternate names sub-line, under the header name ─────────── */

    .pd-alt-names {
        display: flex;
        flex-wrap: wrap;
        gap: 2px 10px;
        font-size: 0.85rem;
        color: var(--text-secondary);
        margin: 4px 0 0;
    }

    .pd-vitals {
        font-size: 0.9rem;
        color: var(--text-secondary);
        margin: 6px 0 0;
    }

    /* ── Family narrative (parents / unions / siblings) ──────────── */

    .pd-family-prose {
        font-size: 0.95rem;
        margin-bottom: 14px;
    }

    .pd-person-chip {
        display: inline-flex;
        align-items: center;
        gap: 3px;
        white-space: nowrap;
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
        margin-bottom: 14px;
    }
    .pd-union:last-child {
        margin-bottom: 0;
    }
    .pd-union-line {
        font-size: 0.95rem;
    }

    .pd-children {
        list-style: none;
        margin: 6px 0 0;
        padding: 0 0 0 4px;
    }
    .pd-children li {
        font-size: 0.92rem;
        padding: 3px 0 3px 14px;
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
        margin-bottom: 12px;
    }
    .pd-sib-group:last-child {
        margin-bottom: 0;
    }
    .pd-sib-group-head {
        font-size: 0.85rem;
        color: var(--text-secondary);
        margin-bottom: 2px;
    }

    /* ── Events timeline (replaces the events table) ──────────────── */

    .pd-timeline {
        list-style: none;
        margin: 0;
        padding: 0;
    }
    .pd-timeline li {
        display: flex;
        gap: 14px;
        padding: 9px 0;
        border-top: 1px solid var(--border);
        font-size: 0.9rem;
    }
    .pd-timeline li:first-child {
        border-top: none;
        padding-top: 2px;
    }
    /* Events directly on the individual or their conjugal family stand out
       from narrative-context events (children, parents, siblings). */
    .pd-timeline li.pd-ev-direct {
        background: rgba(224, 120, 32, 0.08);
        margin: 0 -14px;
        padding-left: 14px;
        padding-right: 14px;
        border-radius: 4px;
    }
    .pd-ev-date {
        flex: none;
        width: 108px;
        font-variant-numeric: tabular-nums;
        color: var(--text-secondary);
        font-size: 0.82rem;
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
        gap: 10px;
    }
    .pd-ev-origin {
        font-size: 0.75rem;
        color: var(--text-muted);
        font-style: italic;
    }
    .pd-ev-sources {
        font-size: 0.75rem;
        color: var(--text-muted);
        font-style: italic;
        margin-top: 2px;
    }
    /* ── Modal / confirmation dialog ─────────────────────────────── */

    .modal-backdrop {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.65);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 200;
        backdrop-filter: blur(4px);
    }

    .modal-card {
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: 24px;
        min-width: 360px;
        max-width: 480px;
        box-shadow: var(--shadow-md);
    }

    .modal-card h3 {
        color: var(--text-primary);
        margin-bottom: 12px;
    }

    .modal-card p {
        color: var(--text-secondary);
    }

    .modal-actions {
        display: flex;
        justify-content: flex-end;
        gap: 8px;
        margin-top: 16px;
    }

    /* ── Tree detail topbar ──────────────────────────────────────── */

    .td-topbar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        height: 48px;
        padding: 0 12px;
        background: var(--bg-panel);
        border-bottom: 1px solid var(--border);
        flex-shrink: 0;
        min-width: 0;
        overflow: hidden;
    }

    .td-bc {
        display: flex;
        align-items: center;
        gap: 6px;
        font-size: 0.88rem;
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
        margin: 0 2px;
        flex: 0 0 auto;
    }

    .td-bc-link {
        color: var(--text-secondary);
        font-size: 0.88rem;
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
        margin-right: 2px;
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
        border-radius: 6px;
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
        gap: 6px;
        margin-left: auto;
        flex: 0 0 auto;
        min-width: 0;
    }

    .td-search-input {
        padding: 4px 8px;
        font-size: 0.8rem;
        border: 1px solid var(--border);
        border-radius: 5px;
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
        padding: 6px 0;
        gap: 2px;
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
        border-radius: 6px;
        cursor: pointer;
        font-size: 1.05rem;
        color: var(--text-secondary);
        transition: background 0.12s, color 0.12s;
        line-height: 1;
        padding: 0;
    }

    .isb-btn:hover { background: var(--bg-card-hover); color: var(--orange); }
    .isb-btn:active { background: rgba(224,120,32,0.12); }
    .isb-btn:disabled {
        color: var(--text-muted);
        cursor: default;
        opacity: 0.45;
    }
    .isb-btn:disabled:hover { background: none; color: var(--text-muted); }

    .isb-hr { width: 28px; height: 1px; background: var(--border); margin: 4px 0; }

    .isb-zoom-val {
        font-size: 0.62rem;
        color: var(--text-muted);
        text-align: center;
        line-height: 1;
        width: 100%;
        padding: 0 2px;
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
        padding: 12px 14px;
        z-index: 20;
        min-width: 170px;
        pointer-events: all;
    }

    .pedigree-depth-row { display: flex; align-items: center; gap: 6px; margin-bottom: 8px; }
    .pedigree-depth-row:last-child { margin-bottom: 0; }

    .pedigree-depth-btn {
        width: 24px;
        height: 24px;
        display: flex;
        align-items: center;
        justify-content: center;
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: 4px;
        cursor: pointer;
        font-size: 1rem;
        font-weight: 600;
        color: var(--text-primary);
        padding: 0;
        line-height: 1;
        transition: background 0.1s;
    }

    .pedigree-depth-btn:hover { background: var(--orange); color: white; border-color: var(--orange); }

    .pedigree-depth-val { width: 20px; text-align: center; font-size: 0.9rem; font-weight: 600; }

    /* ── Event panel ─────────────────────────────────────────────── */

    .ev-panel {
        width: var(--evw);
        min-width: var(--evw);
        background: var(--bg-panel);
        border-left: 1px solid var(--border);
        display: flex;
        flex-direction: column;
        overflow: hidden;
        flex-shrink: 0;
        position: relative;
        transition: width 0.2s, min-width 0.2s;
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
        border-radius: 4px;
        color: var(--text-muted);
        font-size: 1rem;
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
        padding: 0 14px 0 34px;
        border-bottom: 1px solid var(--border);
        display: flex;
        align-items: center;
        font-size: 0.72rem;
        font-weight: 700;
        color: var(--text-secondary);
        text-transform: uppercase;
        letter-spacing: 0.5px;
        flex-shrink: 0;
    }

    .evp-person {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 10px 14px;
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

    .evp-av img {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }

    .evp-name { display: flex; flex-direction: column; min-width: 0; }

    .evp-name strong {
        font-size: 0.88rem;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        color: var(--text-primary);
    }

    .evp-name span { font-size: 0.75rem; color: var(--text-secondary); }

    .evp-list { flex: 1; overflow-y: auto; padding: 6px 0; }

    .evp-empty { padding: 24px 14px; text-align: center; color: var(--text-muted); font-size: 0.82rem; }

    .ev-item {
        display: flex;
        align-items: flex-start;
        gap: 8px;
        padding: 7px 14px;
        border-bottom: 1px solid var(--border);
        transition: background 0.1s;
    }

    .ev-item:last-child { border-bottom: none; }
    .ev-item:hover { background: var(--bg-card-hover); }

    /* Events directly on the selected person or their conjugal family stand
       out from narrative-context events (children, parents, siblings). */
    .ev-item.ev-item-direct { background: rgba(224, 120, 32, 0.08); }
    .ev-item.ev-item-direct:hover { background: rgba(224, 120, 32, 0.14); }

    .ev-ic {
        width: 24px;
        height: 24px;
        border-radius: 5px;
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: 0.75rem;
        flex-shrink: 0;
        margin-top: 1px;
    }

    .ev-ic-birth { background: rgba(78,168,50,0.18);  color: var(--green);  }
    .ev-ic-death { background: rgba(74,144,217,0.15); color: var(--blue);   }
    .ev-ic-marry { background: rgba(224,120,32,0.15); color: var(--orange); }
    .ev-ic-other { background: var(--bg-card-hover); color: var(--text-secondary); }

    .ev-info { display: flex; flex-direction: column; min-width: 0; flex: 1; }

    .ev-type { font-size: 0.78rem; font-weight: 600; color: var(--text-primary); line-height: 1.3; }
    .ev-date { font-size: 0.72rem; color: var(--text-secondary); line-height: 1.3; }
    .ev-place {
        font-size: 0.72rem; color: var(--text-muted); line-height: 1.3;
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
        min-width: 180px;
        padding: 4px 0;
    }

    .context-menu-header {
        padding: 8px 14px;
        font-size: 0.8rem;
        font-weight: 600;
        color: var(--text-secondary);
        border-bottom: 1px solid var(--border);
    }

    .context-menu-item {
        display: block;
        width: 100%;
        padding: 8px 14px;
        text-align: left;
        background: none;
        border: none;
        font-size: 0.85rem;
        cursor: pointer;
        transition: background 0.1s;
        font-family: var(--font-sans);
        color: var(--text-primary);
    }

    .context-menu-item:hover {
        background: var(--bg-card-hover);
    }

    .context-menu-danger {
        color: var(--color-danger-text);
    }

    .context-menu-danger:hover {
        background: rgba(220, 82, 82, 0.1);
    }

    .context-menu-divider {
        border: none;
        border-top: 1px solid var(--border);
        margin: 4px 0;
    }

    .context-menu-back {
        font-weight: 600;
        color: var(--text-secondary);
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
        padding: 10px 14px;
        pointer-events: none;
    }

    .ref-tooltip-label {
        font-family: var(--font-heading);
        font-weight: 700;
        color: var(--orange);
        margin-bottom: 4px;
    }

    .ref-tooltip-meta {
        font-size: 0.8rem;
        font-style: italic;
        color: var(--text-secondary);
        margin-bottom: 6px;
    }

    .ref-tooltip-text {
        font-size: 0.85rem;
        line-height: 1.4;
        color: var(--text-primary);
    }

    /* ── SVG pedigree connector paths ─────────────────────────────── */

    .pedigree-connector-path {
        fill: none;
        stroke: var(--pn-border);
        stroke-width: 1;
    }

    /* ── Mini pedigree (person detail: ancestors/descendants) ────────
       Pannable but not zoomable — fixed scale, drag to move. ────────── */

    .mini-pedigree {
        position: relative;
        overflow: hidden;
        height: 280px;
        border-radius: var(--radius);
        cursor: grab;
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

    /* ── Animated transitions ──────────────────────────────────── */

    .pedigree-animated .pedigree-inner {
        transition: transform 0.3s ease;
    }

    /* ── Active sidebar button ─────────────────────────────────── */

    .isb-btn-active {
        color: var(--orange) !important;
        background: rgba(224,120,32,0.12);
    }

    .isb-depth-wrap {
        position: relative;
    }

    .pedigree-depth-arrow {
        font-size: 1rem;
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
        padding: 6px 14px 2px;
        font-size: 0.75rem;
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
    }

    /* ── Search person (typeahead) ────────────────────────────────── */

    .search-person {
        margin-top: 8px;
    }

    .search-person-input-row {
        display: flex;
        gap: 8px;
        align-items: center;
        margin-bottom: 8px;
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
        gap: 10px;
        width: 100%;
        padding: 8px 12px;
        background: none;
        border: none;
        border-bottom: 1px solid var(--border);
        cursor: pointer;
        font-family: var(--font-sans);
        font-size: 0.85rem;
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
        flex-shrink: 0;
    }

    .sp-result-initials {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 32px;
        height: 32px;
        border-radius: 50%;
        font-size: 0.75rem;
        font-weight: 700;
        background: rgba(128,128,128,0.15);
        color: var(--text-secondary);
        border: 1px solid var(--border);
    }
    .sp-result-initials.male   { background: rgba(74,144,217,0.12); color: var(--blue); border-color: rgba(74,144,217,0.35); }
    .sp-result-initials.female { background: rgba(196,88,122,0.12); color: var(--pink); border-color: rgba(196,88,122,0.35); }

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
    .sp-surname { text-transform: uppercase; font-size: 0.82rem; }
    .sp-given { font-weight: 400; font-size: 0.82rem; }

    .sp-result-dates {
        display: flex;
        gap: 8px;
        font-size: 0.75rem;
        color: var(--text-secondary);
        margin-top: 1px;
    }
    .sp-birth { color: var(--green, #5aab3c); }
    .sp-death { color: var(--blue, #4a90d9); }

    .sp-result-meta {
        font-size: 0.73rem;
        color: var(--text-muted);
        margin-top: 1px;
    }

    .search-person-result.male { border-left: 3px solid rgba(74,144,217,0.4); }
    .search-person-result.female { border-left: 3px solid rgba(196,88,122,0.4); }

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
        box-shadow: var(--shadow-md);
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
        padding: 16px 20px;
        border-bottom: 1px solid var(--border);
    }

    .person-form-header h2,
    .union-form-header h2 {
        margin: 0;
        font-size: 1.1rem;
        color: var(--text-primary);
    }

    .person-form-body,
    .union-form-body {
        flex: 1;
        overflow-y: auto;
        padding: 16px 20px;
    }

    .pf-footer,
    .uf-footer {
        padding: 14px 20px;
        border-top: 1px solid var(--border);
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 10px;
        flex-shrink: 0;
    }

    .pf-footer-right,
    .uf-footer-right {
        display: flex;
        gap: 8px;
        margin-left: auto;
    }

    .pf-footer .error-msg {
        flex: 1;
        margin: 0;
        font-size: 0.8rem;
    }

    .person-form-close {
        background: none;
        border: none;
        font-size: 1.2rem;
        cursor: pointer;
        color: var(--text-secondary);
        padding: 4px 8px;
        border-radius: 4px;
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
        padding: 10px 12px;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        margin-bottom: 8px;
        gap: 12px;
        background: var(--bg-card);
    }

    /* Match every editable field in the modal (birth name, given names,
       dates, notes, ...) to the same background used by a saved
       .person-form-item row (e.g. a created profession), instead of the
       app-wide input background. */
    .person-form-modal input,
    .person-form-modal select,
    .person-form-modal textarea,
    .union-form-modal input,
    .union-form-modal select,
    .union-form-modal textarea,
    .pf-embedded input,
    .pf-embedded select,
    .pf-embedded textarea {
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
    .pf-embedded textarea {
        resize: vertical;
        min-height: 76px;
        overflow-y: auto;
    }

    .person-form-modal textarea::-webkit-scrollbar,
    .union-form-modal textarea::-webkit-scrollbar,
    .pf-embedded textarea::-webkit-scrollbar {
        width: 10px;
    }
    .person-form-modal textarea::-webkit-scrollbar-track,
    .union-form-modal textarea::-webkit-scrollbar-track,
    .pf-embedded textarea::-webkit-scrollbar-track {
        background: transparent;
    }
    .person-form-modal textarea::-webkit-scrollbar-thumb,
    .union-form-modal textarea::-webkit-scrollbar-thumb,
    .pf-embedded textarea::-webkit-scrollbar-thumb {
        background: var(--text-muted);
        border-radius: 5px;
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
    .pf-embedded select {
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
    .pf-embedded select {
        appearance: none;
        -webkit-appearance: none;
        padding-right: 30px;
        background-color: var(--bg-card);
        background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 6'><path d='M1 1l4 4 4-4' fill='none' stroke='%235c5447' stroke-width='1.6' stroke-linecap='round' stroke-linejoin='round'/></svg>");
        background-repeat: no-repeat;
        background-position: right 11px center;
        background-size: 10px 6px;
    }

    /* The arrow is baked into a data URI, so it cannot read a CSS variable —
       the dark palette needs its own copy. */
    :root.dark .person-form-modal select,
    :root.dark .union-form-modal select,
    :root.dark .pf-embedded select {
        background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 6'><path d='M1 1l4 4 4-4' fill='none' stroke='%237a8da8' stroke-width='1.6' stroke-linecap='round' stroke-linejoin='round'/></svg>");
    }

    .person-form-item.editing {
        display: block;
        padding: 12px;
        background: var(--bg-card);
    }

    .person-form-item-info {
        display: flex;
        align-items: center;
        gap: 8px;
        flex-wrap: wrap;
        flex: 1;
        min-width: 0;
    }

    .person-form-item-actions {
        display: flex;
        gap: 4px;
        flex-shrink: 0;
    }

    /* Empty-state placeholder sized like a .person-form-item row instead of
       the much taller generic .empty-state, so an empty list doesn't jump
       in height once its first entry is added. */
    .pf-empty-item {
        padding: 8px 12px;
        border: 1px dashed var(--border);
        border-radius: var(--radius);
        background: var(--bg-panel);
        color: var(--text-secondary);
        text-align: center;
        margin-bottom: 8px;
    }
    .pf-empty-item p { margin: 0; }

    /* Profession(s) / additional-information rows — same height as a plain
       input (8px vertical padding) instead of the slightly taller default
       .person-form-item used for events/notes. */
    .person-form-item.pf-compact-item { padding: 8px 12px; }

    /* ── Person form — section redesign ────────────────────────────── */

    .pf-subtitle {
        font-size: 0.75rem;
        color: var(--text-secondary);
        display: block;
        margin-top: 2px;
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
        gap: 10px;
    }

    /* The toggle is the section's title, and a real button, so the heading is
       reachable by keyboard rather than being a div you must click. */
    .pf-section-toggle {
        flex: 1;
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
        padding: 0;
        background: none;
        border: none;
        cursor: pointer;
        text-align: left;
        font-family: var(--font-sans);
        font-size: 0.68rem;
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

    .pf-section-body { margin-top: 14px; }

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
        gap: 10px;
        font-size: 0.8rem;
        font-weight: 500;
        margin-bottom: 6px;
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
        padding: 4px 10px;
        border: 1px solid var(--border);
        border-radius: 6px;
        background: transparent;
        color: var(--text-secondary);
        font-size: 0.8rem;
        font-weight: 500;
        font-family: var(--font-sans);
        cursor: pointer;
        transition: color 0.15s, border-color 0.15s, background 0.15s;
    }

    .pf-add-btn::before {
        content: "+";
        font-size: 0.95rem;
        line-height: 1;
    }

    .pf-add-btn.is-open::before { content: "\00d7"; }

    .pf-add-btn:hover {
        color: var(--orange);
        border-color: var(--orange);
        background: rgba(224,120,32,0.07);
    }

    .pf-confirm-btn {
        padding: 6px 14px;
        border-radius: 6px;
        border: 1px solid var(--orange);
        background: rgba(224,120,32,0.10);
        color: var(--orange);
        font-size: 0.82rem;
        font-weight: 600;
        font-family: var(--font-sans);
        cursor: pointer;
        transition: background 0.15s;
    }

    .pf-confirm-btn:hover { background: rgba(224,120,32,0.20); }
    .pf-confirm-btn:disabled { opacity: 0.5; cursor: default; }

    /* Row actions stay legible when idle (muted label, no border) rather
       than disappearing until hover — hidden-on-hover controls are
       unreachable on touch — but they only gain a box once pointed at. */
    .pf-row-btn {
        background: none;
        border: 1px solid transparent;
        border-radius: 5px;
        padding: 3px 9px;
        font-size: 0.78rem;
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
        color: var(--color-danger-text);
        border-color: var(--color-danger-text);
        background: rgba(224,80,80,0.08);
    }

    /* An open sub-form ("add a profession", "add a note", ...) sat on
       --bg-deep, which in the light palette is a hair off the modal's own
       --bg-panel — the box had no edge and its fields read as part of the
       surrounding form. Card background + border makes it a distinct
       container you can see the boundaries of. */
    .pf-subform,
    .pf-section .pf-embedded {
        padding: 14px;
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
    }

    .pf-subform { margin-bottom: 12px; }

    .badge-primary {
        background: rgba(224,120,32,0.12);
        border-color: var(--orange);
        color: var(--orange);
    }

    .pf-gender-group {
        display: flex;
        gap: 6px;
        flex-wrap: wrap;
    }

    .pf-gender-btn {
        padding: 7px 18px;
        border-radius: 6px;
        border: 1px solid var(--border);
        background: transparent;
        color: var(--text-secondary);
        cursor: pointer;
        font-size: 0.85rem;
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
        background: rgba(224,120,32,0.10);
    }

    /* ── Date qualifier row ────────────────────────────────────────── */

    .pf-date-row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
    .pf-date-qualifier-select { flex: 0 0 130px; }
    /* Only sizing here: the box itself (background, border, height, radius,
       and the select chevron) comes from the shared .person-form-modal
       input/select rules above, so these fields cannot drift from the rest
       of the modal. */
    .pf-date-widget {
        display: flex;
        flex-direction: column;
        gap: 4px;
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
        font-size: 0.72rem;
        color: var(--text-secondary);
        padding-left: 2px;
    }
    /* Sits exactly where the literal preview would, so the row never jumps. */
    .pf-date-error {
        font-size: 0.72rem;
        color: var(--red);
        padding-left: 2px;
    }
    /* Centred with the 38px controls it sits between, rather than pinned to
       the top of the row by a hand-tuned line-height. */
    .pf-date-separator {
        line-height: 1;
        font-size: 0.82rem;
        color: var(--text-secondary);
        padding: 0 4px;
        white-space: nowrap;
    }

    /* ── Per-event notes & source ──────────────────────────────────── */

    /* The "Notes et source" toggle is a plain row action (.pf-row-btn), and
       .is-active is what marks its panel as open — it had its own
       .pf-ns-toggle style, permanently bordered and orange on hover, which
       made it louder than the Modifier/Supprimer it sits next to. */

    /* Rendered as a sibling right under its .person-form-item row: the row
       loses its bottom rounding and the panel picks it up, so the two read
       as one card. */
    .person-form-item.pf-ns-open { margin-bottom: 0; border-radius: var(--radius) var(--radius) 0 0; }
    .pf-ns-body {
        padding: 12px;
        margin-bottom: 8px;
        border: 1px solid var(--border);
        border-top: none;
        border-radius: 0 0 var(--radius) var(--radius);
        background: var(--bg-card);
    }
    .pf-ns-actions { display: flex; gap: 8px; align-items: center; }

    /* An event's evidence block. Separated by a rule rather than by a heading:
       it sits below the event's own Save button and writes on its own, so the
       line is there to say "past this point, changes are already saved". */
    .pf-ns-block {
        margin-top: 14px;
        padding-top: 12px;
        border-top: 1px solid var(--border);
    }

    .pf-ns-label {
        display: block;
        font-size: 0.72rem;
        text-transform: uppercase;
        letter-spacing: 0.07em;
        color: var(--orange);
        margin-bottom: 2px;
    }

    .pf-ns-hint {
        font-size: 0.7rem;
        color: var(--text-muted);
        margin: 0 0 8px;
    }

    /* ── Witnesses list ────────────────────────────────────────────── */

    .pf-witness-list { margin-bottom: 6px; }
    .pf-witness-row { display: flex; gap: 6px; align-items: center; margin-bottom: 6px; }
    .pf-witness-row input { flex: 1; }
    .pf-witness-name { font-weight: 500; }
    .pf-witness-relation { color: var(--text-secondary); font-size: 0.88rem; }
    .pf-witness-add { display: flex; flex-direction: column; gap: 6px; margin-top: 6px; }
    .pf-witness-remove {
        flex: 0 0 auto;
        background: none;
        border: 1px solid var(--border);
        border-radius: 4px;
        color: var(--text-secondary);
        cursor: pointer;
        padding: 2px 8px;
        font-size: 0.82rem;
        line-height: 1.6;
        transition: border-color 0.15s, color 0.15s;
    }
    .pf-witness-remove:hover { border-color: #e05050; color: #e05050; }


    /* ── Delete person section ─────────────────────────────────────── */

    .pf-delete-section { margin-top: 8px; }
    /* Same line as .person-form-header's border-bottom, with the same
       ~16px breathing room on each side as the header/body padding gives
       it above "État civil". */
    .pf-delete-person-btn {
        margin-top: 12px;
        background: none;
        border: 1px solid rgba(224, 80, 80, 0.35);
        border-radius: 4px;
        color: #e05050;
        cursor: pointer;
        font-size: 0.85rem;
        padding: 6px 14px;
        transition: border-color 0.15s, background 0.15s;
        width: 100%;
        text-align: center;
    }
    .pf-delete-person-btn:hover { border-color: #e05050; background: rgba(224, 80, 80, 0.08); }
    .pf-delete-confirm,
    .uf-child-detach-confirm {
        background: rgba(224, 80, 80, 0.07);
        border: 1px solid rgba(224, 80, 80, 0.3);
    }

    .pf-delete-confirm {
        border-radius: 6px;
        padding: 16px;
        margin-top: 8px;
    }
    .pf-delete-confirm-name {
        font-weight: 600;
        font-size: 0.95rem;
        margin: 0 0 8px;
        color: var(--text-primary);
    }
    .pf-delete-confirm-message {
        font-size: 0.85rem;
        color: var(--text-secondary);
        margin: 0 0 14px;
        line-height: 1.5;
    }
    .pf-delete-confirm-actions { display: flex; gap: 8px; justify-content: flex-end; }

    /* ── Linking panel ─────────────────────────────────────────────── */

    .linking-card {
        margin-bottom: 24px;
        border: 2px solid var(--orange);
    }

    .linking-panel {
        padding: 16px;
        background: var(--bg-card);
        border-radius: var(--radius);
        margin-top: 12px;
    }

    .linking-panel-title {
        font-size: 0.85rem;
        color: var(--text-secondary);
        margin-bottom: 12px;
    }

    .linking-panel-or {
        text-align: center;
        color: var(--text-secondary);
        font-size: 0.85rem;
        margin: 12px 0;
    }

    /* ── Couple modal — person blocks, children ───────────────────── */

    .uf-child-row {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 8px 12px;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        margin-bottom: 6px;
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
        font-size: 0.75rem;
        color: var(--text-secondary);
        flex-shrink: 0;
    }

    .uf-child-info {
        flex: 1;
        display: flex;
        align-items: center;
        gap: 10px;
        min-width: 0;
        flex-wrap: wrap;
        font-size: 0.85rem;
    }

    .uf-child-detach-confirm {
        border-radius: var(--radius);
        padding: 10px 12px;
        margin-bottom: 6px;
        font-size: 0.83rem;
    }

    .uf-child-detach-confirm p {
        margin: 0 0 8px;
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
        padding: 16px 24px;
    }

    .sr-count {
        font-size: 0.85rem;
        color: var(--text-muted);
        margin: 0;
    }

    /* Filters */
    .sr-filters-toggle {
        margin-bottom: 8px;
    }

    .sr-chevron {
        display: inline-block;
        font-size: 0.6rem;
        margin-left: 4px;
        transition: transform 0.2s;
    }

    .sr-chevron.open {
        transform: rotate(180deg);
    }

    .sr-filters {
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: 8px;
        padding: 12px 16px;
        margin-bottom: 12px;
    }

    .sr-filter-row {
        display: flex;
        gap: 16px;
        flex-wrap: wrap;
        align-items: flex-end;
    }

    .sr-filter-group {
        display: flex;
        flex-direction: column;
        gap: 4px;
        min-width: 120px;
    }

    .sr-filter-group label {
        font-size: 0.75rem;
        color: var(--text-muted);
        font-weight: 500;
        margin-bottom: 0;
    }

    .sr-filter-group select,
    .sr-filter-group input {
        padding: 4px 8px;
        font-size: 0.82rem;
        border: 1px solid var(--border);
        border-radius: 4px;
        background: var(--bg-deep);
        color: var(--text-primary);
    }

    .sr-date-range {
        display: flex;
        align-items: center;
        gap: 6px;
    }

    .sr-date-range input {
        width: 60px;
        text-align: center;
    }

    .sr-date-range span {
        color: var(--text-muted);
    }

    .sr-clear-filters {
        background: none;
        border: none;
        color: var(--orange);
        cursor: pointer;
        font-size: 0.82rem;
        padding: 4px 0;
        margin-top: 8px;
    }

    .sr-clear-filters:hover {
        text-decoration: underline;
    }

    /* Toolbar */
    .sr-toolbar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: 12px;
        padding: 8px 12px;
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: 8px;
    }

    .sr-sort {
        display: flex;
        align-items: center;
        gap: 8px;
    }

    .sr-sort label {
        font-size: 0.82rem;
        color: var(--text-muted);
        margin-bottom: 0;
    }

    .sr-sort select {
        padding: 4px 8px;
        font-size: 0.82rem;
        border: 1px solid var(--border);
        border-radius: 4px;
        background: var(--bg-deep);
        color: var(--text-primary);
    }

    .sr-view-modes {
        display: flex;
        gap: 4px;
    }

    .sr-view-btn {
        background: none;
        border: 1px solid var(--border);
        border-radius: 4px;
        color: var(--text-muted);
        cursor: pointer;
        padding: 4px 8px;
        font-size: 1rem;
    }

    .sr-view-btn.active {
        background: var(--orange);
        color: #fff;
        border-color: var(--orange);
    }

    .sr-view-btn:hover:not(.active) {
        background: var(--bg-card-hover);
    }

    /* Pagination */
    .sr-pagination {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 4px;
        margin-top: 20px;
        padding: 12px 0;
    }

    .sr-page-btn {
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: 4px;
        color: var(--text-primary);
        cursor: pointer;
        padding: 6px 10px;
        font-size: 0.82rem;
        min-width: 32px;
        text-align: center;
    }

    .sr-page-btn.active {
        background: var(--orange);
        color: #fff;
        border-color: var(--orange);
    }

    .sr-page-btn:hover:not(.active):not(:disabled) {
        background: var(--bg-card-hover);
    }

    .sr-page-btn:disabled {
        opacity: 0.4;
        cursor: not-allowed;
    }

    .sr-page-info {
        font-size: 0.8rem;
        color: var(--text-muted);
        margin-left: 12px;
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
        border: 1px solid var(--border);
        border-radius: 6px;
        margin-bottom: 4px;
    }
    a.search-person-result {
        text-decoration: none;
        color: inherit;
        cursor: pointer;
    }

    /* Grid (card) view: one mini-pedigree per result */
    .sr-grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(340px, 1fr));
        gap: 14px;
    }

    .sr-grid-card {
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        overflow: hidden;
        display: flex;
        flex-direction: column;
    }
    .sr-grid-card.male   { border-top: 3px solid rgba(74,144,217,0.4); }
    .sr-grid-card.female { border-top: 3px solid rgba(196,88,122,0.4); }

    a.sr-grid-card-hd {
        display: flex;
        align-items: baseline;
        justify-content: space-between;
        gap: 10px;
        padding: 10px 14px;
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
        font-size: 0.82rem;
    }

    /* Empty state */
    .sr-empty {
        text-align: center;
        padding: 48px 24px;
        color: var(--text-muted);
    }

    /* ── Dictionary page ──────────────────────────────────────────── */

    .dict-tabs {
        display: flex;
        gap: 4px;
        margin-bottom: 12px;
        border-bottom: 1px solid var(--border);
    }

    .dict-tab {
        background: none;
        border: none;
        padding: 8px 14px;
        font-size: 0.88rem;
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
        gap: 4px;
        margin-bottom: 8px;
    }

    .dict-letter-strip {
        display: flex;
        flex-wrap: wrap;
        gap: 4px;
    }

    .dict-total-count {
        margin-left: auto;
        text-align: right;
        white-space: nowrap;
    }

    .dict-letter-btn {
        min-width: 26px;
        padding: 4px 6px;
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: 4px;
        color: var(--text-primary);
        font-size: 0.78rem;
        cursor: pointer;
    }

    .dict-letter-btn.active {
        background: var(--orange);
        color: #fff;
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
        gap: 4px;
        margin-bottom: 10px;
    }

    .dict-src-crumb {
        background: none;
        border: none;
        color: var(--text-muted);
        font-size: 0.82rem;
        cursor: pointer;
        padding: 2px 2px;
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
        font-size: 0.78rem;
    }

    .dict-src-summary {
        font-size: 0.85rem;
        color: var(--text-primary);
        margin-bottom: 8px;
    }

    .dict-src-groups-label {
        font-size: 0.78rem;
        color: var(--text-muted);
        margin: 8px 0 6px;
    }

    .dict-filter-row {
        display: flex;
        align-items: center;
        gap: 12px;
        margin-bottom: 12px;
        flex-wrap: wrap;
    }

    .dict-filter-input {
        flex: 1;
        min-width: 160px;
        padding: 6px 10px;
        font-size: 0.85rem;
        border: 1px solid var(--border);
        border-radius: 4px;
        background: var(--bg-deep);
        color: var(--text-primary);
    }

    .dict-page-size {
        display: flex;
        align-items: center;
        gap: 6px;
        font-size: 0.82rem;
        color: var(--text-muted);
    }

    .dict-page-size select {
        height: 32px;
        padding: 0 8px;
        font-size: 0.82rem;
        border: 1px solid var(--border);
        border-radius: 4px;
        background: var(--bg-deep);
        color: var(--text-primary);
    }

    .dict-warning {
        background: rgba(224, 120, 32, 0.12);
        border: 1px solid var(--orange);
        color: var(--orange);
        border-radius: 6px;
        padding: 8px 12px;
        font-size: 0.82rem;
        margin-bottom: 12px;
    }

    .dict-group-header {
        font-family: var(--font-heading);
        font-size: 0.8rem;
        letter-spacing: 0.05em;
        color: var(--orange);
        padding: 10px 4px 4px;
    }

    .dict-list {
        display: flex;
        flex-direction: column;
        gap: 4px;
    }

    .dict-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 8px 12px;
        border: 1px solid var(--border);
        border-radius: 6px;
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
        gap: 2px;
    }

    .dict-row-value {
        font-size: 0.9rem;
        color: var(--text-primary);
    }

    .dict-row-meta {
        font-size: 0.76rem;
        color: var(--text-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .dict-row-count {
        font-size: 0.78rem;
        color: var(--text-muted);
        white-space: nowrap;
    }

    .dict-row-action {
        background: none;
        border: none;
        color: var(--text-muted);
        cursor: pointer;
        font-size: 0.9rem;
        padding: 4px;
    }

    .dict-row-action:hover {
        color: var(--orange);
    }

    /* Bulk surname-particle editor, opened from a family-name row. */
    .dict-particle-modal {
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: 20px 24px 16px;
        width: min(460px, calc(100vw - 32px));
        box-shadow: var(--shadow-md);
    }

    .dict-particle-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        margin-bottom: 12px;
    }

    .dict-particle-header h2 {
        font-size: 1.05rem;
        color: var(--text-primary);
    }

    .dict-particle-intro {
        color: var(--text-primary);
        font-size: 0.9rem;
    }

    .dict-particle-scope {
        color: var(--text-muted);
        font-size: 0.82rem;
        margin: 4px 0 14px;
    }

    .dict-particle-hint {
        color: var(--text-muted);
        font-size: 0.78rem;
        margin-top: 6px;
    }

    .dict-particle-preview {
        margin-top: 14px;
        padding: 10px 12px;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        background: var(--bg-deep);
    }

    .dict-particle-preview-row {
        display: flex;
        justify-content: space-between;
        gap: 12px;
        font-size: 0.85rem;
        padding: 2px 0;
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
        border-radius: 0 0 6px 6px;
        background: var(--bg-deep);
        padding: 10px 14px;
        margin-top: -6px;
    }

    a.dict-accordion-item {
        display: flex;
        align-items: baseline;
        gap: 8px;
        padding: 4px 0;
        color: var(--text-primary);
        text-decoration: none;
        font-size: 0.85rem;
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
        font-size: 0.78rem;
    }

    .dict-accordion-empty {
        color: var(--text-muted);
        font-size: 0.82rem;
        padding: 4px 0;
    }

    /* ── Import overlay (blocking spinner) ───────────────────────── */

    .import-overlay {
        position: fixed;
        inset: 0;
        z-index: 9999;
        background: rgba(0, 0, 0, 0.75);
        backdrop-filter: blur(6px);
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 1.5rem;
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
        font-size: 1.1rem;
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
        gap: 14px;
    }

    .media-tile {
        display: flex;
        flex-direction: column;
        gap: 6px;
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

    .media-thumb img {
        width: 100%;
        height: 100%;
        /* Cover, not contain: a grid of letterboxed scans is mostly
           background, and the tile is a way in, not the document. */
        object-fit: cover;
        display: block;
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
        font-size: 0.8rem;
        letter-spacing: 0.08em;
        color: var(--text-muted);
        border: 1px solid var(--border);
        border-radius: 3px;
        padding: 3px 7px;
    }

    .media-star {
        position: absolute;
        top: 4px;
        right: 5px;
        color: var(--orange);
        font-size: 0.95rem;
        text-shadow: 0 1px 3px rgba(0,0,0,0.6);
        pointer-events: none;
    }

    .media-pages {
        position: absolute;
        bottom: 4px;
        left: 4px;
        font-size: 0.65rem;
        letter-spacing: 0.03em;
        background: rgba(0,0,0,0.62);
        color: #fff;
        border-radius: 3px;
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
        gap: 2px;
        padding: 4px;
        background: linear-gradient(transparent, rgba(0,0,0,0.72));
        opacity: 0;
        transition: opacity 0.15s ease;
    }

    .media-thumb:hover .media-tile-actions,
    .media-tile-actions:focus-within { opacity: 1; }

    .media-act {
        background: none;
        border: none;
        color: #e8e3d8;
        font-size: 0.82rem;
        line-height: 1;
        padding: 4px 5px;
        border-radius: 3px;
        cursor: pointer;
        text-decoration: none;
    }

    .media-act:hover { background: rgba(255,255,255,0.16); }
    .media-act.is-on { color: var(--orange); }
    .media-act.is-danger:hover { background: var(--red, #b8342a); color: #fff; }
    .media-act:disabled { opacity: 0.45; cursor: default; }

    .media-confirm {
        position: absolute;
        inset: 0;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 8px;
        padding: 8px;
        text-align: center;
        font-size: 0.72rem;
        background: rgba(10,11,13,0.9);
        color: var(--text-primary);
    }

    .media-confirm-actions { display: flex; gap: 6px; }

    .media-caption {
        font-size: 0.72rem;
        color: var(--text-muted);
        /* One line, ellipsised: file names run long and a two-line caption
           makes neighbouring tiles sit at different heights. */
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .media-empty {
        font-size: 0.82rem;
        color: var(--text-muted);
        padding: 8px 0;
    }

    /* ── Upload cell ──────────────────────────────────────────────── */

    .media-drop { aspect-ratio: 1; }

    .media-drop-btn {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 4px;
        width: 100%;
        height: 100%;
        padding: 8px;
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
        background: rgba(224,120,32,0.08);
    }

    .media-drop-btn:disabled { cursor: default; }
    .media-drop-icon { font-size: 1.3rem; line-height: 1; }
    .media-drop-label { font-size: 0.74rem; }
    .media-drop-hint {
        font-size: 0.63rem;
        opacity: 0.75;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        max-width: 100%;
    }

    /* ── Inline media edit panel ──────────────────────────────────── */

    .media-panel {
        margin-top: 14px;
        padding: 14px;
        border: 1px solid var(--orange);
        border-radius: var(--radius);
        background: var(--bg-panel);
    }

    .media-panel-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 10px;
        margin-bottom: 4px;
    }

    .media-panel-title {
        font-family: var(--font-heading);
        font-size: 0.92rem;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .media-panel-meta {
        display: flex;
        flex-wrap: wrap;
        gap: 10px;
        margin-bottom: 12px;
        font-size: 0.7rem;
        color: var(--text-muted);
    }

    .media-panel-section { margin-top: 12px; }
    .media-panel-section > label {
        display: block;
        margin-bottom: 6px;
        font-size: 0.72rem;
        color: var(--text-muted);
    }

    .media-panel-actions {
        display: flex;
        justify-content: flex-end;
        margin-top: 12px;
    }

    /* ── Vignette list ────────────────────────────────────────────── */

    .vg-list { display: flex; flex-direction: column; gap: 8px; }
    .vg-empty { font-size: 0.75rem; color: var(--text-muted); }

    .vg-row {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 6px;
        border: 1px solid var(--border);
        border-radius: var(--radius);
    }

    .vg-thumb {
        width: 56px;
        height: 42px;
        object-fit: cover;
        border-radius: 3px;
        flex: 0 0 auto;
        background: var(--bg-deep);
    }

    .vg-body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 4px; }
    .vg-name {
        font-size: 0.78rem;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .vg-select { font-size: 0.72rem; padding: 3px 6px; }
    .vg-actions { display: flex; gap: 4px; flex: 0 0 auto; }

    /* ── Image cropper ────────────────────────────────────────────── */

    .cropper-backdrop {
        position: fixed;
        inset: 0;
        z-index: 1200;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 24px;
        background: rgba(6,7,9,0.78);
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

    .cropper-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 10px 14px;
        border-bottom: 1px solid var(--border);
    }

    .cropper-title {
        font-family: var(--font-heading);
        font-size: 0.92rem;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .cropper-close {
        background: none;
        border: none;
        color: var(--text-muted);
        font-size: 1.3rem;
        line-height: 1;
        cursor: pointer;
        padding: 0 4px;
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
        background: rgba(224,120,32,0.14);
        pointer-events: none;
    }

    /* Crops already recorded, so the user can see what is covered while
       drawing the next one. Dashed and muted so the live selection stays
       the thing the eye goes to. */
    .cropper-existing {
        position: absolute;
        border: 1px dashed rgba(232,223,200,0.65);
        background: rgba(232,223,200,0.06);
        pointer-events: none;
    }

    .cropper-existing-label {
        position: absolute;
        top: 0;
        left: 0;
        font-size: 0.62rem;
        padding: 1px 4px;
        background: rgba(0,0,0,0.6);
        color: #fff;
        white-space: nowrap;
    }

    .cropper-foot {
        display: flex;
        flex-direction: column;
        gap: 10px;
        padding: 12px 14px;
        border-top: 1px solid var(--border);
    }

    .cropper-hint, .cropper-readout {
        font-size: 0.74rem;
        color: var(--text-muted);
    }

    .cropper-fields {
        display: flex;
        flex-wrap: wrap;
        gap: 12px;
    }
    .cropper-fields .form-group { flex: 1 1 200px; margin: 0; }

    .cropper-actions {
        display: flex;
        justify-content: flex-end;
        gap: 8px;
    }

    .cropper-empty {
        padding: 24px;
        font-size: 0.85rem;
        color: var(--text-muted);
    }

    /* ── Media: kinds, remote, viewer (Sprint F.3) ────────────────── */

    .media-thumb[role="button"] { cursor: pointer; }

    .media-thumb-icon {
        flex-direction: column;
        gap: 5px;
    }

    .media-glyph { font-size: 1.5rem; line-height: 1; }
    .media-glyph-large { font-size: 3rem; line-height: 1; }

    /* A media whose bytes are somebody else's. Marked, because a broken tile
       on a remote file means their server, not ours, and the reader needs to
       be able to tell. */
    .media-remote {
        position: absolute;
        top: 4px;
        left: 5px;
        font-size: 0.7rem;
        opacity: 0.85;
        text-shadow: 0 1px 3px rgba(0,0,0,0.6);
        pointer-events: none;
    }

    .media-events {
        display: flex;
        flex-direction: column;
        gap: 4px;
        max-height: 180px;
        overflow-y: auto;
    }

    .media-event-row {
        display: flex;
        align-items: center;
        gap: 8px;
        font-size: 0.76rem;
        cursor: pointer;
    }

    .media-event-row input { margin: 0; flex: 0 0 auto; }

    .media-viewer {
        display: flex;
        flex-direction: column;
        max-width: min(1400px, 100%);
        max-height: 100%;
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        overflow: hidden;
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
    }

    .media-viewer-aside {
        flex: 0 0 300px;
        border-right: 1px solid var(--border);
        overflow-y: auto;
        padding: 14px;
        background: var(--bg-card);
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
        gap: 12px;
    }

    .media-fact {
        display: flex;
        gap: 8px;
        align-items: baseline;
        font-size: 0.82rem;
    }

    /* A description or a note is a paragraph, not a value beside a label. */
    .media-fact.is-prose { flex-direction: column; gap: 3px; }

    .media-fact-label {
        flex: 0 0 auto;
        color: var(--text-muted);
        font-size: 0.72rem;
        text-transform: uppercase;
        letter-spacing: 0.06em;
    }

    .media-fact-value {
        color: var(--text-primary);
        line-height: 1.5;
        margin: 0;
        overflow-wrap: anywhere;
    }

    .media-fact-tags { display: flex; flex-wrap: wrap; gap: 4px; }

    .media-fact-tag {
        background: var(--bg-deep);
        border: 1px solid var(--border);
        border-radius: 10px;
        color: var(--text-secondary);
        font-size: 0.74rem;
        padding: 2px 8px;
    }

    .media-fact-tech {
        display: flex;
        flex-wrap: wrap;
        gap: 10px;
        border-top: 1px solid var(--border);
        padding-top: 10px;
        color: var(--text-muted);
        font-size: 0.72rem;
    }

    .media-facts-edit { width: 100%; margin-top: 14px; }

    /* Inside the viewer the panel is the column, not a card floating in one. */
    .media-panel.is-embedded {
        background: none;
        border: none;
        border-radius: 0;
        margin: 0;
        padding: 0;
    }

    .media-viewer-stage {
        flex: 1;
        min-height: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 12px;
        overflow: auto;
        background: var(--bg-deep);
    }

    /* Zoomed, the stage stops centring: an image wider than its container
       must start at the top-left and be scrolled, or the parts that overflow
       are unreachable in both directions at once. */
    .media-viewer-stage.is-zoomed {
        align-items: flex-start;
        justify-content: flex-start;
    }

    /* Contain, not cover: this is the view where the document is the point,
       so nothing may be cropped out of it. The inline `width` a zoom level
       sets overrides both maxima. */
    .media-viewer-image {
        max-width: 100%;
        max-height: 70vh;
        object-fit: contain;
    }

    .media-viewer-stage.is-zoomed .media-viewer-image {
        /* Nothing to contain into once it is larger than the stage, and
           `contain` would fight the explicit width. */
        object-fit: none;
        height: auto;
    }

    .media-zoom {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 4px;
        padding: 6px 0 0;
    }

    .media-zoom-btn,
    .media-zoom-level {
        background: none;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        color: var(--text-secondary);
        font-family: var(--font-sans);
        font-size: 0.78rem;
        cursor: pointer;
        transition: border-color 0.15s, color 0.15s;
    }

    .media-zoom-btn {
        width: 28px;
        height: 26px;
        line-height: 1;
    }

    /* Wide enough for "400 %" so the row does not shift as the number grows. */
    .media-zoom-level {
        min-width: 66px;
        height: 26px;
        padding: 0 8px;
    }

    .media-zoom-btn:hover:not(:disabled),
    .media-zoom-level:hover { border-color: var(--orange); color: var(--text-primary); }
    .media-zoom-btn:disabled { opacity: 0.4; cursor: default; }
    .media-zoom-level.is-fit { color: var(--text-muted); }

    .media-viewer-audio { width: min(520px, 100%); }

    .media-viewer-fallback {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 10px;
        padding: 32px;
        text-align: center;
        color: var(--text-muted);
        font-size: 0.85rem;
    }

    .media-viewer-path {
        font-size: 0.72rem;
        word-break: break-all;
        max-width: 40ch;
    }

    .media-viewer-desc { font-size: 0.8rem; margin: 0; }

    /* ── Multi-page documents ─────────────────────────────────────── */

    .doc-pages {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(88px, 1fr));
        gap: 8px;
    }

    .doc-page {
        position: relative;
        display: flex;
        flex-direction: column;
        gap: 4px;
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
        font-size: 0.64rem;
        padding: 0 5px;
        border-radius: 3px;
        background: rgba(0,0,0,0.66);
        color: #fff;
    }

    .doc-page-actions { display: flex; justify-content: center; gap: 2px; }

    /* The document's own upload cell sits in the same grid as its pages, so
       "add a page" is the cell after the last one. */
    .doc-pages .media-drop { aspect-ratio: 3 / 4; }

    /* ── Page navigation ──────────────────────────────────────────── */

    .media-pager {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 6px;
        padding: 8px 12px;
        border-top: 1px solid var(--border);
    }

    .media-pager-btn {
        background: none;
        border: 1px solid var(--border);
        border-radius: 3px;
        color: var(--text-primary);
        font-size: 0.8rem;
        line-height: 1;
        padding: 5px 8px;
        cursor: pointer;
    }

    .media-pager-btn:hover:not(:disabled) { border-color: var(--orange); color: var(--orange); }
    .media-pager-btn:disabled { opacity: 0.35; cursor: default; }

    /* Scrolls rather than wraps: a forty-page register must not push the
       image out of the panel to make room for its own page numbers. */
    .media-pager-numbers {
        display: flex;
        gap: 3px;
        overflow-x: auto;
        max-width: min(520px, 60vw);
        padding: 2px;
    }

    .media-pager-num {
        flex: 0 0 auto;
        min-width: 26px;
        background: none;
        border: 1px solid transparent;
        border-radius: 3px;
        color: var(--text-muted);
        font-size: 0.74rem;
        padding: 4px 6px;
        cursor: pointer;
    }

    .media-pager-gap {
        flex: 0 0 auto;
        color: var(--text-muted);
        font-size: 0.74rem;
        padding: 4px 2px;
        user-select: none;
    }

    .media-pager-num:hover { color: var(--text-primary); border-color: var(--border); }
    .media-pager-num.is-current {
        color: var(--orange);
        border-color: var(--orange);
        background: rgba(224,120,32,0.12);
    }

    .media-pager-count { font-size: 0.74rem; color: var(--text-muted); }

    /* ── Event evidence on the profile timeline ───────────────────── */

    .pd-ev-evidence {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
        margin-top: 6px;
    }

    .pd-ev-doc {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 44px;
        height: 44px;
        border: 1px solid var(--border);
        border-radius: 3px;
        overflow: hidden;
        background: var(--bg-deep);
        flex: 0 0 auto;
    }

    .pd-ev-doc:hover { border-color: var(--orange); }
    .pd-ev-doc img { width: 100%; height: 100%; object-fit: cover; }

    /* ── Scrollbar ────────────────────────────────────────────────── */

    ::-webkit-scrollbar { width: 6px; height: 6px; }
    ::-webkit-scrollbar-track { background: var(--bg-deep); }
    ::-webkit-scrollbar-thumb { background: var(--border); border-radius: 3px; }
    ::-webkit-scrollbar-thumb:hover { background: var(--text-muted); }

    /* ── Responsive ───────────────────────────────────────────────── */

    @media (max-width: 640px) {
        .app-nav { padding: 0 1rem; }
        .sub-page-content { padding: 16px 12px; }
        .td-topbar { padding: 10px 12px; }
        .td-bc { gap: 4px; }
        .td-bc-link { max-width: clamp(36px, 22vw, 140px); }
        .td-bc-current { max-width: clamp(32px, 16vw, 96px); }
        .td-search-input { width: clamp(72px, 22vw, 110px); }
        .dict-alphabet {
            flex-wrap: nowrap;
            overflow-x: auto;
            padding-bottom: 4px;
        }
        .dict-letter-btn { flex: 0 0 auto; }
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
        box-shadow: var(--shadow-md);
        width: min(820px, 94vw);
        max-height: 90vh;
        display: flex;
        flex-direction: column;
    }

    .import-modal-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 18px 22px 14px;
        border-bottom: 1px solid var(--border);
    }

    .import-modal-header h2 {
        font-family: var(--font-heading);
        font-size: 1.15rem;
        color: var(--text-primary);
        margin: 0;
    }

    .import-tabs {
        display: flex;
        gap: 4px;
        padding: 10px 22px 0;
        border-bottom: 1px solid var(--border);
    }

    .import-tab {
        background: none;
        border: none;
        border-bottom: 2px solid transparent;
        color: var(--text-muted);
        font-family: var(--font-sans);
        font-size: 0.86rem;
        padding: 8px 14px;
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
        padding: 20px 22px 22px;
        overflow-y: auto;
    }

    /* ── File tab ─────────────────────────────────────────────────── */

    .import-drop {
        border: 2px dashed var(--border);
        border-radius: var(--radius);
        padding: 34px 20px;
        text-align: center;
        cursor: pointer;
        transition: border-color 0.15s, background 0.15s;
    }

    .import-drop:hover,
    .import-drop.is-dragging {
        border-color: var(--orange);
        background: rgba(224, 120, 32, 0.06);
    }

    .import-drop-icon { font-size: 1.9rem; line-height: 1; margin-bottom: 10px; }
    .import-drop-label { color: var(--text-primary); font-size: 0.92rem; }
    .import-drop-name {
        color: var(--text-primary);
        font-size: 0.92rem;
        font-weight: 600;
        /* A long export name truncates from the middle in the summary lines,
           but here it has the width to sit whole. */
        word-break: break-all;
    }
    .import-drop-hint {
        color: var(--text-muted);
        font-size: 0.78rem;
        margin-top: 6px;
    }

    /* ── Shared: stat row, result, warnings ───────────────────────── */

    .import-stats {
        display: grid;
        grid-template-columns: repeat(4, 1fr);
        gap: 10px;
        margin: 16px 0;
    }

    .import-stat {
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: 12px 10px;
        text-align: center;
    }

    .import-stat-value {
        font-family: var(--font-heading);
        font-size: 1.3rem;
        color: var(--orange);
        line-height: 1.1;
    }

    .import-stat-label {
        color: var(--text-muted);
        font-size: 0.72rem;
        margin-top: 4px;
    }

    .import-done { text-align: center; padding: 8px 0; }
    .import-done-icon {
        font-size: 2rem;
        color: var(--green-light);
        line-height: 1;
    }
    .import-done h3 {
        font-family: var(--font-heading);
        color: var(--text-primary);
        margin: 8px 0 4px;
    }

    .import-warnings {
        text-align: left;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: 10px 14px;
        margin-top: 12px;
    }
    .import-warnings summary {
        cursor: pointer;
        color: var(--text-secondary);
        font-size: 0.82rem;
    }
    .import-warnings ul {
        margin: 10px 0 0 18px;
        max-height: 220px;
        overflow-y: auto;
        color: var(--text-muted);
        font-size: 0.78rem;
    }
    .import-warnings li { margin-bottom: 4px; }

    /* ── Geneanet steps ───────────────────────────────────────────── */

    .gn-steps { display: flex; flex-direction: column; gap: 8px; }

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
        gap: 10px;
        width: 100%;
        background: none;
        border: none;
        padding: 12px 14px;
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
        font-size: 0.74rem;
        display: flex;
        align-items: center;
        justify-content: center;
    }

    .gn-step-mark.is-done {
        background: var(--green-accent);
        border-color: var(--green-accent);
        color: #fff;
    }

    .gn-step-title {
        color: var(--text-primary);
        font-size: 0.88rem;
        flex: 0 0 auto;
    }

    /* The receipt of a settled step. Truncates from the end of the line, but
       the counts sit last so they survive — the file name is the part with
       room to spare. */
    .gn-step-summary {
        color: var(--text-muted);
        font-size: 0.78rem;
        flex: 1 1 auto;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gn-step-edit {
        margin-left: auto;
        color: var(--orange);
        font-size: 0.76rem;
        flex: 0 0 auto;
    }

    .gn-step-body {
        padding: 4px 14px 16px 46px;
        border-top: 1px solid var(--border);
    }

    .gn-lead {
        color: var(--text-secondary);
        font-size: 0.84rem;
        line-height: 1.55;
        margin: 12px 0;
    }

    .gn-note {
        color: var(--text-muted);
        font-size: 0.79rem;
        line-height: 1.5;
        margin: 8px 0;
    }

    .gn-howto {
        margin: 10px 0 12px 18px;
        color: var(--text-secondary);
        font-size: 0.82rem;
        line-height: 1.7;
    }

    .gn-aside {
        border-top: 1px solid var(--border);
        padding-top: 10px;
        margin: 12px 0;
    }
    .gn-aside summary {
        cursor: pointer;
        color: var(--orange);
        font-size: 0.79rem;
    }
    .gn-aside p {
        color: var(--text-muted);
        font-size: 0.79rem;
        line-height: 1.55;
        margin-top: 8px;
    }

    .gn-warn-box {
        background: rgba(224, 120, 32, 0.08);
        border-left: 3px solid var(--orange);
        border-radius: 4px;
        padding: 10px 14px;
        color: var(--text-secondary);
        font-size: 0.81rem;
        line-height: 1.5;
        margin: 12px 0;
    }

    .gn-desktop-only {
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: 14px 16px;
        margin: 12px 0;
    }
    .gn-desktop-only strong {
        color: var(--text-primary);
        font-size: 0.86rem;
        display: block;
        margin-bottom: 6px;
    }
    .gn-desktop-only p {
        color: var(--text-muted);
        font-size: 0.8rem;
        line-height: 1.55;
        margin: 0;
    }

    /* ── Archive list ─────────────────────────────────────────────── */

    .gn-archive-list {
        list-style: none;
        margin: 12px 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: 6px;
    }

    .gn-archive-list li {
        display: flex;
        align-items: center;
        gap: 10px;
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        padding: 8px 12px;
    }

    .gn-archive-name {
        color: var(--text-primary);
        font-size: 0.82rem;
        flex: 1 1 auto;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .gn-archive-count {
        color: var(--text-muted);
        font-size: 0.76rem;
        flex: 0 0 auto;
    }

    .gn-archive-remove {
        background: none;
        border: none;
        color: var(--text-muted);
        cursor: pointer;
        font-size: 0.85rem;
        padding: 2px 4px;
        flex: 0 0 auto;
    }
    .gn-archive-remove:hover { color: var(--red); }

    /* ── Progress ─────────────────────────────────────────────────── */

    .gn-progress-block { margin: 14px 0; }

    .gn-progress-label {
        color: var(--text-secondary);
        font-size: 0.82rem;
        margin-bottom: 8px;
    }

    .gn-progress {
        height: 8px;
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: 999px;
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
        font-size: 0.74rem;
        margin-top: 6px;
    }

    /* ── Step 4 findings ──────────────────────────────────────────── */

    .gn-findings {
        list-style: none;
        margin: 14px 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: 8px;
    }

    .gn-findings li {
        font-size: 0.82rem;
        line-height: 1.5;
        padding-left: 22px;
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
        margin: 8px 0 0 16px;
        max-height: 180px;
        overflow-y: auto;
        color: var(--text-muted);
        font-size: 0.78rem;
    }

    .gn-mismatch {
        background: rgba(224, 120, 32, 0.10);
        border: 1px solid rgba(224, 120, 32, 0.35);
        border-radius: var(--radius);
        padding: 14px 16px;
        margin-top: 14px;
    }

    .gn-mismatch p {
        color: var(--text-primary);
        font-size: 0.84rem;
        line-height: 1.55;
        margin: 0;
    }

    /* ── Responsive ───────────────────────────────────────────────── */

    @media (max-width: 900px) {
        .import-stats { grid-template-columns: repeat(2, 1fr); }
        /* The step body loses the indent that lined it up under the title:
           at this width the indent costs more than the alignment buys. */
        .gn-step-body { padding-left: 14px; }
    }

    @media (max-width: 560px) {
        .import-stats { grid-template-columns: 1fr; }
        /* The summary drops below the title rather than competing with it
           for a line that no longer fits both. */
        .gn-step-head { flex-wrap: wrap; }
        .gn-step-summary { flex-basis: 100%; padding-left: 32px; }
        .gn-step-edit { margin-left: 0; }
    }


    /* ── Saving and reloading a Geneanet session ──────────────────── */

    /* Deliberately quiet. These sit inside a step whose own action is the
       thing to do next, so they read as a footnote to it rather than as a
       second choice competing for the same attention. */
    .gn-session {
        border-top: 1px solid var(--border);
        margin-top: 14px;
        padding-top: 10px;
    }

    .gn-session-why summary {
        cursor: pointer;
        color: var(--text-muted);
        font-size: 0.76rem;
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
        font-size: 0.76rem;
        line-height: 1.5;
        margin: 6px 0 0 12px;
    }

    .gn-session-actions {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
        margin-top: 8px;
    }

    .gn-session-btn {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        background: none;
        border: 1px solid var(--border);
        border-radius: var(--radius);
        color: var(--text-secondary);
        font-family: var(--font-sans);
        font-size: 0.76rem;
        padding: 5px 10px;
        cursor: pointer;
        transition: border-color 0.15s, color 0.15s;
    }
    .gn-session-btn:hover:not(:disabled) {
        border-color: var(--orange);
        color: var(--text-primary);
    }
    .gn-session-btn:disabled { opacity: 0.5; cursor: default; }
    .gn-session-icon { font-size: 0.85rem; line-height: 1; }

"#;
