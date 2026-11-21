//! Remote pictures: no element of the interface draws an address without
//! the answer of the remote gate, and both shells' policies allow plain
//! `http` pictures from the catalogue's plain-`http` portals only
//! (docs/cross-cutting.md §7.2).
//!
//! Drift it prevents:
//!
//! - a new `img`, SVG `image`, `video` or `audio` written straight into
//!   some markup with an address that may be somebody else's: a file
//!   received from a third party then makes every reader's engine call the
//!   addresses its author chose — who looks at the tree and when, or a
//!   request to a device of the reader's own network;
//! - a Content-Security-Policy that goes back to `http:` for every host,
//!   or that names a plain-`http` host the archive catalogue no longer
//!   lists, or misses one it now lists.
//!
//! The elements are those of the markup of `crates/oxidgene-ui` — outside
//! its stylesheets and its tests — that the engine fetches a source for.
//! [`DRAWERS`] lists the files that write one, with how many and why each
//! needs no question: it asks the gate itself, it is handed the address a
//! gate let through, or its address is never anybody else's.
//!
//! Fixing a failure: draw the picture with `GatedImage`, `CroppedImage` or
//! `CroppedSvgImage` (`components/remote_media.rs`,
//! `components/cropped_image.rs`); or, for an element that must be written
//! where it is, hand it the address `RemoteGate::picture` gave and count it
//! in [`DRAWERS`]. For the policies: give `img-src` of
//! `docker/security-headers.conf` and of the desktop's `WINDOW_POLICY` the
//! plain-`http` origins of `assets/archives/` and no other.

use std::collections::BTreeSet;

use oxidgene_guards::{
    files, find_styles_const, line_of, read, relative, styles_at, without_test_modules,
};

/// The elements the engine fetches a source for.
const ELEMENTS: &[&str] = &[
    "img {", "image {", "video {", "audio {", "iframe {", "embed {", "object {", "source {",
];

/// The files of the interface that write such an element: how many, and
/// why none of them draws an address nobody asked about.
const DRAWERS: &[(&str, usize, &str)] = &[
    (
        "crates/oxidgene-ui/src/components/remote_media.rs",
        1,
        "`GatedImage`, which asks the gate",
    ),
    (
        "crates/oxidgene-ui/src/components/cropped_image.rs",
        4,
        "portraits and crops, which ask the gate and fall back",
    ),
    (
        "crates/oxidgene-ui/src/components/media_stage.rs",
        1,
        "the viewer's stage, handed the address the viewer's gate let through",
    ),
    (
        "crates/oxidgene-ui/src/components/media_gallery.rs",
        2,
        "the viewer's video and audio players, handed the same address",
    ),
    (
        "crates/oxidgene-ui/src/components/image_cropper.rs",
        1,
        "the cropper's picture, handed the address its gate let through",
    ),
    (
        "crates/oxidgene-ui/src/components/pdf_viewer.rs",
        1,
        "a `blob:` picture pdf.js drew of a file we hold",
    ),
    (
        "crates/oxidgene-ui/src/components/user_avatar.rs",
        1,
        "an avatar the API serves",
    ),
    (
        "crates/oxidgene-ui/src/components/avatar_cropper.rs",
        1,
        "the file the user has just chosen",
    ),
    (
        "crates/oxidgene-ui/src/components/breadcrumb.rs",
        1,
        "the logo, compiled in",
    ),
    (
        "crates/oxidgene-ui/src/components/layout.rs",
        1,
        "the logo, compiled in",
    ),
];

/// `source` without its stylesheet constants, where `img {` is a selector.
fn without_styles(source: &str) -> String {
    let mut markup = String::new();
    let mut rest = source;
    while let Some(at) = find_styles_const(rest) {
        let (_, end) = styles_at(rest, at);
        markup.push_str(&rest[..at]);
        rest = &rest[end..];
    }
    markup.push_str(rest);
    markup
}

/// The byte offset of each fetching element written in the markup of
/// `source`: its name opening a block, first on its line or right after
/// the brace that opens the block it is in.
fn fetching_elements(source: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for element in ELEMENTS {
        for (at, _) in source.match_indices(element) {
            let line_start = source[..at].rfind('\n').map_or(0, |newline| newline + 1);
            let before = source[line_start..at].trim_end();
            let is_comment = before.trim_start().starts_with("//");
            if !is_comment && (before.trim().is_empty() || before.ends_with('{')) {
                found.push(at);
            }
        }
    }
    found.sort_unstable();
    found
}

#[test]
fn every_fetching_element_is_one_the_gate_answers_for() {
    let mut drift = Vec::new();
    let mut seen = BTreeSet::new();
    for path in files("crates/oxidgene-ui/src", "rs") {
        let file = relative(&path);
        let source = read(&file);
        let markup = without_styles(without_test_modules(&source));
        let elements = fetching_elements(&markup);
        let declared = DRAWERS.iter().find(|(drawer, _, _)| *drawer == file);
        match (declared, elements.len()) {
            (None, 0) => {}
            (None, _) => drift.extend(elements.iter().map(|at| {
                format!(
                    "{file}:{}: an element the engine fetches a source for, outside the \
                     components that ask the remote gate",
                    line_of(&markup, *at)
                )
            })),
            (Some((_, count, why)), written) => {
                seen.insert(file.clone());
                if written != *count {
                    drift.push(format!(
                        "{file}: {written} fetching elements where {count} are declared ({why})"
                    ));
                }
            }
        }
    }
    for (drawer, _, _) in DRAWERS {
        if !seen.contains(*drawer) {
            drift.push(format!(
                "{drawer}: declared, and gone or without an element"
            ));
        }
    }
    assert!(
        drift.is_empty(),
        "pictures drawn without asking whether their address may be loaded \
         (docs/cross-cutting.md §7.2):\n{}",
        drift.join("\n")
    );
}

/// The sources directive `name` of `policy` allows.
fn directive<'a>(policy: &'a str, name: &str) -> Vec<&'a str> {
    policy
        .split(';')
        .map(str::trim)
        .find_map(|directive| directive.strip_prefix(name))
        .map(|sources| sources.split_whitespace().collect())
        .unwrap_or_default()
}

/// The Content-Security-Policy the web frontend's nginx sends.
fn web_policy() -> String {
    let conf = read("docker/security-headers.conf");
    let line = conf
        .lines()
        .find(|line| line.starts_with("add_header Content-Security-Policy"))
        .expect("the web policy");
    line.split('"').nth(1).expect("its value").to_string()
}

/// The Content-Security-Policy of the desktop's main window.
fn desktop_policy() -> String {
    let source = read("apps/oxidgene-desktop/src/main.rs");
    let declared = "const WINDOW_POLICY: &str = \"";
    let start = source.find(declared).expect("the desktop policy") + declared.len();
    let end = start + source[start..].find("\";").expect("its end");
    // A backslash continues the literal on the next line.
    source[start..end]
        .split('\\')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The plain-`http` origins the archive catalogue's portals answer on:
/// every `http://` address of its documents but an archive's website and
/// terms, which are links and never a picture's host.
fn plain_http_portals() -> BTreeSet<String> {
    let mut origins = BTreeSet::new();
    for path in files("assets/archives", "json") {
        let document = read(relative(&path));
        for line in document.lines() {
            let line = line.trim();
            if line.starts_with("\"website\"") || line.starts_with("\"terms\"") {
                continue;
            }
            for (at, _) in line.match_indices("\"http://") {
                let address = &line[at + 1..];
                let end = address.find('"').unwrap_or(address.len());
                let address = &address[..end];
                let host_end = address["http://".len()..]
                    .find('/')
                    .map_or(address.len(), |slash| "http://".len() + slash);
                origins.insert(address[..host_end].to_string());
            }
        }
    }
    origins
}

#[test]
fn the_policies_allow_plain_http_pictures_from_the_catalogues_portals_only() {
    let portals = plain_http_portals();
    // The application's own origin where it is plain `http`: WebView2's.
    let policies = [
        ("docker/security-headers.conf", web_policy(), None),
        (
            "apps/oxidgene-desktop/src/main.rs",
            desktop_policy(),
            Some("http://dioxus.index.html"),
        ),
    ];
    let mut drift = Vec::new();
    for (file, policy, own) in &policies {
        for (name, portals) in [("img-src", &portals), ("media-src", &BTreeSet::new())] {
            let sources = directive(policy, name);
            if sources.is_empty() {
                drift.push(format!("{file}: no `{name}`: every address may be drawn"));
                continue;
            }
            for open in ["*", "http:"] {
                if sources.contains(&open) {
                    drift.push(format!("{file}: `{name}` allows `{open}`"));
                }
            }
            let plain: BTreeSet<String> = sources
                .iter()
                .filter(|source| source.starts_with("http://") && Some(**source) != *own)
                .map(|source| (*source).to_string())
                .collect();
            if plain != *portals {
                drift.push(format!(
                    "{file}: `{name}` allows plain http from {plain:?}, the catalogue's \
                     plain-http portals being {portals:?}"
                ));
            }
        }
    }
    assert!(
        drift.is_empty(),
        "a policy and the archive catalogue disagree on plain `http` \
         (docs/cross-cutting.md §7.2, docs/archives.md §3.1):\n{}",
        drift.join("\n")
    );
}

#[test]
fn the_elements_of_some_markup_are_found_where_they_are_written() {
    let markup = r#"
        // img { src: "{commented}" }
        match image {
            Some(image) => rsx! { img { src: "{image}" } },
            None => rsx! {},
        }
        if let Some(image) = image {
            image { "href": "{image}" }
        }
        const SHEET_STYLES: &str = r"
            .thing img { width: 100%; }
        ";
        div {
            video { src: "{url}" }
        }
    "#;
    let found: Vec<usize> = fetching_elements(markup)
        .into_iter()
        .map(|at| line_of(markup, at))
        .collect();
    assert_eq!(found, [4, 8, 14]);
}
