//! Generates OxidGene's place dictionary from open data.
//!
//! `just places` runs it, as often as the sources change. It finds the
//! latest edition of INSEE's Code officiel géographique and of the ONS Index
//! of Place Names by itself, downloads them with geo.api.gouv.fr and
//! Wikidata, then writes one CSV in the layout of Geneanet's
//! `dico_place_fr.csv`, extended with columns of its own, and compresses it
//! into `assets/places/places.csv.br`, which is committed and embedded into
//! the binaries. It also writes the Statistics page's basemap,
//! `assets/basemap/countries.json.br`, from Natural Earth. The format, the sources and their licences are specified in
//! `docs/place-dictionary.md`.

mod basemap;
mod belgium;
mod calendar;
mod fetch;
mod france;
mod germany;
mod italy;
mod luxembourg;
mod netherlands;
mod place;
mod poland;
mod portugal;
mod spain;
mod switzerland;
mod table;
mod uk;
mod usa;
mod wikidata;
mod xlsx;

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::fetch::Fetcher;

const USAGE: &str = "usage: oxidgene-place-dictionary [--out FILE] [--basemap FILE] [--csv FILE] [--cache DIR] [--cached]";

/// The settings every embedded data file is compressed with; the API crate's
/// build script uses the same. Quality 11 with a 16 MiB window gives the best
/// ratio Brotli has, and decoding stays as fast whatever the quality.
const BROTLI_QUALITY: u32 = 11;
const BROTLI_WINDOW_BITS: u32 = 24;

struct Args {
    out: PathBuf,
    /// Where the heat map's basemap goes.
    basemap: PathBuf,
    /// Also write the uncompressed CSV, to read it.
    csv: Option<PathBuf>,
    cache: PathBuf,
    /// Reuse the downloads of an earlier run instead of fetching the
    /// sources again.
    cached: bool,
}

impl Args {
    fn parse() -> Result<Self> {
        let mut args = Self {
            out: PathBuf::from("assets/places/places.csv.br"),
            basemap: PathBuf::from("assets/basemap/countries.json.br"),
            csv: None,
            cache: PathBuf::from("target/place-dictionary"),
            cached: false,
        };
        let mut raw = std::env::args().skip(1);
        while let Some(arg) = raw.next() {
            match arg.as_str() {
                "--out" => args.out = raw.next().context(USAGE)?.into(),
                "--basemap" => args.basemap = raw.next().context(USAGE)?.into(),
                "--csv" => args.csv = Some(raw.next().context(USAGE)?.into()),
                "--cache" => args.cache = raw.next().context(USAGE)?.into(),
                "--cached" => args.cached = true,
                _ => bail!("unknown argument `{arg}`\n{USAGE}"),
            }
        }
        Ok(args)
    }
}

fn compress(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut compressed = Vec::new();
    {
        let mut encoder = brotli::CompressorWriter::new(
            &mut compressed,
            1 << 16,
            BROTLI_QUALITY,
            BROTLI_WINDOW_BITS,
        );
        encoder.write_all(bytes)?;
    }
    Ok(compressed)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse()?;
    let fetcher = Fetcher::new(args.cache, args.cached)?;
    let mut places = all_places(&fetcher).await?;
    let (csv, rows, dropped) = place::render(&mut places);
    eprintln!("{dropped} duplicate rows dropped");

    if let Some(path) = &args.csv {
        std::fs::write(path, &csv).with_context(|| format!("cannot write {}", path.display()))?;
    }
    eprintln!("compressing {rows} places");
    let compressed = compress(csv.as_bytes())?;
    write_asset(&args.out, &compressed)?;
    eprintln!(
        "{rows} places, {} bytes, written to {}",
        compressed.len(),
        args.out.display()
    );

    let map = compress(basemap::basemap(&fetcher).await?.as_bytes())?;
    write_asset(&args.basemap, &map)?;
    eprintln!(
        "basemap, {} bytes, written to {}",
        map.len(),
        args.basemap.display()
    );
    Ok(())
}

/// The places of every country the dictionary covers.
async fn all_places(fetcher: &Fetcher) -> Result<Vec<place::Place>> {
    let mut places = france::places(fetcher).await?;
    places.extend(uk::places(fetcher).await?);
    places.extend(germany::places(fetcher).await?);
    places.extend(italy::places(fetcher).await?);
    places.extend(spain::places(fetcher).await?);
    places.extend(switzerland::places(fetcher).await?);
    places.extend(poland::places(fetcher).await?);
    places.extend(usa::places(fetcher).await?);
    places.extend(portugal::places(fetcher).await?);
    places.extend(belgium::places(fetcher).await?);
    places.extend(luxembourg::places(fetcher).await?);
    places.extend(netherlands::places(fetcher).await?);
    Ok(places)
}

/// Writes `bytes` at `path`, creating its directory.
fn write_asset(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")))?;
    std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}
