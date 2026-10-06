//! The `ead` module: a finding aid with no search form, browsed through the
//! fragments its tree loads.
//!
//! The aid's page lists every commune as a node. A commune's node has the
//! act nodes (`Actes (BMS puis NMD)`, `Tables décennales`), an act node has
//! its collections, and a collection's notice gives one block per register:
//! call number, period, `107 images numériques` and the link to the viewer.
//! A node whose tree fragment is empty is itself the notice. The communes'
//! nodes are listed by the aid's page alone, slow to build: the adapter keeps
//! their index for the session ([`Communes`]).

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::{Found, Register, Search, Settings, choose, get, number_after, unexpected};
use crate::ResolveError;
use crate::citation::CitationParts;
use crate::platform::markup::{self, first_number, fold, is_named, letters};
use crate::platform::select::{Candidate, narrow};
use crate::transport::PortalFetch;

/// The commune index of each finding aid read so far — each commune's node
/// and name, by the address of the aid's page —, kept by the adapter for the
/// application's session, as the resolver keeps its targets (Archive Portals
/// §8).
///
/// The aid's page is the only list of the communes' nodes: some 300 KB
/// that the Côte-d'Or portal builds in one second or, at busy times, in
/// over a minute, past a request's bound, while a commune's fragments
/// answer at once. Its index is a few tens of kilobytes. A node the portal
/// no longer knows, after it published the aid anew, drops the index, which
/// the same resolution reads again.
#[derive(Default)]
pub(super) struct Communes(Mutex<BTreeMap<String, Vec<(String, String)>>>);

impl Communes {
    pub(super) const fn new() -> Self {
        Self(Mutex::new(BTreeMap::new()))
    }

    /// The communes of the aid `settings` name, and whether they were kept
    /// rather than read from its page now.
    pub(super) async fn read(
        &self,
        settings: &Settings,
        fetch: &dyn PortalFetch,
    ) -> Result<(Vec<(String, String)>, bool), ResolveError> {
        let page = settings.search_page();
        if let Some(kept) = self.0.lock().ok().and_then(|kept| kept.get(&page).cloned()) {
            return Ok((kept, true));
        }
        let path = page.strip_prefix(&settings.origin).unwrap_or_default();
        let answer = get(fetch, path).await?;
        Ok((self.keep(settings, &answer)?, false))
    }

    /// Keeps the communes the aid's page lists, and returns them: none is a
    /// changed page.
    pub(super) fn keep(
        &self,
        settings: &Settings,
        page: &str,
    ) -> Result<Vec<(String, String)>, ResolveError> {
        let communes = entries(page);
        if communes.is_empty() {
            return Err(markup::unreadable(
                page,
                "archinoe: the finding aid lists no commune".to_owned(),
            ));
        }
        if let Ok(mut kept) = self.0.lock() {
            kept.insert(settings.search_page(), communes.clone());
        }
        Ok(communes)
    }

    fn forget(&self, settings: &Settings) {
        if let Ok(mut kept) = self.0.lock() {
            kept.remove(&settings.search_page());
        }
    }
}

pub(super) async fn find(
    index: &Communes,
    settings: &Settings,
    citation: &CitationParts,
    localities: &[&str],
    fetch: &dyn PortalFetch,
) -> Result<Found, ResolveError> {
    let Search::Ead { ir, .. } = &settings.search else {
        return Err(unexpected("settings of another search"));
    };
    let action = format!("{}/ir_ead_visu_action.php?ir={ir}", settings.base);
    let Some(title) = settings.act_value(&citation.act) else {
        return Ok(Found::Many(0));
    };
    let wanted: Vec<Vec<char>> = localities
        .iter()
        .map(|locality| letters(locality))
        .collect();

    let acts = loop {
        let (communes, kept) = index.read(settings, fetch).await?;
        let Some((commune, _)) = communes.iter().find(|(_, name)| is_named(name, &wanted)) else {
            return Ok(Found::Many(0));
        };
        let acts = entries(&get(fetch, &format!("{action}&id={commune}&toc=1")).await?);
        // A commune node is never empty: one the portal no longer knows
        // dates the index, which is read again.
        if acts.is_empty() && kept {
            index.forget(settings);
            continue;
        }
        break acts;
    };
    let Some((node, _)) = acts.iter().find(|(_, name)| fold(name) == fold(title)) else {
        return Ok(Found::Many(0));
    };
    let mut collections: Vec<String> =
        entries(&get(fetch, &format!("{action}&id={node}&toc=1")).await?)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
    if collections.is_empty() {
        collections.push(node.clone());
    }

    let locality = localities.first().copied().unwrap_or_default();
    let mut rows = Vec::new();
    for collection in &collections {
        rows.extend(registers(
            &get(fetch, &format!("{action}&id={collection}")).await?,
            locality,
        ));
        if narrow(&rows, citation, localities).len() == 1 {
            break;
        }
    }
    Ok(
        match choose(settings, &rows, citation, localities, fetch).await? {
            Ok(chosen) => Found::One(chosen),
            Err(matches) => Found::Many(matches),
        },
    )
}

/// The nodes a tree fragment or the aid's page lists:
/// `javascript:showEntry(<id>)…>Name<`.
pub(super) fn entries(html: &str) -> Vec<(String, String)> {
    markup::split_after(html, "javascript:showEntry(")
        .into_iter()
        .filter_map(|entry| {
            let id = number_after(entry, "")?;
            let rest = entry.split_once('>')?.1;
            let name = markup::decode_entities(rest[..rest.find('<')?].trim());
            (!name.is_empty()).then_some((id, name))
        })
        .collect()
}

/// The registers of a collection's notice: one `<div id="item_<id>">` block
/// each, whose cells carry the call number, the period and, in the
/// description, the image count. A block without a viewer link has no
/// images and is left out.
pub(super) fn registers(notice: &str, locality: &str) -> Vec<Candidate<Register>> {
    markup::split_after(notice, "<div id=\"item_")
        .into_iter()
        .filter_map(|block| {
            let id = number_after(block, "lienImage(")?;
            // The first text of the cell with the given class.
            let cell = |class: &str| -> Option<String> {
                let at = block.find(&format!("class=\"{class}\""))?;
                let cell = &block[at..];
                let cell = &cell[..cell.find("</td>")?];
                markup::labelled_texts(cell, "id")
                    .into_iter()
                    .map(|(_, text)| text)
                    .next()
            };
            let images = markup::labelled_texts(block, "id")
                .into_iter()
                .find(|(_, text)| fold(text).contains("images numeriques"))
                .and_then(|(_, text)| first_number(&text));
            Some(Candidate {
                locality: Some(locality.to_owned()),
                call_number: cell("cotes"),
                act: None,
                parish: None,
                period: cell("dates").or_else(|| cell("titres")),
                images,
                numbers: None,
                payload: Register { id },
            })
        })
        .collect()
}
