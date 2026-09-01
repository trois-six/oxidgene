mod observability;

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

fn main() {
    let runtime_otlp_endpoint = runtime_otlp_endpoint();
    observability::init(
        LOG_LEVEL,
        runtime_otlp_endpoint.as_deref().or(OTLP_ENDPOINT),
    )
    .expect("failed to initialize browser observability");
    dioxus::launch(WebApp);
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
    use_context_provider(|| ApiClient::new(API_URL));
    use_context_provider(|| oxidgene_ui::ThemeFallback::Light);
    rsx! { oxidgene_ui::App {} }
}
