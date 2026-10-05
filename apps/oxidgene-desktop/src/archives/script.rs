//! The scripts the archive window runs in the portal's page.
//!
//! None of them searches, fills or clicks anything: [`page`] says what each
//! loaded page is — the portal's own, an anti-bot check, or a block —,
//! [`fetch`] sends one request of an adapter from that page and posts back
//! the answer, and [`banner`] shows the reader what OxidGene found. The
//! resolution itself runs in Rust.

use oxidgene_archives::Method;
use oxidgene_archives::transport::{ANTI_BOT_JSON, TIMEOUT};

/// Classifies each main-frame document once it has loaded (`page.js`):
/// the portal's page, an anti-bot check, or a block, by the adapters'
/// anti-bot signatures, which it receives as `antiBot`.
pub(super) fn page() -> String {
    format!(
        "(() => {{\nconst antiBot = {ANTI_BOT_JSON};\n{}\n}})();",
        include_str!("page.js")
    )
}

/// Sends one request from the page and posts the answer back as
/// `{"kind": "fetched", "ticket", "status", "url", "body"}`, or with an
/// `error` of `timeout` or `network`.
///
/// It is the page's own `fetch`, so it carries the portal's cookies, passes
/// its challenge, and reaches an API origin whose CORS admits the portal.
pub(super) fn fetch(
    ticket: u64,
    method: Method,
    url: &str,
    headers: &[(String, String)],
    body: Option<&str>,
) -> String {
    let headers: serde_json::Map<String, serde_json::Value> = headers
        .iter()
        .map(|(name, value)| (name.clone(), value.as_str().into()))
        .collect();
    let options = serde_json::json!({
        "method": method.as_str(),
        "headers": headers,
        "body": body,
        "credentials": "include",
        "redirect": "follow",
        "cache": "no-store",
    });
    // JSON is a JavaScript expression, so the values need no other escaping.
    let url = serde_json::Value::from(url);
    let timeout = TIMEOUT.as_millis();
    format!(
        r#"(async () => {{
    const send = message => window.ipc.postMessage(JSON.stringify(
        Object.assign({{ kind: "fetched", ticket: {ticket} }}, message)));
    try {{
        const options = {options};
        options.signal = AbortSignal.timeout({timeout});
        const response = await fetch({url}, options);
        send({{ status: response.status, url: response.url, body: await response.text() }});
    }} catch (error) {{
        send({{ error: error && error.name === "TimeoutError" ? "timeout" : "network" }});
    }}
}})();"#
    )
}

/// Shows `text` in a banner over the page with a close button labelled
/// `close`, replacing any earlier banner. With an `action` label, the
/// banner also has a button that asks the window, over IPC, to open the
/// page it could not verify in the system browser.
pub(super) fn banner(text: &str, close: &str, action: Option<&str>) -> String {
    let text = serde_json::Value::from(text);
    let close = serde_json::Value::from(close);
    let action = serde_json::Value::from(action);
    format!(
        r#"(() => {{
    document.getElementById("oxidgene-archive-status")?.remove();
    const banner = document.createElement("div");
    banner.id = "oxidgene-archive-status";
    banner.setAttribute("role", "status");
    banner.style.cssText = "position:fixed;z-index:2147483647;top:12px;left:50%;"
        + "transform:translateX(-50%);max-width:min(640px,90vw);display:flex;gap:12px;"
        + "align-items:center;padding:10px 14px;border-radius:8px;background:#1e1a14;"
        + "color:#f6f0e4;font:14px/1.4 system-ui,sans-serif;box-shadow:0 4px 16px #0000004d";
    const label = document.createElement("span");
    label.textContent = {text};
    banner.append(label);
    const action = {action};
    if (action) {{
        const open = document.createElement("button");
        open.type = "button";
        open.textContent = action;
        open.style.cssText = "border:1px solid currentColor;border-radius:6px;background:none;"
            + "color:inherit;font:inherit;padding:4px 10px;cursor:pointer;white-space:nowrap";
        open.addEventListener("click", () =>
            window.ipc.postMessage(JSON.stringify({{ kind: "open_in_browser" }})));
        banner.append(open);
    }}
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = "×";
    button.setAttribute("aria-label", {close});
    button.style.cssText = "border:0;background:none;color:inherit;font-size:18px;"
        + "cursor:pointer;line-height:1";
    button.addEventListener("click", () => banner.remove());
    banner.append(button);
    document.documentElement.append(banner);
}})();"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_script_receives_the_adapters_signatures() {
        let script = page();
        assert!(script.starts_with("(() => {\nconst antiBot = {"));
        assert!(script.contains(r#""vendor": "anubis""#));
        assert!(script.contains("body.localName === \"frameset\""));
        assert!(script.ends_with("})();"));
    }

    #[test]
    fn the_fetch_script_carries_the_request_as_json() {
        let script = fetch(
            7,
            Method::Post,
            "https://archives.example.org/search?q=\"x\"",
            &[("Content-Type".to_owned(), "application/json".to_owned())],
            Some(r#"{"locality":"Exampleville</script>"}"#),
        );
        assert!(script.contains(r#"ticket: 7"#));
        assert!(script.contains(r#""method":"POST""#));
        assert!(script.contains(r#""credentials":"include""#));
        assert!(script.contains(r#""headers":{"Content-Type":"application/json"}"#));
        assert!(
            script.contains(r#"fetch("https://archives.example.org/search?q=\"x\"", options)"#)
        );
        assert!(script.contains(r#""body":"{\"locality\":\"Exampleville</script>\"}""#));
        assert!(script.contains("AbortSignal.timeout(10000)"));

        let get = fetch(8, Method::Get, "https://archives.example.org/", &[], None);
        assert!(get.contains(r#""body":null"#));
    }

    #[test]
    fn the_banner_text_is_set_as_text() {
        let script = banner(
            "No register <b>matches</b> \"the\" citation.",
            "Close",
            None,
        );
        assert!(
            script
                .contains(r#"label.textContent = "No register <b>matches</b> \"the\" citation.";"#)
        );
        assert!(script.contains(r#"setAttribute("aria-label", "Close")"#));
        assert!(script.contains("const action = null;"));
        assert!(!script.contains("innerHTML"));

        let script = banner("Unverified.", "Close", Some("Open in the <browser>"));
        assert!(script.contains(r#"const action = "Open in the <browser>";"#));
        assert!(script.contains(r#"kind: "open_in_browser""#));
    }
}
