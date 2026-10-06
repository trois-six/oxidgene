//! The `ead` module: a finding aid with no search form, browsed through the
//! fragments its tree loads.
//!
//! The aid's page lists every commune as a node. A commune's node has the
//! act nodes (`Actes (BMS puis NMD)`, `Tables décennales`), an act node has
//! its collections, and a collection's notice gives one block per register:
//! call number, period, `107 images numériques` and the link to the viewer.
//! A node whose tree fragment is empty is itself the notice.

use super::{Found, Register, Search, Settings, choose, get, number_after, unexpected};
use crate::ResolveError;
use crate::citation::CitationParts;
use crate::platform::markup::{self, first_number, fold, is_named, letters};
use crate::platform::select::{Candidate, narrow};
use crate::transport::PortalFetch;

pub(super) async fn find(
    settings: &Settings,
    citation: &CitationParts,
    localities: &[&str],
    fetch: &dyn PortalFetch,
) -> Result<Found, ResolveError> {
    let Search::Ead { ir, eadid } = &settings.search else {
        return Err(unexpected("settings of another search"));
    };
    let action = format!("{}/ir_ead_visu_action.php?ir={ir}", settings.base);

    let root = get(
        fetch,
        &format!("{}/ir_ead_visu.php?eadid={eadid}&ir={ir}", settings.base),
    )
    .await?;
    let communes = entries(&root);
    if communes.is_empty() {
        return Err(unexpected("the finding aid lists no commune"));
    }
    let wanted: Vec<Vec<char>> = localities
        .iter()
        .map(|locality| letters(locality))
        .collect();
    let Some((commune, _)) = communes.iter().find(|(_, name)| is_named(name, &wanted)) else {
        return Ok(Found::Many(0));
    };

    let Some(title) = settings.act_value(&citation.act) else {
        return Ok(Found::Many(0));
    };
    let acts = entries(&get(fetch, &format!("{action}&id={commune}&toc=1")).await?);
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
