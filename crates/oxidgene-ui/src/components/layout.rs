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
            div { class: "nav-brand",
                Link { to: Route::Home {}, "OxidGene" }
            }
            div { class: "nav-links",
                Link { to: Route::Home {}, class: "nav-link", "Home" }
                Link { to: Route::TreeList {}, class: "nav-link", "Trees" }
            }
        }

        main { class: "app-main",
            Outlet::<Route> {}
        }
    }
}

/// CSS for the layout shell.
const LAYOUT_STYLES: &str = r#"
    :root {
        --color-bg: #f8f9fa;
        --color-surface: #ffffff;
        --color-primary: #2c6e49;
        --color-primary-hover: #1e4d34;
        --color-text: #212529;
        --color-text-muted: #6c757d;
        --color-border: #dee2e6;
        --color-danger: #dc3545;
        --shadow-sm: 0 1px 3px rgba(0,0,0,0.08);
        --shadow-md: 0 4px 12px rgba(0,0,0,0.1);
        --radius: 8px;
        --font-sans: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    }

    * {
        box-sizing: border-box;
        margin: 0;
        padding: 0;
    }

    body {
        font-family: var(--font-sans);
        background: var(--color-bg);
        color: var(--color-text);
        line-height: 1.6;
    }

    .app-nav {
        display: flex;
        align-items: center;
        justify-content: space-between;
        background: var(--color-primary);
        color: white;
        padding: 0 24px;
        height: 56px;
        box-shadow: var(--shadow-sm);
        position: sticky;
        top: 0;
        z-index: 100;
    }

    .nav-brand a {
        font-size: 1.25rem;
        font-weight: 700;
        color: white;
        text-decoration: none;
        letter-spacing: 0.5px;
    }

    .nav-links {
        display: flex;
        gap: 8px;
    }

    .nav-link {
        color: rgba(255,255,255,0.85);
        text-decoration: none;
        padding: 6px 14px;
        border-radius: var(--radius);
        font-size: 0.9rem;
        transition: background 0.15s, color 0.15s;
    }

    .nav-link:hover {
        background: rgba(255,255,255,0.15);
        color: white;
    }

    .app-main {
        max-width: 1200px;
        margin: 0 auto;
        padding: 24px;
    }

    /* ── Shared utility classes ───────────────────────────────────── */

    .card {
        background: var(--color-surface);
        border: 1px solid var(--color-border);
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
        transition: background 0.15s, box-shadow 0.15s;
    }

    .btn-primary {
        background: var(--color-primary);
        color: white;
    }

    .btn-primary:hover {
        background: var(--color-primary-hover);
        box-shadow: var(--shadow-sm);
    }

    .btn-danger {
        background: var(--color-danger);
        color: white;
    }

    .btn-danger:hover {
        opacity: 0.9;
    }

    .btn-outline {
        background: transparent;
        border: 1px solid var(--color-border);
        color: var(--color-text);
    }

    .btn-outline:hover {
        background: var(--color-bg);
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
    }

    .text-muted {
        color: var(--color-text-muted);
    }

    .loading {
        text-align: center;
        padding: 48px;
        color: var(--color-text-muted);
    }

    .error-msg {
        background: #fff5f5;
        border: 1px solid #feb2b2;
        color: #c53030;
        padding: 12px 16px;
        border-radius: var(--radius);
        margin-bottom: 16px;
    }

    input, select, textarea {
        font-family: var(--font-sans);
        font-size: 0.9rem;
        padding: 8px 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius);
        width: 100%;
        transition: border-color 0.15s;
    }

    input:focus, select:focus, textarea:focus {
        outline: none;
        border-color: var(--color-primary);
        box-shadow: 0 0 0 3px rgba(44, 110, 73, 0.15);
    }

    label {
        display: block;
        font-size: 0.8rem;
        font-weight: 500;
        margin-bottom: 4px;
        color: var(--color-text-muted);
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
        border-bottom: 1px solid var(--color-border);
    }

    th {
        font-size: 0.8rem;
        font-weight: 600;
        color: var(--color-text-muted);
        text-transform: uppercase;
        letter-spacing: 0.5px;
    }

    tr:hover td {
        background: var(--color-bg);
    }

    .empty-state {
        text-align: center;
        padding: 48px 24px;
        color: var(--color-text-muted);
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
        background: var(--color-bg);
        color: var(--color-text-muted);
        border: 1px solid var(--color-border);
    }

    /* ── Section header (title + action button) ─────────────────── */

    .section-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: 16px;
    }

    /* ── Small button variant ─────────────────────────────────────── */

    .btn-sm {
        padding: 4px 10px;
        font-size: 0.8rem;
    }

    /* ── Back link ────────────────────────────────────────────────── */

    .back-link {
        color: var(--color-primary);
        text-decoration: none;
        font-size: 0.9rem;
        font-weight: 500;
        transition: color 0.15s;
    }

    .back-link:hover {
        color: var(--color-primary-hover);
        text-decoration: underline;
    }

    /* ── Modal / confirmation dialog ─────────────────────────────── */

    .modal-backdrop {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.45);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 200;
    }

    .modal-card {
        background: var(--color-surface);
        border-radius: var(--radius);
        padding: 24px;
        min-width: 360px;
        max-width: 480px;
        box-shadow: var(--shadow-md);
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

    /* Ancestor chart: horizontal pedigree flowing left-to-right */
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
        background: var(--color-border);
        margin-left: 4px;
    }

    .ancestor-chart .gen-col:last-child .connector::after {
        display: none;
    }

    /* Descendant chart: vertical tree flowing top-to-bottom */
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
        background: var(--color-border);
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
        background: var(--color-surface);
        border: 1px solid var(--color-border);
        border-radius: var(--radius);
        font-size: 0.85rem;
        white-space: nowrap;
        cursor: pointer;
        transition: border-color 0.15s, box-shadow 0.15s;
        text-decoration: none;
        color: var(--color-text);
    }

    .tree-node:hover {
        border-color: var(--color-primary);
        box-shadow: 0 0 0 2px rgba(44, 110, 73, 0.12);
    }

    .tree-node.current {
        border-color: var(--color-primary);
        background: rgba(44, 110, 73, 0.06);
        font-weight: 600;
    }

    .tree-node .sex-icon {
        font-size: 0.75rem;
        opacity: 0.7;
    }

    .tree-node .sex-icon.male   { color: #2563eb; }
    .tree-node .sex-icon.female { color: #db2777; }

    .gen-label {
        font-size: 0.7rem;
        font-weight: 600;
        color: var(--color-text-muted);
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
"#;
