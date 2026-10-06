// How the browser check reads each platform's viewer (docs/archives.md
// §9.1, step 4): the controls a target opens on, keyed by the adapter's
// platform id. An adapter added to `oxidgene-archives` adds its entry here.

export interface Viewer {
    // The button accepting the reuse licence the viewer shows first, if any.
    licence?: string;
    // The element showing the one-based view number: an input's value or an
    // element's text, whose first number is read.
    view?: string;
    // For a portal whose target is the page a reader starts from rather than
    // its viewer — its entry, behind a reuse licence the reader accepts —
    // the element the page behind the licence shows; `view` is then not
    // read, and the licence must have shown.
    landing?: string;
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
    // The Gers portal: `k / total` in the viewer's list of views, whose
    // chosen option is the view the address names.
    archives32: {
        view: "select#fichier option[selected]",
        viewCount: "select#fichier option[selected]",
        blockImages: true,
    },
    // Arkothèque 8: the viewer's own test hooks.
    arkotheque: {
        licence: 'button[data-cy="accept-license"]',
        view: 'input[data-cy="input-position-image"]',
        viewCount: '[data-cy="nb-total-images"]',
    },
    // Bach (Anaphore): the viewer's page input and its `/ 71` beside it. It
    // shows the view number without the images.
    bach: {
        view: '#currentpage input[type="number"]',
        viewCount: "#currentpage",
        blockImages: true,
    },
    // CAOMEC2 (the Archives nationales d'outre-mer): the OpenSeadragon
    // viewer's page input, and its strip of thumbnails, the last numbered
    // with the count. The viewer has no address per view: the check opens it
    // on its first view.
    caomec2: {
        view: "input#iddoc",
        viewCount: "#imgstrip .thn:last-child .thn_id",
        blockImages: true,
    },
    // GAIA: `Page n de total` in the canvas viewer's page box. The viewer
    // has no address per view: the check opens it on its first view.
    gaia: {
        view: "#pagination input[type=text]",
        viewCount: "#pagination input[type=text]",
        blockImages: true,
    },
    // Ligeo: the Monocle viewer's page navigation, or the Binocle viewer's
    // gallery counter (`<input value="5"> sur 29`) on the portals that run
    // it (Hautes-Alpes).
    ligeo: {
        view: '.monocle-PageNav input[role="spinbutton"], .bn-gallery-counter input.bn-gallery-counter-current',
        viewCount: ".monocle-PageNav-total, .bn-gallery-counter",
    },
    // Mnesys Expo: the media browser, behind the reuse conditions' dialog.
    mnesys: {
        licence: 'input.btn.primary[value="Accepter"]',
        view: ".media-browse .pagination-form input",
        viewCount: ".media-browse .page-count",
        blockImages: true,
    },
    // The older Mnesys interface's viewer host ("Visualiseur v2"): the
    // current view's input and the count beside it.
    "mnesys-inao": {
        view: "#inputvue_actuelle",
        viewCount: "#total",
        blockImages: true,
    },
    // Pleade: Mirador 3 (`Vue <input> / 269` in the canvas navigation) or
    // Mirador 2 (the highlighted thumbnail's label, `82 objets`), both of
    // which number their views from the manifest, without the images.
    pleade: {
        view: '.mirador-canvas-nav input[type="number"], li.highlight .thumb-label',
        viewCount: ".mirador-canvas-nav label, .canvas-count",
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
    // Visualys (the Côtes-d'Armor "salle virtuelle"): the target is a site's
    // entry, whose reuse licence the reader accepts before the search page —
    // the alphabetical list of localities, or the search form.
    visualys: {
        licence: "#btnAccepter",
        landing: "div.Abecedaire, form#frmRecherche",
        blockImages: true,
    },
};
