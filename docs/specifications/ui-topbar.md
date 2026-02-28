# Visual & Functional Specifications — Topbar

> Part of the [OxidGene Specifications](README.md).
> See also: [Homepage](ui-home.md) · [Tree View](ui-genealogy-tree.md) · [Settings](ui-settings.md) · [Person Profile](ui-person-profile.md) · [Search Results](ui-search-results.md) · [Design Tokens](ui-design-tokens.md)

---

## 1. Overview

The topbar is a **shared component** used across all pages. It provides branding, navigation, search (on tree pages), and user actions. Its content adapts based on the current page context.

---

## 2. Dimensions & Positioning

- Height: **56px** (fixed)
- Width: full viewport width
- Position: **sticky** at the top of the viewport (`position: sticky; top: 0; z-index: 100`)
- Background: `var(--bg-panel)` with a frosted glass effect (`backdrop-filter: blur(12px)`)
- Bottom border: `1px solid var(--border)`

---

## 3. Structure

```
┌────────────────────────────────────────────────────────────────────────┐
│  [Logo + Brand]  |  [Navigation / Breadcrumb]        [Actions]        │
└────────────────────────────────────────────────────────────────────────┘
```

### Left zone — Logo & Brand

- Logo: `OxidGene.svg` (from `docs/assets/`), rendered as an inline SVG or `<img>`, height ~28px
- Brand name: "OxidGene" in Cinzel font, orange gradient text (`--orange` to `--orange-light`)
- Logo + brand name together act as a link to the [Homepage](ui-home.md) (`/`)
- A **vertical divider** (1px, `var(--border)`, 24px tall) separates the logo zone from the navigation zone

### Center zone — Navigation or Breadcrumb

The center zone content depends on the current page:

| Page | Center zone content |
|------|---------------------|
| [Homepage](ui-home.md) | Main navigation links: **My Trees** · **Sources** · **Places** · **Help** |
| [Tree View](ui-genealogy-tree.md) | Breadcrumb + Search fields (see §4) |
| [Person Profile](ui-person-profile.md) | Breadcrumb: `My trees › Tree name › Person name` |
| [Search Results](ui-search-results.md) | Breadcrumb + Search fields (pre-filled) |
| [Settings](ui-settings.md) | Breadcrumb: `My trees › Tree name › Settings` |

**Navigation links** are styled as text links with `var(--text-secondary)` color, `var(--text-primary)` on hover, and `var(--orange)` for the active page. Font: Lato, 0.85rem.

**Breadcrumbs** use `›` as separator. Each crumb except the last is a clickable link. The last crumb is `var(--text-primary)` and not clickable. Crumbs use Lato font, 0.85rem.

### Right zone — Actions

| Element | Description | Always visible |
|---------|-------------|----------------|
| Theme toggle | Light/dark mode switch (icon button) | Yes |
| Notifications | Bell icon button with optional badge | Yes |
| Settings | Gear icon button, links to app-level settings | Yes |
| Vertical divider | 1px, 24px tall | Yes |
| User avatar | Initials in an orange gradient circle (32px), dropdown on click | Yes |

Icon buttons: 32×32px, transparent background, `var(--text-secondary)` icon color, `var(--text-primary)` on hover.

---

## 4. Search Fields (Tree Pages Only)

On the [Tree View](ui-genealogy-tree.md) and [Search Results](ui-search-results.md) pages, the center zone includes two search fields after the breadcrumb:

```
My trees › Famille Martin    [Last name ________] [First name ________] [🔍]
```

### Field specifications

- Two independent text inputs: **Last name(s)** and **First name(s)**
- Either field can be used alone, or both combined
- Compact style: height 32px, `var(--bg-card)` background, `var(--border)` border, `var(--text-primary)` text
- Placeholder text in `var(--text-muted)`

### Real-time dropdown (autocomplete)

- Results filtered on each keystroke with **200ms debounce**
- Maximum **7–8 results** displayed
- Each result shows: thumbnail photo (28px square) + full name + birth/death dates
- Click on a result → that person becomes the tree focus, dropdown closes
- "No person found" message if no matches
- Dropdown closes on outside click or `Escape`

### On Enter

- Navigates to the [Search Results](ui-search-results.md) page with the full filtered results list
- The search fields remain pre-filled on the results page

---

## 5. User Avatar Dropdown

Clicking the user avatar opens a dropdown menu anchored to the top-right corner:

| Item | Action |
|------|--------|
| **Profile** | Opens user profile settings |
| **Preferences** | Opens app-level preferences (language, theme defaults) |
| Divider | — |
| **Sign out** | Logs out (post-MVP, EPIC E) |

The dropdown closes on outside click or `Escape`.

---

## 6. Responsive

| Breakpoint | Behavior |
|---|---|
| **≥ 900px** | Full layout: logo + navigation/breadcrumb + search + actions |
| **< 900px** | Navigation links collapse into a hamburger menu (☰) to the right of the logo. Search fields move below the topbar as a collapsible row. Action icons reduce to avatar only (others in hamburger menu). |
| **< 600px** | Brand name hidden, logo only. Breadcrumb truncated to last 2 crumbs. |

---

## 7. Keyboard

| Key | Behavior |
|---|---|
| `/` | Focus the last name search field (when on tree pages) |
| `Escape` | Close any open dropdown (search, avatar menu) |
| `Tab` | Navigate between topbar elements |
