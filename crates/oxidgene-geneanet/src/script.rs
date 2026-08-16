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
//! Two hosts run this. The CLI prints [`COLLECTION_SCRIPT`] for the user to
//! paste into their own browser's console. The desktop wizard opens a WebView
//! on Geneanet and evaluates [`ipc_collection`] inside it once the user has
//! signed in — same requests, same session, no paste.

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

/// Script the user pastes into their browser console.
///
/// Deliberately readable rather than clever: someone is about to run it against
/// their own account, and should be able to see what it does first.
pub fn collection_script() -> String {
    format!(
        r"// OxidGene — collect the Geneanet media mapping.
// Run this on https://www.geneanet.org/media/manager while logged in.
// It only reads, and downloads one JSON file at the end.
(async () => {{{HELPERS}{LOCATE}
  console.log('Listing deposits...');
  const deposits = await pages('/media/api/deposits');
  console.log(deposits.length + ' deposits. Fetching links...');
  const references = await pages('/media/api/references');
  console.log(references.length + ' links.');

  const view_references = await locate(deposits, references);
  console.log('Located links in the multi-page deposits.');

  const blob = new Blob([JSON.stringify({{ deposits, references, view_references }})],
                        {{ type: 'application/json' }});
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = 'geneanet-collection.json';
  a.click();
  console.log('Done. Saved geneanet-collection.json to your downloads.');
}})();
"
    )
}

/// Step-by-step instructions printed alongside the script.
pub const INSTRUCTIONS: &str = "\
1. Log in to Geneanet and open https://www.geneanet.org/media/manager
2. Open the developer console:
     Linux / Windows   F12, then the Console tab
     macOS             Cmd + Option + I, then the Console tab
   Some browsers ask you to type 'allow pasting' once before they accept
   pasted code. That warning is there for good reason — read the script first.
3. Paste the script below and press Enter.
4. It saves geneanet-collection.json to your downloads folder.
5. Feed it back in:
     oxidgene-cli geneanet-media manifest-from-browser \\
       --input ~/Downloads/geneanet-collection.json";

/// Pre-checks "Remember me" on the login form, and hides the checkbox.
///
/// Why: the collection runs in a hidden window after sign-in, and a hidden
/// session the user cannot see should not die under them — with the
/// remember-me cookie in the jar, the WebView silently re-authenticates when
/// the short-lived session cookie expires. The checkbox is hidden because the
/// wizard already made this choice on the user's behalf; unchecking it would
/// be opting out of the design. The download step still only ever reads
/// `gntsess5`: this changes how long the *window's* session lives, not what
/// the rest of the app is handed.
///
/// The script runs at document start, before the form exists, so it watches
/// for the checkbox rather than assuming it is there.
pub const REMEMBER_ME: &str = r#"
(() => {
  const fix = () => {
    const box = document.querySelector('input[name="_remember_me"]');
    if (!box) return false;
    box.checked = true;
    const row = box.closest('.form-check') || box.closest('label') || box.parentElement;
    if (row) row.style.display = 'none';
    return true;
  };
  if (!fix()) {
    const obs = new MutationObserver(() => { if (fix()) obs.disconnect(); });
    obs.observe(document.documentElement, { childList: true, subtree: true });
  }
})();
"#;

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
            collection_script(),
            ipc_collection(),
            ipc_sizes("[]"),
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
            collection_script(),
            ipc_collection(),
            ipc_sizes("[]"),
            PROBE.to_string(),
        ] {
            assert!(script.contains("X-Requested-With"));
        }
    }

    #[test]
    fn the_console_script_names_the_file_the_cli_expects() {
        assert!(collection_script().contains("geneanet-collection.json"));
        assert!(INSTRUCTIONS.contains("geneanet-collection.json"));
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
