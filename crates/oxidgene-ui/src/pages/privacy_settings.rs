//! The Privacy section of the tree settings (`docs/ui-settings.md` §8): the
//! default privacy, the contemporary rule and how hidden persons appear.
//!
//! Every control saves as soon as it changes, with a local override so it
//! answers the click rather than the refetch. The settings that only matter
//! for the tree's other viewers appear when the build offers the surfaces
//! for them ([`crate::sharing`]); elsewhere they apply to the export option
//! that leaves out contemporary and private persons only.

use dioxus::prelude::*;
use oxidgene_core::enums::{ExclusionKind, HiddenDisplay, TreeDefaultPrivacy};
use oxidgene_core::privacy::{MAX_CONTEMPORARY_YEARS, MIN_CONTEMPORARY_YEARS};
use oxidgene_core::types::{PrivacySettings, Tree};
use uuid::Uuid;

use super::settings::SettingsCheck;
use super::sharing_settings::Listed;
use crate::api::{ApiClient, UpdateTreeBody};
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::text_field::TextInput;
use crate::components::tree_cache::{TreeCache, use_tree_cache};
use crate::i18n::{I18n, use_i18n};
use crate::sharing::use_sharing_surfaces;
use crate::ui_observability::use_ui_resource;

/// The tree read the settings page shares.
type TreeResource = Resource<Option<Result<Tree, crate::api::ApiError>>>;

/// Save `body` on tree `tree_id`, the error shown in `error` if it fails.
fn save(
    api: &ApiClient,
    i18n: I18n,
    tree_cache: TreeCache,
    mut error: Signal<Option<String>>,
    tree_id: Option<Uuid>,
    body: UpdateTreeBody,
) {
    let api = api.clone();
    let Some(tree_id) = tree_id else { return };
    spawn(async move {
        match api.update_tree(tree_id, &body).await {
            Ok(_) => {
                error.set(None);
                tree_cache.invalidate();
            }
            Err(e) => error.set(Some(e.message(&i18n))),
        }
    });
}

/// The Privacy section: the default privacy, then the contemporary rule.
#[component]
pub fn PrivacySection(tree_id: String, tree_resource: TreeResource) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();
    let tree_id_parsed = tree_id.parse::<Uuid>().ok();
    let sharing = use_sharing_surfaces();

    let save_error = use_signal(|| None::<String>);
    // Local override so the control answers the click, not the refetch.
    let mut local_privacy_override = use_signal(|| None::<TreeDefaultPrivacy>);

    let tree = match &*tree_resource.read() {
        Some(Some(Ok(tree))) => Some(tree.clone()),
        _ => None,
    };
    let current_privacy = local_privacy_override()
        .unwrap_or_else(|| tree.as_ref().map(|t| t.default_privacy).unwrap_or_default());

    rsx! {
        div { class: "settings-section",
            div { class: "settings-section-eyebrow", {i18n.t("settings.breadcrumb")} }
            h2 { class: "settings-section-title", {i18n.t("settings.privacy")} }
            p { class: "settings-section-subtitle",
                {i18n.t("settings.privacy_desc")}
            }

            div { class: "card settings-card",
                h3 { class: "settings-card-title",
                    {i18n.t("settings.default_privacy")}
                }
                p { class: "settings-section-subtitle",
                    {i18n.t("settings.default_privacy_desc")}
                }
                div { class: "pf-gender-group settings-choices",
                    for (value , label) in [
                        (TreeDefaultPrivacy::Public, i18n.t("privacy.public")),
                        (TreeDefaultPrivacy::Private, i18n.t("privacy.private")),
                    ] {
                        button {
                            key: "{value.as_str()}",
                            class: if current_privacy == value {
                                "pf-gender-btn active"
                            } else {
                                "pf-gender-btn"
                            },
                            r#type: "button",
                            onclick: {
                                let api = api.clone();
                                move |_| {
                                    local_privacy_override.set(Some(value));
                                    save(
                                        &api,
                                        i18n,
                                        tree_cache,
                                        save_error,
                                        tree_id_parsed,
                                        UpdateTreeBody {
                                            default_privacy: Some(value),
                                            ..Default::default()
                                        },
                                    );
                                }
                            },
                            "{label}"
                        }
                    }
                }
                if let Some(err) = &save_error() {
                    div { class: "error-msg settings-feedback", "{err}" }
                }
            }

            if let Some(tree) = tree {
                ContemporaryCard { key: "{tree.id}", tree_id: tree.id, settings: tree.privacy }
                if sharing {
                    ExclusionsCard { key: "exclusions-{tree.id}", tree_id: tree.id }
                }
            }
        }
    }
}

/// The contemporary rule, its threshold, how hidden persons appear and, for
/// a build with other viewers, what they may do (`docs/privacy.md` §9).
#[component]
fn ContemporaryCard(tree_id: Uuid, settings: PrivacySettings) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let tree_cache = use_tree_cache();
    let sharing = use_sharing_surfaces();
    let error = use_signal(|| None::<String>);
    // What the controls show: the stored settings, then each change made.
    let mut shown = use_signal(|| settings);
    let mut years_text = use_signal(|| settings.contemporary_years.to_string());
    let mut confirming_off = use_signal(|| false);

    let current = shown();
    let years_valid = years_text()
        .trim()
        .parse::<i32>()
        .is_ok_and(|years| (MIN_CONTEMPORARY_YEARS..=MAX_CONTEMPORARY_YEARS).contains(&years));
    let change = {
        let api = api.clone();
        move |next: PrivacySettings, body: UpdateTreeBody| {
            shown.set(next);
            save(&api, i18n, tree_cache, error, Some(tree_id), body);
        }
    };
    let (desc, off_message) = if sharing {
        (
            "settings.contemporary_desc",
            "settings.contemporary_off_message",
        )
    } else {
        (
            "settings.contemporary_desc_export",
            "settings.contemporary_off_message_export",
        )
    };

    rsx! {
        div { class: "card settings-card",
            h3 { class: "settings-card-title", {i18n.t("settings.contemporary_title")} }
            p { class: "settings-section-subtitle", {i18n.t(desc)} }
            SettingsCheck {
                checked: current.contemporary_rule,
                label: i18n.t("settings.contemporary_rule"),
                description: Some(i18n.t("settings.contemporary_rule_desc")),
                on_change: {
                    let mut change = change.clone();
                    move |on: bool| {
                        if on {
                            change(
                                PrivacySettings { contemporary_rule: true, ..current },
                                UpdateTreeBody { contemporary_rule: Some(true), ..Default::default() },
                            );
                        } else {
                            confirming_off.set(true);
                        }
                    }
                },
            }
            div { class: "form-group",
                label { r#for: "contemporary-years", {i18n.t("settings.contemporary_years")} }
                TextInput {
                    id: "contemporary-years",
                    r#type: "number",
                    min: "{MIN_CONTEMPORARY_YEARS}",
                    max: "{MAX_CONTEMPORARY_YEARS}",
                    value: "{years_text}",
                    on_input: {
                        let mut change = change.clone();
                        move |text: String| {
                            years_text.set(text.clone());
                            let Ok(years) = text.trim().parse::<i32>() else { return };
                            if (MIN_CONTEMPORARY_YEARS..=MAX_CONTEMPORARY_YEARS).contains(&years)
                                && years != shown().contemporary_years
                            {
                                change(
                                    PrivacySettings { contemporary_years: years, ..shown() },
                                    UpdateTreeBody { contemporary_years: Some(years), ..Default::default() },
                                );
                            }
                        }
                    },
                }
                if !years_valid {
                    p { class: "field-hint field-hint-warn",
                        {i18n.t_args("settings.thresholds_range", &[("min", &MIN_CONTEMPORARY_YEARS.to_string()), ("max", &MAX_CONTEMPORARY_YEARS.to_string())])}
                    }
                }
            }
            div { class: "settings-check-label", {i18n.t("settings.hidden_display")} }
            div { class: "pf-gender-group settings-choices",
                for value in [HiddenDisplay::Stub, HiddenDisplay::Initials, HiddenDisplay::Hidden] {
                    button {
                        key: "{value.as_str()}",
                        class: if current.hidden_display == value { "pf-gender-btn active" } else { "pf-gender-btn" },
                        r#type: "button",
                        onclick: {
                            let mut change = change.clone();
                            move |_| change(
                                PrivacySettings { hidden_display: value, ..current },
                                UpdateTreeBody { hidden_display: Some(value), ..Default::default() },
                            )
                        },
                        {i18n.t(&format!("settings.hidden_display.{}", value.as_str()))}
                    }
                }
            }
            p { class: "settings-check-desc", {i18n.t("settings.hidden_display_desc")} }
            if sharing {
                SettingsCheck {
                    checked: current.hidden_navigation,
                    label: i18n.t("settings.hidden_navigation"),
                    description: Some(i18n.t("settings.hidden_navigation_desc")),
                    on_change: {
                        let mut change = change.clone();
                        move |on: bool| change(
                            PrivacySettings { hidden_navigation: on, ..current },
                            UpdateTreeBody { hidden_navigation: Some(on), ..Default::default() },
                        )
                    },
                }
                SettingsCheck {
                    checked: current.contemporary_photos,
                    label: i18n.t("settings.contemporary_photos"),
                    description: Some(i18n.t("settings.contemporary_photos_desc")),
                    on_change: {
                        let mut change = change.clone();
                        move |on: bool| change(
                            PrivacySettings { contemporary_photos: on, ..current },
                            UpdateTreeBody { contemporary_photos: Some(on), ..Default::default() },
                        )
                    },
                }
                SettingsCheck {
                    checked: current.guests_may_export,
                    label: i18n.t("settings.guests_may_export"),
                    description: Some(i18n.t("settings.guests_may_export_desc")),
                    on_change: {
                        let mut change = change.clone();
                        move |on: bool| change(
                            PrivacySettings { guests_may_export: on, ..current },
                            UpdateTreeBody { guests_may_export: Some(on), ..Default::default() },
                        )
                    },
                }
            }
            if let Some(err) = error() {
                div { class: "error-msg settings-feedback", "{err}" }
            }
        }
        if confirming_off() {
            ConfirmDialog {
                title: i18n.t("settings.contemporary_off_title"),
                message: i18n.t(off_message),
                confirm_label: i18n.t("settings.contemporary_off_confirm"),
                confirm_class: "btn btn-primary".to_string(),
                on_confirm: {
                    let mut change = change.clone();
                    move |_| {
                        confirming_off.set(false);
                        change(
                            PrivacySettings { contemporary_rule: false, ..shown() },
                            UpdateTreeBody { contemporary_rule: Some(false), ..Default::default() },
                        );
                    }
                },
                on_cancel: move |_| confirming_off.set(false),
            }
        }
    }
}

/// The owner's exclusions from sharing (`docs/privacy.md` §6), each with its
/// root, what it covers and the number of persons it covers, and
/// **Remove**. Exclusions are created from the person menu.
#[component]
fn ExclusionsCard(tree_id: Uuid) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let mut error = use_signal(|| None::<String>);
    let mut exclusions = use_ui_resource("exclusions", {
        let api = api.clone();
        move || {
            let api = api.clone();
            async move { api.list_exclusions(tree_id).await }
        }
    });
    let listed = Listed::of(&exclusions.read(), &i18n);
    rsx! {
        div { class: "card settings-card",
            h3 { class: "settings-card-title", {i18n.t("settings.exclusions_title")} }
            p { class: "settings-section-subtitle", {i18n.t("settings.exclusions_desc")} }
            if listed.empty {
                p { class: "settings-check-desc", {i18n.t("settings.exclusions_empty")} }
            }
            if let Some(err) = listed.error {
                div { class: "error-msg settings-feedback", role: "alert", "{err}" }
            }
            for row in listed.rows {
                div { key: "{row.id}", class: "settings-export-row",
                    div { class: "settings-export-info",
                        div { class: "settings-check-label",
                            {row.person_name.clone().unwrap_or_else(|| i18n.t("common.unnamed"))}
                        }
                        p { class: "settings-check-desc", {exclusion_summary(&i18n, &row)} }
                    }
                    button {
                        class: "btn btn-outline",
                        r#type: "button",
                        onclick: {
                            let api = api.clone();
                            move |_| {
                                let api = api.clone();
                                spawn(async move {
                                    match api.delete_exclusion(tree_id, row.id).await {
                                        Ok(()) => exclusions.restart(),
                                        Err(e) => error.set(Some(e.message(&i18n))),
                                    }
                                });
                            }
                        },
                        {i18n.t("settings.exclusions_remove")}
                    }
                }
            }
            if let Some(err) = error() {
                div { class: "error-msg settings-feedback", "{err}" }
            }
        }
    }
}

/// What an exclusion covers, in a line: its kind, its depth and the number
/// of persons it covers now.
fn exclusion_summary(i18n: &crate::i18n::I18n, row: &crate::api::Exclusion) -> String {
    let kind = i18n.t(&format!("settings.exclusion_kind.{}", row.kind.as_str()));
    let depth = match row.depth {
        Some(depth) if row.kind != ExclusionKind::Person => {
            i18n.t_args("settings.exclusion_depth", &[("count", &depth.to_string())])
        }
        _ => String::new(),
    };
    let count = i18n.t_plural("history.persons_count", row.person_count as usize);
    let deleted = if row.person_deleted {
        i18n.t("settings.exclusion_root_deleted")
    } else {
        String::new()
    };
    [kind, depth, count, deleted]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}
