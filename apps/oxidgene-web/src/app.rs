//! The browser application: its configuration, observability and root
//! component.

use dioxus::prelude::*;
use oxidgene_ui::api::ApiClient;

const API_URL: &str = match option_env!("OXIDGENE_API_URL") {
    Some(url) => url,
    None => "http://127.0.0.1:8080",
};
const LOG_LEVEL: &str = match option_env!("OXIDGENE_LOG_LEVEL") {
    Some(level) => level,
    None => "info",
};
const OTLP_ENDPOINT: Option<&str> = option_env!("OTEL_EXPORTER_OTLP_ENDPOINT");

pub fn main() {
    let runtime_otlp_endpoint = runtime_otlp_endpoint();
    crate::observability::init(
        LOG_LEVEL,
        runtime_otlp_endpoint.as_deref().or(OTLP_ENDPOINT),
    )
    .expect("failed to initialize browser observability");
    dioxus::launch(WebApp);
}

/// The API's base URL: the one baked in at build time or, when that is empty
/// (an image serving the API on the page's own origin, behind an ingress), the
/// page's origin. The client needs an absolute URL: a relative one is never
/// requested, and would make every address look like the backend's.
fn api_base_url() -> String {
    if !API_URL.is_empty() {
        return API_URL.to_owned();
    }
    js_sys::Reflect::get(
        &js_sys::global(),
        &wasm_bindgen::JsValue::from_str("location"),
    )
    .and_then(|location| {
        js_sys::Reflect::get(&location, &wasm_bindgen::JsValue::from_str("origin"))
    })
    .ok()
    .and_then(|origin| origin.as_string())
    .expect("a page has an origin")
}

fn runtime_otlp_endpoint() -> Option<String> {
    let value = js_sys::Reflect::get(
        &js_sys::global(),
        &wasm_bindgen::JsValue::from_str("OXIDGENE_OTLP_ENDPOINT"),
    )
    .ok()?
    .as_string()?;
    (!value.is_empty()).then_some(value)
}

#[component]
fn WebApp() -> Element {
    use_context_provider(|| ApiClient::new(&api_base_url()));
    // No `CustomThemeSource` is provided: the browser build has no folder to
    // read themes from, so only the built-in ones are offered.
    rsx! { oxidgene_ui::App {} }
}
