//! Pictures held only as somebody else's address: whether the engine is
//! handed one, and what stands in its place until the reader asks
//! (`docs/cross-cutting.md` §7.2, `docs/ui-common.md` §4.5).
//!
//! A medium may be an address rather than a file we hold, and the engine is
//! what fetches it. A file received from somebody else would make every
//! reader's engine call addresses its author chose — telling a site who
//! looks at the tree and when, or reaching a device of the reader's own
//! network. So no element draws such an address without the answer of
//! [`RemoteGate::picture`]: [`GatedImage`] for a plain picture, the
//! components of `cropped_image` for a portrait or a crop, and the viewer's
//! stage, its players and the cropper through the component that hands them
//! their address. The `remote_pictures` guard keeps the list closed.
//!
//! What the gate lets through: the application's own addresses; the
//! portals of the archive catalogue; the hosts the tree trusts, stored with
//! it and shared by its readers; the hosts this reader asked for, kept
//! until the window closes or reloads. A private, loopback or local address
//! is never drawn. Everything else is [`RemotePlaceholder`]: the host's
//! name, and the click that loads its pictures.

use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;
use oxidgene_archives::ArchiveRegistry;
use oxidgene_core::remote_media::{HostTrust, RemoteLoad, remote_load, trustable_host};
use uuid::Uuid;

use crate::api::ApiClient;
use crate::components::toast;
use crate::components::tree_cache::TreeCache;
use crate::i18n::use_i18n;
use crate::permissions::{Permission, use_can};

/// What the whole application shares of the gate: where its own pictures
/// come from, and the hosts the reader asked for.
#[derive(Clone, Copy)]
struct Session {
    /// The API's address when pictures are drawn from it (the web build),
    /// empty when they come from the application's own origin.
    api_base: Signal<String>,
    /// The hosts whose pictures the reader asked for, by tree. Never
    /// stored: a reader's own choice is theirs, for this window.
    asked: Signal<HashMap<Uuid, HashSet<String>>>,
}

/// Call once in the application shell.
pub fn use_init_remote_media() {
    let api = try_use_context::<ApiClient>();
    use_context_provider(|| Session {
        api_base: Signal::new(
            api.map(|api| api.picture_base().to_string())
                .unwrap_or_default(),
        ),
        asked: Signal::new(HashMap::new()),
    });
}

/// Where the hosts a tree trusts are read.
#[derive(Clone, Copy, PartialEq)]
enum Trusted {
    /// The tree on screen, as the tree cache holds it.
    Cache,
    /// A tree whose list the page already has: a card of the home page.
    Given(Signal<Vec<String>>),
}

/// The tree whose pictures the components below draw.
#[derive(Clone, Copy, PartialEq)]
struct TreeScope {
    tree_id: Uuid,
    trusted: Trusted,
}

/// Says which tree the page under it shows, for the gate: the layout
/// renders it around every routed page, under the provider it keys by the
/// tree. The hosts that tree trusts are those of the tree every tree page
/// reads into the tree cache; until it is read, none is.
#[component]
pub fn RemoteMediaScope(tree_id: Option<Uuid>, children: Element) -> Element {
    use_context_provider(|| {
        tree_id.map(|tree_id| TreeScope {
            tree_id,
            trusted: Trusted::Cache,
        })
    });
    children
}

/// Says that `children` draw pictures of tree `tree_id`, which trusts
/// `hosts`: for a page showing several trees at once, which keys it by the
/// tree.
#[component]
pub fn TrustedMediaHosts(tree_id: Uuid, hosts: Vec<String>, children: Element) -> Element {
    let hosts = crate::utils::use_synced(hosts);
    use_context_provider(|| {
        Some(TreeScope {
            tree_id,
            trusted: Trusted::Given(hosts),
        })
    });
    children
}

/// What becomes of an address an element is about to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picture {
    /// Drawn, from this address: the one given, or its `https` spelling.
    Draw(String),
    /// Not loaded until the reader asks for the pictures of this host.
    Ask(String),
    /// Never loaded: not an address of the public network. The host, when
    /// it names one.
    Refused(Option<String>),
}

impl Picture {
    /// The address to draw from, when it is drawn.
    #[must_use]
    pub fn drawn(self) -> Option<String> {
        match self {
            Self::Draw(address) => Some(address),
            Self::Ask(_) | Self::Refused(_) => None,
        }
    }

    /// What stands in for the picture, when it is not drawn.
    #[must_use]
    pub fn blocked(&self) -> Option<Blocked> {
        match self {
            Self::Draw(_) => None,
            Self::Ask(host) => Some(Blocked::Ask(host.clone())),
            Self::Refused(host) => Some(Blocked::Refused(host.clone())),
        }
    }
}

/// Why a picture is not drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocked {
    /// The reader has not asked for the pictures of this host.
    Ask(String),
    /// The address is never loaded.
    Refused(Option<String>),
}

/// Whether `address` is the application's own: a path of its origin, an
/// inline or in-memory picture, or an address under the API's.
///
/// Read as a browser reads it: the tabs and line breaks it drops anywhere
/// in an address are dropped first, and a path whose second character is a
/// slash or a backslash is no path but another host (`//host`, `/\host`).
fn is_own(address: &str, api_base: &str) -> bool {
    if address.contains(['\t', '\n', '\r']) {
        let read: String = address
            .chars()
            .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
            .collect();
        return is_own(&read, api_base);
    }
    let own_path = address
        .strip_prefix('/')
        .is_some_and(|path| !path.starts_with(['/', '\\']));
    own_path
        || address.starts_with("data:")
        || address.starts_with("blob:")
        || (!api_base.is_empty()
            && address
                .strip_prefix(api_base)
                .is_some_and(|rest| rest.starts_with('/')))
}

/// What becomes of `address`: drawn as it is when it is the application's
/// own, and otherwise as [`remote_load`] says of what `trust` makes of its
/// host.
fn decide(address: &str, api_base: &str, trust: impl FnOnce(&str) -> HostTrust) -> Picture {
    if is_own(address, api_base) {
        return Picture::Draw(address.to_string());
    }
    match remote_load(address, trust) {
        RemoteLoad::Load(address) => Picture::Draw(address),
        RemoteLoad::Ask(host) => Picture::Ask(host),
        RemoteLoad::Refuse(host) => Picture::Refused(host),
    }
}

/// Whether the pictures somebody else serves are loaded, for the tree on
/// screen and this reader.
#[derive(Clone, Copy)]
pub struct RemoteGate {
    session: Option<Session>,
    scope: Option<TreeScope>,
    tree_cache: Option<TreeCache>,
}

/// The gate of the component calling it, which is drawn again when the
/// trust it read changes.
pub fn use_remote_gate() -> RemoteGate {
    RemoteGate {
        session: try_use_context::<Session>(),
        scope: try_use_context::<Option<TreeScope>>().flatten(),
        tree_cache: try_use_context::<TreeCache>(),
    }
}

impl RemoteGate {
    /// What becomes of `address`.
    #[must_use]
    pub fn picture(self, address: &str) -> Picture {
        match self.session {
            Some(session) => session
                .api_base
                .with_peek(|api_base| decide(address, api_base, |host| self.trust(host))),
            None => decide(address, "", |host| self.trust(host)),
        }
    }

    /// Why the first of `addresses` that is not drawn is not: an address
    /// the reader can ask for before one that is never loaded, so that a
    /// tile of several pictures offers what it can.
    #[must_use]
    pub fn blocked<'a>(self, addresses: impl IntoIterator<Item = &'a str>) -> Option<Blocked> {
        let mut refused = None;
        for address in addresses {
            match self.picture(address).blocked() {
                Some(ask @ Blocked::Ask(_)) => return Some(ask),
                Some(other) => refused = refused.or(Some(other)),
                None => {}
            }
        }
        refused
    }

    /// What the catalogue, the tree and the reader make of `host`.
    fn trust(self, host: &str) -> HostTrust {
        let catalogue = ArchiveRegistry::embedded().host_trust(host);
        if catalogue != HostTrust::Unknown {
            return catalogue;
        }
        let Some(scope) = self.scope else {
            return HostTrust::Unknown;
        };
        let stored = match scope.trusted {
            Trusted::Cache => self
                .tree_cache
                .is_some_and(|cache| cache.trusts_media_host(scope.tree_id, host)),
            Trusted::Given(hosts) => hosts.read().iter().any(|trusted| trusted == host),
        };
        let asked = || {
            self.session.is_some_and(|session| {
                session
                    .asked
                    .read()
                    .get(&scope.tree_id)
                    .is_some_and(|hosts| hosts.contains(host))
            })
        };
        if stored || asked() {
            HostTrust::Trusted
        } else {
            HostTrust::Unknown
        }
    }

    /// Load the pictures of `host` in the tree on screen, for this reader,
    /// until the window closes or reloads.
    pub fn load(self, host: &str) {
        let (Some(session), Some(scope)) = (self.session, self.scope) else {
            return;
        };
        let known = session
            .asked
            .peek()
            .get(&scope.tree_id)
            .is_some_and(|hosts| hosts.contains(host));
        if known {
            return;
        }
        let mut asked = session.asked;
        asked
            .write()
            .entry(scope.tree_id)
            .or_default()
            .insert(host.to_string());
    }

    /// Load the pictures of the host of `address`: an address the reader
    /// has just typed, or taken from an archive's portal, is one they asked
    /// for.
    pub fn vouch(self, address: &str) {
        if let Some(host) = trustable_host(address) {
            self.load(&host);
        }
    }

    /// The tree on screen now trusts the host of `address`: what the
    /// server does when somebody adds an address themselves, mirrored so
    /// that the settings list it without another read.
    pub fn trusted(self, address: &str) {
        if let (Some(cache), Some(scope), Some(host)) =
            (self.tree_cache, self.scope, trustable_host(address))
        {
            cache.add_trusted_media_host(scope.tree_id, host);
        }
    }

    /// An address the reader has just saved on a page themselves: loaded
    /// for them, and trusted by the tree as the server recorded. Nothing
    /// for what is no address of a public host.
    pub fn added(self, address: &str) {
        self.vouch(address);
        self.trusted(address);
    }

    /// The tree whose pictures the gate decides for.
    fn tree_id(self) -> Option<Uuid> {
        self.scope.map(|scope| scope.tree_id)
    }
}

/// How much room the placeholder of a picture has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaceholderSize {
    /// The square beside a row: a glyph saying a picture is not loaded.
    #[default]
    Glyph,
    /// A small square with nothing else in it: one control that loads.
    Thumb,
    /// A gallery tile: the host, and the controls.
    Tile,
    /// The viewer's stage: the host, why, and the controls.
    Stage,
}

/// The mark of a picture that is not loaded.
fn glyph() -> Element {
    rsx! {
        svg {
            class: "remote-gate-glyph",
            width: "20", height: "20", fill: "none", "viewBox": "0 0 24 24",
            stroke: "currentColor", "strokeWidth": "1.6", "strokeLinecap": "round",
            "strokeLinejoin": "round", "aria-hidden": "true",
            rect { x: "3", y: "5", width: "18", height: "14", rx: "2" }
            path { d: "M3 16l4.5-4.5 3.5 3.5M14 13l2-2 5 5" }
            path { d: "M4 4l16 16" }
        }
    }
}

/// What stands in for a picture somebody else serves: the host it would
/// be loaded from, and the controls that load it — for this reader, or,
/// for whom may edit the tree, for every reader from now on. A picture
/// that is never loaded says so and offers nothing.
#[component]
pub fn RemotePlaceholder(blocked: Blocked, #[props(default)] size: PlaceholderSize) -> Element {
    let i18n = use_i18n();
    let gate = use_remote_gate();
    let api = try_use_context::<ApiClient>();
    let may_trust = use_can(Permission::Edit);
    let mut saving = use_signal(|| false);
    let toasts = toast::use_toasts();

    let host = match blocked {
        Blocked::Ask(host) => host,
        Blocked::Refused(host) => {
            let text = match &host {
                Some(host) => i18n.t_args("media.remote_refused", &[("host", host.as_str())]),
                None => i18n.t("media.remote_refused_address"),
            };
            return rsx! {
                div {
                    class: "remote-gate is-refused {size_class(size)}",
                    role: "img",
                    aria_label: "{text}",
                    title: "{text}",
                    {glyph()}
                    if matches!(size, PlaceholderSize::Tile | PlaceholderSize::Stage) {
                        span { class: "remote-gate-note", "{text}" }
                    }
                }
            };
        }
    };
    let host_args = [("host", host.as_str())];
    let not_loaded = i18n.t_args("media.remote_not_loaded", &host_args);
    let load_label = i18n.t_args("media.remote_load_from", &host_args);
    let load = {
        let host = host.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            gate.load(&host);
        }
    };
    let always = {
        let host = host.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            let (Some(api), Some(tree_id)) = (api.clone(), gate.tree_id()) else {
                return;
            };
            let host = host.clone();
            saving.set(true);
            spawn(async move {
                match api.trust_media_host(tree_id, &host).await {
                    Ok(tree) => {
                        gate.load(&host);
                        if let Some(cache) = gate.tree_cache {
                            cache.refresh_tree(tree_id, tree);
                        }
                        toasts.success(
                            i18n.t_args("media.remote_trusted", &[("host", host.as_str())]),
                        );
                    }
                    Err(error) => toasts.error(error.message(&i18n)),
                }
                saving.set(false);
            });
        }
    };

    match size {
        PlaceholderSize::Glyph => rsx! {
            span {
                class: "remote-gate is-glyph",
                role: "img",
                aria_label: "{not_loaded}",
                title: "{not_loaded}",
                {glyph()}
            }
        },
        PlaceholderSize::Thumb => rsx! {
            button {
                class: "remote-gate is-thumb",
                r#type: "button",
                aria_label: "{load_label}",
                title: "{load_label}",
                onclick: load,
                onkeydown: move |event: Event<KeyboardData>| event.stop_propagation(),
                {glyph()}
            }
        },
        PlaceholderSize::Tile | PlaceholderSize::Stage => rsx! {
            div {
                class: "remote-gate {size_class(size)}",
                role: "group",
                aria_label: "{not_loaded}",
                // A control of its own inside a tile that opens the viewer:
                // neither its keys nor its presses are the tile's.
                onkeydown: move |event: Event<KeyboardData>| event.stop_propagation(),
                onpointerdown: move |event: Event<PointerData>| event.stop_propagation(),
                {glyph()}
                span { class: "remote-gate-host", title: "{host}", "{host}" }
                if size == PlaceholderSize::Stage {
                    p { class: "remote-gate-note",
                        {i18n.t_args("media.remote_explain", &host_args)}
                    }
                }
                div { class: "remote-gate-actions",
                    button {
                        class: "btn btn-sm remote-gate-load",
                        r#type: "button",
                        aria_label: "{load_label}",
                        title: "{load_label}",
                        onclick: load,
                        {i18n.t("media.remote_load")}
                    }
                    if may_trust && gate.tree_id().is_some() {
                        button {
                            class: "btn btn-sm btn-outline remote-gate-always",
                            r#type: "button",
                            disabled: saving(),
                            aria_label: i18n.t_args("media.remote_always_from", &host_args),
                            title: i18n.t_args("media.remote_always_from", &host_args),
                            onclick: always,
                            {i18n.t("media.remote_always")}
                        }
                    }
                }
            }
        },
    }
}

fn size_class(size: PlaceholderSize) -> &'static str {
    match size {
        PlaceholderSize::Glyph => "is-glyph",
        PlaceholderSize::Thumb => "is-thumb",
        PlaceholderSize::Tile => "is-tile",
        PlaceholderSize::Stage => "is-stage",
    }
}

/// A picture from an address that may be somebody else's: drawn when the
/// gate lets it, and its placeholder otherwise. A picture of ours is drawn
/// as it always was.
#[component]
pub fn GatedImage(
    src: String,
    alt: String,
    #[props(default)] class: Option<String>,
    #[props(default)] placeholder: PlaceholderSize,
) -> Element {
    let gate = use_remote_gate();
    match gate.picture(&src) {
        Picture::Draw(address) => rsx! {
            img {
                class,
                src: "{address}",
                alt,
                loading: "lazy",
                "referrerpolicy": "no-referrer",
            }
        },
        other => match other.blocked() {
            Some(blocked) => rsx! { RemotePlaceholder { blocked, size: placeholder } },
            None => rsx! {},
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const API: &str = "http://127.0.0.1:18080";

    fn unknown(_: &str) -> HostTrust {
        HostTrust::Unknown
    }

    fn trusted(_: &str) -> HostTrust {
        HostTrust::Trusted
    }

    #[test]
    fn the_applications_own_pictures_are_drawn_without_asking_anything() {
        for address in [
            "/oxidgene-media/trees/1/media/2/thumbnail?v=00aa",
            "/silhouettes/unknown.png",
            "data:image/png;base64,AAAA",
            "blob:http://127.0.0.1:18081/6f1c",
            "http://127.0.0.1:18080/api/v1/trees/1/media/2/card?v=00aa",
        ] {
            let picture = decide(address, API, |_| panic!("{address}: the trust was asked"));
            assert_eq!(picture, Picture::Draw(address.to_string()), "{address}");
        }
    }

    #[test]
    fn an_address_that_only_looks_like_ours_is_not_ours() {
        for address in [
            // Another host that starts like the API's.
            "http://127.0.0.1:18080.tracker.example.org/pixel.gif",
            "http://127.0.0.1:180801/pixel.gif",
            // The reader's own machine, on another port.
            "http://127.0.0.1:9000/pixel.gif",
            // Resolved against the page's scheme, not its origin: a
            // backslash is a slash, and a tab or a line break is dropped.
            "//tracker.example.org/pixel.gif",
            "/\\tracker.example.org/pixel.gif",
            "/\t/tracker.example.org/pixel.gif",
            "/\n\\tracker.example.org/pixel.gif",
        ] {
            assert!(!is_own(address, API), "{address}");
            assert!(
                matches!(decide(address, API, trusted), Picture::Refused(_)),
                "{address}"
            );
        }
        // With pictures served from the application's own origin, nothing
        // absolute is ours.
        assert!(!is_own("http://127.0.0.1:18080/api/v1/trees", ""));
    }

    #[test]
    fn somebody_elses_picture_waits_for_the_reader_and_names_its_host() {
        assert_eq!(
            decide("https://media.example.org/group/7.jpg", API, unknown),
            Picture::Ask("media.example.org".into())
        );
        assert_eq!(
            decide("http://media.example.org/group/7.jpg", API, trusted),
            Picture::Draw("https://media.example.org/group/7.jpg".into())
        );
        assert_eq!(
            decide("https://192.168.1.10/camera.jpg", API, trusted),
            Picture::Refused(Some("192.168.1.10".into()))
        );
        assert_eq!(decide("scans/7.jpg", API, trusted), Picture::Refused(None));
    }

    #[test]
    fn only_a_drawn_picture_has_an_address_and_only_another_a_placeholder() {
        let drawn = Picture::Draw("https://media.example.org/7.jpg".into());
        assert_eq!(drawn.blocked(), None);
        assert_eq!(
            drawn.drawn().as_deref(),
            Some("https://media.example.org/7.jpg")
        );
        let asked = Picture::Ask("media.example.org".into());
        assert_eq!(
            asked.blocked(),
            Some(Blocked::Ask("media.example.org".into()))
        );
        assert_eq!(asked.drawn(), None);
        let refused = Picture::Refused(None);
        assert_eq!(refused.blocked(), Some(Blocked::Refused(None)));
        assert_eq!(refused.drawn(), None);
    }

    #[test]
    fn the_catalogues_portals_are_trusted_and_one_over_plain_http() {
        let registry = ArchiveRegistry::embedded();
        let portal = |address: &str| decide(address, API, |host| registry.host_trust(host));
        assert_eq!(
            portal("http://anom.archivesnationales.culture.gouv.fr/view/7.jpg"),
            Picture::Draw("http://anom.archivesnationales.culture.gouv.fr/view/7.jpg".into())
        );
        assert_eq!(
            portal("http://gaia.orne.fr/view/7.jpg"),
            Picture::Draw("https://gaia.orne.fr/view/7.jpg".into())
        );
        assert_eq!(
            portal("https://media.example.org/7.jpg"),
            Picture::Ask("media.example.org".into())
        );
    }
}
