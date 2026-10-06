//! Reading a Bach portal's pages: the classification scheme's entries, a
//! finding aid's tree, a register's viewer links and the viewer's image
//! list.

use serde::Deserialize;

use crate::ResolveError;
use crate::platform::markup::{self, strip_tags};

pub(super) fn unexpected(detail: &str) -> ResolveError {
    ResolveError::UnexpectedResponse(format!("bach: {detail}"))
}

/// An answer the adapter cannot read: an anti-bot page, or a changed shape.
fn unreadable(answer: &str, detail: &str) -> ResolveError {
    markup::unreadable(answer, format!("bach: {detail}"))
}

/// One finding aid the classification scheme lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Entry {
    /// The aid's identifier, `/document/<document>`.
    pub(super) document: String,
    /// The entry's title, `span.cdc_unittitle`.
    pub(super) title: String,
    /// The text of the link to the aid.
    pub(super) link: String,
}

/// The finding aids the classification scheme lists whose identifier
/// starts with `prefix`: one `li.standalone` each, a title and a link.
pub(super) fn entries(page: &str, prefix: &str) -> Result<Vec<Entry>, ResolveError> {
    if !page.contains("css-treeview") {
        return Err(unreadable(
            page,
            "the classification scheme lists no finding aid",
        ));
    }
    let marker = format!("<a href=\"/document/{prefix}");
    Ok(markup::split_after(page, "class=\"standalone\"")
        .into_iter()
        .filter_map(|item| {
            let item = &item[..item.find("</li>").unwrap_or(item.len())];
            let at = item.find(&marker)?;
            let link = &item[at + "<a href=\"/document/".len()..];
            let document = &link[..link.find('"')?];
            let text = &link[link.find('>')? + 1..];
            let text = strip_tags(&text[..text.find("</a>").unwrap_or(text.len())]);
            Some(Entry {
                document: document.to_owned(),
                title: element_text(item, "class=\"cdc_unittitle\"").unwrap_or_default(),
                link: text,
            })
        })
        .collect())
}

/// The text of the first element bearing `marker` up to its first closing
/// `</span>`, tags stripped.
fn element_text(html: &str, marker: &str) -> Option<String> {
    element_text_until(html, marker, "</span>")
}

/// The text of the first element bearing `marker` up to `end`, tags
/// stripped.
fn element_text_until(html: &str, marker: &str, end: &str) -> Option<String> {
    let at = html.find(marker)?;
    let rest = &html[at..];
    let rest = &rest[rest.find('>')? + 1..];
    let text = strip_tags(&rest[..rest.find(end).unwrap_or(rest.len())]);
    (!text.is_empty()).then_some(text)
}

/// One node of a finding aid's tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Node {
    /// The node's identifier within its aid, `de-12` or `tt2-3`: its show
    /// page is `/archives/show/<aid>_<id>` and its anchor `#<id>`.
    pub(super) id: String,
    /// 1 for the aid's top nodes.
    pub(super) depth: usize,
    pub(super) title: String,
    /// The dates the node shows beside its title, `1631-1721`.
    pub(super) date: Option<String>,
    /// The call number it shows, `GG 1`.
    pub(super) call_number: Option<String>,
    /// Whether the node has no child: a register.
    pub(super) leaf: bool,
}

/// A finding aid: its tree, under a root standing for the aid itself,
/// whose title (`Tables décennales de l'état civil`) says what every
/// register holds where the tree does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Aid {
    pub(super) root: Node,
    pub(super) nodes: Vec<Node>,
}

/// What the scan of a tree meets.
enum Token {
    Open,
    Close,
    Node,
}

/// The nodes of a finding aid's tree, `div.css-treeview`, in order: each a
/// `a.display_doc` link to `/archives/show/<document>_<id>` whose title is
/// `span.unit_title_lb`, beside `span.date` and `span.unitid`, its depth
/// the nesting of the `ul` lists around it.
pub(super) fn tree(page: &str, document: &str) -> Result<Aid, ResolveError> {
    let Some(start) = page.find("class=\"css-treeview\"") else {
        return Err(unreadable(page, "the finding aid has no tree"));
    };
    let root = Node {
        id: String::new(),
        depth: 0,
        title: page
            .find("id=\"cdcTitle\"")
            .and_then(|at| element_text_until(&page[at..], "<h2", "</h2>"))
            .unwrap_or_default(),
        date: None,
        call_number: None,
        leaf: false,
    };
    let link = format!("<a class=\"display_doc\" href=\"/archives/show/{document}_");
    let mut nodes: Vec<Node> = Vec::new();
    let mut depth = 0_usize;
    let mut rest = &page[start..];
    while let Some(at) = rest.find('<') {
        rest = &rest[at..];
        let token = if rest.starts_with("<ul>") || rest.starts_with("<ul ") {
            Token::Open
        } else if rest.starts_with("</ul>") {
            Token::Close
        } else if rest.starts_with(&link) {
            Token::Node
        } else {
            rest = &rest[1..];
            continue;
        };
        match token {
            Token::Open => depth += 1,
            Token::Close => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            Token::Node => {
                let anchor = &rest[link.len()..];
                let anchor = &anchor[..anchor.find("</a>").unwrap_or(anchor.len())];
                if let Some(node) = node(anchor, depth) {
                    if let Some(previous) = nodes.last_mut() {
                        previous.leaf = previous.depth >= depth;
                    }
                    nodes.push(node);
                }
            }
        }
        rest = &rest[1..];
    }
    Ok(Aid { root, nodes })
}

/// A node from its link's markup after the aid's identifier.
fn node(anchor: &str, depth: usize) -> Option<Node> {
    let id = &anchor[..anchor.find('"')?];
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return None;
    }
    let title = element_text(anchor, "class=\"unit_title_lb\"").unwrap_or_default();
    let date = element_text(anchor, "class=\"date\"")
        .map(|date| date.trim_start_matches(['•', ' ']).trim().to_owned())
        .filter(|date| !date.is_empty());
    Some(Node {
        id: id.to_owned(),
        depth,
        title,
        date,
        call_number: element_text(anchor, "class=\"unitid\""),
        leaf: true,
    })
}

/// The viewer links of a register's show page, `figure#relative_documents`:
/// none for a register without images. A link to a viewer other than the
/// settings' is a changed portal.
pub(super) fn viewer_links(page: &str, viewer: &str) -> Result<Vec<String>, ResolveError> {
    // The link placing the register in its finding aid.
    if !page.contains("treeLinkResults") {
        return Err(unreadable(page, "the register's page has another shape"));
    }
    let Some(at) = page.find("<figure id=\"relative_documents\"") else {
        return Ok(Vec::new());
    };
    let figure = &page[at..];
    let figure = &figure[..figure.find("</figure>").unwrap_or(figure.len())];
    let series = format!("{viewer}/series/");
    let mut links: Vec<String> = Vec::new();
    for link in markup::attributes(figure, "href") {
        if !link.contains("/series/") {
            continue;
        }
        if !link.starts_with(&series) {
            return Err(unexpected("a viewer link off the settings' viewer"));
        }
        if !links.contains(&link) {
            links.push(link);
        }
    }
    Ok(links)
}

/// The viewer's image list of a register.
#[derive(Debug, Deserialize)]
struct ImageList {
    count: usize,
    data: Vec<Image>,
}

#[derive(Debug, Deserialize)]
struct Image {
    name: String,
}

/// The image names of a register, in view order, from the viewer's list:
/// `count` images, `data[i].name` the file of view `i + 1`.
pub(super) fn image_names(answer: &str) -> Result<Vec<String>, ResolveError> {
    let list: ImageList = serde_json::from_str(answer)
        .map_err(|_| unreadable(answer, "the viewer's image list has another shape"))?;
    if list.data.len() != list.count || list.data.iter().any(|image| image.name.is_empty()) {
        return Err(unexpected(
            "the viewer's image list does not name every image",
        ));
    }
    Ok(list.data.into_iter().map(|image| image.name).collect())
}

/// The image names a viewer link spans with its `s` and `e` parameters,
/// numbered alike (`…_0001.jpg` to `…_0071.jpg`): the names between them,
/// the counter written with the same digits. `None` when the link names no
/// range or two names that differ elsewhere than in their counter.
pub(super) fn range_names(link: &str) -> Option<Vec<String>> {
    let query = link.split_once('?')?.1;
    let parameter = |name: &str| {
        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == name).then(|| percent_decoded(value))
        })
    };
    let (first, last) = (parameter("s")?, parameter("e")?);
    let (head, first_number, tail) = counter(&first)?;
    let (last_head, last_number, last_tail) = counter(&last)?;
    let width = first.len() - head.len() - tail.len();
    if head != last_head || tail != last_tail || last_number < first_number {
        return None;
    }
    Some(
        (first_number..=last_number)
            .map(|number| format!("{head}{number:0width$}{tail}"))
            .collect(),
    )
}

/// A file name around its last run of digits: `(head, number, tail)`.
fn counter(name: &str) -> Option<(&str, u32, &str)> {
    let end = name.rfind(|c: char| c.is_ascii_digit())? + 1;
    let start = name[..end]
        .rfind(|c: char| !c.is_ascii_digit())
        .map_or(0, |at| at + 1);
    let number = name[start..end].parse().ok()?;
    Some((&name[..start], number, &name[end..]))
}

/// `%XX` sequences decoded, for a query value written percent-encoded.
fn percent_decoded(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[index], hex) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                index += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}
