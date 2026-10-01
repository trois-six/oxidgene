//! Couple page — both spouses of a family side by side, with what they share
//! drawn once across the two columns.

use std::collections::HashMap;
use std::sync::Arc;

use dioxus::prelude::*;
use oxidgene_core::Sex;
use oxidgene_core::projection::Pedigree;
use oxidgene_core::types::{FamilySpouse, Note};
use uuid::Uuid;

use crate::api::{ApiClient, ApiError, PersonDetailBundle};
use crate::components::media_gallery::MediaOwner;
use crate::components::pedigree_chart::{Portraits, SharedPedigree};
use crate::components::person_form::{PersonForm, PersonFormCreateContext};
use crate::components::person_profile::{
    EnrichedEvent, EventOrigin, Profile, ProfileMediaCard, SHOW_MANUAL_REFRESH, SectionContext,
    SharedProfile, ancestors_section, build_profile, children_list, couple_sides, family_section,
    header_section, media_event_links, notes_section, refresh_button, timeline_section, union_line,
    use_ancestor_pedigree, use_mini_pedigree, use_sosa_ancestors, use_tree_resource,
};
use crate::components::topbar_search::TopbarSearch;
use crate::components::tree_cache::{use_track_current_person, use_tree_cache};
use crate::components::tree_icon_sidebar::{ProfilePageSidebar, TreeSidebarView};
use crate::components::tree_page::ToolPageFrame;
use crate::components::union_form::UnionForm;
use crate::i18n::{I18n, use_i18n};
use crate::router::{Route, person_route, push_tree_route};
use crate::shared::Shared;
use crate::ui_observability::{UiLoadTrace, UiPage, use_traced_resource, use_ui_load_trace};

/// The family's spouses and each one's detail bundle, loaded together.
struct CoupleData {
    spouses: Vec<FamilySpouse>,
    bundles: HashMap<Uuid, Arc<PersonDetailBundle>>,
}

impl CoupleData {
    fn sex_of(&self, person_id: Uuid) -> Sex {
        self.bundles
            .get(&person_id)
            .and_then(|bundle| bundle.persons.iter().find(|p| p.id == person_id))
            .map_or(Sex::Unknown, |person| person.sex)
    }
}

/// Page rendered at `/trees/:tree_id/couples/:family_id`.
#[component]
pub fn CoupleDetail(tree_id: String, family_id: String) -> Element {
    let i18n = use_i18n();
    let api = use_context::<ApiClient>();
    let nav = use_navigator();
    let tree_cache = use_tree_cache();
    let load_trace = use_ui_load_trace(UiPage::CoupleDetail);
    let mut refresh = use_signal(|| 0u32);
    let media_revision = use_signal(|| 0_u32);
    let mut show_edit_couple = use_signal(|| false);
    let mut show_create_person = use_signal(|| false);

    // Reactive IDs, as on the person page: the router reuses this instance
    // when the spouse selectors move to another couple.
    let tree_id_parsed = crate::utils::use_synced(tree_id.parse::<Uuid>().ok());
    let family_id_parsed = crate::utils::use_synced(family_id.parse::<Uuid>().ok());

    use_effect(move || {
        family_id_parsed();
        document::eval(
            "document.querySelector('.sub-page-content')?.scrollTo({ top: 0, behavior: 'instant' });",
        );
    });

    // ── Resources ────────────────────────────────────────────────────

    let api_couple = api.clone();
    let couple_resource = use_traced_resource(load_trace.clone(), "couple", move || {
        let api = api_couple.clone();
        let _tick = refresh();
        let _media_tick = media_revision();
        let (tid, fid) = (tree_id_parsed(), family_id_parsed());
        async move {
            let (Some(tid), Some(fid)) = (tid, fid) else {
                return Err(ApiError::invalid_ids(&i18n));
            };
            load_couple(api, tid, fid).await
        }
    });

    let sides = use_memo(move || match &*couple_resource.read() {
        Some(Ok(data)) => couple_sides(&data.spouses, |id| data.sex_of(id)),
        _ => (None, None),
    });
    let left_id = use_memo(move || sides().0);
    let right_id = use_memo(move || sides().1);
    use_back_to_tree_when_deleted(&tree_id, couple_resource, left_id, right_id);

    let tree_resource = use_tree_resource(
        load_trace.clone(),
        api.clone(),
        tree_id_parsed,
        refresh,
        i18n,
    );
    let sosa_ancestors_resource = use_sosa_ancestors(
        load_trace.clone(),
        api.clone(),
        tree_id_parsed,
        tree_resource,
    );

    let api_photos = api.clone();
    let photos_resource = use_traced_resource(load_trace.clone(), "portraits", move || {
        let api = api_photos.clone();
        let tid = tree_id_parsed();
        let _ = media_revision();
        let ids: Vec<Uuid> = [left_id(), right_id()].into_iter().flatten().collect();
        async move {
            match tid {
                Some(tid) if !ids.is_empty() => {
                    Shared::new(api.portrait_map_for_ids(tid, &ids).await)
                }
                _ => Portraits::default(),
            }
        }
    });

    let couple_notes = use_notes(
        load_trace.clone(),
        &api,
        tree_id_parsed,
        refresh,
        NotesOf::Family(family_id_parsed),
    );
    let left = use_side(
        load_trace.clone(),
        &api,
        (tree_id_parsed, refresh, i18n),
        couple_resource,
        left_id,
    );
    let right = use_side(
        load_trace.clone(),
        &api,
        (tree_id_parsed, refresh, i18n),
        couple_resource,
        right_id,
    );

    // ── Render ────────────────────────────────────────────────────────

    let tree_name_str = tree_cache
        .loaded_or_cached(
            tree_id_parsed(),
            tree_resource
                .read()
                .as_ref()
                .and_then(|tree| tree.as_ref().ok()),
        )
        .map(|tree| tree.name)
        .unwrap_or_default();
    let self_person_id = match &*tree_resource.read() {
        Some(Ok(tree)) => tree.self_person_id,
        _ => None,
    };

    let (loaded, load_error) = match &*couple_resource.read() {
        Some(Ok(_)) => (true, None),
        Some(Err(error)) => (false, Some(error.to_string())),
        None => (false, None),
    };
    let columns = [left.column(left_id()), right.column(right_id())];
    let unknown = i18n.t("couple.unknown_spouse");
    let title = if loaded {
        i18n.t_args(
            "couple.title",
            &[
                ("left", &columns[0].name(&unknown)),
                ("right", &columns[1].name(&unknown)),
            ],
        )
    } else {
        String::new()
    };

    let sosa_ancestors = sosa_ancestors_resource.read().clone().unwrap_or_default();
    let ctx = tree_id_parsed().map(|tree_id| SectionContext {
        i18n,
        tree_id,
        sosa_ancestors: &sosa_ancestors,
        media_revision,
    });
    let selected_person_id = left_id().or(right_id());
    use_track_current_person(tree_id_parsed(), selected_person_id);
    let on_saved = move |_| {
        tree_cache.invalidate();
        refresh += 1;
    };

    rsx! {
        ToolPageFrame {
            tree_id: tree_id.clone(),
            tree_name: tree_name_str.clone(),
            title: title.clone(),
            topbar: rsx! {
                TopbarSearch { tree_id: tree_id.clone(), from_person: true }
            },
            sidebar: rsx! {
                ProfilePageSidebar {
                    tree_id: tree_id.clone(),
                    active_view: TreeSidebarView::Couple,
                    selected_person_id,
                    couple_family_id: family_id_parsed(),
                    on_add_person: move |_| show_create_person.set(true),
                }
            },
            content_class: "pd-content cp-content",

        // The existing couple edit modal: both spouses, the union's events,
        // its children (detachable), its media — and deleting the couple.
        if let (true, Some(tid), Some(fid)) = (show_edit_couple(), tree_id_parsed(), family_id_parsed()) {
            UnionForm {
                tree_id: tid,
                family_id: fid,
                on_close: move |_| show_edit_couple.set(false),
                on_saved,
            }
        }

        if let (true, Some(tid)) = (show_create_person(), tree_id_parsed()) {
            PersonForm {
                tree_id: tid,
                create_context: PersonFormCreateContext::Standalone,
                on_close: move |_| show_create_person.set(false),
                on_saved,
            }
        }

        match (loaded, ctx.as_ref(), family_id_parsed()) {
            (true, Some(ctx), Some(family_id)) => {
                let view = CoupleView {
                    ctx,
                    tree_id: &tree_id,
                    family_id,
                    nav,
                    self_person_id,
                    photos: photos_resource,
                    couple_notes,
                    columns: &columns,
                    unknown: &unknown,
                };
                rsx! {
                    {couple_actions(&i18n, show_edit_couple, refresh)}
                    {view.spouse_bar()}
                    {view.grid()}
                }
            }
            _ => match load_error {
                Some(error) => rsx! {
                    div { class: "error-msg", {i18n.t_args("couple.load_error", &[("error", &error)])} }
                },
                None => rsx! {
                    div { class: "loading", {i18n.t("couple.loading")} }
                },
            },
        }
        }
    }
}

/// Loads the family (which fails once the couple is deleted), its spouses,
/// and both spouses' bundles, fetched concurrently.
async fn load_couple(api: ApiClient, tid: Uuid, fid: Uuid) -> Result<Arc<CoupleData>, ApiError> {
    let (_family, spouses) =
        futures_util::future::try_join(api.get_family(tid, fid), api.list_family_spouses(tid, fid))
            .await?;
    let bundles = futures_util::future::try_join_all(spouses.iter().map(|spouse| {
        let api = api.clone();
        let pid = spouse.person_id;
        async move {
            api.get_person_detail_bundle(tid, pid)
                .await
                .map(|bundle| (pid, Arc::new(bundle)))
        }
    }))
    .await?;
    Ok(Arc::new(CoupleData {
        spouses,
        bundles: bundles.into_iter().collect(),
    }))
}

/// Deleting the couple from its edit modal leaves nothing to show: go back
/// to the tree, on whichever spouse was on screen.
fn use_back_to_tree_when_deleted(
    tree_id: &str,
    couple: Resource<Result<Arc<CoupleData>, ApiError>>,
    left_id: Memo<Option<Uuid>>,
    right_id: Memo<Option<Uuid>>,
) {
    let nav = use_navigator();
    let mut last_person = use_signal(|| None::<Uuid>);
    use_effect(move || {
        if let Some(person) = left_id().or(right_id()) {
            last_person.set(Some(person));
        }
    });
    let tree_id = tree_id.to_string();
    use_effect(move || {
        if let Some(Err(ApiError::Api { status: 404, .. })) = &*couple.read()
            && let Some(person) = *last_person.peek()
        {
            nav.replace(Route::TreeDetail {
                tree_id: tree_id.clone(),
                person: Some(person.to_string()),
            });
        }
    });
}

/// The notes list a notes resource loads.
type NotesResource = Resource<Option<Result<Vec<Note>, ApiError>>>;

/// One spouse's loads: their notes, ancestors and profile.
struct Side {
    notes: NotesResource,
    pedigree: Resource<Result<Option<Pedigree>, ApiError>>,
    mini: Memo<Option<(Uuid, SharedPedigree)>>,
    profile: Memo<Option<SharedProfile>>,
}

impl Side {
    /// What this side shows in a render.
    fn column(&self, person_id: Option<Uuid>) -> Column {
        Column {
            person_id,
            profile: (self.profile)(),
            notes: self.notes,
            pedigree: self.pedigree,
            mini: (self.mini)(),
        }
    }
}

/// One spouse's column of the couple grid.
struct Column {
    person_id: Option<Uuid>,
    profile: Option<SharedProfile>,
    notes: NotesResource,
    pedigree: Resource<Result<Option<Pedigree>, ApiError>>,
    mini: Option<(Uuid, SharedPedigree)>,
}

impl Column {
    /// The spouse's name, or `unknown` for a missing spouse.
    fn name(&self, unknown: &str) -> String {
        self.profile
            .as_ref()
            .map_or_else(|| unknown.to_string(), |p| p.name.display_name.clone())
    }
}

/// Starts one spouse's loads.
fn use_side(
    load_trace: UiLoadTrace,
    api: &ApiClient,
    (tree_id, refresh, i18n): (Signal<Option<Uuid>>, Signal<u32>, I18n),
    couple: Resource<Result<Arc<CoupleData>, ApiError>>,
    person_id: Memo<Option<Uuid>>,
) -> Side {
    let notes = use_notes(
        load_trace.clone(),
        api,
        tree_id,
        refresh,
        NotesOf::Person(person_id),
    );
    let pedigree = use_ancestor_pedigree(
        load_trace.clone(),
        api.clone(),
        tree_id,
        person_id.into(),
        i18n,
    );
    Side {
        notes,
        pedigree,
        mini: use_mini_pedigree(pedigree),
        profile: use_side_profile(load_trace, couple, person_id, pedigree, i18n),
    }
}

/// The couple's own actions: editing it, and refreshing the page.
fn couple_actions(i18n: &I18n, mut show_edit: Signal<bool>, mut refresh: Signal<u32>) -> Element {
    rsx! {
        div { class: "cp-actions",
            button {
                class: "btn btn-outline pd-header-action-btn",
                title: i18n.t("couple.edit"),
                aria_label: i18n.t("couple.edit"),
                onclick: move |_| show_edit.set(true),
                svg {
                    class: "pd-header-action-icon",
                    width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                    stroke: "currentColor", "strokeWidth": "2",
                    path { d: "M12 20h9" }
                    path { d: "M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z" }
                }
            }
            if SHOW_MANUAL_REFRESH {
                {refresh_button(i18n, move || refresh += 1)}
            }
        }
    }
}

/// A loaded couple, drawn as rows of the grid: what the couple shares spans
/// both columns, and what is each spouse's own sits in their column.
struct CoupleView<'a> {
    ctx: &'a SectionContext<'a>,
    tree_id: &'a str,
    family_id: Uuid,
    nav: dioxus::router::Navigator,
    self_person_id: Option<Uuid>,
    photos: Resource<Portraits>,
    couple_notes: NotesResource,
    columns: &'a [Column; 2],
    unknown: &'a str,
}

impl CoupleView<'_> {
    /// The profile the couple's own rows are read from.
    fn anchor(&self) -> Option<&SharedProfile> {
        self.columns
            .iter()
            .find_map(|column| column.profile.as_ref())
    }

    /// Each spouse's profile, where known.
    fn profiles(&self) -> impl Iterator<Item = Option<&SharedProfile>> {
        self.columns.iter().map(|column| column.profile.as_ref())
    }

    /// What the couple shares: the union's own events and its children's,
    /// told once rather than from each spouse's side.
    fn couple_events(&self) -> Vec<EnrichedEvent> {
        let Some(anchor) = self.anchor() else {
            return Vec::new();
        };
        anchor
            .events
            .iter()
            .filter(|entry| entry.union_id == Some(self.family_id))
            .map(|entry| {
                let mut entry = entry.clone();
                if entry.origin == EventOrigin::ConjugalFamily {
                    entry.context = None;
                }
                entry
            })
            .collect()
    }

    /// A spouse's events outside this couple.
    fn own_events<'p>(&self, profile: &'p Profile) -> impl Iterator<Item = &'p EnrichedEvent> {
        let family_id = self.family_id;
        profile
            .events
            .iter()
            .filter(move |entry| entry.union_id != Some(family_id))
    }

    /// The two spouse selectors around the wedding rings.
    fn spouse_bar(&self) -> Element {
        let i18n = &self.ctx.i18n;
        let [left, right] = self.columns;
        rsx! {
            div { class: "card cp-bar",
                {spouse_select(i18n, self.nav, self.tree_id, self.family_id, right.profile.as_deref(), left.name(self.unknown))}
                span { class: "cp-ring", aria_hidden: "true", "\u{26AD}" }
                {spouse_select(i18n, self.nav, self.tree_id, self.family_id, left.profile.as_deref(), right.name(self.unknown))}
            }
        }
    }

    fn grid(&self) -> Element {
        let couple_events = self.couple_events();
        rsx! {
            div { class: "cp-grid",
                {self.identity_row()}
                {self.notes_rows()}
                {self.media_rows(&couple_events)}
                {self.family_rows()}
                {self.event_rows(&couple_events)}
                {self.ancestor_row()}
            }
        }
    }

    fn identity_row(&self) -> Element {
        rsx! {
            for column in self.columns.iter() {
                div { class: "cp-cell", {self.identity(column)} }
            }
        }
    }

    /// A spouse's header, or why there is none yet.
    fn identity(&self, column: &Column) -> Element {
        let i18n = &self.ctx.i18n;
        match (column.person_id, &column.profile) {
            (Some(_), Some(profile)) => header_section(
                self.ctx,
                profile,
                self.photos
                    .read()
                    .as_ref()
                    .and_then(|photos| photos.get(&profile.person_id).cloned()),
                self.self_person_id == Some(profile.person_id),
                self.open_profile(profile.person_id),
            ),
            (Some(_), None) => rsx! {
                div { class: "card page-header loading", {i18n.t("couple.loading")} }
            },
            (None, _) => rsx! {
                div { class: "card page-header cp-unknown",
                    p { class: "text-muted", "{self.unknown}" }
                }
            },
        }
    }

    fn open_profile(&self, person_id: Uuid) -> Element {
        let open = push_tree_route(self.tree_id, person_route);
        open_profile_button(&self.ctx.i18n, move || open.call(person_id))
    }

    fn notes_rows(&self) -> Element {
        let i18n = &self.ctx.i18n;
        let has_person_notes = self.columns.iter().any(|column| shows_notes(&column.notes));
        rsx! {
            if shows_notes(&self.couple_notes) {
                div { class: "cp-span",
                    {notes_section(i18n, "couple.notes_section", self.couple_notes.read().as_ref().and_then(Option::as_ref))}
                }
            }
            if has_person_notes {
                for column in self.columns.iter() {
                    div { class: "cp-cell",
                        {notes_section(i18n, "person.notes_section", column.notes.read().as_ref().and_then(Option::as_ref))}
                    }
                }
            }
        }
    }

    fn media_rows(&self, couple_events: &[EnrichedEvent]) -> Element {
        let i18n = &self.ctx.i18n;
        let mut media_revision = self.ctx.media_revision;
        rsx! {
            div { class: "cp-span",
                ProfileMediaCard {
                    tree_id: self.ctx.tree_id,
                    owner: MediaOwner::Family(self.family_id),
                    title: i18n.t("couple.media_section"),
                    event_links: media_event_links(couple_events.iter(), i18n),
                    revision: media_revision(),
                    on_changed: move |()| media_revision += 1,
                }
            }
            for profile in self.profiles() {
                div { class: "cp-cell",
                    if let Some(profile) = profile {
                        ProfileMediaCard {
                            key: "{profile.person_id}",
                            tree_id: self.ctx.tree_id,
                            owner: MediaOwner::Person(profile.person_id),
                            title: i18n.t("media.section"),
                            event_links: media_event_links(self.own_events(profile), i18n),
                            preloaded_tiles: Some(profile.profile_tiles(false)),
                            preloaded_bundle: Some(Arc::clone(&profile.bundle.gallery)),
                            preloaded_portrait: profile.person.as_ref().map(|person| (
                                person.portrait_media_id,
                                person.portrait_vignette_id,
                            )),
                            preloaded_vignettes: Some(profile.bundle.profile_vignettes.clone()),
                            revision: media_revision(),
                            on_changed: move |()| media_revision += 1,
                        }
                    }
                }
            }
        }
    }

    fn family_rows(&self) -> Element {
        let ctx = self.ctx;
        let union = self.anchor().and_then(|profile| {
            profile
                .family
                .unions
                .iter()
                .find(|union| union.family_id == self.family_id)
                .map(|union| (profile, union))
        });
        rsx! {
            if let Some((profile, union)) = union {
                div { class: "cp-span",
                    div { class: "card pd-family-card",
                        h2 { style: "font-size: 1.1rem; margin-bottom: 12px;", {ctx.i18n.t("couple.union_section")} }
                        p { class: "pd-union-line", {union_line(ctx, profile, union, false)} }
                        {children_list(ctx, profile, &union.child_ids)}
                    }
                }
            }
            for profile in self.profiles() {
                div { class: "cp-cell",
                    if let Some(profile) = profile {
                        {family_section(ctx, profile, Some(self.family_id))}
                    }
                }
            }
        }
    }

    fn event_rows(&self, couple_events: &[EnrichedEvent]) -> Element {
        let ctx = self.ctx;
        let couple_event_refs: Vec<&EnrichedEvent> = couple_events.iter().collect();
        rsx! {
            if let Some(profile) = self.anchor() {
                div { class: "cp-span",
                    {timeline_section(ctx, profile, ctx.i18n.t("couple.events_section"), &couple_event_refs)}
                }
            }
            for profile in self.profiles() {
                div { class: "cp-cell",
                    if let Some(profile) = profile {
                        {
                            let own: Vec<&EnrichedEvent> = self.own_events(profile).collect();
                            timeline_section(ctx, profile, ctx.i18n.t("person.events_section"), &own)
                        }
                    }
                }
            }
        }
    }

    fn ancestor_row(&self) -> Element {
        let on_navigate = push_tree_route(self.tree_id, person_route);
        rsx! {
            for column in self.columns.iter() {
                div { class: "cp-cell",
                    if column.person_id.is_some() {
                        {ancestors_section(&self.ctx.i18n, &column.pedigree, column.mini.clone(), self.photos.read().clone(), on_navigate)}
                    }
                }
            }
        }
    }
}

/// A person's profile in a tree.
/// Whose notes a notes resource loads.
enum NotesOf {
    Person(Memo<Option<Uuid>>),
    Family(Signal<Option<Uuid>>),
}

/// One notes list — `None` while there is no one to load it for.
fn use_notes(
    load_trace: UiLoadTrace,
    api: &ApiClient,
    tree_id: Signal<Option<Uuid>>,
    refresh: Signal<u32>,
    of: NotesOf,
) -> NotesResource {
    let api = api.clone();
    use_traced_resource(load_trace, "notes", move || {
        let api = api.clone();
        let _tick = refresh();
        let tid = tree_id();
        let (person, family) = match &of {
            NotesOf::Person(person) => (person(), None),
            NotesOf::Family(family) => (None, family()),
        };
        async move {
            let tid = tid?;
            if person.is_none() && family.is_none() {
                return None;
            }
            Some(api.list_notes(tid, person, None, family, None, None).await)
        }
    })
}

/// Whether a notes card has anything to say: notes, or why they failed.
fn shows_notes(notes: &NotesResource) -> bool {
    match &*notes.read() {
        Some(Some(Ok(notes))) => !notes.is_empty(),
        Some(Some(Err(_))) => true,
        _ => false,
    }
}

/// One spouse's profile, derived from the couple's load and their pedigree.
fn use_side_profile(
    load_trace: UiLoadTrace,
    couple: Resource<Result<Arc<CoupleData>, ApiError>>,
    person_id: Memo<Option<Uuid>>,
    pedigree: Resource<Result<Option<Pedigree>, ApiError>>,
    i18n: I18n,
) -> Memo<Option<SharedProfile>> {
    use_memo(move || {
        let pid = person_id()?;
        let data = couple.read();
        let Some(Ok(data)) = &*data else {
            return None;
        };
        let bundle = Arc::clone(data.bundles.get(&pid)?);
        let pedigree = pedigree.read();
        let pedigree = match &*pedigree {
            Some(Ok(Some(pedigree))) => Some(pedigree),
            _ => None,
        };
        let profile = load_trace.measure("person_profile", || {
            build_profile(bundle, pid, pedigree, &i18n)
        });
        Some(SharedProfile::new(profile))
    })
}

/// A spouse selector: the couples of `owner` — the spouse on the other side —
/// each named by the partner it would put on this side. Choosing one opens
/// that couple. Without an owner (an unknown spouse) it only names `current`.
fn spouse_select(
    i18n: &I18n,
    nav: dioxus::router::Navigator,
    tree_id: &str,
    family_id: Uuid,
    owner: Option<&Profile>,
    current: String,
) -> Element {
    let options: Vec<(Uuid, String)> = match owner {
        Some(owner) => owner
            .couples()
            .map(|union| {
                let names = union
                    .partner_ids
                    .iter()
                    .map(|pid| owner.name_of(*pid, i18n))
                    .collect::<Vec<_>>()
                    .join(" & ");
                let label = match &union.marriage_date {
                    Some(date) => format!("{names} \u{2014} {}", date.text),
                    None => names,
                };
                (union.family_id, label)
            })
            .collect(),
        None => Vec::new(),
    };
    let options = if options.iter().any(|(id, _)| *id == family_id) {
        options
    } else {
        vec![(family_id, current)]
    };
    let aria_label = owner
        .map(|owner| i18n.t_args("couple.spouses_of", &[("name", &owner.name.display_name)]))
        .unwrap_or_default();
    let tree_id = tree_id.to_string();
    let single = options.len() < 2;
    rsx! {
        select {
            class: "cp-select",
            aria_label: "{aria_label}",
            disabled: single,
            onchange: move |evt| {
                if let Ok(family_id) = evt.value().parse::<Uuid>() {
                    nav.replace(Route::CoupleDetail {
                        tree_id: tree_id.clone(),
                        family_id: family_id.to_string(),
                    });
                }
            },
            for (id, label) in options {
                option { key: "{id}", value: "{id}", selected: id == family_id, "{label}" }
            }
        }
    }
}

/// A spouse header's link to their own profile.
fn open_profile_button(i18n: &I18n, mut on_open: impl FnMut() + 'static) -> Element {
    rsx! {
        button {
            class: "btn btn-outline pd-header-action-btn",
            title: i18n.t("couple.open_profile"),
            aria_label: i18n.t("couple.open_profile"),
            onclick: move |_| on_open(),
            svg {
                class: "pd-header-action-icon",
                width: "16", height: "16", fill: "none", "viewBox": "0 0 24 24",
                stroke: "currentColor", "strokeWidth": "2",
                circle { cx: "12", cy: "8", r: "4" }
                path { d: "M4 21v-1a6 6 0 0 1 12 0v1" }
            }
        }
    }
}
