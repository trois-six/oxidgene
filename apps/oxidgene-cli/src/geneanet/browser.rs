//! Collecting the mapping through the user's own browser.
//!
//! Cloudflare fronts geneanet.org and can decide, from a client's TLS and
//! HTTP/2 fingerprint, that a non-browser deserves an interactive challenge.
//! When it does, no cookie fixes it and no amount of politeness helps — a
//! standing challenge is not lifted by slowing down.
//!
//! The honest way through is not to look more like a browser. It is to *use*
//! one: the same requests, issued by the user's own browser, on their own
//! session, against their own data — which is exactly what the media manager
//! page does when they click around it. Nothing is impersonated and no
//! challenge is defeated, because there is nothing to defeat.
//!
//! It is also the more portable path. The browser does the talking, so this
//! works identically on Linux, Windows and macOS and depends on nothing about
//! how the CLI was built.

/// Script the user pastes into their browser console.
///
/// Deliberately readable rather than clever: someone is about to run it against
/// their own account, and should be able to see what it does first.
pub const COLLECTION_SCRIPT: &str = r#"// OxidGene — collect the Geneanet media mapping.
// Run this on https://www.geneanet.org/media/manager while logged in.
// It only reads, and downloads one JSON file at the end.
(async () => {
  const api = async (path) => {
    const r = await fetch(path, { headers: { 'X-Requested-With': 'XMLHttpRequest' } });
    if (!r.ok) throw new Error(path + ' -> HTTP ' + r.status);
    return r.json();
  };
  const pages = async (path) => {
    const out = [];
    for (let p = 1; ; p++) {
      const batch = await api(`${path}?page=${p}&per_page=100`);
      out.push(...batch);
      if (batch.length < 100) return out;
    }
  };

  console.log('Listing deposits...');
  const deposits = await pages('/media/api/deposits');
  console.log(deposits.length + ' deposits. Fetching links...');
  const references = await pages('/media/api/references');
  console.log(references.length + ' links.');

  // A multi-page deposit lists every page without saying which one a link is
  // on, so those are probed until each deposit's links are accounted for.
  const expected = {};
  const multi = new Set();
  for (const r of references) {
    expected[r.deposit.id] = (expected[r.deposit.id] || 0) + 1;
    if (r.deposit.views.length > 1) multi.add(r.deposit.id);
  }

  const view_references = {};
  for (const id of multi) {
    const deposit = deposits.find((d) => d.id === id);
    if (!deposit) continue;
    let remaining = expected[id] || 0;
    for (const view of deposit.views) {
      if (remaining === 0) break;
      const found = await api(`/media/api/deposits/${id}/views/${view.id}/references`);
      if (found.length) {
        remaining -= found.length;
        view_references[id + ':' + view.id] = found;
      }
    }
  }
  console.log('Located links in ' + multi.size + ' multi-page deposit(s).');

  const blob = new Blob([JSON.stringify({ deposits, references, view_references })],
                        { type: 'application/json' });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = 'geneanet-collection.json';
  a.click();
  console.log('Done. Saved geneanet-collection.json to your downloads.');
})();
"#;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_only_reads() {
        // A user is going to run this against their own account. Nothing in it
        // should mutate anything, so no write verbs.
        for verb in [
            "method: 'POST'",
            "method:'POST'",
            "method: 'DELETE'",
            "'PUT'",
        ] {
            assert!(
                !COLLECTION_SCRIPT.contains(verb),
                "the collection script must not {verb}"
            );
        }
    }

    #[test]
    fn the_script_sends_the_header_the_api_demands() {
        // Without this every call 403s, which would look like an auth problem.
        assert!(COLLECTION_SCRIPT.contains("X-Requested-With"));
    }

    #[test]
    fn the_script_names_the_file_the_cli_expects() {
        assert!(COLLECTION_SCRIPT.contains("geneanet-collection.json"));
        assert!(INSTRUCTIONS.contains("geneanet-collection.json"));
    }
}
