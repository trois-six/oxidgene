// How the browser check reads each platform's viewer (docs/archives.md
// §9.1, step 4): the controls a target opens on, keyed by the adapter's
// platform id. They live in oxidgene-archives'
// `platform/viewers.json`, which the desktop's archive window reads too to
// bring a viewer without an address per view to the cited view; an adapter
// added to `oxidgene-archives` adds its entry there.

import { readFileSync } from "node:fs";

export interface Viewer {
    // The button accepting the reuse licence the viewer shows first, if any,
    // or the one of the licence page a target stands behind (its report's
    // `licence`), which the check passes before opening the target.
    licence?: string;
    // The element showing the one-based view number: an input's value or an
    // element's text, whose first number is read.
    view?: string;
    // The element showing the register's view count, if the viewer shows it:
    // its last number is read (`/ 46`, `5/267`).
    view_count?: string;
    // Whether the check aborts the viewer's image requests: a viewer that
    // shows its view number without them would otherwise download a full
    // image and its neighbours' thumbnails at each opening.
    block_images?: boolean;
    // A pattern (a regular expression's source) of the viewer page's source
    // that shows it asks the reader to accept a reuse licence before showing
    // an image. An `iiif` archive, whose images the desktop window shows
    // without a portal step, must not match it (docs/archives.md §3.1): the
    // archive is `portal` instead.
    licence_wall?: string;
    // For a viewer without an address per view, how its page-number control
    // brings it to a view (the desktop's `go_to.js` reads it).
    go_to?: { input?: string; submit: "change" | "enter" | "blur" | "click"; button?: string };
}

export const viewers: Record<string, Viewer> = JSON.parse(
    readFileSync(new URL("../../crates/oxidgene-archives/src/platform/viewers.json", import.meta.url), "utf8"),
);
