# Visual & Functional Specifications — App Settings

> Part of the [OxidGene Specifications](README.md).
> See also: [Homepage](ui-home.md) (gear icon in header) · [Topbar](ui-topbar.md) · [Design Tokens](ui-design-tokens.md) (theme switching)

---

## 1. Overview

The app settings page (`/settings`) is a dedicated full-page interface for configuring **application-level** preferences that are not tied to any specific tree. It is accessed via the gear icon in the [Homepage](ui-home.md) page header.

This page is distinct from [Tree Settings](ui-settings.md), which configure per-tree options.

---

## 2. Layout

Uses the standard `sub-page` layout pattern (see [General](general.md) section 8).

```
+----------------------------------------------------------------------+
| NAVBAR                                                                |
+----------------------------------------------------------------------+
| Home / Settings                                                      |  <- td-topbar
+----------------------------------------------------------------------+
|                                                                       |
|   +------------------+---------------------------------------------+ |
|   |                  |                                              | |
|   | LEFT NAVIGATION  |   CONTENT AREA                              | |
|   | (200px)          |                                              | |
|   |                  |   Section eyebrow                            | |
|   | Preferences      |   Section title                              | |
|   | - Appearance     |   Content (cards, toggles, options)          | |
|   | - Language       |                                              | |
|   |                  |                                              | |
|   +------------------+---------------------------------------------+ |
|                                                                       |
+----------------------------------------------------------------------+
```

Content: `max-width: 1200px`, centered, scrollable. Left navigation + content area use a flex row layout (`.settings-layout`).

---

## 3. Topbar

Uses the shared `td-topbar` + `td-bc` breadcrumb component:

```
Home / Settings
```

- "Home" (`.td-bc-link`) links to the homepage (`/`)
- `/` separator (`.td-bc-sep`)
- "Settings" (`.td-bc-current`) — not clickable

---

## 4. Left Navigation

Fixed width: 200px. One group labeled "Preferences" (uppercase, orange).

Items:
| Item | Section |
|---|---|
| Appearance | Theme toggle |
| Language | Language selection |

Active item: orange text, bold weight, subtle background (`var(--bg-card)`).

---

## 5. Section: Appearance

### Header

- Eyebrow: "Appearance" (uppercase, orange)
- Title: "Appearance" (Cinzel font)
- Subtitle: "Customise the look and feel of the application."

### Theme Toggle

Displayed in a card (`.app-settings-card`):

```
+-----------------------------------------------------------+
|  Theme                                                     |
|  Light theme is active / Dark theme is active             |
|                                                            |
|  [ (sun) Light ][ (moon) Dark ]                           |
+-----------------------------------------------------------+
```

- Label: "Theme" (bold) + current state hint (muted)
- Toggle group: two buttons side by side in a bordered container
- Each button: icon (sun/moon SVG) + label text
- Active button: `var(--orange)` background, white text
- Inactive button: transparent background, muted text

Clicking a button immediately applies the theme (no save step). The preference is persisted in `localStorage('oxidgene-theme')`.

---

## 6. Section: Language

### Header

- Eyebrow: "Language" (uppercase, orange)
- Title: "Language" (Cinzel font)
- Subtitle: "Choose your preferred language."

### Language Options

Displayed in a card:

```
+-----------------------------------------------------------+
|  [flag] English                                    [check] |
|  [flag] Francais                                          |
+-----------------------------------------------------------+
```

- Each language is a full-width button with: flag emoji, language name, optional checkmark for active
- Active language: orange border, subtle orange tint background
- Clicking a language immediately switches the UI language (no save step)
- The preference is persisted in `localStorage('oxidgene-lang')`

---

## 7. Responsive

- Content max-width: 1200px, responsive padding
- Below **640px**: left navigation stacks above the content area (flex column). Navigation items flow horizontally as a row of buttons.
- Theme toggle and language options remain full-width cards

---

## 8. Future Sections

Additional sections may be added in future EPICs:

| Section | Description |
|---|---|
| Account | User profile, email, password (EPIC E) |
| Notifications | Notification preferences (EPIC E) |
| Data & Privacy | Data export, account deletion (EPIC E) |
