//! Reading the wizard's pages and the search's answers.
//!
//! The pages are ISO-8859-1: a transport decoding them as UTF-8 leaves a
//! U+FFFD for each accented letter, so labels are compared by
//! [`is_named`](crate::platform::markup::is_named) and read with that in
//! mind. Every address the adapter follows is rebuilt from the numbers it
//! reads, never taken from the page as written.

use super::{Settings, unexpected};
use crate::ResolveError;
use crate::platform::markup::{self, decode_entities, first_number, fold, lossy_letters};
use crate::platform::select::{act_code, number_range};

/// One choice of a wizard's list: the address that makes it, and its label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Link {
    pub(super) path: String,
    pub(super) label: String,
}

/// One register an answer lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub(super) title: String,
    pub(super) call_number: Option<String>,
    pub(super) period: Option<String>,
    /// The viewer's path up to its window size, for a register with images.
    pub(super) viewer: Option<String>,
}

/// The error of a page the adapter cannot read: an anti-bot page, or a
/// changed shape.
pub(super) fn unreadable(page: &str, detail: &str) -> ResolveError {
    markup::unreadable(page, format!("gaia: {detail}"))
}

/// The wizard's address a page names for `mode` (`A` the year form, `F`
/// the step's "search all" form, `T` the search), rebuilt on the settings'
/// path: `requeteConstructor/<theme>/<step>/<mode>/0/0`.
pub(super) fn action(page: &str, settings: &Settings, mode: char) -> Option<String> {
    let marker = format!("requeteConstructor/{}/", settings.theme);
    markup::split_after(page, &marker)
        .into_iter()
        .find_map(|rest| {
            let (step, rest) = rest.split_once('/')?;
            let step: u16 = step.parse().ok()?;
            rest.strip_prefix(&format!("{mode}/0/0"))
                .filter(|after| !after.starts_with(|c: char| c.is_ascii_alphanumeric()))
                .map(|_| settings.step(step, mode, "0", "0"))
        })
}

/// The choices of a step's list (`a.color_liens`): localities, kinds of
/// registers, years. A page with no wizard at all is unreadable.
pub(super) fn links(page: &str, settings: &Settings) -> Result<Vec<Link>, ResolveError> {
    if !page.contains("rechercheTheme") {
        return Err(unreadable(
            page,
            "a page of another shape than the wizard's",
        ));
    }
    let marker = format!("requeteConstructor/{}/", settings.theme);
    Ok(markup::split_after(page, "class=\"color_liens\"")
        .into_iter()
        .filter_map(|fragment| {
            let tag_end = fragment.find('>')?;
            let href = markup::attribute(&fragment[..tag_end], "href")?;
            let (_, rest) = href.split_once(&marker)?;
            let mut parts = rest.split('/');
            let step: u16 = parts.next()?.parse().ok()?;
            let id = parts.next().filter(|mode| *mode == "A").and(parts.next())?;
            if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            let text = &fragment[tag_end + 1..];
            let text = decode_entities(&text[..text.find('<').unwrap_or(text.len())]);
            let label = text.split('\u{a0}').next().unwrap_or_default().trim();
            (!label.is_empty()).then(|| Link {
                path: settings.step(step, 'A', id, "x"),
                label: label.to_owned(),
            })
        })
        .collect())
}

/// A locality list's label, read: the name a citation may give, and what
/// follows it, a parish or a body (`Exampleville, paroisse Saint-Exemple`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Entry<'l> {
    pub(super) name: &'l str,
    pub(super) detail: &'l str,
}

/// Reads a locality's label: without the settings' `prefix`; cut at a
/// comma, a full stop or a bracketed note (`Exampleville, paroisse …`,
/// `Exampleville. - Subdivision …`, `EXAMPLEVILLE [jusqu'en 1789]`); without
/// the parenthesised qualifiers other than a leading article written after
/// the name (`Exampleville (après 1793)`, `Exampleville (Département)`, but
/// `Bourg (Le)` kept).
pub(super) fn entry<'l>(label: &'l str, prefix: Option<&str>) -> Entry<'l> {
    let label = prefix
        .and_then(|prefix| {
            label
                .get(..prefix.len())
                .filter(|start| start.eq_ignore_ascii_case(prefix))
                .map(|_| &label[prefix.len()..])
        })
        .unwrap_or(label)
        .trim();
    let cut = [", ", ". ", " ["]
        .iter()
        .filter_map(|separator| label.find(separator))
        .min();
    let (mut name, detail) = match cut {
        Some(at) if label[at..].starts_with(" [") => (&label[..at], ""),
        Some(at) => (&label[..at], label[at + 2..].trim()),
        None => (label, ""),
    };
    name = name.trim_end_matches('.').trim();
    while let Some((before, qualifier)) = name
        .strip_suffix(')')
        .and_then(|rest| rest.rsplit_once(" ("))
    {
        if before.is_empty() || is_article(qualifier) {
            break;
        }
        name = before.trim_end();
    }
    Entry { name, detail }
}

fn is_article(text: &str) -> bool {
    ["le", "la", "les", "l'", "l\u{2019}"]
        .iter()
        .any(|article| text.trim().eq_ignore_ascii_case(article))
}

/// Whether a label's detail names the cited parish: its letters, U+FFFD
/// standing for any one.
pub(super) fn contains_letters(detail: &str, parish: &[char]) -> bool {
    if parish.is_empty() {
        return false;
    }
    let text = lossy_letters(detail);
    text.windows(parish.len()).any(|window| {
        window
            .iter()
            .zip(parish)
            .all(|(known, letter)| known.is_none_or(|known| known == *letter))
    })
}

/// The locality a title starts with, before its first comma or full stop:
/// `Exampleville` in `Exampleville, matricules n° 1-500.`.
pub(super) fn title_locality(title: &str) -> &str {
    title.split([',', '.']).next().unwrap_or_default().trim()
}

/// The acts a title names, as `select` reads them: `TD` for a table, the
/// kinds of words or codes (`Naissances, mariages, décès.`, `BMS`, `N + T`)
/// otherwise. The vowel the decoding lost in `décès` and `sépultures` is an
/// `e`, which every accented letter of those words is.
pub(super) fn title_act(title: &str) -> Option<String> {
    let text = title.replace('\u{fffd}', "e");
    if fold(&text).split(' ').any(|word| word.starts_with("tabl")) {
        return Some("TD".to_owned());
    }
    act_code(&text, true)
}

/// The act or matricule numbers a title spans: `matricules n° 1-496`. The
/// character the decoding lost after an `n` is the `°` of `n°`.
pub(crate) fn title_numbers(title: &str) -> Option<(u32, u32)> {
    number_range(&title.replace('\u{fffd}', "°"), true)
}

/// The registers of a search's answer and the count it states. A register
/// without the viewer's link has no images; it is kept, so that pages are
/// counted, but has no `viewer`.
pub(super) fn rows(answer: &str, settings: &Settings) -> Result<(Vec<Row>, usize), ResolveError> {
    let total = count(answer).ok_or_else(|| unreadable(answer, "an answer without its count"))?;
    let rows: Vec<Row> = answer
        .split("<div class=\"spacer\"")
        .filter(|chunk| chunk.contains("class=\"reponsemot\""))
        .map(|chunk| row(chunk, settings))
        .collect::<Result<_, _>>()?;
    if rows.is_empty() && total > 0 {
        return Err(unexpected("an answer counting registers lists none"));
    }
    Ok((rows, total))
}

/// The count after `LISTE DES REPONSES`: `2 réponses`, `1 réponse`,
/// `Aucune réponse`.
fn count(answer: &str) -> Option<usize> {
    let (_, rest) = answer.split_once("LISTE DES REPONSES")?;
    let cell = rest.split("<td").nth(1)?;
    let text = markup::strip_tags(&format!("<td{}", cell.split("</td>").next()?));
    first_number(&text).or_else(|| text.contains("Aucune").then_some(0))
}

fn row(chunk: &str, settings: &Settings) -> Result<Row, ResolveError> {
    let title = chunk
        .split_once("<a id=\"openDetail")
        .and_then(|(_, rest)| rest.split_once('>'))
        .map(|(_, rest)| markup::strip_tags(rest.split("</a>").next().unwrap_or_default()))
        .ok_or_else(|| unexpected("a register without its title"))?;
    // `<span class="reponsemot" …><span>CALL NUMBER</span><span …>PERIOD</span>`
    let cells: Vec<String> = chunk
        .split_once("<span class=\"reponsemot\"")
        .map(|(_, rest)| {
            markup::split_after(rest, "<span")
                .into_iter()
                .take(2)
                .map(|cell| {
                    let text = cell.split_once('>').map_or("", |(_, text)| text);
                    markup::strip_tags(text.split("</span>").next().unwrap_or_default())
                })
                .collect()
        })
        .unwrap_or_default();
    let cell = |index: usize| {
        cells
            .get(index)
            .filter(|text| !text.is_empty())
            .map(String::to_owned)
    };
    Ok(Row {
        title,
        call_number: cell(0),
        period: cell(1),
        viewer: viewer(chunk, settings),
    })
}

/// The viewer's path a register's paperclip opens:
/// `window.open('<base>/index.php/docnumViewer/calculHierarchieDocNum/<unit>/<hierarchy>/'+screen.height…`.
fn viewer(chunk: &str, settings: &Settings) -> Option<String> {
    const MARKER: &str = "/index.php/docnumViewer/calculHierarchieDocNum/";
    let (_, rest) = chunk.split_once(MARKER)?;
    let path = rest.split('\'').next()?;
    let mut parts = path.trim_end_matches('/').split('/');
    let unit = parts.next()?;
    let hierarchy = parts.next()?;
    let unit_ok = !unit.is_empty() && unit.bytes().all(|byte| byte.is_ascii_digit());
    let hierarchy_ok = !hierarchy.is_empty()
        && hierarchy
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b':');
    (unit_ok && hierarchy_ok && parts.next().is_none())
        .then(|| format!("{}{MARKER}{unit}/{hierarchy}/", settings.base))
}

/// The number of views of a viewer's page: one object per view in its
/// `docs` array.
#[cfg(any(test, feature = "live"))]
pub(super) fn views(viewer: &str) -> Option<usize> {
    let (_, docs) = viewer.split_once("docs :")?;
    let docs = docs.split("numPage").next()?;
    let count = docs.matches("\"typeMedia\"").count();
    (count > 0).then_some(count)
}
