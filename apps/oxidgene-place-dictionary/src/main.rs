//! Generates OxidGene's place dictionaries from open data.
//!
//! `just places` runs it, as often as the sources change. It finds the
//! latest edition of INSEE's Code officiel géographique and of the ONS Index
//! of Place Names by itself, downloads them with geo.api.gouv.fr and
//! Wikidata, then writes one CSV per language in the layout of Geneanet's
//! `dico_place_*.csv`, extended with columns of its own. The format, the
//! sources and their licences are specified in
//! `docs/place-dictionary.md`.

mod fetch;
mod france;
mod place;
mod table;
mod uk;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::fetch::Fetcher;
use crate::place::Language;

const USAGE: &str = "usage: oxidgene-place-dictionary [--out DIR] [--cache DIR] [--cached]";

struct Args {
    out: PathBuf,
    cache: PathBuf,
    /// Reuse the downloads of an earlier run instead of fetching the
    /// sources again.
    cached: bool,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut args = Self {
            out: PathBuf::from("target/place-dictionary"),
            cache: PathBuf::from("target/place-dictionary/sources"),
            cached: false,
        };
        let mut raw = std::env::args().skip(1);
        while let Some(arg) = raw.next() {
            match arg.as_str() {
                "--out" => args.out = raw.next().context(USAGE)?.into(),
                "--cache" => args.cache = raw.next().context(USAGE)?.into(),
                "--cached" => args.cached = true,
                _ => bail!("unknown argument `{arg}`\n{USAGE}"),
            }
        }
        Ok(args)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse()?;
    let fetcher = Fetcher::new(args.cache, args.cached)?;
    let mut places = france::places(&fetcher).await?;
    places.extend(uk::places(&fetcher).await?);
    for language in Language::ALL {
        let path = args.out.join(format!("places.{}.csv", language.code()));
        let written = place::write(&mut places, language, &path)?;
        eprintln!("{written} places written to {}", path.display());
    }
    Ok(())
}
