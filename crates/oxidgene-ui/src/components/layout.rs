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
/// falling back to the OS-level `prefers-color-scheme` media query.
/// Returns the shared signal so the Layout can consume it if needed.
pub fn use_init_theme() -> Signal<bool> {
    let mut is_dark = use_context_provider(|| Signal::new(false));

    use_effect(move || {
        spawn(async move {
            let result = document::eval(
                r#"
                let theme = localStorage.getItem('oxidgene-theme');
                if (theme === 'dark' || (!theme && window.matchMedia('(prefers-color-scheme: dark)').matches)) {
                    document.documentElement.classList.add('dark');
                    return 'dark';
                }
                return 'light';
                "#,
            );
            if let Ok(val) = result.await
                && val.as_str() == Some("dark")
            {
                is_dark.set(true);
            }
        });
    });

    is_dark
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
    let _theme_signal = use_init_theme();
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
        --bg-deep:        #f4f2ee;
        --bg-panel:       #ede9e2;
        --bg-card:        #ffffff;
        --bg-card-hover:  #f5f3ef;
        --border:         #d4ccc0;
        --border-glow:    #e07820;
        --orange:         #e07820;
        --orange-light:   #f5a03a;
        --green:          #4ea832;
        --green-light:    #7ec45f;
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
        --pn-text:        #111111;
        --pn-text-muted:  #555555;
    }

    :root.dark {
        /* ── Dark pedigree node overrides ───────────────────────────── */
        --pn-bg:         #1e2330;
        --pn-spouse-bg:  #252d3d;
        --pn-text:       #e8dfc8;
        --pn-text-muted: #7a8da8;
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
    }

    .td-bc {
        display: flex;
        align-items: center;
        gap: 6px;
        font-size: 0.88rem;
        min-width: 0;
        flex-shrink: 1;
    }

    .td-bc a {
        color: var(--text-secondary);
        text-decoration: none;
        transition: color 0.15s;
    }

    .td-bc a:hover { color: var(--orange); }

    .td-bc-sep { color: var(--text-muted); margin: 0 2px; }

    .td-bc-link {
        color: var(--text-secondary);
        font-size: 0.88rem;
    }

    .td-bc-current {
        color: var(--text-primary);
        font-weight: 600;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        max-width: 220px;
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

    .pedigree-inner {
        position: absolute;
        top: 0;
        left: 0;
        width: 100%;
        height: 100%;
        transform-origin: 0 0;
    }

    .pedigree-tree {
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

    /* ── SVG person group hover ──────────────────────────────────────*/

    .pedigree-tree {
        position: relative;
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
        /* Event panel as drawer on mobile */
        .ev-panel {
            position: absolute;
            right: 0;
            top: 0;
            bottom: 0;
            z-index: 50;
            box-shadow: var(--shadow-md);
        }
        .ev-panel-collapsed {
            width: 0;
            min-width: 0;
            border-left: none;
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

    /* ── Person form modal ────────────────────────────────────────── */

    .person-form-backdrop {
        display: flex;
        align-items: center;
        justify-content: center;
    }

    .person-form-modal {
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        box-shadow: var(--shadow-md);
        width: 700px;
        max-width: 95vw;
        max-height: 85vh;
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    .person-form-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 16px 20px;
        border-bottom: 1px solid var(--border);
    }

    .person-form-header h2 {
        margin: 0;
        font-size: 1.1rem;
        color: var(--text-primary);
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

    .person-form-body {
        flex: 1;
        overflow-y: auto;
        padding: 16px 20px;
    }

    .person-form-section { }

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

    /* ── Person form — section redesign ────────────────────────────── */

    .pf-subtitle {
        font-size: 0.75rem;
        color: var(--text-secondary);
        display: block;
        margin-top: 2px;
    }

    .pf-section-title {
        font-size: 0.68rem;
        font-weight: 700;
        letter-spacing: 0.12em;
        text-transform: uppercase;
        color: var(--orange);
        margin-bottom: 14px;
        display: flex;
        align-items: center;
        gap: 10px;
    }

    .pf-section-title::after {
        content: "";
        flex: 1;
        height: 1px;
        background: rgba(255,255,255,0.07);
    }

    .pf-section-title.has-action::after { display: none; }

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
        border-color: rgba(255,255,255,0.25);
        color: var(--text-primary);
    }

    .pf-gender-btn.active {
        border-color: var(--orange);
        color: var(--orange);
        background: rgba(224,120,32,0.10);
    }

    .pf-footer {
        padding: 14px 20px;
        border-top: 1px solid var(--border);
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 10px;
        flex-shrink: 0;
    }

    .pf-footer-right {
        display: flex;
        gap: 8px;
        margin-left: auto;
    }

    .pf-footer .error-msg {
        flex: 1;
        margin: 0;
        font-size: 0.8rem;
    }

    /* ── Date qualifier row ────────────────────────────────────────── */

    .pf-date-row { display: flex; gap: 8px; align-items: flex-start; flex-wrap: wrap; }
    .pf-date-qualifier-select { flex: 0 0 130px; }
    .pf-date-input { flex: 1; min-width: 100px; }
    .pf-date-separator {
        line-height: 36px;
        font-size: 0.82rem;
        color: var(--text-secondary);
        padding: 0 4px;
        white-space: nowrap;
        align-self: flex-start;
        padding-top: 7px;
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

    /* ── Collapsible additional fields ─────────────────────────────── */

    .pf-collapsible-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        cursor: pointer;
        user-select: none;
    }
    .pf-collapsible-toggle {
        background: none;
        border: 1px solid var(--border);
        border-radius: 4px;
        color: var(--text-secondary);
        cursor: pointer;
        font-size: 0.75rem;
        padding: 2px 10px;
        line-height: 1.6;
        transition: border-color 0.15s, color 0.15s;
    }
    .pf-collapsible-toggle:hover { border-color: var(--orange); color: var(--orange); }

    .pf-additional-body { margin-top: 14px; display: flex; flex-direction: column; gap: 14px; }
    .pf-additional-group {
        background: rgba(255,255,255,0.03);
        border: 1px solid var(--border);
        border-radius: 6px;
        padding: 12px 14px;
    }
    .pf-additional-group-title {
        font-size: 0.72rem;
        font-weight: 700;
        color: var(--text-secondary);
        text-transform: uppercase;
        letter-spacing: 0.1em;
        margin-bottom: 10px;
    }

    /* ── Delete person section ─────────────────────────────────────── */

    .pf-delete-section { margin-top: 8px; }
    .pf-delete-divider { border: none; border-top: 1px solid var(--border); margin: 0; }
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
    .pf-delete-confirm {
        background: rgba(224, 80, 80, 0.07);
        border: 1px solid rgba(224, 80, 80, 0.3);
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

    /* ── Union form modal ─────────────────────────────────────────── */

    .union-form-backdrop { }

    .union-form-modal {
        background: var(--bg-panel);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        box-shadow: var(--shadow-md);
        width: 720px;
        max-width: 95vw;
        max-height: 90vh;
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    .union-form-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 16px 20px;
        border-bottom: 1px solid var(--border);
    }

    .union-form-header h2 {
        margin: 0;
        font-size: 1.1rem;
        color: var(--text-primary);
    }

    .union-form-body {
        flex: 1;
        overflow-y: auto;
        padding: 16px 20px;
    }

    .union-form-section {
        margin-bottom: 24px;
    }

    .union-form-section:last-child {
        margin-bottom: 0;
    }

    /* ── Couple modal — person blocks, children, footer ───────────── */

    .uf-footer {
        padding: 14px 20px;
        border-top: 1px solid var(--border);
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 10px;
        flex-shrink: 0;
    }

    .uf-footer-right {
        display: flex;
        gap: 8px;
        margin-left: auto;
    }

    .uf-section-toggle {
        display: flex;
        align-items: center;
        justify-content: space-between;
        width: 100%;
        background: none;
        border: none;
        cursor: pointer;
        padding: 0;
        font-family: var(--font-sans);
        text-align: left;
    }

    .uf-section-toggle .pf-section-title {
        flex: 1;
        margin-bottom: 0;
    }

    .uf-chevron {
        font-size: 0.7rem;
        color: var(--text-secondary);
        transition: transform 0.15s;
        margin-left: 8px;
    }

    .uf-chevron.open {
        transform: rotate(90deg);
    }

    .uf-person-block {
        margin-bottom: 24px;
        padding-bottom: 4px;
    }

    .uf-person-block .pf-embedded {
        margin-top: 12px;
        padding: 14px;
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
    }

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
        background: rgba(224, 80, 80, 0.07);
        border: 1px solid rgba(224, 80, 80, 0.3);
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

    /* Empty state */
    .sr-empty {
        text-align: center;
        padding: 48px 24px;
        color: var(--text-muted);
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

    .import-overlay-text {
        font-family: var(--font-heading);
        font-size: 1.1rem;
        color: var(--text-primary);
        letter-spacing: 0.04em;
    }

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
    }
"#;
