# Visual & Functional Specifications — Design Tokens

> Part of the [OxidGene Specifications](README.md).
> See also: [Shared Components](ui-shared-components.md) · [Topbar](ui-topbar.md)

---

## 1. Overview

All visual styling uses CSS custom properties (design tokens) defined in `crates/oxidgene-ui/src/components/layout.rs` (`LAYOUT_STYLES`).

**Light theme is the default.** Dark theme is activated via `<html class="dark">`. The active theme is persisted in `localStorage('oxidgene-theme')`. If no preference is saved, the system preference (`prefers-color-scheme`) is respected.

A toggle button in the navbar switches between themes at runtime.

---

## 2. Color Tokens

### Core palette

| Token | Light value | Dark value | Purpose |
|---|---|---|---|
| `--bg-deep` | `#f4f2ee` | `#0d0f14` | Page background, deepest layer |
| `--bg-panel` | `#ede9e2` | `#111318` | Panel / sidebar backgrounds, topbar |
| `--bg-card` | `#ffffff` | `#16191f` | Card backgrounds, input backgrounds |
| `--bg-card-hover` | `#f5f3ef` | `#1c2030` | Card hover state |
| `--border` | `#d4ccc0` | `#252d3d` | Borders, dividers, separators |
| `--border-glow` | `#e07820` | `#e07820` | Focus/active border glow |
| `--sel-bg` | `#e8e0d4` | `#192038` | Selected item background |
| `--connector` | `#a0937f` | `#2e4a6a` | Pedigree tree connectors |
| `--nav-bg` | `rgba(244,242,238,0.92)` | `rgba(10,11,13,0.92)` | Navbar frosted glass background |
| `--tree-visual-bg` | `#e8e0d4` | `#0d1018` | Tree card SVG illustration background |
| `--tree-visual-branch` | `#b0a898` | `#3a4458` | Tree card SVG branches |

### Accent colors

Unchanged between themes:

| Token | Value | Purpose |
|---|---|---|
| `--orange` | `#e07820` | Primary accent: buttons, links, active states, focus borders |
| `--orange-light` | `#f5a03a` | Hover state for orange elements, gradient end |
| `--green` | `#4ea832` | Birth dates, positive states, success |
| `--green-light` | `#7ec45f` | Hover for green elements |
| `--blue` | `#4a90d9` | Death dates, connectors, info states |
| `--pink` | `#c4587a` | Female gender indicator |

### Text colors

| Token | Light value | Dark value | Purpose |
|---|---|---|---|
| `--text-primary` | `#1e1a14` | `#ddd8cc` | Primary text, headings, names |
| `--text-secondary` | `#5c5447` | `#7a8da8` | Secondary text, labels, metadata |
| `--text-muted` | `#9e9488` | `#404f65` | Muted text, placeholders, disabled states |
| `--color-danger-text` | `#dc2626` | `#f87171` | Error messages, danger button text |

### Semantic aliases

These aliases map to core tokens for consistency across components:

| Alias | Resolves to | Purpose |
|---|---|---|
| `--color-bg` | `var(--bg-deep)` | Generic background |
| `--color-surface` | `var(--bg-card)` | Component surface |
| `--color-primary` | `var(--orange)` | Primary action color |
| `--color-primary-hover` | `var(--orange-light)` | Primary hover |
| `--color-text` | `var(--text-primary)` | Generic text |
| `--color-text-muted` | `var(--text-secondary)` | Generic muted text |
| `--color-border` | `var(--border)` | Generic border |
| `--color-danger` | `#e05252` | Destructive actions (delete, remove) |

---

## 3. Typography

### Font families

| Token | Value | Usage |
|---|---|---|
| `--font-heading` | `'Cinzel', Georgia, serif` | Titles, brand name, page headings, tree names |
| `--font-sans` | `'Lato', -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif` | Body text, labels, inputs, all other text |

Both fonts are loaded via Google Fonts `@import` at the top of `LAYOUT_STYLES`.

### Font sizes (reference scale)

| Usage | Size | Weight | Font |
|---|---|---|---|
| Page title | 1.3rem | 700 | Cinzel |
| Section heading | 1.05rem | 600 | Cinzel |
| Card title / tree name | 0.95rem | 600 | Cinzel |
| Body text | 0.85rem | 400 | Lato |
| Label / metadata | 0.78rem | 400 | Lato |
| Small text (dates, stats) | 0.72rem | 400 | Lato |
| Tiny text (badges) | 0.65rem | 600 | Lato |

---

## 4. Spacing & Sizing

### Layout dimensions

| Token | Value | Purpose |
|---|---|---|
| `--sb` | `46px` | Left icon sidebar width (tree view) |
| `--evw` | `275px` | Events panel width (tree view) |

### Common spacing scale

| Name | Value | Usage |
|---|---|---|
| xs | 4px | Compact spacing, icon gaps |
| sm | 8px | Intra-component spacing |
| md | 12–16px | Section padding, card padding |
| lg | 20–24px | Section gaps, modal padding |
| xl | 32px | Page-level vertical spacing |

---

## 5. Elevation & Shadows

| Token | Light value | Dark value | Usage |
|---|---|---|---|
| `--shadow-sm` | `0 1px 3px rgba(0,0,0,0.08)` | `0 1px 3px rgba(0,0,0,0.35)` | Cards, dropdowns |
| `--shadow-md` | `0 4px 16px rgba(0,0,0,0.12)` | `0 4px 16px rgba(0,0,0,0.55)` | Modals, popovers, navbar |

---

## 6. Border Radius

| Token | Value | Usage |
|---|---|---|
| `--radius` | `8px` | Cards, buttons, inputs, modals |

Smaller elements (badges, chips) use `4px`. Fully round elements (avatars) use `50%`.

---

## 7. Gender Colors

Used on person card left borders, avatar backgrounds, and gender indicators.

| Gender | Color | Token |
|---|---|---|
| Male | Blue | `var(--blue)` / `#4a90d9` |
| Female | Pink | `var(--pink)` / `#c4587a` |
| Unknown | Grey | `var(--text-muted)` |

---

## 8. Interactive States

### Buttons

| State | Style |
|---|---|
| Default | `var(--bg-card)` background, `var(--border)` border, `var(--text-primary)` text |
| Hover | `var(--orange)` border, slight orange tint on background |
| Active | `var(--orange)` background, white text |
| Disabled | `0.5` opacity, no pointer events |
| Danger | `var(--color-danger)` text and border; solid red background on hover |

### Primary button (CTA)

Background: linear gradient from `var(--orange)` to `var(--orange-light)`. White text. On hover: slight brightness increase. Shadow: `var(--shadow-sm)`.

### Inputs

| State | Style |
|---|---|
| Default | `var(--bg-card)` background, `var(--border)` border |
| Focus | `var(--border-glow)` border, subtle orange glow |
| Error | Red border, red hint text below |
| Disabled | `0.5` opacity |

### Cards

| State | Style |
|---|---|
| Default | `var(--bg-card)` background, `var(--border)` border, `var(--shadow-sm)` |
| Hover | `var(--bg-card-hover)` background, `var(--orange)` border, lifted shadow, 2px upward translate |
| Selected | `var(--sel-bg)` background, `var(--orange)` border |

---

## 9. Responsive Breakpoints

| Breakpoint | Target |
|---|---|
| `≥ 1200px` | Full desktop layout |
| `900–1199px` | Reduced desktop (tree view single-column family connections) |
| `600–899px` | Tablet (reduced card sizes, collapsed sidebars, stacked layouts) |
| `< 600px` | Mobile (full-screen modals, single column, icon-only buttons) |

### Person card sizes

| Breakpoint | Dimensions |
|---|---|
| `≥ 900px` | 180×80px |
| `< 900px` | 130×64px |

---

## 10. Theme Switching

### Implementation

- `:root` defines **light theme** values (default)
- `:root.dark` overrides with **dark theme** values
- Toggle button in the navbar (sun/moon icon)
- Persisted in `localStorage('oxidgene-theme')` — values: `'light'` or `'dark'`
- On first visit with no saved preference: respects `prefers-color-scheme` system media query
- Dark-only effect: `body::before` radial light leaks are scoped to `:root.dark body::before`
