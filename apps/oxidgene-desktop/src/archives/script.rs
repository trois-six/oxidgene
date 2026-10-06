//! The scripts the archive window runs in the portal's page.
//!
//! None of them searches or fills anything: [`page`] says what each loaded
//! page is — the portal's own, an anti-bot check, or a block —, [`fetch`]
//! sends one request of an adapter from that page and posts back the answer,
//! [`overlay`] covers the page with the resolution's progress, and
//! [`banner`] shows the reader what OxidGene found. The resolution itself
//! runs in Rust. The one control clicked is a cookie banner's refusal
//! ([`consent`]): OxidGene refuses on the reader's behalf, never accepts.

use std::time::Duration;

use oxidgene_archives::Method;
use oxidgene_archives::transport::{ANTI_BOT_JSON, TIMEOUT};
use oxidgene_ui::components::layout::SPINNER_STYLES;

use super::transport::{Progress, Stage, Texts};

/// Says when each main-frame document starts, as `{"kind": "document"}`,
/// and classifies it once it has loaded (`page.js`): the portal's page, an
/// anti-bot check, or a block, by the adapters' anti-bot signatures, which
/// it receives as `antiBot`.
pub(super) fn page() -> String {
    format!(
        "(() => {{\nconst antiBot = {ANTI_BOT_JSON};\n{}\n}})();",
        include_str!("page.js")
    )
}

/// The consent managers [`consent`] recognizes, and their controls.
const CONSENT_JSON: &str = include_str!("consent.json");

/// Refuses a recognized cookie banner's consent in each main-frame document
/// (`consent.js`), with the managers of `consent.json`, which it receives as
/// `consent`. Posts `{"kind": "consent", "state", "manager"}`.
pub(super) fn consent() -> String {
    format!(
        "(() => {{\nconst consent = {CONSENT_JSON};\n{}\n}})();",
        include_str!("consent.js")
    )
}

/// Covers the page with the progress overlay of `shown` — the resolution's
/// overlay and how long its stage has lasted —, or removes the overlay
/// (`overlay.js`), in the application's theme and with its one spinner
/// ([`SPINNER_STYLES`]). Its button cancelling the lookup posts
/// `{"kind": "cancel"}`; there is none once the resolution is landing.
pub(super) fn overlay(shown: Option<(&Progress, Duration)>, texts: &Texts) -> String {
    let overlay = shown.map_or(serde_json::Value::Null, |(progress, lasted)| {
        let landing = matches!(progress.stage, Stage::Opening { .. });
        serde_json::json!({
            "style": format!(":host {{\n{}}}\n{SPINNER_STYLES}", texts.palette),
            "heading": texts.searching,
            "archive": progress.archive,
            "citation": progress.citation,
            "step": progress.stage.text(texts),
            "elapsed": u64::try_from(lasted.as_millis()).unwrap_or(u64::MAX),
            "seconds": texts.elapsed,
            "cancel": (!landing).then_some(&texts.cancel),
        })
    });
    format!(
        "(() => {{\nconst overlay = {overlay};\n{}\n}})();",
        include_str!("overlay.js")
    )
}

/// Sends one request from the page and posts the answer back as
/// `{"kind": "fetched", "ticket", "status", "url", "body"}`, or with an
/// `error` of `timeout` or `network`.
///
/// It is the page's own `fetch`, so it carries the portal's cookies, passes
/// its challenge, and reaches an API origin whose CORS admits the portal.
/// Its credentials are the page origin's only (`same-origin`): a request to
/// another origin goes without cookies, which an origin answering any
/// origin (`Access-Control-Allow-Origin: *`) requires of the browser.
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
        "credentials": "same-origin",
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

/// A banner's button: its label, and the `kind` of the IPC message its
/// click posts — `open_in_browser` to open the page the window could not
/// verify in the system browser, `attach` to attach the views on screen.
pub(super) type Action<'a> = (&'a str, &'a str);

/// Shows `text` in a banner over the page with a close button labelled
/// `close`, replacing any earlier banner, and the button of `action` when
/// given. Closing it posts `{"kind": "dismiss"}`: the window then stops
/// showing it again over the pages that follow.
pub(super) fn banner(text: &str, close: &str, action: Option<Action<'_>>) -> String {
    let text = serde_json::Value::from(text);
    let close = serde_json::Value::from(close);
    let (action, kind) = match action {
        Some((label, kind)) => (
            serde_json::Value::from(label),
            serde_json::json!({ "kind": kind }).to_string(),
        ),
        None => (serde_json::Value::Null, "null".to_owned()),
    };
    let kind = serde_json::Value::from(kind);
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
        const message = {kind};
        open.addEventListener("click", () => window.ipc.postMessage(message));
        banner.append(open);
    }}
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = "×";
    button.setAttribute("aria-label", {close});
    button.style.cssText = "border:0;background:none;color:inherit;font-size:18px;"
        + "cursor:pointer;line-height:1";
    button.addEventListener("click", () => {{
        banner.remove();
        window.ipc.postMessage(JSON.stringify({{ kind: "dismiss" }}));
    }});
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
    fn the_consent_script_receives_the_managers() {
        let script = consent();
        assert!(script.starts_with("(() => {\nconst consent = {"));
        assert!(script.contains(r#""name": "tarteaucitron""#));
        assert!(script.contains("new MutationObserver"));
        assert!(script.ends_with("})();"));
        let managers: serde_json::Value = serde_json::from_str(CONSENT_JSON).unwrap();
        assert!(
            managers["managers"]
                .as_array()
                .is_some_and(|list| !list.is_empty())
        );
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
        assert!(script.contains(r#""credentials":"same-origin""#));
        assert!(script.contains(r#""headers":{"Content-Type":"application/json"}"#));
        assert!(
            script.contains(r#"fetch("https://archives.example.org/search?q=\"x\"", options)"#)
        );
        assert!(script.contains(r#""body":"{\"locality\":\"Exampleville</script>\"}""#));
        assert!(script.contains(&format!("AbortSignal.timeout({})", TIMEOUT.as_millis())));

        let get = fetch(8, Method::Get, "https://archives.example.org/", &[], None);
        assert!(get.contains(r#""body":null"#));
    }

    fn texts() -> Texts {
        Texts {
            searching: "Looking for the register…".to_owned(),
            connecting: "Connecting…".to_owned(),
            looking_up: "Searching…".to_owned(),
            opening: "Opening…".to_owned(),
            opening_view: "Opening view {view}…".to_owned(),
            elapsed: "{seconds} s".to_owned(),
            cancel: "Cancel".to_owned(),
            palette: "    --bg-deep: #000001;\n".to_owned(),
            close: "Close".to_owned(),
            challenge: "Answer the check.".to_owned(),
            #[cfg(any(
                target_os = "linux",
                target_os = "dragonfly",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd"
            ))]
            certificate: "Unverified.".to_owned(),
            #[cfg(any(
                target_os = "linux",
                target_os = "dragonfly",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd"
            ))]
            open_in_browser: "Open".to_owned(),
        }
    }

    /// The model `overlay.js` receives, read back from the script.
    fn model(script: &str) -> serde_json::Value {
        let start = script.find("const overlay = ").unwrap() + "const overlay = ".len();
        let end = script[start..].find(";\n").unwrap();
        serde_json::from_str(&script[start..start + end]).unwrap()
    }

    #[test]
    fn the_overlay_carries_its_texts_as_json() {
        let progress = Progress {
            archive: "Archives of <Example>".to_owned(),
            citation: "AD00 - \"Exampleville\" - N - 1877".to_owned(),
            stage: Stage::Searching,
        };
        let script = overlay(Some((&progress, Duration::from_millis(4200))), &texts());
        assert_eq!(
            model(&script),
            serde_json::json!({
                // The application's theme, on the overlay's host, and its
                // one spinner.
                "style": format!(":host {{\n    --bg-deep: #000001;\n}}\n{SPINNER_STYLES}"),
                "heading": "Looking for the register…",
                "archive": "Archives of <Example>",
                "citation": "AD00 - \"Exampleville\" - N - 1877",
                "step": "Searching…",
                "elapsed": 4200,
                "seconds": "{seconds} s",
                "cancel": "Cancel",
            })
        );
        assert!(script.contains("attachShadow"));
        assert!(!script.contains("innerHTML"));

        // Landing: nothing left to cancel.
        let landing = Progress {
            stage: Stage::Opening { view: Some(5) },
            ..progress
        };
        let model = model(&overlay(Some((&landing, Duration::ZERO)), &texts()));
        assert_eq!(model["step"], "Opening view 5…");
        assert_eq!(model["cancel"], serde_json::Value::Null);

        assert!(overlay(None, &texts()).contains("const overlay = null;\n"));
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
        // Closed, it is not shown again over the pages that follow.
        assert!(script.contains(r#"window.ipc.postMessage(JSON.stringify({ kind: "dismiss" }));"#));

        let script = banner(
            "Unverified.",
            "Close",
            Some(("Open in the <browser>", "open_in_browser")),
        );
        assert!(script.contains(r#"const action = "Open in the <browser>";"#));
        assert!(script.contains(r#"const message = "{\"kind\":\"open_in_browser\"}";"#));

        let script = banner("A view to keep.", "Close", Some(("Attach", "attach")));
        assert!(script.contains(r#"const message = "{\"kind\":\"attach\"}";"#));
    }
}
