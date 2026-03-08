# Visual & Functional Specifications — Topbar

> Part of the [OxidGene Specifications](README.md).
> See also: [Homepage](ui-home.md) · [Tree View](ui-genealogy-tree.md) · [Settings](ui-settings.md) · [App Settings](ui-app-settings.md) · [Person Profile](ui-person-profile.md) · [Search Results](ui-search-results.md) · [Design Tokens](ui-design-tokens.md)

---

## 1. Overview

The application has two distinct top-level bars:

1. **Navbar** (`app-nav`) — minimal branding bar, always visible on every page
2. **Page topbar** (`td-topbar`) — contextual breadcrumb + actions, shown on tree-related pages

---

## 2. Navbar

### Dimensions & Positioning

- Height: auto (compact, ~48px)
- Width: full viewport width
- Position: part of the normal flow (not sticky)
- Background: `var(--nav-bg)` with frosted glass effect (`backdrop-filter: blur(12px)`)
- Bottom border: `1px solid var(--border)`

### Content

The navbar is deliberately minimal in MVP:

- **Logo**: `OxidGene.svg` (from `docs/assets/`), rendered as an `<img>`, height ~32px
- Logo acts as a link to the [Homepage](ui-home.md) (`/`)
- No brand text, no navigation links, no right-side actions in MVP

### Future (post-MVP)

The following elements will be added in future EPICs:

| Element | Description |
|---------|-------------|
| Brand name | "OxidGene" in Cinzel font, orange gradient text, next to the logo |
| Navigation links | My Trees / Sources / Places / Help |
| Theme toggle | Light/dark mode switch (icon button) |
| Notifications | Bell icon button with optional badge |
| Settings | Gear icon button, links to app-level settings |
| User avatar | Initials in an orange gradient circle (32px), dropdown on click |

---

## 3. Page Topbar (`td-topbar`)

### Dimensions

- Height: auto (compact, ~40px)
- Width: full viewport width
- Padding: `10px 16px`
- Background: transparent (inherits from page background)
- Bottom border: `1px solid var(--border)`

### Structure

```
+------------------------------------------------------------------------+
|  [logo] tree_name / Page Label        [search fields] [actions]         |
+------------------------------------------------------------------------+
```

The topbar has two zones:

**Left zone — Breadcrumb** (`.td-bc`):
- Small logo icon (links to homepage)
- Tree name as a link (`.td-bc-link`, links to tree view)
- `/` separator (`.td-bc-sep`)
- Current page label (`.td-bc-current`, not clickable)

**Right zone** — varies by page (search fields, action buttons, etc.)

### Breadcrumb per page

| Page | Breadcrumb | Right zone |
|------|------------|------------|
| [Tree View](ui-genealogy-tree.md) | `logo` tree_name `/` Tree | Search fields + magnifying glass |
| [Settings](ui-settings.md) | `logo` tree_name `/` Settings | (empty) |
| [Search Results](ui-search-results.md) | `logo` tree_name `/` Search | Search fields (pre-filled) + fit button |
| [Person Profile](ui-person-profile.md) | `logo` tree_name `/` Person Name | (empty) |
| [App Settings](ui-app-settings.md) | Home `/` Settings | (empty) |

### Styling

- Breadcrumb font: Lato, 0.85rem
- `.td-bc-link`: `var(--text-secondary)` color, hover: `var(--orange)`
- `.td-bc-sep`: `var(--text-muted)`, padding `0 6px`
- `.td-bc-current`: `var(--text-primary)`, font-weight 500

---

## 4. Search Fields (Tree Pages Only)

On the [Tree View](ui-genealogy-tree.md) and [Search Results](ui-search-results.md) pages, the topbar right zone includes search fields:

```
[Last name ________] [First name ________] [magnifying glass] [fit]
```

### Field specifications

- Two independent text inputs: **Last name(s)** and **First name(s)**
- Either field can be used alone, or both combined
- Compact style: height 32px, `var(--bg-card)` background, `var(--border)` border, `var(--text-primary)` text
- Placeholder text in `var(--text-muted)`
- Magnifying glass icon button triggers the search
- Fit-to-screen icon button (four corners) navigates back to tree view (on search results page)

### On Enter (or click magnifying glass)

- Navigates to the [Search Results](ui-search-results.md) page with the full filtered results list
- The search fields remain pre-filled on the results page

---

## 5. User Avatar Dropdown (Future)

Clicking the user avatar opens a dropdown menu anchored to the top-right corner:

| Item | Action |
|------|--------|
| **Profile** | Opens user profile settings |
| **Preferences** | Opens app-level preferences (language, theme defaults) |
| Divider | --- |
| **Sign out** | Logs out (post-MVP, EPIC E) |

The dropdown closes on outside click or `Escape`.

---

## 6. Responsive

| Breakpoint | Behavior |
|---|---|
| **>= 900px** | Full layout: logo + breadcrumb + search + actions |
| **< 900px** | Search fields stack below breadcrumb. Person card sizes reduced. |
| **< 640px** | Reduced padding on topbar (`10px 12px`). Breadcrumb truncated to last 2 crumbs. |

---

## 7. Keyboard

| Key | Behavior |
|---|---|
| `/` | Focus the last name search field (when on tree pages) |
| `Escape` | Close any open dropdown (search, avatar menu) |
| `Tab` | Navigate between topbar elements |
