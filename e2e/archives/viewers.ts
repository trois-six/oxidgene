// How the browser check reads each platform's viewer (docs/archives.md
// §9.1, step 4): the controls a target opens on, keyed by the adapter's
// platform id. An adapter added to `oxidgene-archives` adds its entry here.

export interface Viewer {
    // The button accepting the reuse licence the viewer shows first, if any.
    licence?: string;
    // The element showing the one-based view number: an input's value or
    // an element's text, whose first number is read.
    view: string;
    // The element showing the register's view count, if the viewer shows it.
    viewCount?: string;
}

export const viewers: Record<string, Viewer> = {
    // Arkothèque 8: the viewer's own test hooks.
    arkotheque: {
        licence: 'button[data-cy="accept-license"]',
        view: 'input[data-cy="input-position-image"]',
        viewCount: '[data-cy="nb-total-images"]',
    },
};
