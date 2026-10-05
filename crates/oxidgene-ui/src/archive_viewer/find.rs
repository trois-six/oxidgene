//! The "Find in the archives" dialog (docs/archives.md §6.5): a citation
//! whose archive OxidGene recognized but whose act or locality it could not
//! read, prefilled with what it did read, for the reader to complete.
//!
//! Nothing is written unless the reader asks: a checkbox adds the completed
//! parts to the citation's page, in the archive's language, so that the
//! citation opens directly next time.

use dioxus::prelude::*;
use oxidgene_archives::{Act, Archive, SuppliedParts};

use super::{ArchiveFind, ArchiveLink};
use crate::components::modal::Modal;
use crate::i18n::{I18n, use_i18n};

/// The document kinds an archive's collections hold, each once, in
/// catalogue order: a register's kinds one by one, tables and series as
/// they are.
pub(super) fn kinds_held(archive: &Archive) -> Vec<Act> {
    let mut kinds: Vec<Act> = Vec::new();
    for act in archive
        .collections
        .iter()
        .flat_map(|collection| &collection.acts)
    {
        let singles: Vec<Act> = match act {
            Act::Register(held) => held.iter().map(|kind| Act::Register(vec![*kind])).collect(),
            other => vec![other.clone()],
        };
        for single in singles {
            if !kinds.contains(&single) {
                kinds.push(single);
            }
        }
    }
    kinds
}

/// A document kind in the interface language: `Births`, `Decennial
/// tables`, `Censuses`; a combined register names each of its kinds.
pub(super) fn kind_label(i18n: &I18n, act: &Act) -> String {
    let key = |code: &str| format!("archive_viewer.kind.{}", code.to_lowercase());
    match act {
        Act::Register(kinds) => kinds
            .iter()
            .map(|kind| i18n.t(&key(&kind.letter().to_string())))
            .collect::<Vec<_>>()
            .join(" / "),
        Act::Table(code) if KNOWN_TABLES.contains(&code.as_str()) => i18n.t(&key(code)),
        Act::Table(code) => i18n.t_args("archive_viewer.kind.table", &[("code", code)]),
        Act::Series(series) => i18n.t(&key(series.code())),
    }
}

/// The tables with a name of their own.
const KNOWN_TABLES: [&str; 5] = ["TD", "TB", "TM", "TS", "TN"];

#[component]
pub(super) fn FindInArchives(
    find: ArchiveFind,
    /// The completed link, and the parts to add to the citation's page when
    /// the reader asked to keep them.
    on_find: EventHandler<(ArchiveLink, Option<String>)>,
    on_close: EventHandler<()>,
) -> Element {
    let i18n = use_i18n();
    let found = find.found.clone();
    let mut locality = use_signal(|| found.locality.clone().unwrap_or_default());
    let mut act = use_signal(|| {
        found
            .act
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default()
    });
    let mut year = use_signal(|| found.year.map(|year| year.to_string()).unwrap_or_default());
    let mut view = use_signal(|| {
        found
            .views
            .first()
            .map(|view| view.view.to_string())
            .unwrap_or_default()
    });
    let mut keep = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let mut kinds = kinds_held(find.archive);
    if let Some(found_act) = &found.act
        && !kinds.contains(found_act)
    {
        kinds.insert(0, found_act.clone());
    }
    let title = i18n.t("archive_viewer.find_title");
    let intro = i18n.t_args(
        "archive_viewer.find_intro",
        &[("archive", &find.archive.name)],
    );
    let can_keep = find.citation_id.is_some();

    let submit = {
        let find = find.clone();
        move |_| {
            let text =
                |value: String| Some(value.trim().to_owned()).filter(|value| !value.is_empty());
            let supplied = SuppliedParts {
                locality: text(locality()),
                act: text(act()).and_then(|code| Act::from_code(&code)),
                year: text(year()).and_then(|year| year.parse().ok()),
                view: text(view()).and_then(|view| view.parse().ok()),
            };
            match find.complete(find.changes(supplied)) {
                Some((link, written)) => {
                    error.set(None);
                    on_find.call((link, written.filter(|_| keep() && can_keep)));
                }
                None => error.set(Some(i18n.t("archive_viewer.find_missing"))),
            }
        }
    };

    rsx! {
        Modal { label: title.clone(), on_close,
            h3 { "{title}" }
            p { class: "confirm-message", "{intro}" }
            div { class: "form-group",
                label { {i18n.t("archive_viewer.archive")} }
                input { r#type: "text", value: "{find.archive.name}", readonly: true }
            }
            div { class: "form-group",
                label { {i18n.t("archive_viewer.find_locality")} }
                input {
                    r#type: "text",
                    value: "{locality}",
                    oninput: move |e: Event<FormData>| locality.set(e.value()),
                }
            }
            div { class: "form-group",
                label { {i18n.t("archive_viewer.find_kind")} }
                select {
                    value: "{act}",
                    onchange: move |e: Event<FormData>| act.set(e.value()),
                    option { value: "", selected: act().is_empty(), "—" }
                    for kind in kinds {
                        option {
                            key: "{kind}",
                            value: "{kind}",
                            selected: act() == kind.to_string(),
                            {kind_label(&i18n, &kind)}
                        }
                    }
                }
            }
            div { class: "form-row",
                div { class: "form-group",
                    label { {i18n.t("archive_viewer.find_year")} }
                    input {
                        r#type: "number",
                        min: "1000",
                        max: "2100",
                        value: "{year}",
                        oninput: move |e: Event<FormData>| year.set(e.value()),
                    }
                }
                div { class: "form-group",
                    label { {i18n.t("archive_viewer.view")} }
                    input {
                        r#type: "number",
                        min: "1",
                        value: "{view}",
                        oninput: move |e: Event<FormData>| view.set(e.value()),
                    }
                }
            }
            if can_keep {
                label { class: "media-event-row",
                    input {
                        r#type: "checkbox",
                        checked: keep(),
                        onchange: move |e: Event<FormData>| keep.set(e.checked()),
                    }
                    span { {i18n.t("archive_viewer.find_keep")} }
                }
            }
            if let Some(message) = error() {
                div { class: "error-msg", role: "alert", "{message}" }
            }
            div { class: "modal-actions",
                button { class: "btn btn-outline", onclick: move |_| on_close.call(()),
                    {i18n.t("common.cancel")}
                }
                button { class: "btn btn-primary", onclick: submit,
                    {i18n.t("archive_viewer.find_submit")}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use oxidgene_archives::{ArchiveRegistry, CitationEvidence, Part};
    use uuid::Uuid;

    use super::super::ArchiveOffer;
    use super::*;
    use crate::i18n::Language;

    fn find(title: &str, page: Option<&str>) -> ArchiveFind {
        let evidence = CitationEvidence {
            title: title.to_owned(),
            page: page.map(str::to_owned),
            ..CitationEvidence::default()
        };
        match ArchiveOffer::of(Uuid::nil(), Some(Uuid::nil()), evidence) {
            Some(ArchiveOffer::Find(find)) => find,
            other => panic!("a citation to complete: {other:?}"),
        }
    }

    #[test]
    fn the_kinds_offered_are_those_the_archive_holds() {
        let sarthe = ArchiveRegistry::embedded()
            .archives()
            .iter()
            .find(|archive| archive.id == "fr-ad72")
            .unwrap();
        let codes: Vec<String> = kinds_held(sarthe).iter().map(ToString::to_string).collect();
        assert_eq!(codes, ["B", "M", "S", "N", "D", "TD"]);
        let i18n = I18n::new(Language::english());
        for kind in kinds_held(sarthe) {
            let label = kind_label(&i18n, &kind);
            assert!(!label.starts_with("archive_viewer."), "{label}");
        }
        assert_eq!(
            kind_label(&i18n, &Act::from_code("BMS").unwrap())
                .matches(" / ")
                .count(),
            2
        );
    }

    #[test]
    fn the_reader_completes_what_was_not_recognized() {
        let find = find("AD72, 4E 1234", Some("v. 45, n° 312"));
        assert_eq!(find.missing, [Part::Act, Part::Locality]);
        assert_eq!(find.found.views[0].view, 45);

        // Still missing the locality.
        let partial = SuppliedParts {
            act: Act::from_code("N"),
            ..SuppliedParts::default()
        };
        assert!(find.complete(partial).is_none());

        let supplied = SuppliedParts {
            locality: Some("Exampleville".to_owned()),
            act: Act::from_code("N"),
            year: Some(1872),
            view: None,
        };
        let (link, written) = find.complete(supplied.clone()).expect("a register");
        assert_eq!(link.citation.locality, "Exampleville");
        assert_eq!(link.citation.views[0].view, 45);
        assert_eq!(link.supplied, Some(supplied));
        assert_eq!(written.as_deref(), Some("Exampleville, naissance 1872"));

        // What the reader left as recognized is not theirs.
        let unchanged = SuppliedParts {
            locality: Some("Exampleville".to_owned()),
            view: Some(45),
            ..SuppliedParts::default()
        };
        assert_eq!(
            find.changes(unchanged),
            SuppliedParts {
                locality: Some("Exampleville".to_owned()),
                ..SuppliedParts::default()
            }
        );

        // An act the archive does not hold is refused.
        let refused = SuppliedParts {
            locality: Some("Exampleville".to_owned()),
            act: Act::from_code("RP"),
            ..SuppliedParts::default()
        };
        assert!(find.complete(refused).is_none());
    }
}
