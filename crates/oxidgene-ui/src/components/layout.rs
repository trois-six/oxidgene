//! Application layout with navigation bar.
//!
//! Wraps all routed pages with a consistent header/nav and renders the
//! active route via [`Outlet`].

use dioxus::prelude::*;

use crate::router::Route;

/// Shared layout rendered around every page.
///
/// Contains a navigation bar and an [`Outlet`] for the matched child route.
#[component]
pub fn Layout() -> Element {
    rsx! {
        style { {LAYOUT_STYLES} }

        nav { class: "app-nav",
            Link { to: Route::Home {}, class: "nav-logo",
                span { class: "nav-logo-text", "OxidGene" }
            }
            div { class: "nav-links",
                Link { to: Route::TreeList {}, class: "nav-link", "Trees" }
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
        /* ── Dark palette ─────────────────────────────────────────── */
        --bg-deep:        #0d0f14;
        --bg-panel:       #111318;
        --bg-card:        #16191f;
        --bg-card-hover:  #1c2030;
        --border:         #252d3d;
        --border-glow:    #e07820;
        --orange:         #e07820;
        --orange-light:   #f5a03a;
        --green:          #4ea832;
        --green-light:    #7ec45f;
        --blue:           #4a90d9;
        --pink:           #c4587a;
        --sel-bg:         #192038;
        --text-primary:   #ddd8cc;
        --text-secondary: #7a8da8;
        --text-muted:     #404f65;

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
        --shadow-sm:  0 1px 3px rgba(0,0,0,0.35);
        --shadow-md:  0 4px 16px rgba(0,0,0,0.55);
        --radius: 8px;
        --font-sans:    'Lato', -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
        --font-heading: 'Cinzel', Georgia, serif;
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

    /* Subtle radial light leaks on the page background */
    body::before {
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
        background: rgba(10, 11, 13, 0.92);
        backdrop-filter: blur(12px);
        -webkit-backdrop-filter: blur(12px);
        color: var(--text-primary);
        padding: 0 2.5rem;
        height: 64px;
        border-bottom: 1px solid var(--border);
        box-shadow: 0 2px 40px rgba(0,0,0,0.6);
        position: sticky;
        top: 0;
        z-index: 100;
    }

    .nav-logo {
        display: flex;
        align-items: center;
        text-decoration: none;
    }

    .nav-logo-text {
        font-family: var(--font-heading);
        font-size: 1.35rem;
        font-weight: 700;
        background: linear-gradient(135deg, var(--orange) 0%, var(--green) 100%);
        -webkit-background-clip: text;
        -webkit-text-fill-color: transparent;
        background-clip: text;
        letter-spacing: 0.04em;
    }

    .nav-links {
        display: flex;
        gap: 8px;
    }

    .nav-link {
        color: var(--text-secondary);
        text-decoration: none;
        padding: 6px 14px;
        border-radius: var(--radius);
        font-size: 0.9rem;
        transition: background 0.15s, color 0.15s;
    }

    .nav-link:hover {
        background: rgba(255,255,255,0.07);
        color: var(--text-primary);
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

    /* Standard scrollable page wrapper (tree-list, person-detail…) */
    .page-content {
        max-width: 1200px;
        margin: 0 auto;
        padding: 24px;
        overflow-y: auto;
        width: 100%;
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
        background: rgba(255,255,255,0.05);
        color: var(--text-primary);
        border-color: var(--text-secondary);
    }

    .page-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: 24px;
    }

    .page-header h1 {
        font-size: 1.5rem;
        font-weight: 600;
        font-family: var(--font-heading);
        color: var(--text-primary);
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
        color: #f87171;
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

    .table-wrapper {
        overflow-x: auto;
    }

    table {
        width: 100%;
        border-collapse: collapse;
    }

    th, td {
        text-align: left;
        padding: 10px 14px;
        border-bottom: 1px solid var(--border);
    }

    th {
        font-size: 0.8rem;
        font-weight: 600;
        color: var(--text-secondary);
        text-transform: uppercase;
        letter-spacing: 0.5px;
    }

    tr:hover td {
        background: rgba(255,255,255,0.03);
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

    .badge {
        display: inline-block;
        padding: 2px 8px;
        font-size: 0.75rem;
        font-weight: 500;
        border-radius: 12px;
        background: rgba(255,255,255,0.05);
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

    .back-link {
        color: var(--orange);
        text-decoration: none;
        font-size: 0.9rem;
        font-weight: 500;
        transition: color 0.15s;
    }

    .back-link:hover {
        color: var(--orange-light);
        text-decoration: underline;
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

    /* ── Ancestry / descendant chart ──────────────────────────────── */

    .chart-container {
        overflow-x: auto;
        padding: 16px 0;
    }

    .ancestor-chart {
        display: flex;
        align-items: center;
    }

    .ancestor-chart .gen-col {
        display: flex;
        flex-direction: column;
        justify-content: center;
        gap: 8px;
        margin-right: 4px;
    }

    .ancestor-chart .gen-col .connector {
        display: flex;
        align-items: center;
    }

    .ancestor-chart .gen-col .connector::after {
        content: "";
        display: inline-block;
        width: 20px;
        height: 2px;
        background: var(--border);
        margin-left: 4px;
    }

    .ancestor-chart .gen-col:last-child .connector::after {
        display: none;
    }

    .descendant-chart {
        display: flex;
        flex-direction: column;
        align-items: center;
    }

    .descendant-chart .gen-row {
        display: flex;
        justify-content: center;
        gap: 12px;
        margin-bottom: 8px;
        position: relative;
    }

    .descendant-chart .gen-row::before {
        content: "";
        position: absolute;
        top: -8px;
        left: 50%;
        transform: translateX(-50%);
        width: 2px;
        height: 8px;
        background: var(--border);
    }

    .descendant-chart .gen-row:first-child::before {
        display: none;
    }

    /* Shared person node card */
    .tree-node {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 6px 12px;
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-radius: var(--radius);
        font-size: 0.85rem;
        white-space: nowrap;
        cursor: pointer;
        transition: border-color 0.15s, box-shadow 0.15s;
        text-decoration: none;
        color: var(--text-primary);
    }

    .tree-node:hover {
        border-color: var(--orange);
        box-shadow: 0 0 0 2px rgba(224, 120, 32, 0.12);
    }

    .tree-node.current {
        border-color: var(--orange);
        background: rgba(224, 120, 32, 0.08);
        font-weight: 600;
    }

    .tree-node .sex-icon {
        font-size: 0.75rem;
        opacity: 0.7;
    }

    .tree-node .sex-icon.male   { color: #60a5fa; }
    .tree-node .sex-icon.female { color: #f472b6; }

    .gen-label {
        font-size: 0.7rem;
        font-weight: 600;
        color: var(--text-secondary);
        text-transform: uppercase;
        letter-spacing: 0.5px;
        text-align: center;
        margin-bottom: 4px;
    }

    .depth-group {
        margin-bottom: 16px;
    }

    .depth-group-nodes {
        display: flex;
        flex-wrap: wrap;
        gap: 8px;
    }

    /* ── GEDCOM import / export ──────────────────────────────────── */

    .gedcom-textarea {
        font-family: "Courier New", Courier, monospace;
        font-size: 0.8rem;
        line-height: 1.4;
        min-height: 200px;
        max-height: 400px;
        resize: vertical;
        white-space: pre;
        overflow-wrap: normal;
        overflow-x: auto;
    }

    .gedcom-result {
        background: rgba(90, 171, 60, 0.07);
        border: 1px solid rgba(90, 171, 60, 0.3);
        border-radius: var(--radius);
        padding: 16px;
        margin-top: 12px;
    }

    .gedcom-result h4 {
        font-size: 0.9rem;
        font-weight: 600;
        margin-bottom: 8px;
        color: var(--green-light);
    }

    .result-stats {
        display: flex;
        flex-wrap: wrap;
        gap: 12px;
        margin-bottom: 8px;
    }

    .result-stat {
        display: flex;
        align-items: center;
        gap: 4px;
        font-size: 0.85rem;
    }

    .result-stat .stat-value {
        font-weight: 600;
        color: var(--text-primary);
    }

    .result-stat .stat-label {
        color: var(--text-secondary);
    }

    .gedcom-warnings {
        margin-top: 8px;
        padding: 12px;
        background: rgba(245, 157, 58, 0.08);
        border: 1px solid rgba(245, 157, 58, 0.3);
        border-radius: var(--radius);
        font-size: 0.8rem;
        color: var(--orange-light);
    }

    .gedcom-warnings summary {
        cursor: pointer;
        font-weight: 500;
        margin-bottom: 4px;
    }

    .gedcom-warnings ul {
        list-style: disc;
        padding-left: 20px;
        margin-top: 4px;
    }

    .gedcom-warnings li {
        margin-bottom: 2px;
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

    .td-bc-sep { color: var(--text-muted); }

    .td-bc-current {
        color: var(--text-primary);
        font-weight: 600;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        max-width: 220px;
    }

    .td-actions {
        display: flex;
        align-items: center;
        gap: 6px;
        flex-shrink: 0;
    }

    .td-btn {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        padding: 4px 10px;
        border: 1px solid var(--border);
        border-radius: 5px;
        background: rgba(255,255,255,0.04);
        color: var(--text-secondary);
        font-size: 0.8rem;
        cursor: pointer;
        font-family: var(--font-sans);
        transition: background 0.12s, color 0.12s, border-color 0.12s;
        white-space: nowrap;
    }

    .td-btn:hover:not(:disabled) {
        background: rgba(255,255,255,0.08);
        color: var(--text-primary);
        border-color: var(--text-secondary);
    }

    .td-btn:disabled { opacity: 0.45; cursor: default; }

    .td-btn-danger { color: var(--color-danger); border-color: rgba(220,82,82,0.35); }
    .td-btn-danger:hover:not(:disabled) { background: rgba(220,82,82,0.12); color: #f87171; }

    .td-btn-primary {
        background: linear-gradient(135deg, var(--orange), var(--orange-light));
        border-color: var(--orange);
        color: #fff;
    }
    .td-btn-primary:hover:not(:disabled) { opacity: 0.88; color: #fff; }

    .td-select {
        padding: 3px 8px;
        font-size: 0.8rem;
        border: 1px solid var(--border);
        border-radius: 5px;
        background: rgba(255,255,255,0.04);
        color: var(--text-primary);
        cursor: pointer;
        max-width: 200px;
        width: auto;
    }

    /* ── Tree edit form (inline below topbar) ────────────────────── */

    .td-edit-form {
        padding: 10px 12px;
        background: var(--bg-panel);
        border-bottom: 1px solid var(--border);
        flex-shrink: 0;
        display: flex;
        gap: 10px;
        align-items: flex-end;
        flex-wrap: wrap;
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

    .isb-btn:hover { background: rgba(255,255,255,0.07); color: var(--orange); }
    .isb-btn:active { background: rgba(224,120,32,0.12); }

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
        user-select: none;
    }

    .pedigree-viewport:active { cursor: grabbing; }

    .pedigree-inner {
        position: absolute;
        top: 0;
        left: 0;
        width: 100%;
        height: 100%;
        transform-origin: center center;
        display: flex;
        align-items: center;
        justify-content: center;
    }

    .pedigree-tree {
        display: flex;
        flex-direction: column;
        align-items: stretch;
        min-width: 320px;
        padding: 24px 32px;
    }

    .pedigree-gen-row {
        display: flex;
        align-items: center;
        justify-content: stretch;
        min-height: 72px;
    }

    .pedigree-desc-row { justify-content: center; gap: 8px; flex-wrap: wrap; }

    .pedigree-slot-cell {
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 2px 3px;
    }

    /* ── Person card ─────────────────────────────────────────────── */

    .pedigree-node {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 7px;
        padding: 6px 8px;
        background: var(--bg-card);
        border: 1px solid var(--border);
        border-left: 3px solid var(--border);
        border-radius: 0 var(--radius) var(--radius) 0;
        font-size: 0.82rem;
        cursor: pointer;
        transition: border-color 0.15s, box-shadow 0.15s, background 0.15s;
        color: var(--text-primary);
        width: 100%;
        max-width: 168px;
        min-width: 100px;
    }

    .pedigree-node.male   { border-left-color: rgba(74,144,217,0.55); }
    .pedigree-node.female { border-left-color: rgba(196,88,122,0.55); }

    .pedigree-node:hover { box-shadow: 0 0 0 1px rgba(224,120,32,0.2); }

    .pedigree-node.selected,
    .pedigree-node.current {
        background: var(--sel-bg);
        border-left-color: var(--orange);
        box-shadow: 0 0 0 1px rgba(224,120,32,0.2);
    }

    .pedigree-node.male.selected,
    .pedigree-node.male.current   { border-left-color: var(--blue); }
    .pedigree-node.female.selected,
    .pedigree-node.female.current { border-left-color: var(--pink); }

    .pedigree-node.empty-slot {
        background: rgba(255,255,255,0.02);
        border-style: dashed;
        border-left-style: dashed;
        align-items: center;
        justify-content: center;
        color: var(--text-muted);
        font-size: 1.1rem;
        font-weight: 300;
        min-height: 32px;
    }

    .pedigree-node.empty-slot:hover { border-color: var(--orange); color: var(--orange); }
    .pedigree-node.empty-slot.disabled { opacity: 0.3; cursor: default; pointer-events: none; }

    /* Avatar circle */
    .pc-ph {
        width: 32px;
        height: 32px;
        border-radius: 50%;
        background: var(--bg-panel);
        border: 1px solid var(--border);
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: 0.65rem;
        font-weight: 700;
        color: var(--text-muted);
        flex-shrink: 0;
        text-transform: uppercase;
        letter-spacing: 0.03em;
    }

    .pedigree-node.male   .pc-ph { background: rgba(74,144,217,0.12); border-color: rgba(74,144,217,0.35); color: var(--blue); }
    .pedigree-node.female .pc-ph { background: rgba(196,88,122,0.12); border-color: rgba(196,88,122,0.35); color: var(--pink); }

    .pc-body { display: flex; flex-direction: column; min-width: 0; flex: 1; gap: 2px; }

    .pc-name { display: flex; flex-direction: column; min-width: 0; }

    .pc-last {
        font-size: 0.77rem;
        font-weight: 700;
        text-transform: uppercase;
        letter-spacing: 0.03em;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        line-height: 1.25;
    }

    .pc-first {
        font-size: 0.75rem;
        color: var(--text-secondary);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        line-height: 1.25;
    }

    .pc-dates { display: flex; flex-direction: column; gap: 1px; margin-top: 2px; }

    .pc-born { font-size: 0.68rem; color: var(--green); line-height: 1.3; }
    .pc-died { font-size: 0.68rem; color: var(--blue);  line-height: 1.3; }

    /* Root wrapper + FAB */
    .pedigree-root-wrapper { display: flex; flex-direction: column; align-items: center; gap: 6px; }

    .pedigree-edit-fab {
        width: 26px;
        height: 26px;
        border-radius: 50%;
        background: var(--orange);
        color: #fff;
        border: 2px solid var(--bg-deep);
        box-shadow: 0 1px 4px rgba(0,0,0,0.4);
        cursor: pointer;
        font-size: 13px;
        line-height: 1;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 0;
        transition: background 0.15s, transform 0.1s;
        flex-shrink: 0;
    }

    .pedigree-edit-fab:hover { background: var(--orange-light); transform: scale(1.12); }

    /* ── Ancestor connector rows ─────────────────────────────────── */

    .pedigree-connector-row { display: flex; min-height: 36px; align-items: stretch; }

    .connector-group { display: flex; flex-direction: column; }

    .pedigree-marriage-date {
        font-size: 0.68rem;
        color: var(--text-secondary);
        text-align: center;
        padding: 1px 2px;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        line-height: 1.3;
        flex-shrink: 0;
    }

    .connector-arms { display: flex; flex: 1; }

    .connector-arm-left {
        flex: 1;
        border-right: 2px solid #2e4a6a;
        border-bottom: 2px solid #2e4a6a;
        border-bottom-right-radius: 4px;
    }

    .connector-arm-right {
        flex: 1;
        border-left: 2px solid #2e4a6a;
        border-bottom: 2px solid #2e4a6a;
        border-bottom-left-radius: 4px;
    }

    .connector-stem { flex: 1; display: flex; justify-content: center; }
    .connector-stem::after { content: ''; width: 2px; background: #2e4a6a; height: 100%; }

    /* ── Descendant connector ─────────────────────────────────────── */

    .pedigree-desc-connector { height: 24px; display: flex; justify-content: center; }
    .pedigree-desc-connector::after { content: ''; width: 2px; background: #2e4a6a; height: 100%; }

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

    .pedigree-depth-title {
        font-size: 0.72rem;
        font-weight: 700;
        color: var(--text-secondary);
        text-transform: uppercase;
        letter-spacing: 0.5px;
        margin-bottom: 10px;
    }

    .pedigree-depth-row { display: flex; align-items: center; gap: 6px; margin-bottom: 8px; }
    .pedigree-depth-row:last-child { margin-bottom: 0; }
    .pedigree-depth-label { font-size: 0.82rem; flex: 1; color: var(--text-primary); }

    .pedigree-depth-btn {
        width: 24px;
        height: 24px;
        display: flex;
        align-items: center;
        justify-content: center;
        background: rgba(255,255,255,0.05);
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
    }

    .evp-hd {
        padding: 10px 14px;
        border-bottom: 1px solid var(--border);
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
        font-size: 0.75rem;
        font-weight: 700;
        color: var(--text-secondary);
        flex-shrink: 0;
        text-transform: uppercase;
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
        border-bottom: 1px solid rgba(37,45,61,0.6);
        transition: background 0.1s;
    }

    .ev-item:last-child { border-bottom: none; }
    .ev-item:hover { background: rgba(255,255,255,0.03); }

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
    .ev-ic-other { background: rgba(255,255,255,0.06);color: var(--text-secondary); }

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
        background: rgba(255,255,255,0.05);
    }

    .context-menu-danger {
        color: #f87171;
    }

    .context-menu-danger:hover {
        background: rgba(220, 82, 82, 0.1);
    }

    .context-menu-divider {
        border: none;
        border-top: 1px solid var(--border);
        margin: 4px 0;
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
        max-height: 200px;
        overflow-y: auto;
        border: 1px solid var(--border);
        border-radius: var(--radius);
    }

    .search-person-result {
        display: flex;
        align-items: center;
        justify-content: space-between;
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
        background: rgba(255,255,255,0.05);
    }

    .search-person-name {
        font-weight: 500;
    }

    .search-person-sex {
        font-size: 0.75rem;
    }

    /* ── Root person selector ─────────────────────────────────────── */

    .root-selector {
        display: flex;
        align-items: center;
        gap: 12px;
        margin-bottom: 16px;
        flex-wrap: wrap;
    }

    .root-selector label {
        display: inline;
        font-size: 0.85rem;
        font-weight: 500;
        margin-bottom: 0;
        color: var(--text-primary);
    }

    .root-selector select {
        width: auto;
        min-width: 200px;
    }

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
        background: rgba(255,255,255,0.07);
        color: var(--text-primary);
    }

    .person-form-tabs {
        display: flex;
        border-bottom: 1px solid var(--border);
        padding: 0 16px;
        gap: 0;
        overflow-x: auto;
    }

    .person-form-tab {
        background: none;
        border: none;
        border-bottom: 2px solid transparent;
        padding: 10px 14px;
        font-size: 0.85rem;
        font-weight: 500;
        color: var(--text-secondary);
        cursor: pointer;
        white-space: nowrap;
        transition: color 0.15s, border-color 0.15s;
        font-family: var(--font-sans);
    }

    .person-form-tab:hover {
        color: var(--text-primary);
    }

    .person-form-tab.active {
        color: var(--orange);
        border-bottom-color: var(--orange);
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
        background: rgba(255,255,255,0.02);
    }

    .person-form-item.editing {
        display: block;
        padding: 12px;
        background: rgba(255,255,255,0.03);
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

    /* ── Linking panel ─────────────────────────────────────────────── */

    .linking-card {
        margin-bottom: 24px;
        border: 2px solid var(--orange);
    }

    .linking-panel {
        padding: 16px;
        background: rgba(255,255,255,0.03);
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
        width: 600px;
        max-width: 95vw;
        max-height: 85vh;
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

    /* ── Scrollbar ────────────────────────────────────────────────── */

    ::-webkit-scrollbar { width: 6px; height: 6px; }
    ::-webkit-scrollbar-track { background: var(--bg-deep); }
    ::-webkit-scrollbar-thumb { background: var(--border); border-radius: 3px; }
    ::-webkit-scrollbar-thumb:hover { background: var(--text-muted); }

    /* ── Responsive ───────────────────────────────────────────────── */

    @media (max-width: 640px) {
        .app-nav { padding: 0 1rem; }
    }
"#;
