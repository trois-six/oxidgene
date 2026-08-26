use dioxus::prelude::*;
use oxidgene_ui::api::ApiClient;

const API_URL: &str = match option_env!("OXIDGENE_API_URL") {
    Some(url) => url,
    None => "http://127.0.0.1:8080",
};

fn main() {
    dioxus::launch(WebApp);
}

#[component]
fn WebApp() -> Element {
    use_context_provider(|| ApiClient::new(API_URL));
    rsx! { oxidgene_ui::App {} }
}
