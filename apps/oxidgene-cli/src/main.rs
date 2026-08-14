//! OxidGene command-line tool.

mod geneanet;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "oxidgene", version, about = "OxidGene CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Recover the person↔photo links a Geneanet export drops.
    ///
    /// A Geneanet GEDCOM/.gw export carries at most one photo per individual
    /// and no group photos at all. These commands rebuild the full mapping
    /// from Geneanet's media API, keyed by GeneWeb reference so it can be
    /// joined back onto a .gw import.
    #[command(subcommand, name = "geneanet-media")]
    GeneanetMedia(GeneanetMedia),
}

#[derive(Subcommand)]
enum GeneanetMedia {
    /// Collect the deposit → view → person mapping as JSON.
    ///
    /// Roughly fifteen small requests; downloads no media.
    Manifest {
        #[command(flatten)]
        session: Session,

        /// Where to write the manifest.
        #[arg(long, default_value = "geneanet-media/manifest.json")]
        out: PathBuf,
    },

    /// Print a script that collects the mapping using your own browser.
    ///
    /// Use this when Cloudflare challenges the CLI: it fronts geneanet.org and
    /// can decide, from a client's TLS fingerprint, that a non-browser needs an
    /// interactive challenge. No cookie fixes that, and disguising the client
    /// to get past it is bot-detection evasion, which this tool will not do.
    ///
    /// So it uses a browser instead — yours. Same requests, your session, your
    /// data, nothing impersonated. Works identically on Linux, Windows and
    /// macOS, since the browser does the talking.
    BrowserScript,

    /// Build a manifest from what the browser script collected.
    ///
    /// Offline: no cookie, no network.
    ManifestFromBrowser {
        /// The geneanet-collection.json the script saved.
        #[arg(long)]
        input: PathBuf,

        /// Where to write the manifest.
        #[arg(long, default_value = "geneanet-media/manifest.json")]
        out: PathBuf,
    },

    /// Report how much of a manifest joins onto a .gw export.
    ///
    /// Offline: needs no cookie and touches no network. Run it before
    /// building a .gdz to see what will land where.
    Check {
        /// The .gw export to join against.
        #[arg(long)]
        gw: PathBuf,

        /// Manifest produced by `manifest`.
        #[arg(long, default_value = "geneanet-media/manifest.json")]
        manifest: PathBuf,

        /// List every reference that could not be attached.
        #[arg(long)]
        verbose: bool,
    },

    /// Build a .gdz holding the tree and every medium attached to a person.
    ///
    /// The endpoint of the pipeline: one file carrying the genealogy and its
    /// photos, instead of the unusable URLs a Geneanet export produces.
    Gedzip {
        #[command(flatten)]
        session: Session,

        /// The .gw export to build from.
        #[arg(long)]
        gw: PathBuf,

        /// Manifest produced by `manifest`.
        #[arg(long, default_value = "geneanet-media/manifest.json")]
        manifest: PathBuf,

        /// Directory of already-downloaded originals — typically the unpacked
        /// "all my data" archive. Files are matched by exact byte size, so
        /// anything found here costs no download.
        #[arg(long)]
        local_media: Option<PathBuf>,

        /// Fetch full-resolution pages of multi-page deposits.
        ///
        /// Geneanet exposes no per-page original, so by default a page of a
        /// scanned dossier is taken from its downsized rendition. This pulls
        /// the whole deposit archive instead and extracts the page from it —
        /// costly, but the right choice when the pages are documents you need
        /// to read.
        #[arg(long)]
        multipage_originals: bool,

        /// Where to write the archive.
        #[arg(long, short, default_value = "geneanet-media/tree.gdz")]
        out: PathBuf,
    },

    /// Download each deposit's original file.
    ///
    /// One request per deposit, hundreds of megabytes in total. Resumable:
    /// files already on disk are skipped.
    Fetch {
        #[command(flatten)]
        session: Session,

        /// Manifest produced by `manifest`; updated in place with the local
        /// path of each downloaded file.
        #[arg(long, default_value = "geneanet-media/manifest.json")]
        manifest: PathBuf,

        /// Directory to write the media into.
        #[arg(long, default_value = "geneanet-media/files")]
        media_dir: PathBuf,
    },
}

#[derive(Args)]
struct Session {
    /// Geneanet session cookie. The media are private, so this is required.
    ///
    /// Only ONE cookie is actually needed, and `gntsess5` is the one to use:
    ///
    ///   --cookie 'gntsess5=<value>'
    ///
    /// Get it from the browser: developer tools → Application → Cookies →
    /// https://www.geneanet.org → copy the value of `gntsess5`.
    ///
    /// `REMEMBERME` also works, but prefer not to: it is long-lived (months)
    /// and mints fresh sessions on demand, so leaking it is far worse than
    /// leaking a session id. Everything else a browser sends — cf_clearance,
    /// __cf_bm, autolang, mbox, tarteaucitron, the forum cookies — is ignored
    /// by this API and only widens what you are pasting around.
    ///
    /// Prefer the environment variable over the flag, so the value stays out
    /// of your shell history:
    ///
    ///   export GENEANET_COOKIE='gntsess5=<value>'
    #[arg(long, env = "GENEANET_COOKIE", hide_env_values = true)]
    cookie: String,

    /// Pause after each request, in milliseconds.
    ///
    /// Requests are issued one at a time. Collecting the whole mapping costs
    /// roughly fifteen of them, so there is nothing to gain from going faster
    /// and something to lose: volume is what gets a client challenged.
    #[arg(long, default_value_t = 100)]
    delay_ms: u64,

    /// Geneanet base URL. Override only for testing.
    #[arg(long)]
    base_url: Option<String>,
}

impl Session {
    fn into_client(self) -> Result<geneanet::Client> {
        let throttle = geneanet::throttle(self.delay_ms)?;
        geneanet::Client::new(&self.cookie, self.base_url, throttle)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::GeneanetMedia(GeneanetMedia::Manifest { session, out }) => {
            geneanet::manifest(session.into_client()?, &out).await?;
        }
        Command::GeneanetMedia(GeneanetMedia::BrowserScript) => {
            geneanet::browser_script();
        }
        Command::GeneanetMedia(GeneanetMedia::ManifestFromBrowser { input, out }) => {
            geneanet::manifest_from_browser(&input, &out).await?;
        }
        Command::GeneanetMedia(GeneanetMedia::Check {
            gw,
            manifest,
            verbose,
        }) => {
            geneanet::check(&gw, &manifest, verbose).await?;
        }
        Command::GeneanetMedia(GeneanetMedia::Gedzip {
            session,
            gw,
            manifest,
            local_media,
            multipage_originals,
            out,
        }) => {
            geneanet::build_gedzip(
                session.into_client()?,
                &gw,
                &manifest,
                local_media.as_deref(),
                multipage_originals,
                &out,
            )
            .await?;
        }
        Command::GeneanetMedia(GeneanetMedia::Fetch {
            session,
            manifest,
            media_dir,
        }) => {
            geneanet::fetch(session.into_client()?, &manifest, &media_dir).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        use clap::CommandFactory;

        Cli::command().debug_assert();
    }
}
