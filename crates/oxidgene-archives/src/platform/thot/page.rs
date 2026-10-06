//! Reading THOT's pages: the cookie check, the search form, the result
//! list, the viewer page and its slide file.
//!
//! The pages are windows-1252, which the transports read as UTF-8: every
//! accented letter arrives as U+FFFD. The adapter reads the parts that are
//! ASCII — the form's labels and values, call numbers, periods, the
//! viewer's arguments — and reads the act words with U+FFFD as `e`, the
//! accented letter of every French act word (`Baptêmes`, `Décès`,
//! `Sépultures`).

use super::unexpected;
use crate::ResolveError;
use crate::platform::markup::{self, attribute, first_number, fold, split_after, strip_tags};
use crate::platform::select::{act_code, number_range};

/// The address the portal's cookie check sends the browser to:
/// `<base>/FrmAccueilDroite.asp?checkCookie=<timestamp>`, from the script of
/// the first page.
pub(super) fn cookie_check(page: &str, base: &str) -> Option<String> {
    let marker = format!("{base}/FrmAccueilDroite.asp?checkCookie=");
    let start = page.find(&marker)?;
    let rest = &page[start + marker.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then(|| format!("{marker}{digits}"))
}

/// Whether the first page belongs to a session that passed the cookie
/// check: it sends the browser on to the summary,
/// `window.open( "FrmSommaireFrame.asp", "_top" )`.
pub(super) fn session_open(page: &str) -> bool {
    page.contains("\"FrmSommaireFrame.asp\"")
}

/// What a page the portal answered without the session it expects says.
pub(super) fn session_refused(page: &str) -> Option<&'static str> {
    if page.contains("bloque les cookies") {
        Some("the portal refused the session cookie")
    } else if page.contains("Votre session a expir") {
        Some("the portal's session expired")
    } else {
        None
    }
}

/// The search form, `FormRecherche`, of a module.
pub(super) struct Form<'p> {
    html: &'p str,
}

impl<'p> Form<'p> {
    /// The form of the page, or `None` when the page has none.
    pub(super) fn of(page: &'p str) -> Option<Self> {
        let start = page.find("name=\"FormRecherche\"")?;
        let html = &page[start..];
        let end = html.find("</form>").unwrap_or(html.len());
        Some(Self { html: &html[..end] })
    }

    /// The hidden inputs and their values, in order: what the page's own
    /// submission sends besides the criteria it fills.
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

    /// The values a criterion's list offers: its `select`'s options
    /// (`txt_CIN_LISTE<k>`), or its checkboxes (`cbx_txt_CIN_CH<k>`).
    pub(super) fn values(&self, criterion: u8) -> Vec<String> {
        let select = format!("id=\"txt_CIN_LISTE{criterion}\"");
        if let Some(start) = self.html.find(&select) {
            let list = &self.html[start..];
            let list = &list[..list.find("</select>").unwrap_or(list.len())];
            return markup::attributes(list, "value")
                .into_iter()
                .filter(|value| !value.trim().is_empty())
                .collect();
        }
        let name = format!("name=\"{}\"", checkbox(criterion));
        split_after(self.html, "<input")
            .into_iter()
            .filter(|tag| tag[..tag.find('>').unwrap_or(tag.len())].contains(&name))
            .filter_map(|tag| attribute(&tag[..tag.find('>').unwrap_or(tag.len())], "value"))
            .collect()
    }

    /// Whether a criterion is a set of checkboxes rather than a list.
    pub(super) fn has_checkboxes(&self, criterion: u8) -> bool {
        self.html
            .contains(&format!("name=\"{}\"", checkbox(criterion)))
    }

    /// Whether the form has an input of that name.
    pub(super) fn has_input(&self, name: &str) -> bool {
        self.html.contains(&format!("name=\"{name}\""))
    }
}

/// The checkboxes' name of a criterion.
pub(super) fn checkbox(criterion: u8) -> String {
    format!("cbx_txt_CIN_CH{criterion}")
}

/// The text of a page with each U+FFFD, an accented letter the transport
/// lost, read as `e`.
pub(super) fn repaired(text: &str) -> String {
    text.replace('\u{fffd}', "e")
}

/// What a column of the result list holds, by its heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    CallNumber,
    Locality,
    Title,
    Act,
    Period,
    Other,
}

impl Column {
    fn of(heading: &str) -> Self {
        let heading = fold(&repaired(heading));
        if heading.starts_with("cote") {
            Self::CallNumber
        } else if heading.starts_with("commune") || heading.starts_with("bureau") {
            Self::Locality
        } else if heading.starts_with("intitule") {
            Self::Title
        } else if heading.starts_with("type d acte") || heading.starts_with("type de document") {
            Self::Act
        } else if heading.starts_with("date") {
            Self::Period
        } else {
            Self::Other
        }
    }
}

/// The arguments of a row's viewer link, `openLot(<idfic>, <idlot>, <ref>,
/// <base>, '', <flag>)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Lot {
    pub(super) idfic: String,
    pub(super) idlot: String,
    pub(super) reference: String,
    pub(super) application: String,
}

impl Lot {
    /// The link's arguments, or `None` for a row that opens no viewer: a
    /// restricted register (flag `1`), one not digitized (`cfecFichier`) or
    /// without images (no lot).
    fn parse(row: &str) -> Option<Self> {
        let start = row.find("openLot(")? + "openLot(".len();
        let arguments = &row[start..];
        let arguments = &arguments[..arguments.find(')')?];
        let values: Vec<String> = arguments
            .split(',')
            .map(|value| value.trim().trim_matches('\'').to_owned())
            .collect();
        let [idfic, idlot, reference, application, _, flag] = values.as_slice() else {
            return None;
        };
        let identifier = |text: &str| {
            !text.is_empty()
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        };
        let usable = flag != "1"
            && idlot != "cfecFichier"
            && [idfic, idlot, reference, application]
                .iter()
                .all(|value| identifier(value));
        usable.then(|| Self {
            idfic: idfic.clone(),
            idlot: idlot.clone(),
            reference: reference.clone(),
            application: application.clone(),
        })
    }
}

/// One row of the result list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Row {
    pub(super) call_number: Option<String>,
    /// The locality cell, as the portal writes it, where the list has one.
    pub(super) locality: Option<String>,
    pub(super) title: Option<String>,
    /// The act cell, or else the title: what names the kinds of acts.
    pub(super) acts: Option<String>,
    pub(super) period: Option<String>,
    pub(super) lot: Lot,
}

impl Row {
    /// The act code a register's text names, as selection compares it:
    /// `TD` for any table (`Tables Baptêmes Mariages Sépultures`, `Tables
    /// décennales`), the kinds otherwise (`NMD` for `Naissances/Mariages/
    /// Décès`).
    pub(super) fn act_code(&self) -> Option<String> {
        let text = repaired(self.acts.as_deref()?);
        if fold(&text).split(' ').any(|word| word.starts_with("table")) {
            return Some("TD".to_owned());
        }
        act_code(&text, false)
    }

    /// The first and last matricule numbers the title spans, as a military
    /// register's volume shows them (`numéros matricules 1-500`).
    pub(super) fn numbers(&self) -> Option<(u32, u32)> {
        number_range(&repaired(self.title.as_deref()?), true)
    }
}

/// One page of the result list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Results {
    /// The number of records the search found, all pages together.
    pub(super) total: usize,
    pub(super) rows: Vec<Row>,
    /// The number of pages, the first included.
    pub(super) pages: usize,
}

/// The result list's page: the count it announces, its rows that open a
/// viewer, and its number of pages.
pub(super) fn results(page: &str) -> Result<Results, ResolveError> {
    let Some(count) = markup::text_after(page, "<div class=\"resultatrech\">") else {
        return Err(match session_refused(page) {
            Some(detail) => unexpected(detail),
            None => markup::unreadable(page, "thot: the result page has no count".to_owned()),
        });
    };
    let folded = fold(&repaired(&count));
    let total = if folded.starts_with("aucune") {
        0
    } else if folded.starts_with("une seule") {
        1
    } else {
        first_number(&count).ok_or_else(|| unexpected("the result count has no number"))?
    };
    if total == 0 {
        return Ok(Results {
            total,
            rows: Vec::new(),
            pages: 1,
        });
    }
    let table = page
        .find("<table class=\"tabListe")
        .map(|start| &page[start..])
        .ok_or_else(|| unexpected("the result page has no list"))?;
    let table = &table[..table.find("</table>").unwrap_or(table.len())];
    let head = &table[..table.find("<tbody").unwrap_or(table.len())];
    let columns: Vec<Column> = elements(head, "th")
        .into_iter()
        .map(|heading| Column::of(&strip_tags(&format!("<th{}", until(heading, "</th>")))))
        .collect();
    if !columns.contains(&Column::CallNumber) || !columns.contains(&Column::Period) {
        return Err(unexpected(
            "the result list lacks its call number or date column",
        ));
    }
    let rows = split_after(table, "<tr class=\"l")
        .into_iter()
        .filter_map(|row| {
            let row = &row[..row.find("</tr>").unwrap_or(row.len())];
            let cells: Vec<String> = elements(row, "td")
                .into_iter()
                .map(|cell| strip_tags(&format!("<td{}", until(cell, "</td>"))))
                .collect();
            let cell = |wanted: Column| {
                columns
                    .iter()
                    .position(|column| *column == wanted)
                    .and_then(|at| cells.get(at))
                    .filter(|text| !text.is_empty())
                    .cloned()
            };
            let title = cell(Column::Title);
            Some(Row {
                call_number: cell(Column::CallNumber),
                locality: cell(Column::Locality),
                acts: cell(Column::Act).or_else(|| title.clone()),
                title,
                period: cell(Column::Period),
                lot: Lot::parse(row)?,
            })
        })
        .collect();
    let pages = split_after(page, "RechDoc=1&amp;page=")
        .into_iter()
        .filter_map(first_number::<usize>)
        .max()
        .unwrap_or(1);
    Ok(Results { total, rows, pages })
}

/// What follows each opening tag of `name` in `html`: `<th scope="col">`,
/// `<th>`, never `<thead>`.
fn elements<'h>(html: &'h str, name: &str) -> Vec<&'h str> {
    split_after(html, &format!("<{name}"))
        .into_iter()
        .filter(|rest| rest.starts_with(|c: char| c == '>' || c.is_ascii_whitespace()))
        .collect()
}

/// `text` up to `end`, or all of it.
fn until<'t>(text: &'t str, end: &str) -> &'t str {
    &text[..text.find(end).unwrap_or(text.len())]
}

/// The path of a register's slide file, from its viewer page's
/// `zSlidePath=<path>`.
pub(super) fn slide_path(page: &str, base: &str) -> Option<String> {
    let start = page.find("zSlidePath=")? + "zSlidePath=".len();
    let rest = &page[start..];
    let path = &rest[..rest.find(['"', '&', '\''])?];
    let clean = path.starts_with(&format!("{base}/"))
        && !path.contains(['?', '#', ' ', '\\'])
        && path.ends_with(".xml");
    clean.then(|| path.to_owned())
}

/// A register's slide file: one `<SLIDE>` per view, in order, and where the
/// portal publishes them, each view's persistent address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Slides {
    pub(super) count: usize,
    /// The ARK base, `<origin><base>/ark:/<naan>`, from `URLARK`.
    pub(super) ark: Option<String>,
    /// Each view's `LIENARK`, `<name>/<register>/<view>`.
    pub(super) links: Vec<String>,
}

pub(super) fn slides(file: &str) -> Result<Slides, ResolveError> {
    if !file.contains("<SLIDEDATA") {
        return Err(markup::unreadable(
            file,
            "thot: the slide file has no SLIDEDATA".to_owned(),
        ));
    }
    let views = split_after(file, "<SLIDE ");
    if views.is_empty() {
        return Err(unexpected("the slide file lists no view"));
    }
    let links: Vec<String> = views
        .iter()
        .filter_map(|slide| attribute(&slide[..slide.find('>').unwrap_or(slide.len())], "LIENARK"))
        .collect();
    Ok(Slides {
        count: views.len(),
        ark: attribute(file, "URLARK"),
        links,
    })
}
