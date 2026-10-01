//! The one tab bar, and the hook remembering which tab a viewer left a page
//! on.

use dioxus::prelude::*;

use crate::prefs::{store, stored};

/// A page's tabs: each named in storage and in its label's i18n key.
pub trait Tab: Copy + PartialEq + 'static {
    /// Every tab, in the bar's order; the first is the default.
    const ALL: &'static [Self];

    /// The tab's name in storage and in its label's key.
    fn key(self) -> &'static str;

    /// The tab named `key`, if any.
    fn parse(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|tab| tab.key() == key)
    }
}

/// A tab bar: one `tab` button per tab, `current` selected, its label read
/// from `labels`. Nothing is selected while `current` is unknown.
#[component]
pub fn Tabs<T: Copy + PartialEq + 'static>(
    tabs: Vec<(T, String)>,
    current: Option<T>,
    on_select: EventHandler<T>,
    /// Classes added to the bar.
    #[props(default)]
    class: String,
) -> Element {
    rsx! {
        div { class: "dict-tabs {class}", role: "tablist",
            for (index , (tab, label)) in tabs.into_iter().enumerate() {
                button {
                    key: "{index}",
                    r#type: "button",
                    role: "tab",
                    "aria-selected": current == Some(tab),
                    class: if current == Some(tab) { "dict-tab active" } else { "dict-tab" },
                    onclick: move |_| on_select.call(tab),
                    "{label}"
                }
            }
        }
    }
}

/// The tab a viewer left a page on, kept in the browser under `storage_key`.
///
/// `None` until the browser answers, so no tab mounts — and asks for its
/// data — before the one the viewer left; then the stored tab, else the
/// first. Choosing a tab through the returned handler stores it.
pub fn use_stored_tab<T: Tab>(storage_key: &'static str) -> (Signal<Option<T>>, EventHandler<T>) {
    let mut tab = use_signal(|| None::<T>);
    use_effect(move || {
        spawn(async move {
            let stored_tab = stored(storage_key).await.as_deref().and_then(T::parse);
            tab.set(Some(stored_tab.unwrap_or(T::ALL[0])));
        });
    });
    let choose = use_callback(move |value: T| {
        tab.set(Some(value));
        store(storage_key, value.key());
    });
    (tab, choose)
}
