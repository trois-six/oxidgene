//! A read-only value with a button copying it to the clipboard: the one
//! copy control of the application.

use dioxus::prelude::*;

use crate::i18n::use_i18n;
use crate::utils::sleep_ms;

/// A read-only field with a copy button, shared by the API connection details,
/// the assistant command and the dates the Tools page writes out: a URL, a
/// command line, a JSON configuration and a sentence are the same
/// interaction, one line versus several.
///
/// Nothing here is saved anywhere — copying changes no setting, so the field
/// carries no `oninput` and no state beyond the brief "Copied" feedback.
#[component]
pub fn CopyField(label: String, value: String, multiline: bool) -> Element {
    let i18n = use_i18n();
    let mut copied = use_signal(|| false);
    let copy_value = value.clone();
    // Tall enough for the whole value, so a short example is not padded out
    // and a long configuration still leaves room for the rest of the page.
    let rows = value.lines().count().clamp(2, 12);

    let onclick = move |_| {
        let value = copy_value.clone();
        spawn(async move {
            if copy_to_clipboard(&value).await {
                copied.set(true);
                sleep_ms(1500).await;
                copied.set(false);
            }
        });
    };

    rsx! {
        div { class: "copy-field",
            span { class: "app-settings-option-label", "{label}" }
            div { class: "copy-field-row",
                if multiline {
                    textarea {
                        class: "copy-field-value",
                        readonly: true,
                        rows: "{rows}",
                        "aria-label": "{label}",
                        value: "{value}",
                    }
                } else {
                    input {
                        class: "copy-field-value",
                        r#type: "text",
                        readonly: true,
                        "aria-label": "{label}",
                        value: "{value}",
                    }
                }
                button {
                    class: "btn btn-outline btn-sm copy-field-btn",
                    r#type: "button",
                    onclick,
                    {if *copied.read() { i18n.t("common.copied") } else { i18n.t("common.copy") }}
                }
            }
        }
    }
}

/// Writes `text` to the system clipboard through `navigator.clipboard`, the
/// one clipboard API available in both the browser build and the desktop
/// WebView.
///
/// Returns whether the write reported success, so the caller shows "Copied"
/// only when it actually happened rather than after a write an insecure
/// context or a denied permission silently dropped.
pub(crate) async fn copy_to_clipboard(text: &str) -> bool {
    let Ok(js_text) = serde_json::to_string(text) else {
        return false;
    };
    let script = format!(
        r#"
        try {{
            await navigator.clipboard.writeText({js_text});
            return true;
        }} catch (e) {{
            return false;
        }}
        "#
    );
    document::eval(&script)
        .await
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}
