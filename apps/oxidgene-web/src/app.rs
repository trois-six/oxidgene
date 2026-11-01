//! The browser application: its configuration, observability and root
//! component.

use std::sync::Arc;

use dioxus::prelude::*;
use oxidgene_ui::account::{AccountHost, Accounts};
use oxidgene_ui::api::ApiClient;
use oxidgene_ui::sharing::SharingSurfaces;
use wasm_bindgen::JsCast as _;

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

/// The page's `location`.
fn location() -> Option<wasm_bindgen::JsValue> {
    js_sys::Reflect::get(
        &js_sys::global(),
        &wasm_bindgen::JsValue::from_str("location"),
    )
    .ok()
}

/// The browser page the account UI signs in and out from
/// (docs/authentication.md §10): only the web build has accounts.
struct BrowserAccounts;

impl AccountHost for BrowserAccounts {
    fn navigate(&self, url: &str) {
        let Some(location) = location() else { return };
        let assign = js_sys::Reflect::get(&location, &wasm_bindgen::JsValue::from_str("assign"))
            .ok()
            .and_then(|function| function.dyn_into::<js_sys::Function>().ok());
        if let Some(assign) = assign {
            let _ = assign.call1(&location, &wasm_bindgen::JsValue::from_str(url));
        }
    }

    fn query(&self) -> String {
        location()
            .and_then(|location| {
                js_sys::Reflect::get(&location, &wasm_bindgen::JsValue::from_str("search")).ok()
            })
            .and_then(|search| search.as_string())
            .unwrap_or_default()
    }
}

#[component]
fn WebApp() -> Element {
    let api = use_context_provider(|| ApiClient::new(&api_base_url()));
    // Accounts are the web build's: the desktop provides no such capability.
    use_context_provider(|| Accounts::new(Arc::new(BrowserAccounts)));
    // The surfaces for a tree's other viewers appear once the server says it
    // authenticates them; without authentication the interface is the
    // desktop's (docs/authentication.md §2, §7).
    let mut sharing = use_context_provider(|| Signal::new(None::<SharingSurfaces>));
    use_future(move || {
        let api = api.clone();
        async move {
            if api.auth_mode().await.is_ok_and(|mode| mode == "oidc") {
                sharing.set(Some(SharingSurfaces));
            }
        }
    });
    // No `CustomThemeSource` is provided: the browser build has no folder to
    // read themes from, so only the built-in ones are offered.
    rsx! { oxidgene_ui::App {} }
}
