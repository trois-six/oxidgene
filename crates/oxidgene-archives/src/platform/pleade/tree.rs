//! The `tree` mode: a walk down a finding aid's table of contents.
//!
//! The table of contents comes in fragments, each listing a node's children
//! and grandchildren with whether they have images (`image_illustrated`, a
//! register) or descendants that do (`anc_illustrated`). The localities'
//! nodes stand at one depth of the aid, titled as `locality_label` says
//! (`{locality}`, `Bureau de recrutement de {locality}`); every other node
//! names a kind of document (`Baptêmes, mariages, sépultures`,
//! `Naissances`, `Tables décennales (TD)`, `Registres matricules du
//! recrutement`), years (`1793-1806`, `Classe 1900`) or a copy (`Collection
//! communale`). The walk keeps the cited locality's node, prunes the nodes
//! naming another kind or other years, and reads the fragments of the nodes
//! it keeps until it reaches registers.

use super::page::{self, Illustrated, Node};
use super::{Found, Register, Search, Settings, get, unexpected};
use crate::ResolveError;
use crate::citation::CitationParts;
use crate::platform::locality::label_name;
use crate::platform::markup::fold;
use crate::platform::select::{Candidate, covers, holds_act, number_range};
use crate::transport::PortalFetch;

/// The most table-of-contents fragments read below the aid's own.
const MAX_FRAGMENTS: usize = 6;

/// The words a title of years may hold besides them: `Classe 1900`.
const YEAR_WORDS: [&str; 4] = ["classe", "classes", "annee", "annees"];

/// The fragment of the table of contents below `node`, or the aid's own.
pub(super) async fn fragment(
    settings: &Settings,
    aid: &str,
    node: &str,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Node>, ResolveError> {
    let answer = get(
        fetch,
        &format!(
            "{}/functions/ead/get-toc-fragment/{aid}/{node}.ajax-html",
            settings.path
        ),
    )
    .await?;
    page::toc(&answer)
}

/// Whether a title names only years: `1793-1806`, `Classe 1900`.
pub(super) fn is_period(title: &str) -> bool {
    let folded = fold(title);
    let mut years = false;
    for word in folded.split(' ') {
        if word.len() == 4 && word.bytes().all(|byte| byte.is_ascii_digit()) {
            years = true;
        } else if !YEAR_WORDS.contains(&word) && !word.is_empty() {
            return false;
        }
    }
    years
}

/// The locality a locality node's title names, as a citation writes it:
/// the title less the label's text around `{locality}`, its article in
/// front (`Bourg (Le)`).
pub(super) fn locality_of(title: &str, label: &str) -> Option<String> {
    let (before, after) = label.split_once("{locality}")?;
    let name = title.strip_prefix(before)?.strip_suffix(after)?;
    (!name.trim().is_empty()).then(|| label_name(name))
}

/// The call number a register's title carries after a bullet: `Matricules
/// 1-502 • R 1535`.
fn titled_call_number(title: &str) -> Option<String> {
    let (_, call_number) = title.rsplit_once(" \u{2022} ")?;
    Some(call_number.trim().to_owned()).filter(|text| !text.is_empty())
}

/// What a walk carries down a branch: the most specific kind and years its
/// nodes named.
#[derive(Debug, Clone, Default)]
struct Branch {
    act: Option<String>,
    period: Option<String>,
}

/// Where the localities' nodes stand and how they are titled.
#[derive(Clone, Copy)]
struct Localities<'s> {
    depth: u8,
    label: &'s str,
    /// The cited locality's forms, folded.
    wanted: &'s [String],
}

/// One step of the walk at a node of `depth`: `None` when its title prunes
/// it, the branch below it otherwise.
fn enter(
    node: &Node,
    depth: u8,
    branch: &Branch,
    citation: &CitationParts,
    localities: Localities<'_>,
) -> Option<Branch> {
    let Localities {
        depth: locality_depth,
        label,
        wanted,
    } = localities;
    if node.illustrated == Illustrated::No {
        return None;
    }
    if depth == locality_depth {
        let name = locality_of(&node.title, label)?;
        return wanted.contains(&fold(&name)).then(|| branch.clone());
    }
    let mut below = branch.clone();
    if is_period(&node.title) {
        if let Some(year) = citation.year
            && !covers(&node.title, year)
        {
            return None;
        }
        below.period = Some(node.title.clone());
    } else if let Some(code) = page::act_of(&node.title) {
        if !holds_act(Some(&code), &citation.act) {
            return None;
        }
        below.act = Some(code);
    }
    Some(below)
}

/// A register the walk reached.
fn candidate(node: &Node, branch: &Branch, locality: &str) -> Candidate<Register> {
    let period = if is_period(&node.title) {
        Some(node.title.clone())
    } else {
        branch.period.clone()
    };
    Candidate {
        locality: Some(locality.to_owned()),
        call_number: titled_call_number(&node.title),
        act: branch.act.clone(),
        parish: None,
        period,
        images: None,
        numbers: number_range(&node.title, true),
        payload: Register::Component(node.id.clone()),
    }
}

pub(super) async fn find(
    settings: &Settings,
    citation: &CitationParts,
    localities: &[&str],
    fetch: &dyn PortalFetch,
) -> Result<Found, ResolveError> {
    let Search::Tree { aid, depth, label } = &settings.search else {
        return Err(unexpected("settings of another mode"));
    };
    let wanted: Vec<String> = localities.iter().map(|locality| fold(locality)).collect();
    let locality = localities.first().copied().unwrap_or_default();
    let mut found = Found {
        candidates: Vec::new(),
        results_url: None,
        total: 0,
    };
    // Nodes to look at: the node, its depth, the branch above it.
    let mut pending: Vec<(Node, u8, Branch)> = fragment(settings, aid, aid, fetch)
        .await?
        .into_iter()
        .map(|node| (node, 1, Branch::default()))
        .collect();
    if pending.is_empty() {
        return Err(unexpected("the finding aid's table of contents is empty"));
    }
    let mut read = 0;
    while let Some((node, at, branch)) = pending.pop() {
        let sought = Localities {
            depth: *depth,
            label,
            wanted: &wanted,
        };
        let Some(below) = enter(&node, at, &branch, citation, sought) else {
            continue;
        };
        if node.illustrated == Illustrated::Images {
            // A register above the localities is none of the locality's.
            if at > *depth {
                found.candidates.push(candidate(&node, &below, locality));
            }
            continue;
        }
        let children = if node.children.is_empty() {
            if read == MAX_FRAGMENTS {
                // The walk stops: what is left is counted, not read.
                found.total += 1;
                continue;
            }
            read += 1;
            fragment(settings, aid, &node.id, fetch).await?
        } else {
            node.children
        };
        // In the listed order: the stack is read from its end.
        pending.extend(
            children
                .into_iter()
                .rev()
                .map(|child| (child, at + 1, below.clone())),
        );
    }
    found.total += found.candidates.len();
    Ok(found)
}
