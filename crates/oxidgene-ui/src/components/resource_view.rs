//! How a load is shown while it runs and when it failed
//! (`docs/cross-cutting.md` §6, `docs/ui-common.md` §4.20): one skeleton,
//! one error with its retry, and [`ResourceView`], which draws whichever a
//! resource is in, or the caller's content once it has its value.
//!
//! Content that is genuinely empty is neither: that is `EmptyState`.

use dioxus::prelude::*;

use crate::api::ApiError;
use crate::i18n::{I18n, use_i18n};
use crate::router::Route;

/// A load's failure, as the reader is told it.
pub trait LoadFailure {
    /// Why the load failed, in the language of `i18n`.
    fn told(&self, i18n: &I18n) -> String;
}

impl LoadFailure for ApiError {
    fn told(&self, i18n: &I18n) -> String {
        self.message(i18n)
    }
}

/// A failure already put into the reader's words.
impl LoadFailure for String {
    fn told(&self, _i18n: &I18n) -> String {
        self.clone()
    }
}

/// What the content being loaded will look like, roughly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SkeletonShape {
    /// A few lines of text: a card's body, a sentence, a short section.
    #[default]
    Lines,
    /// The rows of a list or a table.
    Rows,
    /// A grid of cards.
    Cards,
}

impl SkeletonShape {
    const fn class(self) -> &'static str {
        match self {
            Self::Lines => "skeleton-lines",
            Self::Rows => "skeleton-rows",
            Self::Cards => "skeleton-cards",
        }
    }

    /// How many placeholder bars the shape draws.
    const fn bars(self) -> usize {
        match self {
            Self::Lines => 3,
            Self::Rows => 6,
            Self::Cards => 4,
        }
    }
}

/// The loading state: grey bars where the content will be, named for
/// assistive technology by the translation of `label_key`. It stays
/// invisible for the first instants, so a load answered at once — from the
/// cache, or by the desktop's own backend — never flashes it, and holds its
/// place from the start, so nothing moves when it appears.
#[component]
pub fn Skeleton(
    #[props(default)] shape: SkeletonShape,
    /// The translation key of what is loading.
    #[props(default = "common.loading")]
    label_key: &'static str,
    /// Further classes, for a page's own spacing.
    #[props(default)]
    class: String,
) -> Element {
    let i18n = use_i18n();
    rsx! {
        div {
            class: "skeleton {shape.class()} {class}",
            role: "status",
            "aria-busy": "true",
            span { class: "visually-hidden", {i18n.t(label_key)} }
            for bar in 0..shape.bars() {
                span { key: "{bar}", class: "skeleton-bar", "aria-hidden": "true" }
            }
        }
    }
}

/// How many times the browser reported the network back since the
/// application started: provided by the shell, counted on the web only.
#[derive(Clone, Copy)]
pub struct Reconnections(pub Signal<u32>);

/// Follow the browser's `online` and `offline` events (web only): each is
/// said in a toast, and a reconnection asks every failed load on screen
/// again ([`LoadError`]). The desktop's backend is in the process: the
/// machine's network says nothing about it.
pub fn use_init_connectivity() {
    let reconnections = use_context_provider(|| Reconnections(Signal::new(0)));
    #[cfg(target_arch = "wasm32")]
    use_network_events(reconnections);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = reconnections;
}

/// Say the browser's `online` and `offline` events and count the first.
#[cfg(target_arch = "wasm32")]
fn use_network_events(Reconnections(mut reconnections): Reconnections) {
    let toasts = crate::components::toast::use_toasts();
    let language: Signal<crate::i18n::Language> = use_context();
    use_future(move || async move {
        let mut events = document::eval(
            "window.addEventListener('online', () => dioxus.send(true));
             window.addEventListener('offline', () => dioxus.send(false));",
        );
        while let Ok(online) = events.recv::<bool>().await {
            let i18n = I18n::new(*language.peek());
            if online {
                toasts.success(i18n.t("connectivity.restored"));
                reconnections += 1;
            } else {
                toasts.error(i18n.t("connectivity.lost"));
            }
        }
    });
}

/// The error state of a load: what failed, in the reader's language, with
/// an action that asks again — and, when the whole page failed (`page`), a
/// way out to the list of trees. It asks again by itself when the browser
/// reports the network back.
#[component]
pub fn LoadError(
    /// Why it failed: [`LoadFailure::told`].
    message: String,
    onretry: EventHandler<()>,
    /// Whether the page has nothing else to show.
    #[props(default)]
    page: bool,
    #[props(default)] class: String,
) -> Element {
    let i18n = use_i18n();
    let reconnections = try_use_context::<Reconnections>();
    let at_mount = use_hook(|| reconnections.map(|Reconnections(count)| *count.peek()));
    use_effect(move || {
        if let Some(Reconnections(count)) = reconnections
            && Some(count()) != at_mount
        {
            onretry.call(());
        }
    });
    rsx! {
        div { class: "load-error {class}", role: "alert",
            div { class: "load-error-text",
                strong { {i18n.t("load_error.title")} }
                span { "{message}" }
            }
            div { class: "load-error-actions",
                button {
                    r#type: "button",
                    class: "btn btn-sm btn-outline",
                    onclick: move |_| onretry.call(()),
                    {i18n.t("load_error.retry")}
                }
                if page {
                    Link { to: Route::Home {}, class: "btn btn-sm btn-outline",
                        {i18n.t("not_found.go_home")}
                    }
                }
            }
        }
    }
}

/// How a resource is drawn in each of its states: the skeleton while it
/// loads, [`LoadError`] with a retry that starts it again when it failed,
/// and the caller's content once it has a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceView {
    shape: SkeletonShape,
    label_key: &'static str,
    page: bool,
}

impl ResourceView {
    /// The view of a load the whole page waits for: its error offers the
    /// way back to the list of trees beside the retry.
    pub const fn page(shape: SkeletonShape) -> Self {
        Self {
            shape,
            label_key: "common.loading",
            page: true,
        }
    }

    /// The view of a load one part of a page waits for.
    pub const fn section(shape: SkeletonShape) -> Self {
        Self {
            shape,
            label_key: "common.loading",
            page: false,
        }
    }

    /// Name what is loading by the translation of `label_key`.
    pub const fn labelled(self, label_key: &'static str) -> Self {
        Self { label_key, ..self }
    }

    /// Draw `resource`: `content` gets its value once it loaded.
    pub fn show<T: 'static, E: LoadFailure + 'static>(
        self,
        i18n: &I18n,
        resource: Resource<Result<T, E>>,
        content: impl FnOnce(&T) -> Element,
    ) -> Element {
        let state = resource.read();
        self.state(i18n, state.as_ref(), retry_of(resource), content)
    }

    /// Draw a resource that asks nothing until it knows what to ask
    /// (`None` from its future): that wait is drawn as loading.
    pub fn show_gated<T: 'static, E: LoadFailure + 'static>(
        self,
        i18n: &I18n,
        resource: Resource<Option<Result<T, E>>>,
        content: impl FnOnce(&T) -> Element,
    ) -> Element {
        let state = resource.read();
        let loaded = state.as_ref().and_then(Option::as_ref);
        self.state(i18n, loaded, retry_of(resource), content)
    }

    /// Draw a resource that has no value to show yet: its error when it
    /// failed, the skeleton otherwise. For a caller that draws the value
    /// itself, beside other things.
    pub fn pending<T: 'static, E: LoadFailure + 'static>(
        self,
        i18n: &I18n,
        resource: Resource<Result<T, E>>,
    ) -> Element {
        match &*resource.read() {
            Some(Err(error)) => self.failed(error.told(i18n), retry_of(resource)),
            _ => self.loading(),
        }
    }

    /// Draw a load from its state, for a caller that holds something else
    /// than a resource: `retry` asks again.
    pub fn state<T, E: LoadFailure>(
        self,
        i18n: &I18n,
        state: Option<&Result<T, E>>,
        retry: impl FnMut() + 'static,
        content: impl FnOnce(&T) -> Element,
    ) -> Element {
        match state {
            None => self.loading(),
            Some(Err(error)) => self.failed(error.told(i18n), retry),
            Some(Ok(value)) => content(value),
        }
    }

    /// The loading state.
    pub fn loading(self) -> Element {
        rsx! {
            Skeleton { shape: self.shape, label_key: self.label_key }
        }
    }

    /// The error state: `message` says why, `retry` asks again.
    pub fn failed(self, message: String, mut retry: impl FnMut() + 'static) -> Element {
        rsx! {
            LoadError { message, page: self.page, onretry: move |()| retry() }
        }
    }
}

/// What asks `resource` again: its failure leaves the screen at once, so
/// the retry is seen to run, and its future starts over.
pub fn retry_of<T: 'static>(mut resource: Resource<T>) -> impl FnMut() + 'static {
    move || {
        resource.clear();
        resource.restart();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_has_a_class_of_its_own_and_draws_bars() {
        let shapes = [
            SkeletonShape::Lines,
            SkeletonShape::Rows,
            SkeletonShape::Cards,
        ];
        let classes: std::collections::BTreeSet<_> =
            shapes.iter().map(|shape| shape.class()).collect();
        assert_eq!(classes.len(), shapes.len());
        for shape in shapes {
            assert!(shape.bars() > 0);
            assert!(
                crate::components::layout::LAYOUT_STYLES.contains(&format!(".{}", shape.class())),
                "{shape:?} is styled"
            );
        }
    }
}
