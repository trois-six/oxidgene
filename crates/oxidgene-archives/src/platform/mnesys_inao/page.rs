//! Reading the older Mnesys interface's pages: a guided search's answer,
//! whose hits are nodes of finding aids, and a node's notice.

use crate::ResolveError;
use crate::platform::markup::{self, decode_entities, first_number, strip_tags};

fn unreadable(page: &str, detail: &str) -> ResolveError {
    markup::unreadable(page, format!("mnesys-inao: {detail}"))
}

/// The page without its comments, which keep older copies of the cells
/// (`<!--<div class="date">…</div>-->`).
fn uncommented(page: &str) -> String {
    let mut kept = String::with_capacity(page.len());
    let mut rest = page;
    while let Some(start) = rest.find("<!--") {
        kept.push_str(&rest[..start]);
        rest = rest[start..]
            .find("-->")
            .map_or("", |end| &rest[start + end + 3..]);
    }
    kept.push_str(rest);
    kept
}

/// One hit of a search: a node of a finding aid, as the list shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Node {
    /// The notice's path: `/?id=<form>_detail&doc=…&page_ref=<node>`.
    pub(super) detail: String,
    pub(super) title: String,
    pub(super) period: Option<String>,
    /// Empty on a heading over registers, which are listed themselves.
    pub(super) call_number: Option<String>,
    /// The breadcrumb's entries after the finding aid: the commune, a
    /// parish, a series, or a range of call numbers.
    pub(super) context: Vec<String>,
}

/// A search's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Answer {
    /// The hits the portal counts.
    pub(super) total: usize,
    /// The pages of twenty it lists them on.
    pub(super) pages: usize,
    pub(super) nodes: Vec<Node>,
}

/// The text of the first `<div class="<class>">` of `item`.
fn div_text(item: &str, class: &str) -> Option<String> {
    let marker = format!("<div class=\"{class}\">");
    let start = item.find(&marker)? + marker.len();
    let end = item[start..]
        .find("</div>")
        .map_or(item.len(), |end| start + end);
    Some(strip_tags(&item[start..end])).filter(|text| !text.is_empty())
}

fn node(item: &str) -> Option<Node> {
    let title_start = item.find("<div class=\"title\">")?;
    let title_block = &item[title_start..];
    let link = &title_block[title_block.find("<a ")?..];
    let tag = &link[..link.find('>')?];
    let detail = markup::attribute(tag, "href").filter(|href| href.starts_with("/?id="))?;
    let title = strip_tags(&link[..link.find("</a>").unwrap_or(link.len())]);
    let call_number = item.find("<div class=\"cote\">").and_then(|at| {
        let cell = &item[at..];
        let start = cell.find("<strong>")? + "<strong>".len();
        let end = cell[start..].find("</strong>")? + start;
        Some(strip_tags(&cell[start..end])).filter(|text| !text.is_empty())
    });
    let context = item
        .find("<div class=\"ariane\">")
        .map(|at| {
            let ariane = &item[at..item[at..].find("</div>").map_or(item.len(), |end| at + end)];
            markup::split_after(ariane, "class=\"various link_ariane\"")
                .into_iter()
                .skip(1)
                .filter_map(|entry| {
                    let text = &entry[entry.find('>')? + 1..];
                    Some(decode_entities(text[..text.find('<')?].trim()))
                })
                .filter(|text| !text.is_empty())
                .collect()
        })
        .unwrap_or_default();
    Some(Node {
        detail,
        title,
        period: div_text(item, "date"),
        call_number,
        context,
    })
}

/// Reads a search's answer: the count (`20 réponses`, `1 réponse`), the
/// pages (`&page=<n>` links) and the hits. An answer without a count is the
/// search form again, which shows no hit, or not the portal's page.
pub(super) fn answer(page: &str) -> Result<Answer, ResolveError> {
    let page = uncommented(page);
    let Some(count_at) = page.find("class=\"nb_reponses\"") else {
        if page.contains("id=\"F_search\"") {
            return Ok(Answer {
                total: 0,
                pages: 0,
                nodes: Vec::new(),
            });
        }
        return Err(unreadable(&page, "the answer has neither hits nor form"));
    };
    let count = &page[count_at..];
    let total = first_number(&strip_tags(
        &count[count.find('>').map_or(0, |end| end + 1)
            ..count.find("</span>").unwrap_or(count.len())],
    ))
    .unwrap_or(0);
    let pages = ["&page=", "&amp;page="]
        .iter()
        .flat_map(|marker| markup::split_after(&page, marker))
        .filter_map(|rest| first_number::<usize>(&rest[..rest.find('&').unwrap_or(rest.len())]))
        .max()
        .unwrap_or(1);
    let list = page
        .find("<div class='list'>")
        .map_or("", |start| &page[start..]);
    let nodes = markup::split_after(list, "<li>")
        .into_iter()
        .filter_map(node)
        .collect();
    Ok(Answer {
        total,
        pages,
        nodes,
    })
}

/// A notice's link to its register's images: the viewer's address and the
/// call number its text names (`- Voir : 4E 346`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Media {
    pub(super) url: String,
    pub(super) call_number: Option<String>,
}

/// The digitized document a node's notice links (`a.media_link`); `None`
/// for a notice without one. A page that is no notice is unreadable.
pub(super) fn media(page: &str) -> Result<Option<Media>, ResolveError> {
    if !page.contains("id=\"notice_detail\"") {
        return Err(unreadable(page, "the notice page has no notice"));
    }
    Ok(markup::split_after(page, "<a href=\"")
        .into_iter()
        .find_map(|link| {
            let tag = &link[..link.find('>')?];
            if !tag.contains("class=\"media_link\"") {
                return None;
            }
            let url = decode_entities(&tag[..tag.find('"')?]);
            let text = strip_tags(&link[tag.len() + 1..link.find("</a>").unwrap_or(link.len())]);
            let call_number = text
                .split_once(':')
                .map(|(_, call_number)| call_number.trim().to_owned())
                .filter(|call_number| !call_number.is_empty());
            Some(Media { url, call_number })
        }))
}
