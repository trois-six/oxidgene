//! The one "between two years" filter.

use dioxus::prelude::*;

use crate::i18n::use_i18n;

/// Two year fields under `label`, the first year and the last: each typed
/// value is reported as it is typed, unparsed, so a half-typed year is not
/// rejected mid-keystroke.
#[component]
pub fn YearRange(
    label: String,
    from: String,
    to: String,
    on_from: EventHandler<String>,
    on_to: EventHandler<String>,
) -> Element {
    let i18n = use_i18n();
    rsx! {
        div { class: "sr-filter-group",
            label { "{label}" }
            div { class: "sr-date-range",
                input {
                    r#type: "number",
                    placeholder: "1800",
                    aria_label: i18n.t("common.from"),
                    value: "{from}",
                    oninput: move |e: Event<FormData>| on_from.call(e.value()),
                }
                span { "aria-hidden": "true", "\u{2013}" }
                input {
                    r#type: "number",
                    placeholder: "2000",
                    aria_label: i18n.t("common.to"),
                    value: "{to}",
                    oninput: move |e: Event<FormData>| on_to.call(e.value()),
                }
            }
        }
    }
}
