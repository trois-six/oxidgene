//! Collecting the mapping through a real browser.
//!
//! Cloudflare fronts geneanet.org and can decide, from a client's TLS and
//! HTTP/2 fingerprint, that a non-browser deserves an interactive challenge.
//! When it does, no cookie fixes it and no amount of politeness helps — a
//! standing challenge is not lifted by slowing down.
//!
//! The honest way through is not to look more like a browser. It is to *use*
//! one: the same requests, issued by a browser, on the user's own session,
//! against their own data — which is exactly what the media manager page does
//! when they click around it. Nothing is impersonated and no challenge is
//! defeated, because there is nothing to defeat.
//!
//! The desktop wizard opens a WebView on Geneanet and evaluates
//! [`ipc_collection`] inside it once the user has signed in, then
//! [`ipc_sizes`] and [`ipc_fetch`] for the media themselves. Same requests,
//! same session, nothing pasted anywhere.

/// Shared request helpers, so the console and IPC scripts cannot drift apart
/// on the one detail that matters: the `X-Requested-With` header, without which
/// every endpoint answers 403 with an HTML page even on a valid session.
const HELPERS: &str = r"
  const api = async (path) => {
    const r = await fetch(path, { headers: { 'X-Requested-With': 'XMLHttpRequest' } });
    if (!r.ok) throw new Error(path + ' -> HTTP ' + r.status);
    return r.json();
  };
  const pages = async (path, onPage) => {
    const out = [];
    for (let p = 1; ; p++) {
      const batch = await api(`${path}?page=${p}&per_page=100`);
      out.push(...batch);
      if (onPage) onPage(out.length);
      if (batch.length < 100) return out;
    }
  };
";

/// The multi-page pass, shared for the same reason as [`HELPERS`].
///
/// A deposit with several pages is listed in full by the bulk endpoint without
/// saying which page a link sits on, so those are probed until each deposit's
/// links are accounted for. Links cluster on page 1.
const LOCATE: &str = r"
  const locate = async (deposits, references, onProbe) => {
    const expected = {};
    const multi = new Set();
    for (const r of references) {
      expected[r.deposit.id] = (expected[r.deposit.id] || 0) + 1;
      if (r.deposit.views.length > 1) multi.add(r.deposit.id);
    }
    const view_references = {};
    let probes = 0;
    for (const id of multi) {
      const deposit = deposits.find((d) => d.id === id);
      if (!deposit) continue;
      let remaining = expected[id] || 0;
      for (const view of deposit.views) {
        if (remaining === 0) break;
        const found = await api(`/media/api/deposits/${id}/views/${view.id}/references`);
        probes += 1;
        if (onProbe) onProbe(probes, multi.size);
        if (found.length) {
          remaining -= found.length;
          view_references[id + ':' + view.id] = found;
        }
      }
    }
    return view_references;
  };
";

/// Pre-ticks "Remember me" on the login form, and changes nothing else.
///
/// Why tick it: after sign-in the window stops being something the user
/// watches, and a collection can run for a while. With the remember-me cookie
/// in the jar the WebView silently re-authenticates when the short-lived
/// session cookie expires, instead of the run dying halfway through.
///
/// Why the box is left visible and still works: an earlier version hid it, on
/// the grounds that the wizard had already made the choice. That left the
/// caption behind — a stray sentence with nothing to tick — and hiding it too
/// meant guessing at a third-party page's markup, which breaks the moment
/// Geneanet reshuffles a class name. Silently *overriding* the box instead
/// would be worse again: a control that does nothing is a lie, and someone
/// unticking this on a shared machine means it.
///
/// Leaving it honest costs nothing here, because the window runs in an
/// **incognito web context** — the token is valid for months but dies with
/// the window rather than reaching the app's cookie jar on disk (see
/// `with_incognito` in `apps/oxidgene-desktop/src/geneanet.rs`). A user who
/// unticks it simply gets the shorter session, and the probe that watches for
/// a re-login already handles the expiry.
///
/// The script runs at document start, before the form exists, so it watches
/// for the checkbox rather than assuming it is there.
pub const REMEMBER_ME: &str = r#"
(() => {
  const fix = () => {
    const box = document.querySelector('input[name="_remember_me"]');
    if (!box) return false;
    // Ticked, not hidden and not forced: the user can still change it, and
    // the form behaves exactly as Geneanet built it.
    box.checked = true;
    return true;
  };
  if (!fix()) {
    const observer = new MutationObserver(() => { if (fix()) observer.disconnect(); });
    observer.observe(document.documentElement, { childList: true, subtree: true });
  }
})();
"#;

/// Covers the Geneanet page with OxidGene's own status panel.
///
/// The window is not hidden once the user has signed in, and not left showing
/// Geneanet either. Hiding it breaks cancellation outright — a hidden window
/// can never emit a close request, so a stalled collection becomes
/// unrecoverable — and leaving the site visible tells the user nothing about
/// what the app is doing or whether they may touch it.
///
/// Deliberately carries **no numbers**. The wizard's modal owns the progress
/// bars; two counters for one operation can only disagree. This says what is
/// happening in words and that closing the window cancels.
///
/// An overlay rather than a navigation: the collection runs on this page's
/// origin, and navigating away would take its session cookie out of scope for
/// the `fetch` calls that follow.
pub fn status_overlay(heading: &str, message: &str, hint: &str) -> String {
    // Everything user-visible is JSON-encoded rather than interpolated raw:
    // these strings come from the UI's translations, and a quote in one of
    // them would otherwise end the statement it sits in.
    let heading = json_string(heading);
    let message = json_string(message);
    let hint = json_string(hint);

    format!(
        r#"
(() => {{
  const ID = 'oxidgene-status';
  let panel = document.getElementById(ID);
  if (!panel) {{
    panel = document.createElement('div');
    panel.id = ID;
    panel.setAttribute('role', 'status');
    panel.style.cssText = [
      'position:fixed', 'inset:0', 'z-index:2147483647',
      'display:flex', 'flex-direction:column',
      'align-items:center', 'justify-content:center', 'gap:14px',
      'background:#0a0b0d', 'color:#e8dfc8',
      'font:400 15px/1.5 system-ui,-apple-system,Segoe UI,sans-serif',
      'text-align:center', 'padding:28px'
    ].join(';');
    panel.innerHTML =
      '<div id="' + ID + '-h" style="font-size:17px;font-weight:600"></div>' +
      '<div id="' + ID + '-m"></div>' +
      '<div id="' + ID + '-s" style="width:180px;height:6px;border-radius:999px;' +
        'background:#252d3d;overflow:hidden">' +
        '<div style="width:35%;height:100%;background:#e07820;' +
          'animation:oxg 1.3s ease-in-out infinite"></div></div>' +
      '<div id="' + ID + '-t" style="font-size:13px;color:#8a8172"></div>' +
      '<style>@keyframes oxg{{0%{{margin-left:-35%}}100%{{margin-left:100%}}}}</style>';
    document.documentElement.appendChild(panel);
  }}
  document.getElementById(ID + '-h').textContent = {heading};
  document.getElementById(ID + '-m').textContent = {message};
  document.getElementById(ID + '-t').textContent = {hint};
}})();
"#
    )
}

/// Minimal JSON string escaping, so a translated string can be embedded in a
/// script without a quote in it ending the statement.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Runs on every page the login WebView loads, and says whether the session is
/// established yet.
///
/// Asking the API rather than reading the page is what makes this survive a
/// Geneanet redesign: the wizard needs to know that *the media API* answers,
/// which is the only thing the next step depends on. A login page, a captcha
/// and a Cloudflare interstitial are all simply "not yet".
pub const PROBE: &str = r"
(async () => {
  const send = (m) => window.ipc.postMessage(JSON.stringify(m));
  try {
    const r = await fetch('/media/api/deposits?page=1&per_page=1',
                          { headers: { 'X-Requested-With': 'XMLHttpRequest' } });
    if (r.ok) {
      const total = parseInt(r.headers.get('x-gnt-media-total') || '0', 10);
      send({ kind: 'auth', signed_in: true, deposits: total });
    } else {
      send({ kind: 'auth', signed_in: false, status: r.status });
    }
  } catch (e) {
    send({ kind: 'auth', signed_in: false, status: 0 });
  }
})();
";

/// The collection, reporting progress and its result over the WebView's IPC
/// channel instead of downloading a file.
///
/// Same requests as [`collection_script`] — this is the desktop wizard's step 3,
/// and the two stages it reports are the two bars the spec draws.
pub fn ipc_collection() -> String {
    format!(
        r"
(async () => {{
  const send = (m) => window.ipc.postMessage(JSON.stringify(m));
{HELPERS}{LOCATE}
  try {{
    send({{ kind: 'progress', stage: 'deposits', done: 0, total: 0 }});
    const deposits = await pages('/media/api/deposits',
      (n) => send({{ kind: 'progress', stage: 'deposits', done: n, total: 0 }}));

    const references = await pages('/media/api/references',
      (n) => send({{ kind: 'progress', stage: 'references', done: n, total: 0 }}));

    const view_references = await locate(deposits, references,
      (done, total) => send({{ kind: 'progress', stage: 'locate', done, total }}));

    send({{ kind: 'collected',
           data: JSON.stringify({{ deposits, references, view_references }}) }});
  }} catch (e) {{
    send({{ kind: 'error', message: String(e && e.message ? e.message : e) }});
  }}
}})();
"
    )
}

/// Fetches media bytes and hands them back over IPC, one at a time.
///
/// This exists because **no direct download works**: every request from an
/// HTTP client is challenged by Cloudflare, whatever the cookie and whatever
/// the stack, so the bytes have to come through the browser that is already
/// authenticated. That is not a workaround for the check — a real browser on
/// the user's own session is the thing it is asking for.
///
/// One message per file rather than one at the end, so a run of several
/// hundred never assembles a single enormous IPC payload. `data` is base64
/// because IPC carries text.
///
/// # Same-origin
///
/// Renditions live on `gw.geneanet.org` while the collection runs on
/// `www.geneanet.org`, and a cross-origin `fetch` would be refused. The window
/// is therefore navigated to the host it is about to read from before this is
/// evaluated — the session cookie is set on `.geneanet.org`, so it covers both.
pub fn ipc_fetch(urls_json: &str) -> String {
    format!(
        r"
(async () => {{
  const send = (m) => window.ipc.postMessage(JSON.stringify(m));
  const urls = {urls_json};

  const encode = (buffer) => {{
    const bytes = new Uint8Array(buffer);
    let binary = '';
    // Chunked: `apply` on a several-megabyte array overflows the call stack.
    for (let i = 0; i < bytes.length; i += 0x8000) {{
      binary += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
    }}
    return btoa(binary);
  }};

  for (let i = 0; i < urls.length; i += 1) {{
    const url = urls[i];
    try {{
      const r = await fetch(url, {{ headers: {{ 'X-Requested-With': 'XMLHttpRequest' }} }});
      if (!r.ok) {{
        send({{ kind: 'fetched', url, error: 'HTTP ' + r.status }});
      }} else {{
        send({{ kind: 'fetched', url, data: encode(await r.arrayBuffer()) }});
      }}
    }} catch (e) {{
      send({{ kind: 'fetched', url, error: String(e && e.message ? e.message : e) }});
    }}
    send({{ kind: 'fetching', done: i + 1, total: urls.length }});
  }}

  send({{ kind: 'fetch_done' }});
}})();
"
    )
}

/// Asks Geneanet each single-page deposit's exact byte length.
///
/// This is stage 2 of step 3, and the whole of the archive matching: a `HEAD`
/// returns a `Content-Length` and no body, so several hundred of them transfer
/// nothing while telling us exactly which local file is which. `deposit_ids`
/// is JSON — only single-page deposits belong in it, because a multi-page
/// deposit downloads as a ZIP Geneanet streams without a length at all.
///
/// Runs in the login window for the same reason the collection does: these are
/// requests to geneanet.org, and they come from the browser that holds the
/// session.
pub fn ipc_sizes(deposit_ids_json: &str) -> String {
    format!(
        r"
(async () => {{
  const send = (m) => window.ipc.postMessage(JSON.stringify(m));
  const ids = {deposit_ids_json};
  const sizes = {{}};
  try {{
    for (let i = 0; i < ids.length; i++) {{
      const r = await fetch('/media/download/?deposits[]=' + ids[i],
                            {{ method: 'HEAD',
                              headers: {{ 'X-Requested-With': 'XMLHttpRequest' }} }});
      const length = r.headers.get('content-length');
      if (r.ok && length) sizes[ids[i]] = parseInt(length, 10);
      if (i % 10 === 0 || i === ids.length - 1) {{
        send({{ kind: 'sizing', done: i + 1, total: ids.length }});
      }}
    }}
    send({{ kind: 'sized', sizes }});
  }} catch (e) {{
    // A failed sizing pass is not a failed step: every photo it could not
    // measure is simply downloaded instead of matched.
    send({{ kind: 'sized', sizes }});
  }}
}})();
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_remember_me_box_is_ticked_but_left_alone_otherwise() {
        // It stays visible, stays usable, and is not re-ticked behind the
        // user's back. A control that ignores the person operating it is a
        // lie, and this one guards a months-long credential.
        assert!(REMEMBER_ME.contains("box.checked = true"));
        assert!(
            !REMEMBER_ME.contains("style.display"),
            "the box and its caption must not be hidden"
        );
        assert!(
            !REMEMBER_ME.contains("addEventListener('submit'"),
            "the box must not be overridden at submit time"
        );
    }

    #[test]
    fn a_quote_in_a_translation_cannot_break_out_of_the_overlay_script() {
        // The overlay's strings come from the UI's translation tables, which
        // hold apostrophes and quotation marks in both languages. Interpolated
        // raw, one of them would end the statement it sits in and the panel
        // would silently fail to appear.
        let script = status_overlay(
            r#"He said "hello""#,
            "L\u{2019}import est en cours",
            "Backslash \\ and newline \n",
        );

        assert!(script.contains(r#"\"hello\""#), "quotes must be escaped");
        assert!(!script.contains("\n\"") || script.contains("\\n"));
        // The panel is still assembled: escaping must not mangle the script.
        assert!(script.contains("oxidgene-status"));
    }

    #[test]
    fn the_overlay_carries_no_progress_numbers() {
        // The wizard's modal owns the counters. Two counters for one operation
        // can only disagree, and the window is the one that cannot be kept in
        // step with the import.
        let script = status_overlay("Working", "Reading your photo list", "Close to cancel");

        for numeric in ["done", "total", "%", "/ "] {
            assert!(
                !script.contains(&format!("textContent = {numeric}")),
                "the overlay must not render {numeric}"
            );
        }
    }

    #[test]
    fn the_overlay_updates_in_place_rather_than_stacking() {
        // It is evaluated again on every stage change, so it must find the
        // panel it already made instead of appending a second one.
        let script = status_overlay("a", "b", "c");
        assert!(script.contains("getElementById(ID)"));
        assert!(script.contains("if (!panel)"));
    }

    #[test]
    fn the_fetch_script_streams_one_file_at_a_time() {
        // A run can cover several hundred files. Collecting them into one IPC
        // message would build an enormous string in the page before anything
        // crossed the boundary.
        let script = ipc_fetch(r#"["/a.jpg","/b.jpg"]"#);

        assert!(script.contains("kind: 'fetched'"));
        assert!(script.contains("kind: 'fetch_done'"));
        assert!(script.contains("for (let i = 0"), "must loop, not batch");
    }

    #[test]
    fn the_fetch_script_chunks_its_base64() {
        // `String.fromCharCode.apply` over a multi-megabyte array overflows
        // the call stack — a scan is exactly that size.
        assert!(ipc_fetch("[]").contains("i += 0x8000"));
    }

    #[test]
    fn a_failed_fetch_is_reported_per_file_not_thrown() {
        // One unreachable medium must not abandon the several hundred after
        // it; the import reports it as skipped and carries on.
        let script = ipc_fetch("[]");
        assert!(script.contains("error: 'HTTP ' + r.status"));
        assert!(script.contains("catch (e)"));
    }

    #[test]
    fn the_sizing_pass_asks_for_no_body() {
        // The whole point: several hundred requests that transfer nothing. A
        // GET here would pull the entire account down to learn its lengths.
        let script = ipc_sizes("[1,2,3]");
        assert!(script.contains("method: 'HEAD'"));
        assert!(script.contains("content-length"));
        // The trailing slash is not decoration — without it Geneanet answers
        // 301 to the same path with one, doubling the request count.
        assert!(script.contains("/media/download/?deposits[]="));
    }

    #[test]
    fn the_scripts_only_read() {
        // A user is going to run these against their own account. Nothing in
        // them should mutate anything, so no write verbs.
        for script in [
            ipc_collection(),
            ipc_sizes("[]"),
            ipc_fetch("[]"),
            PROBE.to_string(),
        ] {
            for verb in [
                "method: 'POST'",
                "method:'POST'",
                "method: 'DELETE'",
                "'PUT'",
            ] {
                assert!(
                    !script.contains(verb),
                    "the collection scripts must not {verb}"
                );
            }
        }
    }

    #[test]
    fn every_script_sends_the_header_the_api_demands() {
        // Without this every call 403s, which would look like an auth problem.
        for script in [
            ipc_collection(),
            ipc_sizes("[]"),
            ipc_fetch("[]"),
            PROBE.to_string(),
        ] {
            assert!(script.contains("X-Requested-With"));
        }
    }

    #[test]
    fn the_ipc_script_reports_the_shape_the_manifest_builder_parses() {
        // The three keys `BrowserCollection` deserialises. Renaming one here
        // without renaming it there would only fail at runtime, on a live
        // account, after a login — so it is pinned.
        let script = ipc_collection();
        for key in ["deposits", "references", "view_references"] {
            assert!(script.contains(key), "the IPC collection must send {key}");
        }
        assert!(script.contains("window.ipc.postMessage"));
    }

    #[test]
    fn the_ipc_script_never_downloads_a_file() {
        // The console script's blob-and-click ending has no place in a WebView
        // the user never sees a downloads bar for.
        let script = ipc_collection();
        assert!(!script.contains("createObjectURL"));
        assert!(!script.contains("a.click()"));
    }
}
