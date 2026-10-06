//! Reading the pages of a "salle virtuelle": the alphabetical list of
//! localities, a locality's lots of images, the search form of a site that
//! searches by criteria, and its answer.
//!
//! Every list is a `table.v2Liste` of rows whose cells the adapter reads
//! by position, the identifiers out of the `javascript:` links the cells
//! carry. The markup is upper case (`<TR>`, `<TD>`), written by an ASP.NET
//! WebForms application; the scans ignore case.

use crate::ResolveError;
use crate::platform::markup::{self, decode_entities, first_number, strip_tags};

fn unreadable(page: &str, detail: &str) -> ResolveError {
    markup::unreadable(page, format!("visualys: {detail}"))
}

/// The rows of the page's `v2Liste` tables, each as its cells' markup.
fn rows(page: &str) -> Vec<Vec<&str>> {
    let lower = page.to_ascii_lowercase();
    let mut rows = Vec::new();
    let mut from = 0;
    while let Some(at) = lower[from..].find("class=\"v2liste\"") {
        let start = from + at;
        let end = lower[start..]
            .find("</table>")
            .map_or(page.len(), |end| start + end);
        let table = &lower[start..end];
        let mut row_from = 0;
        while let Some(row_at) = table[row_from..].find("<tr") {
            let row_start = row_from + row_at;
            let row_end = table[row_start + 3..]
                .find("<tr")
                .map_or(table.len(), |next| row_start + 3 + next);
            rows.push(cells(&page[start + row_start..start + row_end]));
            row_from = row_end;
        }
        from = end;
    }
    rows
}

/// The markup inside each `<TD>` of a row.
fn cells(row: &str) -> Vec<&str> {
    let lower = row.to_ascii_lowercase();
    let mut cells = Vec::new();
    let mut from = 0;
    while let Some(at) = lower[from..].find("<td") {
        let open = from + at;
        let Some(content) = lower[open..].find('>').map(|end| open + end + 1) else {
            break;
        };
        let close = lower[content..]
            .find("</td>")
            .map_or(row.len(), |end| content + end);
        cells.push(&row[content..close]);
        from = close;
    }
    cells
}

/// The first argument of a `javascript:<function>('…')` link in `cell`.
fn link_argument(cell: &str, function: &str) -> Option<String> {
    let marker = format!("javascript:{function}('");
    let start = cell.find(&marker)? + marker.len();
    let id = &cell[start..start + cell[start..].find('\'')?];
    (!id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())).then(|| id.to_owned())
}

/// A row of the alphabetical list: a locality, or one of its parishes or
/// bodies listed apart (`Saint-Exemple`, `Hospice`), whose lots open with
/// its identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Locality {
    pub(super) id: String,
    /// The name as listed: `Bourg (Le)`.
    pub(super) name: String,
    /// The parish or body the row is for, if any.
    pub(super) parish: Option<String>,
}

/// The rows of the alphabetical list of an initial (`commune.aspx`). A page
/// without the list's initials is not the list.
pub(super) fn localities(page: &str) -> Result<Vec<Locality>, ResolveError> {
    if !page.contains("javascript:lettre(") {
        return Err(unreadable(page, "the locality list has no initials"));
    }
    Ok(rows(page)
        .into_iter()
        .filter_map(|cells| {
            let name_cell = cells.get(1)?;
            let id = link_argument(name_cell, "lot")?;
            let name = strip_tags(name_cell);
            let parish = cells
                .get(2)
                .map(|cell| strip_tags(cell))
                .filter(|parish| !parish.is_empty());
            (!name.is_empty()).then_some(Locality { id, name, parish })
        })
        .collect())
}

/// The two blocks of a locality's lots, parish registers (`r=0`) and civil
/// status (`r=1`), each listed only while the session holds it open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Blocks {
    /// `None` when the locality has no such block.
    pub(super) parish: Option<bool>,
    pub(super) civil: Option<bool>,
}

/// Which blocks the lots page has, and which are open: the block's icon is
/// `tree_moins` while it is open, `tree_plus` while it is closed.
pub(super) fn blocks(page: &str) -> Result<Blocks, ResolveError> {
    if !page.contains("openBloc(") && !page.contains("id=\"LabelMessage\"") {
        return Err(unreadable(page, "the lots page has no blocks"));
    }
    let open = |icon: &str| {
        let marker = format!("id=\"{icon}\"");
        let at = page.find(&marker)?;
        let tag_start = page[..at].rfind('<')?;
        Some(page[tag_start..at].contains("tree_moins"))
    };
    Ok(Blocks {
        parish: open("ImageRP"),
        civil: open("ImageEC"),
    })
}

/// One lot of images: a register, or the part of one, of one locality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Lot {
    pub(super) id: String,
    /// The first and last years, as listed.
    pub(super) period: String,
    /// The acts, as codes: `BMS`, `N`, `TD`.
    pub(super) act: String,
    pub(super) images: Option<u16>,
}

/// The lots the open blocks list: rows whose first year opens the lot's
/// thumbnails (`mini('<lot>','1','1')`), then the last year, the act and the
/// image count.
pub(super) fn lots(page: &str) -> Vec<Lot> {
    rows(page)
        .into_iter()
        .filter_map(|cells| {
            let start = cells
                .iter()
                .position(|cell| cell.contains("javascript:mini("))?;
            let id = link_argument(cells[start], "mini")?;
            let first = strip_tags(cells[start]);
            let last = strip_tags(cells.get(start + 1)?);
            Some(Lot {
                id,
                period: format!("{first}-{last}"),
                act: strip_tags(cells.get(start + 2)?),
                images: first_number(&strip_tags(cells.get(start + 3)?)),
            })
        })
        .collect()
}

/// A WebForms search form: where it posts, its hidden state, and the
/// options of its selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Form {
    pub(super) action: String,
    pub(super) hidden: Vec<(String, String)>,
}

/// The page's search form (`frmRecherche`), its action relative to `base`.
pub(super) fn form(page: &str, base: &str) -> Result<Form, ResolveError> {
    let missing = || unreadable(page, "the search page has no search form");
    let start = page.find("id=\"frmRecherche\"").ok_or_else(missing)?;
    let tag_start = page[..start].rfind('<').ok_or_else(missing)?;
    let tag = &page[tag_start..start + page[start..].find('>').ok_or_else(missing)?];
    let action = markup::attribute(tag, "action").ok_or_else(missing)?;
    let action = action.strip_prefix("./").unwrap_or(&action);
    if action.contains(['/', '\\', '#', ' ']) || action.contains("//") {
        return Err(unreadable(page, "the search form posts elsewhere"));
    }
    let end = page[start..]
        .find("</form>")
        .map_or(page.len(), |end| start + end);
    let body = &page[tag_start..end];
    let hidden = markup::split_after(body, "<input type=\"hidden\"")
        .into_iter()
        .filter_map(|input| {
            let tag = &input[..input.find('>')?];
            Some((
                markup::attribute(tag, "name")?,
                markup::attribute(tag, "value").unwrap_or_default(),
            ))
        })
        .collect();
    Ok(Form {
        action: format!("{base}/{action}"),
        hidden,
    })
}

/// The values a select of the page offers, its empty choice left out.
pub(super) fn options(page: &str, name: &str) -> Vec<String> {
    let marker = format!("<select name=\"{name}\"");
    let Some(start) = page.find(&marker) else {
        return Vec::new();
    };
    let end = page[start..]
        .find("</select>")
        .map_or(page.len(), |end| start + end);
    markup::split_after(&page[start..end], "<option")
        .into_iter()
        .filter_map(|option| {
            let tag = &option[..option.find('>')?];
            markup::attribute(tag, "value").filter(|value| !value.trim().is_empty())
        })
        .map(|value| decode_entities(&value))
        .collect()
}

/// One register a search by criteria lists: its office, year, call number
/// and lot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Volume {
    pub(super) lot: String,
    pub(super) office: String,
    pub(super) year: String,
    pub(super) call_number: Option<String>,
}

/// The registers of a search's answer: rows whose last cell opens a lot
/// (`mini('<lot>')`) after the office, year and call number cells. The
/// answer must still hold the form, which it posts back to.
pub(super) fn volumes(page: &str) -> Result<Vec<Volume>, ResolveError> {
    if !page.contains("id=\"frmRecherche\"") {
        return Err(unreadable(page, "the search's answer has no form"));
    }
    Ok(rows(page)
        .into_iter()
        .filter_map(|cells| {
            let (link, before) = cells.split_last()?;
            let lot = link_argument(link, "mini")?;
            let [.., office, year, call_number] = before else {
                return None;
            };
            Some(Volume {
                lot,
                office: strip_tags(office),
                year: strip_tags(year),
                call_number: Some(strip_tags(call_number)).filter(|text| !text.is_empty()),
            })
        })
        .collect())
}
