// How the browser check reads each platform's viewer (docs/archives.md
// §9.1, step 4): the controls a target opens on, keyed by the adapter's
// platform id. An adapter added to `oxidgene-archives` adds its entry here.

export interface Viewer {
    // The button accepting the reuse licence the viewer shows first, if any.
    licence?: string;
    // The element showing the one-based view number: an input's value or an
    // element's text, whose first number is read.
    view: string;
    // The element showing the register's view count, if the viewer shows it:
    // its last number is read (`/ 46`, `5/267`).
    viewCount?: string;
    // Whether the check aborts the viewer's image requests: a viewer that
    // shows its view number without them would otherwise download a full
    // image and its neighbours' thumbnails at each opening.
    blockImages?: boolean;
}

export const viewers: Record<string, Viewer> = {
    // Archinoë: `n/total` in the viewer's pagination.
    archinoe: {
        view: "#visu_pagination",
        viewCount: "#visu_pagination",
    },
    // Arkothèque 8: the viewer's own test hooks.
    arkotheque: {
        licence: 'button[data-cy="accept-license"]',
        view: 'input[data-cy="input-position-image"]',
        viewCount: '[data-cy="nb-total-images"]',
    },
    // GAIA: `Page n de total` in the canvas viewer's page box. The viewer
    // has no address per view: the check opens it on its first view.
    gaia: {
        view: "#pagination input[type=text]",
        viewCount: "#pagination input[type=text]",
        blockImages: true,
    },
    // Ligeo: the Monocle viewer's page navigation.
    ligeo: {
        view: '.monocle-PageNav input[role="spinbutton"]',
        viewCount: ".monocle-PageNav-total",
    },
    // Mnesys Expo: the media browser, behind the reuse conditions' dialog.
    mnesys: {
        licence: 'input.btn.primary[value="Accepter"]',
        view: ".media-browse .pagination-form input",
        viewCount: ".media-browse .page-count",
        blockImages: true,
    },
    // Prismia Vision: `n` and `total` in the view-number button.
    prismia: {
        view: 'button[aria-label="Numéro de la vue"]',
        viewCount: 'button[aria-label="Numéro de la vue"]',
    },
    // THOT: the Zoomify viewer's view input and its ` / 13`, shown without
    // the image tiles.
    thot: {
        view: "input#imageNum",
        viewCount: "h3#imageNumMax",
        blockImages: true,
    },
};
