//! Reading Pleade's answers: the search form, the result fragment, the
//! finding aid's table of contents and component fragments, the IIIF
//! manifest.

use serde::Deserialize;
use serde::de::IgnoredAny;

use super::unexpected;
use crate::ResolveError;
use crate::platform::markup::{self, attribute, first_number, split_after, strip_tags};
use crate::platform::select::act_code;

/// A register's persistent address, `ark:/<naan>/<name>`, under the
/// portal's path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Ark {
    pub(super) naan: String,
    pub(super) name: String,
}

impl Ark {
    /// The ARK of a viewer link on the portal, `<origin><path>/ark:/<naan>/
    /// <name>/<file or view>[?context=…]`; `None` for any other address.
    pub(super) fn of_link(link: &str, prefix: &str) -> Option<Self> {
        let rest = link.strip_prefix(prefix)?.strip_prefix("/ark:/")?;
        let mut parts = rest.split('/');
        let naan = parts.next()?;
        let name = parts.next()?;
        let valid = !naan.is_empty()
            && naan.bytes().all(|byte| byte.is_ascii_digit())
            && !name.is_empty()
            && name.bytes().all(|byte| byte.is_ascii_alphanumeric());
        valid.then(|| Self {
            naan: naan.to_owned(),
            name: name.to_owned(),
        })
    }

    /// The path of the register's viewer at `view`, one-based.
    pub(super) fn view_path(&self, path: &str, view: u16) -> String {
        format!("{path}/ark:/{}/{}/f{view}", self.naan, self.name)
    }

    /// The path of the register's IIIF manifest.
    pub(super) fn manifest_path(&self, path: &str) -> String {
        format!("{path}/iiif/ark:/{}/{}/manifest.json", self.naan, self.name)
    }
}

/// The act code a title or the acts of a row name, as selection compares
/// it: a series in the citation vocabulary (`RM` for `Registres
/// matricules`), `TD` for decennial tables, the kinds (`BMS`, `N`); an
/// initial in a title is no act.
pub(super) fn act_of(text: &str) -> Option<String> {
    act_code(text, false)
}

// ------------------------------------------------------------------ form

/// The search form of a `form` collection.
pub(super) struct Form<'p> {
    html: &'p str,
}

impl<'p> Form<'p> {
    pub(super) fn of(page: &'p str) -> Option<Self> {
        let start = page.find("<form ")?;
        let html = &page[start..];
        if !html[..html.find('>')?].contains("pl-form-advanced-srch") {
            return None;
        }
        let end = html.find("</form>").unwrap_or(html.len());
        Some(Self { html: &html[..end] })
    }

    /// The results page the form submits to, absolute.
    pub(super) fn action(&self) -> Option<String> {
        attribute(&self.html[..self.html.find('>')?], "action")
    }

    /// The hidden inputs and their values, in order.
    pub(super) fn hidden(&self) -> Vec<(String, String)> {
        split_after(self.html, "<input")
            .into_iter()
            .filter_map(|tag| {
                let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
                if !tag.contains("type=\"hidden\"") {
                    return None;
                }
                Some((
                    attribute(tag, "name")?,
                    attribute(tag, "value").unwrap_or_default(),
                ))
            })
            .collect()
    }

    /// The options of the `select` named `name`, the empty one left out.
    pub(super) fn options(&self, name: &str) -> Vec<String> {
        split_after(self.html, "<select")
            .into_iter()
            .find(|select| {
                let tag = &select[..select.find('>').unwrap_or(select.len())];
                attribute(tag, "name").as_deref() == Some(name)
            })
            .map(|select| {
                let select = &select[..select.find("</select>").unwrap_or(select.len())];
                markup::attributes(select, "value")
                    .into_iter()
                    .filter(|value| !value.trim().is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether the form has an input or select of that name.
    pub(super) fn has(&self, name: &str) -> bool {
        self.html.contains(&format!("name=\"{name}\""))
    }
}

/// One row of a result fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Row {
    pub(super) locality: Option<String>,
    pub(super) period: Option<String>,
    /// The kind of document and the acts, one per paragraph: `Registres
    /// d'actes`, `Baptêmes`, `Mariages`.
    pub(super) acts: Vec<String>,
    pub(super) call_number: Option<String>,
    pub(super) ark: Ark,
}

/// A page of results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Results {
    pub(super) total: usize,
    pub(super) pages: usize,
    pub(super) rows: Vec<Row>,
}

/// The text of the first cell of `class` in a row.
fn cell<'r>(row: &'r str, class: &str) -> Option<&'r str> {
    let start = row.find(&format!("<td class=\"{class}\""))?;
    let cell = &row[start..];
    Some(&cell[..cell.find("</td>").unwrap_or(cell.len())])
}

fn text(html: &str) -> Option<String> {
    Some(strip_tags(html)).filter(|text| !text.is_empty())
}

/// The result fragment of a search: `N résultats` and `Page 1 de N`, or
/// `Aucun résultat`, and the rows that link a viewer, `prefix` being the
/// portal's origin and path.
pub(super) fn results(answer: &str, prefix: &str) -> Result<Results, ResolveError> {
    if answer.contains("class=\"pl-results-zero\"") {
        return Ok(Results {
            total: 0,
            pages: 1,
            rows: Vec::new(),
        });
    }
    let Some(count) = markup::text_after(answer, "<span class=\"nbresults\">") else {
        return Err(markup::unreadable(
            answer,
            "pleade: the results have no count".to_owned(),
        ));
    };
    let total = first_number(&count).ok_or_else(|| unexpected("the result count has no number"))?;
    let pages = answer
        .find("<span class=\"nbpages\">")
        .map(|start| &answer[start..])
        .and_then(|span| {
            let span = &span[..span.find("</span>")?];
            strip_tags(span)
                .rsplit(' ')
                .next()
                .and_then(|last| last.parse().ok())
        })
        .unwrap_or(1);
    let rows = split_after(answer, "<tr class=\"")
        .into_iter()
        .filter(|row| row.starts_with("odd\"") || row.starts_with("even\""))
        .filter_map(|row| {
            let row = &row[..row.find("</tr>").unwrap_or(row.len())];
            let link = attribute(cell(row, "img-tab")?, "href")?;
            let acts = cell(row, "type")
                .map(|cell| {
                    split_after(cell, "<p>")
                        .into_iter()
                        .filter_map(|paragraph| {
                            text(&paragraph[..paragraph.find("</p>").unwrap_or(paragraph.len())])
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(Row {
                locality: cell(row, "commune").and_then(text),
                period: cell(row, "date").and_then(text),
                acts,
                call_number: cell(row, "cote").and_then(text),
                ark: Ark::of_link(&link, prefix)?,
            })
        })
        .collect();
    Ok(Results { total, pages, rows })
}

// ------------------------------------------------------------------ tree

/// Whether a node of a finding aid has images, or descendants that do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Illustrated {
    /// The node itself opens a viewer: a register.
    Images,
    /// Some descendants do.
    Descendants,
    No,
}

/// A node of a finding aid's table of contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Node {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) illustrated: Illustrated,
    /// The children the fragment lists; empty when it lists none, and then
    /// read from the node's own fragment.
    pub(super) children: Vec<Node>,
}

/// The nodes of a table-of-contents fragment, `ul#treeRoot`: the children
/// of the node asked for, each with its own children, nested as listed.
pub(super) fn toc(fragment: &str) -> Result<Vec<Node>, ResolveError> {
    let Some(start) = fragment.find("<ul id=\"treeRoot\">") else {
        return Err(markup::unreadable(
            fragment,
            "pleade: the table of contents has no tree".to_owned(),
        ));
    };
    let mut rest = &fragment[start + "<ul id=\"treeRoot\">".len()..];
    Ok(nodes(&mut rest))
}

/// The `li` elements of a list, up to its closing `</ul>`, consumed from
/// `rest`.
fn nodes(rest: &mut &str) -> Vec<Node> {
    let mut found = Vec::new();
    loop {
        let next_item = rest.find("<li ");
        let close = rest.find("</ul>");
        match (next_item, close) {
            (Some(item), Some(close)) if item < close => {
                *rest = &rest[item..];
                if let Some(node) = node(rest) {
                    found.push(node);
                }
            }
            (_, Some(close)) => {
                *rest = &rest[close + "</ul>".len()..];
                return found;
            }
            (Some(item), None) => {
                *rest = &rest[item..];
                if let Some(node) = node(rest) {
                    found.push(node);
                }
            }
            (None, None) => {
                *rest = "";
                return found;
            }
        }
    }
}

/// One `li` at the start of `rest`, with its nested list, consumed.
fn node(rest: &mut &str) -> Option<Node> {
    let tag_end = rest.find('>')?;
    let id = attribute(&rest[..tag_end], "id");
    *rest = &rest[tag_end + 1..];
    // The item's own markup runs to its nested list or its end.
    let own_end = [rest.find("<ul"), rest.find("</li>"), rest.find("<li ")]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(rest.len());
    let own = &rest[..own_end];
    let illustrated = match attribute(own, "class").as_deref() {
        Some("image_illustrated") => Illustrated::Images,
        Some("anc_illustrated") => Illustrated::Descendants,
        _ => Illustrated::No,
    };
    let title = own
        .find("name=\"link\"")
        .map(|at| &own[at..])
        .and_then(|link| link.split_once('>'))
        .map(|(_, text)| {
            markup::decode_entities(text[..text.find('<').unwrap_or(text.len())].trim())
        })
        .unwrap_or_default();
    *rest = &rest[own_end..];
    let mut children = Vec::new();
    if rest.starts_with("<ul") {
        let list_start = rest.find('>').map_or(rest.len(), |end| end + 1);
        *rest = &rest[list_start..];
        children = nodes(rest);
    }
    if let Some(end) = rest.find("</li>")
        && rest[..end].trim().is_empty()
    {
        *rest = &rest[end + "</li>".len()..];
    }
    Some(Node {
        id: id?,
        title,
        illustrated,
        children,
    })
}

/// What a component's fragment, `ead-fragment.xsp?c=<id>`, says of a
/// register: its call number, its dates and its viewer's ARK.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Component {
    pub(super) call_number: Option<String>,
    pub(super) period: Option<String>,
    pub(super) ark: Option<Ark>,
}

pub(super) fn component(fragment: &str, prefix: &str) -> Result<Component, ResolveError> {
    if !fragment.contains("class=\"pl-pgd-component\"") {
        return Err(markup::unreadable(
            fragment,
            "pleade: the component fragment has no component".to_owned(),
        ));
    }
    // The first table of identification is the component's own; its
    // children's notices follow it.
    let own = &fragment[..fragment
        .find("class=\"pl-pgd-children")
        .unwrap_or(fragment.len())];
    let value = |class: &str| {
        let start = own.find(&format!("<td class=\"{class}\""))?;
        let cell = &own[start..];
        text(&cell[..cell.find("</td>").unwrap_or(cell.len())])
    };
    let ark = own
        .find("class=\"pl-pgd-medias-thumbnail\"")
        .map(|at| &own[at..])
        .and_then(|link| attribute(&link[..link.find('>').unwrap_or(link.len())], "href"))
        .and_then(|href| Ark::of_link(&href, prefix));
    Ok(Component {
        call_number: value("pl-tbl-unitid"),
        period: value("pl-tbl-unitdate"),
        ark,
    })
}

// ------------------------------------------------------------- manifest

#[derive(Deserialize)]
struct Manifest {
    sequences: Vec<Sequence>,
}

#[derive(Deserialize)]
struct Sequence {
    canvases: Vec<IgnoredAny>,
}

/// The number of views of a register's IIIF Presentation 2 manifest.
pub(super) fn view_count(manifest: &str) -> Result<usize, ResolveError> {
    let manifest: Manifest = serde_json::from_str(manifest).map_err(|_| {
        markup::unreadable(
            manifest,
            "pleade: the manifest lacks sequences[].canvases".to_owned(),
        )
    })?;
    match manifest
        .sequences
        .first()
        .map(|sequence| sequence.canvases.len())
    {
        Some(count) if count > 0 => Ok(count),
        _ => Err(unexpected("the manifest lists no canvas")),
    }
}
