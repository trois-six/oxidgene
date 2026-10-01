//! Vertical bidirectional pedigree chart with pan/zoom, icon sidebar, and event panel.
//!
//! Layout: `.pedigree-outer` (flex row)
//!   -> `.isb` (icon sidebar: depth/zoom controls)
//!   -> `.pedigree-viewport` (pannable/zoomable canvas)
//!   -> `.ev-panel` (selected-person event list)
//!
//! Cards are positioned using the Reingold-Tilford (Buchheim variant) algorithm,
//! connectors are drawn via SVG overlay with Bézier curves.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use chrono::NaiveDate;
use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::CroppedSource;
use crate::components::cropped_image::CroppedSvgImage;
use crate::components::pedigree_theme::{
    CardFrame, FrameStroke, LinkSpec, PedigreeMetrics, PedigreeTheme, Point, frame_path, link_path,
};
use crate::components::person_profile::{sort_unions_chronologically, union_sort_date};
use crate::components::tree_cache::{
    PedigreeViewState, ViewStateCache, use_track_current_person, use_view_state_cache,
};
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};

use oxidgene_core::projection::{Pedigree, ProfileEvent};
use oxidgene_core::types::{
    Event as DomainEvent, FamilyChild, FamilySpouse, Person, PersonName, Place, QualifiedYear,
};
use oxidgene_core::{ChildType, DateQualifier, EventType, Privacy, Sex, SpouseRole};

use crate::components::pedigree_view::PedigreeView;
use crate::i18n::{I18n, use_i18n};
use crate::prefs::use_pedigree_defaults;
use crate::shared::Shared;

use crate::utils::{escape_xml, event_type_label_key, truncate_text_to_fit};

mod ancestors;
mod circular;
mod controls;
mod descent;
mod event_panel;
mod lineage;

// ── Viewport / zoom ──────────────────────────────────────────────────────

const VIEWPORT_DEFAULT_W: f64 = 800.0;
const VIEWPORT_DEFAULT_H: f64 = 600.0;
const FIT_SIDE_PADDING_RATIO: f64 = 0.05;
const EVENT_PANEL_AUTO_COLLAPSE_WIDTH: f64 = 600.0;
const EVENT_PANEL_MANUAL_STORAGE_KEY: &str = "oxidgene-ev-panel-manual";
const EVENT_PANEL_RATIO_STORAGE_KEY: &str = "oxidgene-ev-panel-ratio";
/// Bounds on the panel's rendered width. Kept in sync with the `clamp()` around
/// `--evw` in `LAYOUT_STYLES`, which enforces them again once a stored ratio is
/// re-applied to a window of a different size.
const EVENT_PANEL_MIN_WIDTH: f64 = 220.0;
const EVENT_PANEL_MAX_WIDTH: f64 = 640.0;
/// Never let the panel eat more than this share of the space left of it, so a
/// width chosen on a wide window stays usable on a narrow one.
const EVENT_PANEL_MAX_RATIO: f64 = 0.45;
const EVENT_PANEL_KEYBOARD_STEP: f64 = 16.0;
const ZOOM_FACTOR: f64 = 1.2;
const ZOOM_MIN: f64 = 0.3;
/// Four steps past the 200 % the chart used to stop at: close enough to read
/// the smallest line of a compact card on a large screen.
const ZOOM_MAX: f64 = 4.0;
// ── Default portraits (embedded as data URIs) ────────────────────────────

const PORTRAIT_MALE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/portrait_male.b64"
));
const PORTRAIT_FEMALE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/portrait_female.b64"
));
const PORTRAIT_UNKNOWN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/portrait_unknown.b64"
));

pub(crate) fn default_portrait(sex: Sex) -> &'static str {
    match sex {
        Sex::Male => PORTRAIT_MALE,
        Sex::Female => PORTRAIT_FEMALE,
        Sex::Unknown => PORTRAIT_UNKNOWN,
    }
}

/// The silhouette's own bytes, for a shell that serves it as a file.
///
/// Decoded once from the same embedded data URL the web build inlines, so
/// there is one copy of the picture and the two platforms cannot drift apart:
/// whichever path a card takes, it draws the identical PNG.
#[must_use]
pub fn silhouette_png(sex: Sex) -> &'static [u8] {
    use base64::Engine as _;
    use std::sync::OnceLock;

    static DECODED: OnceLock<[Vec<u8>; 3]> = OnceLock::new();
    let decoded = DECODED.get_or_init(|| {
        [Sex::Male, Sex::Female, Sex::Unknown].map(|sex| {
            let data_url = default_portrait(sex);
            let encoded = data_url
                .split_once(";base64,")
                .expect("the embedded silhouette is a base64 data URL")
                .1;
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .expect("the embedded silhouette is valid base64")
        })
    });
    match sex {
        Sex::Male => &decoded[0],
        Sex::Female => &decoded[1],
        Sex::Unknown => &decoded[2],
    }
}

// ── Helper functions ─────────────────────────────────────────────────────

/// Format a "birth-death" lifespan string from optional years.
///
/// Each year carries its own precision mark, GeneWeb-style, so a card reads
/// `ca 1849-< 1917` — "about 1849 to before 1917" — instead of flattening two
/// hedged dates into a pair of bare numbers that claim more than the records
/// do. See [`DateQualifier::short_prefix`].
pub(crate) fn format_lifespan(
    birth: Option<QualifiedYear>,
    death: Option<QualifiedYear>,
) -> String {
    join_lifespan(birth, death, QualifiedYear::wide)
}

/// [`format_lifespan`] in the form that always spends one year per date.
///
/// A range gives up its far end here but keeps the `..`/`|` mark saying it is
/// one, so the card understates rather than misleads.
fn format_lifespan_narrow(birth: Option<QualifiedYear>, death: Option<QualifiedYear>) -> String {
    join_lifespan(birth, death, QualifiedYear::narrow)
}

/// The `birth-death` shape both forms share. A missing year keeps its dash:
/// "born then, and nothing is known after" is not the same as saying nothing.
fn join_lifespan(
    birth: Option<QualifiedYear>,
    death: Option<QualifiedYear>,
    render: impl Fn(&QualifiedYear) -> String,
) -> String {
    match (birth, death) {
        (Some(b), Some(d)) => format!("{}-{}", render(&b), render(&d)),
        (Some(b), None) => format!("{}-", render(&b)),
        (None, Some(d)) => format!("-{}", render(&d)),
        _ => String::new(),
    }
}

/// The widest lifespan that fits `max_width_px`, and the text for it.
///
/// Ranges are worth their width — "between 1691 and 1693" is a fact a card can
/// carry — but two of them run to 105.8px, past even the 175px card's 105px
/// text column, and a compact card only has 72px. So the wide form is used
/// when it fits and the narrow one when it does not, rather than compressing
/// glyphs to the point of illegibility. The tooltip always has the full text.
fn fit_lifespan(
    birth: Option<QualifiedYear>,
    death: Option<QualifiedYear>,
    max_width_px: f32,
    font_size_px: f32,
) -> String {
    let wide = format_lifespan(birth, death);
    if crate::utils::estimate_text_width_px(&wide, font_size_px) <= max_width_px {
        return wide;
    }
    format_lifespan_narrow(birth, death)
}

/// The lifespan spelled out for a tooltip — « Environ 1849 – Avant 1917 » —
/// so the terse marks on the card have somewhere to explain themselves.
///
/// Empty when neither year is qualified: a tooltip that only repeats the text
/// already on the card is noise, and an empty string is how the caller knows
/// to omit the `<title>` entirely.
pub(crate) fn lifespan_tooltip(
    i18n: &I18n,
    birth: Option<QualifiedYear>,
    death: Option<QualifiedYear>,
) -> String {
    if !matches!(birth, Some(y) if y.qualifier != DateQualifier::Exact)
        && !matches!(death, Some(y) if y.qualifier != DateQualifier::Exact)
    {
        return String::new();
    }
    let spell = |y: QualifiedYear| match (y.qualifier, y.year2) {
        (DateQualifier::Exact, _) => y.year.to_string(),
        // A range reads as one phrase — « Entre 1691 et 1693 » — rather than
        // as a qualifier stuck in front of a lone year.
        (DateQualifier::Between, Some(year2)) => format!(
            "{} {} {} {}",
            i18n.t("date_qualifier.between"),
            y.year,
            i18n.t("common.and"),
            year2
        ),
        (DateQualifier::Or, Some(year2)) => format!(
            "{} {} {}",
            y.year,
            i18n.t("date_qualifier.or").to_lowercase(),
            year2
        ),
        (q, _) => format!("{} {}", i18n.t(&format!("date_qualifier.{q}")), y.year),
    };
    match (birth, death) {
        (Some(b), Some(d)) => format!("{} \u{2013} {}", spell(b), spell(d)),
        (Some(b), None) => spell(b),
        (None, Some(d)) => spell(d),
        _ => String::new(),
    }
}

/// CSS variable for the gender-coded card border stroke.
fn gender_stroke(sex: Sex) -> &'static str {
    match sex {
        Sex::Male => "var(--pn-male-line)",
        Sex::Female => "var(--pn-female-line)",
        _ => "var(--pn-border)",
    }
}

/// CSS variable for the card background fill.
fn card_bg(is_focus: bool, is_sibling: bool) -> &'static str {
    if is_focus {
        "var(--pn-root-bg)"
    } else if is_sibling {
        "var(--pn-spouse-bg)"
    } else {
        "var(--pn-bg)"
    }
}

/// Returns `(icon, css_class, i18n_key)` for an event type.
///
/// The third element is an i18n key that must be resolved via `i18n.t()`.
fn event_ui(et: EventType) -> (&'static str, &'static str, &'static str) {
    match et {
        EventType::Birth => ("\u{2726}", "ev-ic ev-ic-birth", "event.type.birth"),
        EventType::Baptism => ("\u{271F}", "ev-ic ev-ic-birth", "event.type.baptism"),
        EventType::Death => ("\u{271D}", "ev-ic ev-ic-death", "event.type.death"),
        EventType::Burial => ("\u{26B0}", "ev-ic ev-ic-death", "event.type.burial"),
        EventType::Cremation => ("\u{271D}", "ev-ic ev-ic-death", "event.type.cremation"),
        EventType::Marriage => ("\u{1F48D}", "ev-ic ev-ic-marry", "event.type.marriage"),
        EventType::Engagement => ("\u{1F48D}", "ev-ic ev-ic-marry", "event.type.engagement"),
        EventType::MarriageBann => ("\u{1F48D}", "ev-ic ev-ic-marry", "event.short.banns"),
        EventType::MarriageContract => ("\u{1F48D}", "ev-ic ev-ic-marry", "event.short.contract"),
        EventType::MarriageLicense => ("\u{1F48D}", "ev-ic ev-ic-marry", "event.short.license"),
        EventType::MarriageSettlement => {
            ("\u{1F48D}", "ev-ic ev-ic-marry", "event.short.settlement")
        }
        EventType::Divorce => ("\u{2696}", "ev-ic ev-ic-other", "event.type.divorce"),
        EventType::Annulment => ("\u{2696}", "ev-ic ev-ic-other", "event.type.annulment"),
        EventType::CivilUnion => ("\u{1F48D}", "ev-ic ev-ic-marry", "event.type.civil_union"),
        EventType::Separation => ("\u{2696}", "ev-ic ev-ic-other", "event.type.separation"),
        EventType::DivorceFiled => ("\u{2696}", "ev-ic ev-ic-other", "event.type.divorce_filed"),
        EventType::Census => ("\u{1F4DC}", "ev-ic ev-ic-other", "event.type.census"),
        EventType::Occupation => ("\u{2692}", "ev-ic ev-ic-other", "event.type.occupation"),
        EventType::Residence => ("\u{1F3E1}", "ev-ic ev-ic-other", "event.type.residence"),
        EventType::Will => ("\u{1F4DC}", "ev-ic ev-ic-other", "event.type.will"),
        EventType::Probate => ("\u{1F4DC}", "ev-ic ev-ic-other", "event.type.probate"),
        EventType::Adoption => ("\u{1FAC2}", "ev-ic ev-ic-other", "event.type.adoption"),
        EventType::Education => ("\u{1F393}", "ev-ic ev-ic-other", "event.type.education"),
        EventType::MarriagesCount => (
            "\u{1F48D}",
            "ev-ic ev-ic-other",
            "event.type.marriages_count",
        ),
        EventType::Religion => ("\u{271F}", "ev-ic ev-ic-other", "event.type.religion"),
        // Types without a dedicated icon still retain their localized name.
        _ => ("\u{25C6}", "ev-ic ev-ic-other", event_type_label_key(et)),
    }
}

// ── Data model ───────────────────────────────────────────────────────────

/// Data needed to render the pedigree chart, pre-computed from API data.
#[derive(Clone, Debug)]
pub struct PedigreeData {
    pub persons: HashMap<Uuid, Person>,
    pub names: HashMap<Uuid, Vec<PersonName>>,
    pub spouses_by_family: HashMap<Uuid, Vec<FamilySpouse>>,
    pub children_by_family: HashMap<Uuid, Vec<FamilyChild>>,
    pub families_as_child: HashMap<Uuid, Vec<Uuid>>,
    pub families_as_spouse: HashMap<Uuid, Vec<Uuid>>,
    pub events_by_person: HashMap<Uuid, Vec<DomainEvent>>,
    pub events_by_family: HashMap<Uuid, Vec<DomainEvent>>,
    pub places: HashMap<Uuid, Place>,
    /// The person this tree identifies as the current user.
    pub self_person_id: Option<Uuid>,
    /// How many generations each way the data was fetched for, when it came
    /// from a pedigree. The chart never lays out deeper than this: a deeper
    /// view is waiting on a fetch, and has nothing more to draw until then.
    pub ancestor_depth_loaded: Option<usize>,
    pub descendant_depth_loaded: Option<usize>,
}

/// A [`PedigreeData`] shared between the page that assembled it, the handlers
/// that read it and the chart that draws it, compared by identity (see
/// [`Shared`]).
pub type SharedPedigree = Shared<PedigreeData>;

/// The SOSA root's ancestors, shared by identity: the set runs to thousands
/// of persons on a large tree, and the chart compares it on every render to
/// decide whether its layout is still current.
pub type AncestorSet = Shared<HashSet<Uuid>>;

/// person_id → the picture their portrait is drawn from, built by
/// [`crate::api::ApiClient::portrait_map_for_ids`]. Absent means no portrait:
/// the card draws the silhouette rather than asking for bytes that do not
/// exist. A portrait that arrives as a region of a larger photograph carries
/// that region, and the card cuts it itself.
///
/// Kept apart from the [`PedigreeData`] and the layout: portraits arrive
/// after the pedigree, and on the web each one is a `data:` URL tens of
/// kilobytes long. Folded into the pedigree, their arrival built a new one,
/// which laid the chart out again and redrew every card, and each picture
/// was copied into every node of the layout.
pub type Portraits = Shared<HashMap<Uuid, CroppedSource>>;

/// The portraits a chart's cards draw, provided by the chart to the pictures
/// inside it. A signal, so that portraits arriving redraw only the pictures
/// that read it — not the layout, the canvas or the cards around them.
#[derive(Clone, Copy)]
struct ChartPortraits(Signal<Option<Portraits>>);

/// Provides `portraits` to the pictures of the chart being rendered, and
/// keeps them current when a later render passes others.
fn use_chart_portraits(portraits: Option<&Portraits>) {
    let mut provided = use_context_provider(|| ChartPortraits(Signal::new(portraits.cloned()))).0;
    if provided.peek().as_ref() != portraits {
        provided.set(portraits.cloned());
    }
}

/// The portrait of `person` among the chart's, when it has one.
fn chart_portrait(person: Uuid) -> Option<CroppedSource> {
    let ChartPortraits(portraits) = try_use_context::<ChartPortraits>()?;
    portraits
        .read()
        .as_ref()
        .and_then(|portraits| portraits.get(&person).cloned())
}

/// A card's picture: the person's portrait, else the silhouette of their sex.
///
/// A component of its own, the only part of a card that reads the chart's
/// portraits, so their arrival redraws the pictures and nothing else.
#[component]
fn CardPortrait(person: Uuid, sex: Sex, x: f64, y: f64, width: f64, height: f64) -> Element {
    let silhouette = CroppedSource::silhouette(sex);
    let image = chart_portrait(person).unwrap_or_else(|| silhouette.clone());
    rsx! {
        CroppedSvgImage { image, x, y, width, height, fallback: silhouette }
    }
}

/// Turn a projection event back into the domain shape the chart and the events
/// panel already speak.
///
/// Every date column travels: the value, the second value an `Or`/`Between`
/// needs, the calendar, the qualifier and the sort date. That is the whole
/// point — the panel renders through `format_event_date`, which can only write
/// « entre 11 nov. 1691 et 20 août 1693 » if all of them arrive.
fn profile_event_to_domain(
    pe: &ProfileEvent,
    tree_id: Uuid,
    person_id: Option<Uuid>,
    family_id: Option<Uuid>,
    now: chrono::DateTime<chrono::Utc>,
) -> DomainEvent {
    DomainEvent {
        id: pe.event_id,
        tree_id,
        event_type: pe.event_type,
        date_value: pe.date_value.clone(),
        date_sort: pe.date_sort,
        date_qualifier: pe.date_qualifier,
        date_value2: pe.date_value2.clone(),
        calendar: pe.calendar,
        cause: None,
        place_id: pe.place_id,
        person_id,
        family_id,
        // The projection denormalizes the place *name* beside its id. The chart
        // resolves ids through its own `places` map, which holds only the
        // places the pedigree pulled in, so the name is kept here as the
        // fallback the panel reads when the id resolves to nothing.
        description: pe.description.clone().or_else(|| pe.place_name.clone()),
        created_at: now,
        updated_at: now,
        deleted_at: None,
    }
}

/// What a pedigree projection says about one person: enough to draw a card
/// and list their birth and death.
struct PersonSummary<'a> {
    person_id: Uuid,
    sex: Sex,
    given_names: &'a Option<String>,
    surname: &'a Option<String>,
    birth: Option<&'a ProfileEvent>,
    death: Option<&'a ProfileEvent>,
}

/// The synthetic domain records built from [`PersonSummary`]s: a person, one
/// birth name, and their birth and death events.
struct SummarisedPeople {
    tree_id: Uuid,
    now: chrono::DateTime<chrono::Utc>,
    persons: HashMap<Uuid, Person>,
    names: HashMap<Uuid, Vec<PersonName>>,
    events_by_person: HashMap<Uuid, Vec<DomainEvent>>,
}

impl SummarisedPeople {
    fn new(tree_id: Uuid, now: chrono::DateTime<chrono::Utc>) -> Self {
        Self {
            tree_id,
            now,
            persons: HashMap::new(),
            names: HashMap::new(),
            events_by_person: HashMap::new(),
        }
    }

    /// Record one person, replacing any earlier record of them.
    fn add(&mut self, summary: PersonSummary<'_>) {
        let (tree_id, now, person_id) = (self.tree_id, self.now, summary.person_id);
        self.persons.insert(
            person_id,
            Person {
                id: person_id,
                tree_id,
                sex: summary.sex,
                privacy: Privacy::default(),
                portrait_media_id: None,
                portrait_vignette_id: None,
                created_at: now,
                updated_at: now,
                deleted_at: None,
            },
        );
        let name = PersonName {
            id: Uuid::nil(),
            person_id,
            name_type: oxidgene_core::NameType::Birth,
            given_names: summary.given_names.clone(),
            surname: summary.surname.clone(),
            surname_prefix: None,
            prefix: None,
            suffix: None,
            nickname: None,
            is_primary: true,
            sort_order: 0,
            created_at: now,
            updated_at: now,
        };
        self.names.insert(person_id, vec![name]);

        let events: Vec<DomainEvent> = [summary.birth, summary.death]
            .into_iter()
            .flatten()
            .map(|pe| profile_event_to_domain(pe, tree_id, Some(person_id), None, now))
            .collect();
        if !events.is_empty() {
            self.events_by_person.insert(person_id, events);
        }
    }
}

/// Who belongs to which family, as spouse or as child, in both directions.
struct FamilyLinks {
    spouses_by_family: HashMap<Uuid, Vec<FamilySpouse>>,
    children_by_family: HashMap<Uuid, Vec<FamilyChild>>,
    families_as_child: HashMap<Uuid, Vec<Uuid>>,
    families_as_spouse: HashMap<Uuid, Vec<Uuid>>,
}

impl FamilyLinks {
    /// Read the links of a pedigree's families.
    ///
    /// PedigreeFamily carries full family membership (spouses + children),
    /// covering childless couples that produce no PedigreeEdge. The edges
    /// supplement it with each child's type. A spouse's role comes from their
    /// sex in `persons`.
    fn from_pedigree(pedigree: &Pedigree, persons: &HashMap<Uuid, Person>) -> Self {
        // Build child_type lookup from edges.
        let mut child_type_map: HashMap<(Uuid, Uuid), ChildType> = HashMap::new();
        for edge in &pedigree.edges {
            child_type_map.insert((edge.family_id, edge.child_id), edge.edge_type);
        }

        let mut links = Self {
            spouses_by_family: HashMap::new(),
            children_by_family: HashMap::new(),
            families_as_child: HashMap::new(),
            families_as_spouse: HashMap::new(),
        };
        for (family_id, family) in &pedigree.families {
            for (i, &spouse_id) in family.spouse_ids.iter().enumerate() {
                let role = spouse_role(persons.get(&spouse_id).map(|p| p.sex), i);
                links.add_spouse(*family_id, spouse_id, role, i);
            }
            for (i, &child_id) in family.children_ids.iter().enumerate() {
                let child_type = child_type_map
                    .get(&(*family_id, child_id))
                    .copied()
                    .unwrap_or(ChildType::Biological);
                links.add_child(*family_id, child_id, child_type, i);
            }
        }
        sort_and_dedup_values(&mut links.families_as_spouse);
        sort_and_dedup_values(&mut links.families_as_child);
        links
    }

    fn add_spouse(&mut self, family_id: Uuid, person_id: Uuid, role: SpouseRole, index: usize) {
        self.spouses_by_family
            .entry(family_id)
            .or_default()
            .push(FamilySpouse {
                id: Uuid::nil(),
                family_id,
                person_id,
                role,
                sort_order: index as i32,
            });
        self.families_as_spouse
            .entry(person_id)
            .or_default()
            .push(family_id);
    }

    fn add_child(&mut self, family_id: Uuid, person_id: Uuid, child_type: ChildType, index: usize) {
        self.children_by_family
            .entry(family_id)
            .or_default()
            .push(FamilyChild {
                id: Uuid::nil(),
                family_id,
                person_id,
                child_type,
                sort_order: index as i32,
            });
        self.families_as_child
            .entry(person_id)
            .or_default()
            .push(family_id);
    }
}

/// A spouse's role, told by their sex, or else by their place in the couple:
/// the first one husband, any other wife.
fn spouse_role(sex: Option<Sex>, index: usize) -> SpouseRole {
    match sex {
        Some(Sex::Male) => SpouseRole::Husband,
        Some(Sex::Female) => SpouseRole::Wife,
        _ if index == 0 => SpouseRole::Husband,
        _ => SpouseRole::Wife,
    }
}

/// Sort every list of the map and drop its duplicates.
fn sort_and_dedup_values(map: &mut HashMap<Uuid, Vec<Uuid>>) {
    for ids in map.values_mut() {
        ids.sort();
        ids.dedup();
    }
}

impl PedigreeData {
    /// Build chart data from a [`Pedigree`] returned by the projection API.
    ///
    /// Creates synthetic domain objects (Person, PersonName, Event) from the
    /// denormalized pedigree nodes for layout and rendering.
    pub fn from_pedigree(pedigree: &Pedigree) -> Self {
        use chrono::Utc;

        let tree_id = pedigree.tree_id;
        let mut people = SummarisedPeople::new(tree_id, Utc::now());

        // ── Persons, names, and the birth/death events the projection
        // carries whole ──
        for node in pedigree.persons.values() {
            people.add(PersonSummary {
                person_id: node.person_id,
                sex: node.sex,
                given_names: &node.given_names,
                // Projection surnames already carry their particle, so there
                // is nothing to re-attach here.
                surname: &node.surname,
                birth: node.birth.as_ref(),
                death: node.death.as_ref(),
            });
        }

        // ── Family relationships from PedigreeFamily + PedigreeEdge ──
        //
        // Roles are assigned before the members outside the pedigree window
        // are added below: only the pedigree's own persons tell a spouse's
        // role by sex.
        let links = FamilyLinks::from_pedigree(pedigree, &people.persons);

        // ── Reconstruct family events from the pedigree payload ──
        let mut events_by_family: HashMap<Uuid, Vec<DomainEvent>> = HashMap::new();
        for (family_id, events) in &pedigree.family_events {
            let domain_events: Vec<DomainEvent> = events
                .iter()
                .map(|ce| profile_event_to_domain(ce, tree_id, None, Some(*family_id), people.now))
                .collect();
            events_by_family.insert(*family_id, domain_events);
        }

        // ── Synthetic events + names for family members outside the pedigree window ──
        for member in pedigree
            .families
            .values()
            .flat_map(|family| &family.members)
        {
            // Skip members already in the pedigree persons map.
            if people.persons.contains_key(&member.person_id) {
                continue;
            }
            // Same conversion as the pedigree nodes above.
            people.add(PersonSummary {
                person_id: member.person_id,
                sex: member.sex,
                given_names: &member.given_names,
                surname: &member.surname,
                birth: member.birth.as_ref(),
                death: member.death.as_ref(),
            });
        }

        Self {
            persons: people.persons,
            names: people.names,
            spouses_by_family: links.spouses_by_family,
            children_by_family: links.children_by_family,
            families_as_child: links.families_as_child,
            families_as_spouse: links.families_as_spouse,
            events_by_person: people.events_by_person,
            events_by_family,
            places: HashMap::new(),
            self_person_id: None,
            ancestor_depth_loaded: Some(pedigree.ancestor_depth_loaded as usize),
            descendant_depth_loaded: Some(pedigree.descendant_depth_loaded as usize),
        }
    }

    /// Compute the set of all ancestors of a given person (excluding the person).
    pub fn ancestor_set(&self, person_id: Uuid) -> std::collections::HashSet<Uuid> {
        let mut result = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(person_id);
        while let Some(pid) = queue.pop_front() {
            let (father, mother) = self.parents_of(pid);
            if let Some(f) = father
                && result.insert(f)
            {
                queue.push_back(f);
            }
            if let Some(m) = mother
                && result.insert(m)
            {
                queue.push_back(m);
            }
        }
        result
    }

    /// The person's spouses, in the order of their unions, each once.
    pub(crate) fn spouses_of(&self, person_id: Uuid) -> Vec<Uuid> {
        let mut seen = HashSet::new();
        self.families_as_spouse
            .get(&person_id)
            .into_iter()
            .flatten()
            .filter_map(|fid| self.spouses_by_family.get(fid))
            .flatten()
            .map(|spouse| spouse.person_id)
            .filter(|pid| *pid != person_id && seen.insert(*pid))
            .collect()
    }

    /// The person's children, in the order of their unions and of births,
    /// each once.
    pub(crate) fn children_of(&self, person_id: Uuid) -> Vec<Uuid> {
        let mut seen = HashSet::new();
        self.families_as_spouse
            .get(&person_id)
            .into_iter()
            .flatten()
            .flat_map(|fid| {
                let mut children = self
                    .children_by_family
                    .get(fid)
                    .cloned()
                    .unwrap_or_default();
                children.sort_by_key(|child| child.sort_order);
                children
            })
            .map(|child| child.person_id)
            .filter(|pid| seen.insert(*pid))
            .collect()
    }

    /// The person's father and mother, as far as the tree records them.
    pub(crate) fn parents_of(&self, person_id: Uuid) -> (Option<Uuid>, Option<Uuid>) {
        let Some(family_ids) = self.families_as_child.get(&person_id) else {
            return (None, None);
        };
        let Some(fid) = family_ids.first() else {
            return (None, None);
        };
        let Some(spouses) = self.spouses_by_family.get(fid) else {
            return (None, None);
        };

        let mut father = None;
        let mut mother = None;

        for sp in spouses {
            if let Some(person) = self.persons.get(&sp.person_id) {
                match person.sex {
                    Sex::Male => {
                        if father.is_none() {
                            father = Some(sp.person_id);
                        }
                    }
                    Sex::Female => {
                        if mother.is_none() {
                            mother = Some(sp.person_id);
                        }
                    }
                    Sex::Unknown => {
                        if father.is_none() {
                            father = Some(sp.person_id);
                        } else if mother.is_none() {
                            mother = Some(sp.person_id);
                        }
                    }
                }
            }
        }

        (father, mother)
    }

    fn sex_of(&self, person_id: Uuid) -> Sex {
        self.persons
            .get(&person_id)
            .map(|p| p.sex)
            .unwrap_or(Sex::Unknown)
    }

    pub fn name_parts(&self, person_id: Uuid) -> (Option<String>, Option<String>, Option<String>) {
        let Some(names) = self.names.get(&person_id) else {
            return (None, None, None);
        };
        let name = names
            .iter()
            .find(|n| n.is_primary)
            .or_else(|| names.first());
        match name {
            // Full surname: card labels are display, so the particle belongs.
            // This is the single choke point every `PersonNode` label flows
            // through, so fixing it here covers the whole canvas.
            Some(n) => (n.given_names.clone(), n.full_surname(), n.nickname.clone()),
            None => (None, None, None),
        }
    }

    /// Resolve a full display name for a person.
    ///
    /// Takes `i18n` because the fallback for a person with no usable name is
    /// itself a translated string.
    pub fn display_name(&self, person_id: Uuid, i18n: &I18n) -> String {
        crate::utils::resolve_name(person_id, &self.names, i18n)
    }

    /// The year to date this person's life *from*, with its precision.
    ///
    /// Prefers a birth, falls back to a baptism, and — like the projection —
    /// skips over a dateless stub rather than letting it mask a dated
    /// sacrament: `qualified_year()` is `None` for an event with no date, so
    /// `find_map` simply carries on to the next candidate.
    pub(crate) fn qualified_birth_year(&self, person_id: Uuid) -> Option<QualifiedYear> {
        self.first_qualified_year(person_id, &[EventType::Birth, EventType::Baptism])
    }

    /// The year to date this person's life *to*. See [`Self::qualified_birth_year`].
    pub(crate) fn qualified_death_year(&self, person_id: Uuid) -> Option<QualifiedYear> {
        self.first_qualified_year(person_id, &[EventType::Death, EventType::Burial])
    }

    fn first_qualified_year(
        &self,
        person_id: Uuid,
        preference: &[EventType],
    ) -> Option<QualifiedYear> {
        let events = self.events_by_person.get(&person_id)?;
        preference.iter().find_map(|wanted| {
            events
                .iter()
                .filter(|e| e.event_type == *wanted)
                .find_map(|e| e.qualified_year())
        })
    }

    fn marriage_date_for_family(&self, family_id: Uuid) -> Option<String> {
        let events = self.events_by_family.get(&family_id)?;
        events
            .iter()
            .find(|e| e.event_type == EventType::Marriage)
            .and_then(DomainEvent::qualified_year)
            .map(|year| year.to_string())
    }

    /// Resolve a place_id to its name.
    fn place_name(&self, place_id: Uuid) -> Option<&str> {
        self.places.get(&place_id).map(|p| p.name.as_str())
    }

    /// Get unions for a person: Vec<(family_id, partner_name, marriage_year)>.
    pub fn unions_for_person(&self, person_id: Uuid, i18n: &I18n) -> Vec<(Uuid, String, String)> {
        let Some(family_ids) = self.families_as_spouse.get(&person_id) else {
            return vec![];
        };
        family_ids
            .iter()
            .map(|&fid| {
                let partner_name = self
                    .spouses_by_family
                    .get(&fid)
                    .and_then(|sps| {
                        sps.iter()
                            .find(|s| s.person_id != person_id)
                            .map(|s| s.person_id)
                    })
                    .map(|pid| {
                        let (g, s, _) = self.name_parts(pid);
                        let gs = g.unwrap_or_default();
                        let ss = s.unwrap_or_default();
                        if gs.is_empty() && ss.is_empty() {
                            i18n.t("couple.unknown_spouse")
                        } else {
                            format!("{} {}", gs, ss).trim().to_string()
                        }
                    })
                    .unwrap_or_else(|| i18n.t("couple.unknown_spouse"));
                let marriage_year = self.marriage_date_for_family(fid).unwrap_or_default();
                (fid, partner_name, marriage_year)
            })
            .collect()
    }
}

// ── RT Layout Engine ─────────────────────────────────────────────────────
//
// Reingold-Tilford (Buchheim variant) algorithm.
// All traversals use explicit stacks to avoid stack overflows on deep trees.

/// SOSA badge type for a node.
#[derive(Clone, Debug, PartialEq)]
enum SosaBadge {
    None,
    Root,
    Direct,
}

/// A node in the layout tree arena.
#[derive(Clone, Debug)]
struct TreeNode {
    id: Option<Uuid>,
    depth: i32,
    sex: Sex,
    label_surname: String,
    label_given: String,
    birth_year: Option<QualifiedYear>,
    death_year: Option<QualifiedYear>,
    sosa_badge: SosaBadge,
    is_self: bool,
    /// Indices into the TreeNode arena of children (for RT traversal).
    children: Vec<usize>,
    /// Spouse node indices (siblings in RT terms).
    siblings: Vec<usize>,
    parent2: Option<usize>,
    /// 0 = male-first ordering, 1 = female-first.
    after: i32,
    is_sibling: bool,
    before_sibling: bool,
    after_sibling: bool,
    x: f64,
    y: f64,
    /// For empty ancestor slots: which child they belong to.
    child_of: Option<Uuid>,
    /// For empty ancestor slots: is this the father slot?
    is_father: bool,
}

impl TreeNode {
    #[allow(clippy::too_many_arguments)]
    fn new_real(
        id: Uuid,
        depth: i32,
        sex: Sex,
        given: String,
        surname: String,
        birth_year: Option<QualifiedYear>,
        death_year: Option<QualifiedYear>,
        sosa_badge: SosaBadge,
        is_self: bool,
        after: i32,
        before_sibling: bool,
        after_sibling: bool,
    ) -> Self {
        Self {
            id: Some(id),
            depth,
            sex,
            label_surname: surname,
            label_given: given,
            birth_year,
            death_year,
            sosa_badge,
            is_self,
            children: vec![],
            siblings: vec![],
            parent2: None,
            after,
            is_sibling: false,
            before_sibling,
            after_sibling,
            x: 0.0,
            y: 0.0,
            child_of: None,
            is_father: false,
        }
    }

    fn new_empty(depth: i32, child_of: Option<Uuid>, is_father: bool) -> Self {
        Self {
            id: None,
            depth,
            sex: Sex::Unknown,
            label_surname: String::new(),
            label_given: String::new(),
            birth_year: None,
            death_year: None,
            sosa_badge: SosaBadge::None,
            is_self: false,
            children: vec![],
            siblings: vec![],
            parent2: None,
            after: 0,
            is_sibling: false,
            before_sibling: false,
            after_sibling: false,
            x: 0.0,
            y: 0.0,
            child_of,
            is_father,
        }
    }
}

/// Working node for the Reingold-Tilford algorithm.
#[derive(Clone, Debug)]
struct WrapNode {
    orig: usize,
    parent: Option<usize>,
    children: Vec<usize>,
    siblings: Vec<usize>,
    parent2: Option<usize>,
    z: f64,
    m: f64,
    c: f64,
    s: f64,
    t: Option<usize>,
    /// ancestor pointer (self-index by default)
    a: usize,
    i: usize,
}

/// Connectivity for a single person extracted from pedigree data.
struct PersonNode {
    sex: Sex,
    given: String,
    surname: String,
    birth_year: Option<QualifiedYear>,
    death_year: Option<QualifiedYear>,
    sosa_badge: SosaBadge,
    is_self: bool,
}

impl PersonNode {
    fn from_data(
        id: Uuid,
        data: &PedigreeData,
        sosa_root_id: Option<Uuid>,
        sosa_ancestors: &HashSet<Uuid>,
    ) -> Self {
        let sex = data.sex_of(id);
        let (given, surname, _) = data.name_parts(id);
        let given = given.unwrap_or_default();
        let surname = surname.unwrap_or_default();

        // Same resolution as the side panel, so a card and the panel beside it
        // never disagree about when someone lived.
        let birth_year = data.qualified_birth_year(id);
        let death_year = data.qualified_death_year(id);

        let sosa_badge = if sosa_root_id == Some(id) {
            SosaBadge::Root
        } else if sosa_ancestors.contains(&id) {
            SosaBadge::Direct
        } else {
            SosaBadge::None
        };

        PersonNode {
            sex,
            given,
            surname,
            birth_year,
            death_year,
            sosa_badge,
            is_self: data.self_person_id == Some(id),
        }
    }

    /// The card of this person drawn at (`x`, `y`), outside the tree layout:
    /// one of the root's siblings on the root's row, or an ancestor in one of
    /// the ancestor-only views.
    fn card_at(self, id: Uuid, x: f64, y: f64) -> LayoutNode {
        LayoutNode {
            id: Some(id),
            x,
            y,
            sex: self.sex,
            label_surname: self.surname,
            label_given: self.given,
            birth_year: self.birth_year,
            death_year: self.death_year,
            sosa_badge: self.sosa_badge,
            is_self: self.is_self,
            is_compact: false,
            child_of: None,
            is_father: false,
            is_sibling: false,
            has_more_relations: false,
        }
    }
}

/// Build the ascending (ancestor) tree into the arena.
/// Returns index of root node in the arena.
fn build_ascending_tree(
    root_id: Uuid,
    data: &PedigreeData,
    max_ascendants: usize,
    sosa_root_id: Option<Uuid>,
    sosa_ancestors: &HashSet<Uuid>,
) -> Vec<TreeNode> {
    let mut arena: Vec<TreeNode> = Vec::new();

    // Check if root has siblings (children of same parent family).
    let (before_sibling, after_sibling) = {
        let siblings = get_siblings(root_id, data);
        let idx = siblings.iter().position(|&s| s == root_id).unwrap_or(0);
        (idx > 0, idx < siblings.len().saturating_sub(1))
    };

    let root_pn = PersonNode::from_data(root_id, data, sosa_root_id, sosa_ancestors);
    let root_after = if root_pn.sex == Sex::Female { 1 } else { 0 };
    arena.push(TreeNode::new_real(
        root_id,
        0,
        root_pn.sex,
        root_pn.given,
        root_pn.surname,
        root_pn.birth_year,
        root_pn.death_year,
        root_pn.sosa_badge,
        root_pn.is_self,
        root_after,
        before_sibling,
        after_sibling,
    ));

    // Iterative BFS to build ancestor tree.
    // Stack items: (arena_index, current_depth).
    let mut work: Vec<(usize, i32)> = vec![(0, 0)];
    while let Some((node_idx, depth)) = work.pop() {
        if depth.unsigned_abs() as usize >= max_ascendants {
            continue;
        }
        let pid = match arena[node_idx].id {
            Some(p) => p,
            None => continue,
        };
        let (father_id, mother_id) = data.parents_of(pid);
        let child_depth = depth - 1;

        let mut child_indices = Vec::new();

        if let Some(fid) = father_id {
            let pn = PersonNode::from_data(fid, data, sosa_root_id, sosa_ancestors);
            let idx = arena.len();
            arena.push(TreeNode::new_real(
                fid,
                child_depth,
                pn.sex,
                pn.given,
                pn.surname,
                pn.birth_year,
                pn.death_year,
                pn.sosa_badge,
                pn.is_self,
                0,
                false,
                false,
            ));
            child_indices.push(idx);
            work.push((idx, child_depth));
        } else {
            // Empty father slot (always shown when father is missing).
            let idx = arena.len();
            arena.push(TreeNode::new_empty(child_depth, Some(pid), true));
            child_indices.push(idx);
        }

        if let Some(mid) = mother_id {
            let pn = PersonNode::from_data(mid, data, sosa_root_id, sosa_ancestors);
            let idx = arena.len();
            arena.push(TreeNode::new_real(
                mid,
                child_depth,
                pn.sex,
                pn.given,
                pn.surname,
                pn.birth_year,
                pn.death_year,
                pn.sosa_badge,
                pn.is_self,
                1,
                false,
                false,
            ));
            child_indices.push(idx);
            work.push((idx, child_depth));
        } else {
            // Empty mother slot (always shown when mother is missing).
            let idx = arena.len();
            arena.push(TreeNode::new_empty(child_depth, Some(pid), false));
            child_indices.push(idx);
        }

        arena[node_idx].children = child_indices;
    }

    arena
}

/// Get ordered siblings of a person from their parent family.
fn get_siblings(pid: Uuid, data: &PedigreeData) -> Vec<Uuid> {
    let Some(fids) = data.families_as_child.get(&pid) else {
        return vec![pid];
    };
    let Some(&fid) = fids.first() else {
        return vec![pid];
    };
    let children: Vec<Uuid> = data
        .children_by_family
        .get(&fid)
        .map(|cs| cs.iter().map(|c| c.person_id).collect())
        .unwrap_or_default();
    if children.is_empty() {
        vec![pid]
    } else {
        children
    }
}

/// Build the descending (descendant) tree into the arena.
fn build_descending_tree(
    root_id: Uuid,
    data: &PedigreeData,
    max_descendants: usize,
    sosa_root_id: Option<Uuid>,
    sosa_ancestors: &HashSet<Uuid>,
) -> Vec<TreeNode> {
    let mut arena: Vec<TreeNode> = Vec::new();

    let (before_sibling, after_sibling) = {
        let siblings = get_siblings(root_id, data);
        let idx = siblings.iter().position(|&s| s == root_id).unwrap_or(0);
        (idx > 0, idx < siblings.len().saturating_sub(1))
    };

    let root_pn = PersonNode::from_data(root_id, data, sosa_root_id, sosa_ancestors);
    let root_after = if root_pn.sex == Sex::Female { 1 } else { 0 };
    arena.push(TreeNode::new_real(
        root_id,
        0,
        root_pn.sex,
        root_pn.given,
        root_pn.surname,
        root_pn.birth_year,
        root_pn.death_year,
        root_pn.sosa_badge,
        root_pn.is_self,
        root_after,
        before_sibling,
        after_sibling,
    ));

    // Iterative DFS to build descendant tree.
    let mut visited: HashSet<Uuid> = HashSet::new();
    let mut work: Vec<(usize, i32)> = vec![(0, 0)];

    while let Some((node_idx, depth)) = work.pop() {
        let pid = match arena[node_idx].id {
            Some(p) => p,
            None => continue,
        };
        if visited.contains(&pid) {
            continue;
        }
        visited.insert(pid);

        let family_ids: Vec<Uuid> = data
            .families_as_spouse
            .get(&pid)
            .cloned()
            .unwrap_or_default();

        for fid in family_ids {
            let spouse_id = data
                .spouses_by_family
                .get(&fid)
                .and_then(|sps| sps.iter().find(|s| s.person_id != pid))
                .map(|s| s.person_id);

            // Attach point for children: the newly created spouse node, if
            // any. An unrecorded/unknown co-parent must not hide the
            // children — many older records name only one parent. In that
            // case we still render an empty "+" placeholder for the missing
            // spouse, mirroring the always-shown empty parent slots on the
            // ascending side.
            let spouse_arena_idx = match spouse_id {
                Some(sid) if visited.contains(&sid) => continue,
                Some(sid) => {
                    let spn = PersonNode::from_data(sid, data, sosa_root_id, sosa_ancestors);
                    let spouse_after = if spn.sex == Sex::Female { 1 } else { 0 };
                    let spouse_arena_idx = arena.len();
                    let mut spouse_node = TreeNode::new_real(
                        sid,
                        depth,
                        spn.sex,
                        spn.given,
                        spn.surname,
                        spn.birth_year,
                        spn.death_year,
                        spn.sosa_badge,
                        spn.is_self,
                        spouse_after,
                        false,
                        false,
                    );
                    spouse_node.is_sibling = true;
                    arena.push(spouse_node);
                    arena[node_idx].siblings.push(spouse_arena_idx);
                    Some(spouse_arena_idx)
                }
                None => {
                    let empty_arena_idx = arena.len();
                    arena.push(TreeNode::new_empty(depth, Some(pid), false));
                    arena[node_idx].siblings.push(empty_arena_idx);
                    None
                }
            };

            if depth < max_descendants as i32 {
                let children: Vec<Uuid> = data
                    .children_by_family
                    .get(&fid)
                    .map(|cs| cs.iter().map(|c| c.person_id).collect())
                    .unwrap_or_default();

                for child_id in children {
                    if visited.contains(&child_id) {
                        continue;
                    }
                    let cpn = PersonNode::from_data(child_id, data, sosa_root_id, sosa_ancestors);
                    let child_after = if cpn.sex == Sex::Female { 1 } else { 0 };
                    let child_arena_idx = arena.len();
                    let mut child_node = TreeNode::new_real(
                        child_id,
                        depth + 1,
                        cpn.sex,
                        cpn.given,
                        cpn.surname,
                        cpn.birth_year,
                        cpn.death_year,
                        cpn.sosa_badge,
                        cpn.is_self,
                        child_after,
                        false,
                        false,
                    );
                    child_node.parent2 = spouse_arena_idx;
                    arena.push(child_node);
                    arena[node_idx].children.push(child_arena_idx);
                    work.push((child_arena_idx, depth + 1));
                }
            }
        }
    }

    arena
}

// ── Reingold-Tilford core ────────────────────────────────────────────────

fn wrap_tree(arena: &[TreeNode]) -> Vec<WrapNode> {
    let n = arena.len();
    // Pre-allocate all wrap nodes (one per arena node plus a virtual root).
    // We use indices into this vec. The virtual root is at index n.
    let mut wrap: Vec<WrapNode> = Vec::with_capacity(n + 1);

    // Initialize one WrapNode per TreeNode.
    for (i, tn) in arena.iter().enumerate() {
        wrap.push(WrapNode {
            orig: i,
            parent: None,
            children: tn.children.clone(),
            siblings: tn.siblings.clone(),
            parent2: tn.parent2,
            z: 0.0,
            m: 0.0,
            c: 0.0,
            s: 0.0,
            t: None,
            a: i, // self by default
            i: 0,
        });
    }

    // Wire parent pointers and child indices.
    // Process children (wire parent = this node, i = position among children).
    for wi in 0..n {
        let children = wrap[wi].children.clone();
        for (ci, &child_idx) in children.iter().enumerate() {
            wrap[child_idx].parent = Some(wi);
            wrap[child_idx].i = ci;
        }
    }

    wrap
}

fn tree_left(wrap: &[WrapNode], v: usize) -> Option<usize> {
    let children = &wrap[v].children;
    if !children.is_empty() {
        Some(children[0])
    } else {
        wrap[v].t
    }
}

fn tree_right(wrap: &[WrapNode], v: usize) -> Option<usize> {
    let children = &wrap[v].children;
    if !children.is_empty() {
        Some(*children.last().unwrap())
    } else {
        wrap[v].t
    }
}

fn tree_move(wrap: &mut [WrapNode], wm: usize, wp: usize, shift: f64) {
    let range = (wrap[wp].i as f64) - (wrap[wm].i as f64);
    if range > 0.0 {
        let change = shift / range;
        wrap[wp].c -= change;
        wrap[wm].c += change;
    }
    wrap[wp].s += shift;
    wrap[wp].z += shift;
    wrap[wp].m += shift;
}

fn tree_ancestor(wrap: &[WrapNode], vim: usize, v: usize, ancestor: usize) -> usize {
    let vim_a = wrap[vim].a;
    if wrap[vim_a].parent == wrap[v].parent {
        vim_a
    } else {
        ancestor
    }
}

fn tree_shift(wrap: &mut [WrapNode], node: usize) {
    let children = wrap[node].children.clone();
    let mut shift = 0.0f64;
    let mut change = 0.0f64;
    for i in (0..children.len()).rev() {
        let w = children[i];
        wrap[w].z += shift;
        wrap[w].m += shift;
        change += wrap[w].c;
        shift += wrap[w].s + change;
    }
}

/// Horizontal separation, in card widths, between two nodes on the same row.
///
/// Only the current node's depth decides it (matching the JS reference this
/// is ported from): the compact deepest ancestor row packs at the theme's
/// compact separation, every other row at a full card.
fn tree_separation(depth: i32, last_level: i32, compact_sep: f64) -> f64 {
    if depth == last_level {
        compact_sep
    } else {
        1.0
    }
}

#[allow(clippy::too_many_arguments)]
fn apportion(
    wrap: &mut [WrapNode],
    arena: &[TreeNode],
    v: usize,
    w: Option<usize>,
    ancestor_in: usize,
    last_level: i32,
    compact_sep: f64,
) -> usize {
    let mut ancestor = ancestor_in;
    let Some(w) = w else { return ancestor };

    let mut vip = v;
    let mut vop = v;
    let mut vim = w;
    let vom_start = wrap[v].parent.map(|p| wrap[p].children[0]).unwrap_or(v);
    let mut vom = vom_start;
    let mut sip = wrap[vip].m;
    let mut sop = wrap[vop].m;
    let mut sim = wrap[vim].m;
    let mut som = wrap[vom].m;

    loop {
        let vim_right = tree_right(wrap, vim);
        let vip_left = tree_left(wrap, vip);
        if vim_right.is_none() || vip_left.is_none() {
            break;
        }
        let vim_next = vim_right.unwrap();
        let vip_next = vip_left.unwrap();

        // Update vom and vop.
        let vom_left = tree_left(wrap, vom);
        let vop_right = tree_right(wrap, vop);
        if vom_left.is_none() || vop_right.is_none() {
            break;
        }
        vom = vom_left.unwrap();
        let vop_next = vop_right.unwrap();
        wrap[vop_next].a = v;

        // Compute siblings contribution for shift.
        let mut sibling_z: f64 = 0.0;
        for &si in &wrap[vim_next].siblings.clone() {
            sibling_z += wrap[si].z;
        }

        let sep = tree_separation(arena[wrap[vim_next].orig].depth, last_level, compact_sep);
        let shift = wrap[vim_next].z + sim + sibling_z - wrap[vip_next].z - sip + sep;
        if shift > 0.0 {
            let anc = tree_ancestor(wrap, vim_next, v, ancestor);
            tree_move(wrap, anc, v, shift);
            sip += shift;
            sop += shift;
        }

        sim += wrap[vim_next].m;
        sip += wrap[vip_next].m;
        som += wrap[vom].m;
        sop += wrap[vop_next].m;

        vim = vim_next;
        vip = vip_next;
        vop = vop_next;
    }

    if tree_right(wrap, vim).is_some() && tree_right(wrap, vop).is_none() {
        let vim_r = tree_right(wrap, vim).unwrap();
        wrap[vop].t = Some(vim_r);
        wrap[vop].m += sim - sop;
    }
    if tree_left(wrap, vip).is_some() && tree_left(wrap, vom).is_none() {
        let vip_l = tree_left(wrap, vip).unwrap();
        wrap[vom].t = Some(vip_l);
        wrap[vom].m += sip - som;
        ancestor = v;
    }

    ancestor
}

fn first_walk(
    wrap: &mut [WrapNode],
    arena: &[TreeNode],
    root: usize,
    last_level: i32,
    compact_sep: f64,
) {
    for v in post_order(wrap, root) {
        let parent = wrap[v].parent;
        let siblings_in_parent = parent.map(|p| wrap[p].children.clone()).unwrap_or_default();
        let prev_sibling = if wrap[v].i > 0 {
            siblings_in_parent.get(wrap[v].i - 1).copied()
        } else {
            None
        };

        let effective_children = effective_children(wrap, arena, v);
        if !effective_children.is_empty() {
            centre_over_children(
                wrap,
                arena,
                v,
                &effective_children,
                prev_sibling,
                last_level,
                compact_sep,
            );
        } else if let Some(w) = prev_sibling {
            wrap[v].z = next_to_sibling(wrap, arena, v, w, last_level, compact_sep);
        }

        place_spouses(wrap, arena, v, effective_children.len());

        // Apportion.
        let _new_ancestor = apportion(
            wrap,
            arena,
            v,
            prev_sibling,
            siblings_in_parent.first().copied().unwrap_or(v),
            last_level,
            compact_sep,
        );
    }
}

/// Every node under `root`, children before their parent.
fn post_order(wrap: &[WrapNode], root: usize) -> Vec<usize> {
    // Iterative post-order via explicit stack.
    let mut post_order: Vec<usize> = Vec::new();
    let mut stack = vec![root];
    while let Some(v) = stack.pop() {
        post_order.push(v);
        for &c in &wrap[v].children {
            stack.push(c);
        }
    }
    post_order.reverse();
    post_order
}

/// The children `v` is centred over: all of them, or, when `v` has spouses,
/// only those of its first spouse.
fn effective_children(wrap: &[WrapNode], arena: &[TreeNode], v: usize) -> Vec<usize> {
    let Some(&first_sib) = wrap[v].siblings.first() else {
        return wrap[v].children.clone();
    };
    // Filter children belonging to first sibling (spouse). A child with no
    // recorded second parent (`parent2 == None`) belongs to an empty/unknown
    // first-sibling placeholder — without this, such children match neither
    // branch, the effective children come back empty, and the
    // centering/shift logic is skipped entirely for this node, leaving its
    // subtree adrift.
    let first_sib_orig = wrap[first_sib].orig;
    let first_sib_is_empty = arena[first_sib_orig].id.is_none();
    wrap[v]
        .children
        .iter()
        .copied()
        .filter(|&ci| {
            wrap[ci].parent2.map(|p2| wrap[p2].orig) == Some(first_sib_orig)
                || (first_sib_is_empty && wrap[ci].parent2.is_none())
        })
        .collect()
}

/// The z of the last spouse drawn beside `v`, or 0 when it has none.
fn last_spouse_z(wrap: &[WrapNode], v: usize) -> f64 {
    wrap[v].siblings.last().map(|&s| wrap[s].z).unwrap_or(0.0)
}

/// Where `v` sits after its previous sibling `w` and that sibling's spouses.
///
/// Consecutive siblings share the same parent, so they sit exactly one
/// separation apart.
fn next_to_sibling(
    wrap: &[WrapNode],
    arena: &[TreeNode],
    v: usize,
    w: usize,
    last_level: i32,
    compact_sep: f64,
) -> f64 {
    let sep = tree_separation(arena[wrap[v].orig].depth, last_level, compact_sep);
    wrap[w].z + last_spouse_z(wrap, w) + sep
}

/// Centre `v` over its effective children, or, when it follows a sibling,
/// place it after that sibling and keep the offset its subtree must move by.
fn centre_over_children(
    wrap: &mut [WrapNode],
    arena: &[TreeNode],
    v: usize,
    effective_children: &[usize],
    prev_sibling: Option<usize>,
    last_level: i32,
    compact_sep: f64,
) {
    tree_shift(wrap, v);
    let midpoint = children_midpoint(wrap, arena, v, effective_children, last_level, compact_sep);
    match prev_sibling {
        Some(w) => {
            wrap[v].z = next_to_sibling(wrap, arena, v, w, last_level, compact_sep);
            wrap[v].m = wrap[v].z - midpoint;
        }
        None => {
            wrap[v].z = midpoint;
        }
    }
}

/// The point midway over `v`'s effective children, spouses included.
fn children_midpoint(
    wrap: &[WrapNode],
    arena: &[TreeNode],
    v: usize,
    effective_children: &[usize],
    last_level: i32,
    compact_sep: f64,
) -> f64 {
    let mut midpoint = 0.0f64;
    if !wrap[v].siblings.is_empty() && (arena[v].after != 1 || effective_children.len() == 1) {
        midpoint -= 0.5;
    }

    // Adjustment for female-first nodes (after=1) with siblings.
    let first_child = effective_children[0];
    let last_child = *effective_children.last().unwrap();
    let mut m_adj = 0.0f64;
    m_adj += female_first_offset(wrap, arena, first_child);
    m_adj += female_first_offset(wrap, arena, last_child);

    let last_sib_z = last_spouse_z(wrap, last_child);
    midpoint += (wrap[first_child].z + wrap[last_child].z + last_sib_z + m_adj) / 2.0;

    // Special case for 2 children at deepest level: the midpoint above
    // was computed as if they stood a full card apart, so give back
    // half of whatever the compact row actually saves. A theme that
    // packs its top row at full width saves nothing and needs no
    // correction, which is what this expression says at 1.0.
    if effective_children.len() == 2 && arena[wrap[first_child].orig].depth == last_level {
        midpoint -= (1.0 - compact_sep) / 2.0;
    }
    midpoint
}

/// How far a female-first child (after=1) reaches past its own card: the z of
/// its last spouse. Nothing for any other child.
fn female_first_offset(wrap: &[WrapNode], arena: &[TreeNode], child: usize) -> f64 {
    if arena[wrap[child].orig].after == 1 {
        last_spouse_z(wrap, child)
    } else {
        0.0
    }
}

/// Multi-spouse positioning (simplified port): place `v`'s spouses beside it,
/// each over the children they had together.
fn place_spouses(wrap: &mut [WrapNode], arena: &[TreeNode], v: usize, effective_children: usize) {
    let mut last_z = 0.0f64;
    let node_siblings_clone = wrap[v].siblings.clone();
    let orig_children_clone = wrap[v].children.clone();

    if !node_siblings_clone.is_empty() && arena[v].after == 1 && effective_children != 1 {
        wrap[v].m -= 0.5;
        let first_sib_with_child =
            first_sib_with_child(wrap, arena, &node_siblings_clone, &orig_children_clone);
        wrap[v].m -= first_sib_with_child as f64;
    }

    for (si, &sib_wi) in node_siblings_clone.iter().enumerate() {
        let sib_children: Vec<usize> = orig_children_clone
            .iter()
            .copied()
            .filter(|&ci| wrap[ci].parent2 == Some(sib_wi))
            .collect();

        if si == 0 {
            wrap[sib_wi].z = 1.0;
            last_z = wrap[sib_wi].z;
        } else if !sib_children.is_empty() {
            let first_sc = sib_children[0];
            let last_sc = *sib_children.last().unwrap();
            let mut mp = (wrap[first_sc].z + wrap[last_sc].z) / 2.0;
            mp += if sib_children.len() > 1 { 0.0 } else { 0.5 };
            if arena[v].after == 1 {
                mp = (wrap[first_sc].z + wrap[last_sc].z) / 2.0 + 0.5;
            }
            // Adjust relative to parent position.
            let parent_z = if !orig_children_clone.is_empty() {
                wrap[orig_children_clone[0]]
                    .parent
                    .map(|p| wrap[p].m)
                    .unwrap_or(0.0)
            } else {
                0.0
            };
            wrap[sib_wi].z = (mp - parent_z).max(last_z + 1.0);
            last_z = wrap[sib_wi].z;
        } else {
            last_z += 1.0;
            wrap[sib_wi].z = last_z;
        }

        wrap[sib_wi].m = wrap[v].m;
    }
}

/// Port of JS `firstSibWithChild`, the correction a female-first node's
/// offset takes from the spouse its first child descends from.
///
/// If the FIRST sibling (index 0) is the parent2 of the children,
/// firstSibWithChild = 0 - 1 = -1 → node.m -= (-1) → m += 1. Net result for
/// the common case (first spouse has the children): m += 0.5.
fn first_sib_with_child(
    wrap: &[WrapNode],
    arena: &[TreeNode],
    siblings: &[usize],
    children: &[usize],
) -> i32 {
    let mut first_sib_with_child = 0i32;
    let Some(&first_child) = children.first() else {
        return first_sib_with_child;
    };
    let first_child_p2 = wrap[first_child].parent2;
    for (index, &sib_wi) in siblings.iter().enumerate() {
        let sib_is_empty = arena[wrap[sib_wi].orig].id.is_none();
        if first_child_p2 == Some(sib_wi) || (first_child_p2.is_none() && sib_is_empty) {
            first_sib_with_child = index as i32 - 1;
        }
    }
    first_sib_with_child
}

fn second_walk(wrap: &mut [WrapNode], arena: &mut [TreeNode], root: usize) {
    // Iterative pre-order.
    let mut stack = vec![root];
    while let Some(v) = stack.pop() {
        // Get parent m.
        let parent_m = wrap[v].parent.map(|p| wrap[p].m).unwrap_or(0.0);

        if arena[v].after == 1
            && let Some(&last_sib) = wrap[v].siblings.last()
        {
            wrap[v].z += wrap[last_sib].z;
        }

        let node_x = wrap[v].z + parent_m;
        arena[v].x = node_x;
        wrap[v].m += parent_m;

        // Position siblings (spouses).
        let sibs = wrap[v].siblings.clone();
        for &sib_wi in &sibs {
            let sib_x = if arena[v].after == 1 {
                let last_sib_z = last_spouse_z(wrap, v);
                node_x - last_sib_z + wrap[sib_wi].z - 1.0
            } else {
                node_x + wrap[sib_wi].z
            };
            arena[sib_wi].x = sib_x;
        }

        // Process children.
        for &c in &wrap[v].children.clone() {
            stack.push(c);
        }
    }
}

/// How far a subtree whose leftmost card per depth is `curr_min` must move
/// right to clear the row contour `contour_max` — negative to close a gap.
///
/// Tightest depth wins: after shifting, every depth the two sides share must
/// keep a 1.0-unit gap; depths only one side occupies are unconstrained, so a
/// subtree sharing no depth does not move.
fn contour_shift(curr_min: &HashMap<i32, f64>, contour_max: &HashMap<i32, f64>) -> f64 {
    let mut shift = f64::NEG_INFINITY;
    for (d, cmin) in curr_min {
        if let Some(pmax) = contour_max.get(d) {
            shift = shift.max(1.0 - (cmin - pmax));
        }
    }
    if !shift.is_finite() {
        shift = 0.0;
    }
    shift
}

/// If `curr` starts a new parent2 (half-sibling) group, shift the matching
/// spouse sibling of the parent, and every spouse after it, along with it.
fn shift_spouse_group(
    arena: &mut [TreeNode],
    siblings: &[usize],
    prev: usize,
    curr: usize,
    shift: f64,
) {
    if arena[curr].parent2 != arena[prev].parent2
        && let Some(pos) = siblings
            .iter()
            .position(|&s| Some(s) == arena[curr].parent2)
    {
        for &si in &siblings[pos..] {
            arena[si].x += shift;
        }
    }
}

fn fix_spouse_group_overlaps(arena: &mut Vec<TreeNode>, node: usize) {
    let children = arena[node].children.clone();

    // Recurse into children FIRST (post-order). A shift applied while fixing
    // a deeper level can widen this node's own subtree (e.g. pushing a leaf
    // sibling's married-in spouse rightward), so the gap check below must see
    // the final, already-corrected width of each child — not the
    // RT-computed width from before any deeper fix-up ran. Checking gaps
    // top-down (parent before children) let an inner shift silently eat into
    // a buffer an outer check had already validated, re-introducing the
    // exact overlap this pass exists to prevent.
    for &ci in &children {
        fix_spouse_group_overlaps(arena, ci);
    }

    let siblings = arena[node].siblings.clone();

    // This pass only ever matters for nodes that themselves have a recorded
    // spouse (`siblings` non-empty) — that's how the descending tree marks a
    // couple whose children may come from more than one spouse. Ascending
    // trees never populate `siblings`, so this is a no-op there and the
    // RT-computed father/mother spacing (including the compact 0.5-unit
    // separation at the deepest level) is left untouched.
    if !siblings.is_empty() && !children.is_empty() {
        // Sweep every adjacent pair of children — full siblings as well as
        // half-sibling group boundaries. This also covers the case where a
        // child is itself a leaf with a married-in spouse card, which the
        // Reingold-Tilford contour walk does not always widen for.
        //
        // The gap can drift in EITHER direction from the ideal 1.0-unit
        // separation, not just too tight:
        //
        // `first_walk`'s `apportion`/`tree_move`/`tree_shift` is a direct
        // port of the classic Buchheim linear-time RT algorithm, which
        // deliberately distributes the extra width a wide subtree needs
        // *backward* across its earlier siblings (walking `node`'s children
        // right-to-left and accumulating `.s`/`.c`) rather than confining
        // that slack to the wide branch alone. That's correct/intentional
        // for the classic algorithm's "no kinks" guarantee on deep contour
        // conflicts between distant cousins, but for direct siblings it
        // means a sibling with a small subtree (or none) gets dragged away
        // from its neighbor just because a LATER sibling's subtree is wide
        // — e.g. a childless first child ends up many card-widths from its
        // very next sibling, even though neither of their own subtrees
        // needed that room. Any gap wider than 1.0 unit here is unused
        // slack from that redistribution, not real content — compact it
        // away symmetrically with the widen case by allowing `shift` to go
        // negative.
        //
        // Gaps are measured PER DEPTH LEVEL against the running contour of
        // everything already placed in this row, not against each subtree's
        // whole bounding box. A bounding-box check forces a childless
        // sibling a full card away from a neighbor's *deepest* descendant
        // row even though the two never share a row — per-level contours
        // let shallow cards tuck in right next to their direct neighbor
        // (the classic RT contour behaviour) while still guaranteeing the
        // 1.0-unit separation on every row the two sides actually share.
        let mut contour_max: HashMap<i32, f64> = HashMap::new();
        {
            let mut first_min = HashMap::new();
            collect_depth_extents(arena, children[0], &mut first_min, &mut contour_max);
        }
        for i in 1..children.len() {
            let prev = children[i - 1];
            let curr = children[i];

            let mut curr_min: HashMap<i32, f64> = HashMap::new();
            let mut curr_max: HashMap<i32, f64> = HashMap::new();
            collect_depth_extents(arena, curr, &mut curr_min, &mut curr_max);

            let shift = contour_shift(&curr_min, &contour_max);
            if shift.abs() > 1e-9 {
                for &ci in &children[i..] {
                    shift_subtree(arena, ci, shift);
                }
                shift_spouse_group(arena, &siblings, prev, curr, shift);
            }

            // Fold `curr`'s (post-shift) extents into the row contour so the
            // NEXT sibling is checked against everything placed so far — a
            // subtree two positions back can still be the rightmost content
            // on a deep row when the in-between sibling is shallow.
            for (d, cmax) in curr_max {
                let v = cmax + shift;
                contour_max
                    .entry(d)
                    .and_modify(|m| *m = m.max(v))
                    .or_insert(v);
            }
        }

        // Re-center the COUPLE GROUP — `node` together with its spouse
        // card(s) — over the true horizontal midpoint of ALL its children's
        // subtrees, not just the first and last child's own row.
        //
        // `first_walk`'s midpoint formula (the ported RT algorithm) only
        // centers a parent between its FIRST and LAST child's z, a classic
        // RT simplification that implicitly assumes subtree widths are
        // roughly symmetric across the row. When a MIDDLE child's subtree
        // is far wider than its neighbors (e.g. one branch has 10 recorded
        // descendants and the branches on either side of it have 1-3), that
        // assumption breaks down: the parent ends up visibly off-center
        // relative to the full row even though no individual gap overlaps
        // or looks unreasonably wide on its own. Recompute the row's true
        // bounding box across every child (not just the two ends) and
        // recenter unconditionally — this also subsumes the narrower
        // "only recenter after a gap-fix shift" case, since a shift only
        // ever changes what this bounding box already captures.
        //
        // Centering the couple's own bounding box (rather than the primary
        // card alone) is what keeps the visual convention intact: the
        // couple pair sits symmetrically over its children row. Centering
        // just the primary card left every children row half a card
        // off-center from the couple (a full card and more when the node
        // has several spouse cards trailing to one side).
        let mut row_min = f64::INFINITY;
        let mut row_max = f64::NEG_INFINITY;
        for &ci in &children {
            collect_min_x(arena, ci, &mut row_min);
            collect_max_x(arena, ci, &mut row_max);
        }

        let mut group_min = arena[node].x;
        let mut group_max = arena[node].x;
        for &si in &siblings {
            group_min = group_min.min(arena[si].x);
            group_max = group_max.max(arena[si].x);
        }

        let delta = (row_min + row_max) / 2.0 - (group_min + group_max) / 2.0;
        if delta.abs() > 1e-9 {
            arena[node].x += delta;
            for &si in &siblings {
                arena[si].x += delta;
            }
        }
    }
}

fn collect_max_x(arena: &[TreeNode], node: usize, max_x: &mut f64) {
    if arena[node].x > *max_x {
        *max_x = arena[node].x;
    }
    for &si in &arena[node].siblings {
        if arena[si].x > *max_x {
            *max_x = arena[si].x;
        }
    }
    for &ci in &arena[node].children {
        collect_max_x(arena, ci, max_x);
    }
}

fn collect_min_x(arena: &[TreeNode], node: usize, min_x: &mut f64) {
    if arena[node].x < *min_x {
        *min_x = arena[node].x;
    }
    for &si in &arena[node].siblings {
        if arena[si].x < *min_x {
            *min_x = arena[si].x;
        }
    }
    for &ci in &arena[node].children {
        collect_min_x(arena, ci, min_x);
    }
}

/// Per-depth horizontal extents of a subtree (node + spouse cards +
/// descendants), keyed by tree depth. Unlike the flat
/// [`collect_min_x`]/[`collect_max_x`] bounding box, this keeps each row's
/// extent separate, so two subtrees may interleave horizontally on rows
/// where only one of them has content.
fn collect_depth_extents(
    arena: &[TreeNode],
    node: usize,
    min_by_depth: &mut HashMap<i32, f64>,
    max_by_depth: &mut HashMap<i32, f64>,
) {
    fn record(
        depth: i32,
        x: f64,
        min_by_depth: &mut HashMap<i32, f64>,
        max_by_depth: &mut HashMap<i32, f64>,
    ) {
        min_by_depth
            .entry(depth)
            .and_modify(|m| *m = m.min(x))
            .or_insert(x);
        max_by_depth
            .entry(depth)
            .and_modify(|m| *m = m.max(x))
            .or_insert(x);
    }

    record(arena[node].depth, arena[node].x, min_by_depth, max_by_depth);
    for &si in &arena[node].siblings {
        record(arena[si].depth, arena[si].x, min_by_depth, max_by_depth);
    }
    for &ci in &arena[node].children {
        collect_depth_extents(arena, ci, min_by_depth, max_by_depth);
    }
}

fn shift_subtree(arena: &mut Vec<TreeNode>, node: usize, shift: f64) {
    arena[node].x += shift;
    let sibs = arena[node].siblings.clone();
    for si in sibs {
        arena[si].x += shift;
    }
    let children = arena[node].children.clone();
    for ci in children {
        shift_subtree(arena, ci, shift);
    }
}

fn size_node(
    arena: &mut Vec<TreeNode>,
    node: usize,
    translate_x: f64,
    translate_depth: i32,
    last_level: i32,
    metrics: &PedigreeMetrics,
) {
    let tn = &arena[node];
    let depth = tn.depth - translate_depth;
    // Determine card height for this depth.
    let sh = if tn.depth > 0 {
        metrics.desc_h
    } else if translate_depth < 0 && translate_depth == last_level {
        metrics.compact_h
    } else {
        metrics.card_h
    };

    let pixel_x = (arena[node].x - translate_x) * metrics.card_w;
    let pixel_y = if depth > 0 {
        (depth as f64 - 1.0) * metrics.card_h + sh
    } else {
        0.0
    };
    arena[node].x = pixel_x;
    arena[node].y = pixel_y;

    // Size siblings (spouses).
    let sibs = arena[node].siblings.clone();
    for si in sibs {
        size_node(arena, si, translate_x, translate_depth, last_level, metrics);
    }
}

/// Collect all nodes flat from the tree arena (including siblings).
fn collect_all_nodes(arena: &[TreeNode]) -> Vec<usize> {
    let mut result = Vec::new();
    let mut stack = vec![0usize];
    let mut visited: HashSet<usize> = HashSet::new();
    while let Some(n) = stack.pop() {
        if !visited.insert(n) {
            continue;
        }
        result.push(n);
        for &si in &arena[n].siblings {
            result.push(si);
        }
        for &ci in &arena[n].children {
            stack.push(ci);
        }
    }
    result
}

/// Entry point: run the full RT layout on an arena.
fn layout_tree(
    arena: &mut Vec<TreeNode>,
    last_level: i32,
    metrics: &PedigreeMetrics,
) -> (f64, f64) {
    if arena.is_empty() {
        return (0.0, 0.0);
    }

    let mut wrap = wrap_tree(arena);
    first_walk(&mut wrap, arena, 0, last_level, metrics.compact_separation);

    // Adjust root's parent m so root starts at 0.
    if let Some(p) = wrap[0].parent {
        wrap[p].m = -wrap[0].z;
    }

    second_walk(&mut wrap, arena, 0);
    fix_spouse_group_overlaps(arena, 0);

    // Compute bounding box.
    let all = collect_all_nodes(arena);
    let min_x = all
        .iter()
        .map(|&i| arena[i].x)
        .fold(f64::INFINITY, f64::min);
    let max_x = all
        .iter()
        .map(|&i| arena[i].x)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_depth = all.iter().map(|&i| arena[i].depth).min().unwrap_or(0);
    let max_depth = all.iter().map(|&i| arena[i].depth).max().unwrap_or(0);

    let translate_x = min_x;
    let translate_depth = min_depth;

    // Size all nodes (convert tree units → pixels).
    // Note: size_node handles siblings internally (matching JS sizeNode which calls
    // node.siblings.forEach(sizeNode)). preOrderTraversal only visits children.
    let mut stack = vec![0usize];
    let mut visited: HashSet<usize> = HashSet::new();
    while let Some(n) = stack.pop() {
        if !visited.insert(n) {
            continue;
        }
        size_node(arena, n, translate_x, translate_depth, last_level, metrics);
        let children = arena[n].children.clone();
        for ci in children {
            stack.push(ci);
        }
    }

    let tree_h = (max_depth - min_depth) as f64 * metrics.card_h;
    let tree_w = (max_x - min_x) * metrics.card_w;
    (tree_w.max(metrics.card_w), tree_h.max(metrics.card_h))
}

// ── Link/path collection ──────────────────────────────────────────────────

/// Collects the `d` attribute of every SVG connector between placed nodes.
fn collect_links(arena: &[TreeNode], last_level: i32, theme: &PedigreeTheme) -> Vec<String> {
    let metrics = &theme.metrics;
    let style = theme.link_style;
    let mut links = Vec::new();
    let mut stack = vec![0usize];
    let mut visited: HashSet<usize> = HashSet::new();

    while let Some(ni) = stack.pop() {
        if !visited.insert(ni) {
            continue;
        }
        let node = &arena[ni];

        if !node.siblings.is_empty() {
            // Spouse links + child links.
            let center = {
                let exit = metrics.card_h - metrics.card_bottom_offset;
                let base = exit
                    .min((metrics.sibling_vertical_step * node.siblings.len() as f64 + exit) / 2.0);
                base.max(metrics.sibling_min_offset)
            };

            for (si, &sib_ni) in node.siblings.iter().enumerate() {
                let y = if node.after != 1 {
                    (center - metrics.sibling_vertical_step * si as f64)
                        .max(metrics.sibling_min_offset)
                } else {
                    (center - metrics.sibling_vertical_step * (node.siblings.len() - si) as f64)
                        .max(metrics.sibling_min_offset)
                };

                // Spouse connector.
                links.push(link_path(
                    &LinkSpec::Spouse {
                        from: Point::new(node.x, node.y),
                        to: Point::new(arena[sib_ni].x, arena[sib_ni].y),
                        y_offset: y,
                    },
                    style,
                    metrics,
                ));

                // Children of this spouse. A child with no recorded second
                // parent (`parent2 == None`) is attributed to the empty
                // spouse placeholder (`id.is_none()`), if one is present.
                let sib_node = &arena[sib_ni];
                let children_of_sib: Vec<usize> = node
                    .children
                    .iter()
                    .copied()
                    .filter(|&ci| {
                        arena[ci].parent2 == Some(sib_ni)
                            || (arena[ci].parent2.is_none() && sib_node.id.is_none())
                    })
                    .collect();

                for (ci, &child_ni) in children_of_sib.iter().enumerate() {
                    let is_edge = ci == 0 || ci == children_of_sib.len() - 1;
                    links.push(link_path(
                        &LinkSpec::Child {
                            from: Point::new(sib_node.x, sib_node.y),
                            to: Point::new(arena[child_ni].x, arena[child_ni].y),
                            parent_after: node.after,
                            y_offset: y,
                            is_edge,
                        },
                        style,
                        metrics,
                    ));
                    // Push child onto stack.
                    stack.push(child_ni);
                }
            }
        } else {
            // Simple parent→child links.
            for (ci, &child_ni) in node.children.iter().enumerate() {
                if arena[child_ni].depth < 0 {
                    // Ascending: child → ancestor.
                    links.push(link_path(
                        &LinkSpec::Ancestor {
                            from: Point::new(node.x, node.y),
                            to: Point::new(arena[child_ni].x, arena[child_ni].y),
                            from_has_prev_sibling: node.before_sibling,
                            from_has_next_sibling: node.after_sibling,
                            from_depth: node.depth,
                            to_depth: arena[child_ni].depth,
                            last_level,
                        },
                        style,
                        metrics,
                    ));
                } else {
                    let is_edge = ci == 0 || ci == node.children.len() - 1;
                    links.push(link_path(
                        &LinkSpec::SimpleChild {
                            from: Point::new(node.x, node.y),
                            to: Point::new(arena[child_ni].x, arena[child_ni].y),
                            is_edge,
                        },
                        style,
                        metrics,
                    ));
                }
                stack.push(child_ni);
            }
        }
    }

    links
}

// ── Flat layout result ────────────────────────────────────────────────────

/// A positioned node on the canvas (derived from TreeNode after layout).
#[derive(Clone, Debug)]
struct LayoutNode {
    id: Option<Uuid>,
    x: f64,
    y: f64,
    sex: Sex,
    label_surname: String,
    label_given: String,
    birth_year: Option<QualifiedYear>,
    death_year: Option<QualifiedYear>,
    sosa_badge: SosaBadge,
    is_self: bool,
    is_compact: bool,
    /// For empty ancestor slots: which child they belong to.
    child_of: Option<Uuid>,
    is_father: bool,
    is_sibling: bool,
    /// True when this person has parents, spouses, or children recorded in
    /// the tree that are not rendered anywhere in the current layout (cut
    /// off by depth limits, or simply not part of the direct ascending/
    /// descending line). Drives the small "+" badge on the card.
    has_more_relations: bool,
}

/// Result of the pedigree layout computation.
///
/// The ascending and descending trees are kept in their own coordinate spaces so
/// that their SVG groups can each receive the correct `translate()` transform.
#[cfg_attr(test, derive(Default))]
struct PedigreeLayout {
    /// Ascending tree nodes in ascending-tree coordinate space.
    asc_nodes: Vec<LayoutNode>,
    /// Descending tree nodes in descending-tree coordinate space.
    desc_nodes: Vec<LayoutNode>,
    /// SVG connector paths for the ascending tree (in ascending coordinate space).
    asc_links: Vec<String>,
    /// SVG connector paths for the descending tree (in descending coordinate space).
    desc_links: Vec<String>,
    /// X translate applied to the outer SVG group (shifts content so x ≥ 0).
    main_tx: f64,
    /// Y translate applied to the outer SVG group (shifts content so y ≥ 0).
    main_ty: f64,
    /// X translate for the descending `<g>` (aligns desc root X with asc root X).
    desc_tx: f64,
    /// Y translate for the descending `<g>` (= asc root y, aligns vertically).
    desc_ty: f64,
    /// Total SVG viewport width.
    total_w: f64,
    /// Total SVG viewport height.
    total_h: f64,
    /// Real graph content centre x in final SVG coordinates, excluding margins.
    content_cx: f64,
    /// Real graph content centre y in final SVG coordinates, excluding margins.
    content_cy: f64,
    /// Real graph content width, excluding margins.
    content_w: f64,
    /// Real graph content height, excluding margins.
    content_h: f64,
    /// Root card centre x in final SVG coordinates (for auto-centering).
    root_cx: f64,
    /// Root card centre y in final SVG coordinates (for auto-centering).
    root_cy: f64,
}

/// True when `pid` has a recorded parent, spouse, or child that is not part
/// of `rendered_ids` — i.e. a relation the current ascending/descending
/// layout doesn't show (cut off by depth limits, or simply off the direct
/// line, e.g. an ancestor's other children or an extra marriage).
fn person_has_hidden_relations(
    pid: Uuid,
    data: &PedigreeData,
    rendered_ids: &HashSet<Uuid>,
) -> bool {
    if let Some(fam_ids) = data.families_as_child.get(&pid) {
        for fid in fam_ids {
            if let Some(spouses) = data.spouses_by_family.get(fid)
                && spouses
                    .iter()
                    .any(|sp| !rendered_ids.contains(&sp.person_id))
            {
                return true;
            }
        }
    }
    if let Some(fam_ids) = data.families_as_spouse.get(&pid) {
        for fid in fam_ids {
            if let Some(spouses) = data.spouses_by_family.get(fid)
                && spouses
                    .iter()
                    .any(|sp| sp.person_id != pid && !rendered_ids.contains(&sp.person_id))
            {
                return true;
            }
            if let Some(children) = data.children_by_family.get(fid)
                && children
                    .iter()
                    .any(|c| !rendered_ids.contains(&c.person_id))
            {
                return true;
            }
        }
    }
    false
}

/// A computed layout, shared with the canvas that draws it.
///
/// Equality is identity: the canvas redraws when the layout is recomputed and
/// is skipped when the chart re-renders around an unchanged one.
#[derive(Clone)]
struct SharedLayout(Rc<PedigreeLayout>);

impl PartialEq for SharedLayout {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl std::ops::Deref for SharedLayout {
    type Target = PedigreeLayout;

    fn deref(&self) -> &PedigreeLayout {
        &self.0
    }
}

/// What a chart's layout was computed from.
struct LayoutKey {
    root: Uuid,
    data: SharedPedigree,
    sosa_root: Option<Uuid>,
    sosa_ids: Option<AncestorSet>,
    shape: SceneShape,
}

/// The part of a [`LayoutKey`] that is not read off the chart's props.
#[derive(Clone, Copy, PartialEq)]
struct SceneShape {
    view: PedigreeView,
    ancestor_levels: usize,
    descendant_levels: usize,
    theme: &'static PedigreeTheme,
}

impl LayoutKey {
    fn of(props: &PedigreeChartProps, shape: SceneShape) -> Self {
        Self {
            root: props.root_person_id,
            data: props.data.clone(),
            sosa_root: props.sosa_root_person_id,
            sosa_ids: props.sosa_ancestor_ids.clone(),
            shape,
        }
    }

    fn matches(&self, props: &PedigreeChartProps, shape: SceneShape) -> bool {
        self.root == props.root_person_id
            && self.data == props.data
            && self.sosa_root == props.sosa_root_person_id
            && self.shape == shape
            && self.sosa_ids == props.sosa_ancestor_ids
    }
}

/// A laid-out chart in whichever view the viewer chose.
#[derive(Clone, PartialEq)]
enum ChartScene {
    Tree(SharedLayout),
    Circular(circular::SharedCircular),
    Lineage(lineage::SharedLineage),
}

impl ChartScene {
    /// How far the view may be zoomed: [`ZOOM_MAX`], or further for a
    /// circular chart whose narrowest labels need it to reach their size.
    fn max_zoom(&self) -> f64 {
        match self {
            Self::Circular(layout) => layout.max_zoom,
            Self::Tree(_) | Self::Lineage(_) => ZOOM_MAX,
        }
    }

    /// What a fit frames, whichever view is drawn.
    fn fit_target(&self) -> FitTarget {
        match self {
            Self::Tree(layout) => FitTarget::of(layout),
            Self::Circular(layout) => layout.fit_target(),
            Self::Lineage(layout) => layout.fit_target(),
        }
    }
}

/// The SOSA root's ancestors: the server's set when it sent one, else a
/// traversal that only sees the pedigree window.
fn resolve_sosa_ancestors(props: &PedigreeChartProps) -> AncestorSet {
    props
        .sosa_ancestor_ids
        .clone()
        .or_else(|| {
            props
                .sosa_root_person_id
                .map(|sosa_id| Shared::new(props.data.ancestor_set(sosa_id)))
        })
        .unwrap_or_default()
}

/// Lays the chart out in `shape`'s view.
fn compute_scene(props: &PedigreeChartProps, shape: SceneShape) -> ChartScene {
    let sosa_ancestors = resolve_sosa_ancestors(props);
    // Every person an ancestor-only view draws is an ancestor of its root, so
    // the badge marking the SOSA root's ancestors would say nothing there; it
    // keeps the SOSA root's own mark and the user's.
    let no_ancestor_badges = HashSet::new();
    // A descendant chart keeps them: among the descendants, the badge traces
    // the line that leads to the SOSA root.
    let descendant_circular = |arc| {
        ChartScene::Circular(circular::SharedCircular(Rc::new(
            crate::ui_observability::measure_ui("pedigree_layout", || {
                circular::descendant_circular_layout(
                    arc,
                    props.root_person_id,
                    &props.data,
                    shape.descendant_levels,
                    props.sosa_root_person_id,
                    &sosa_ancestors,
                )
            }),
        )))
    };
    let horizontal = |layout: &dyn Fn() -> lineage::LineageLayout| {
        ChartScene::Lineage(lineage::SharedLineage(Rc::new(
            crate::ui_observability::measure_ui("pedigree_layout", layout),
        )))
    };
    let circular = |arc| {
        ChartScene::Circular(circular::SharedCircular(Rc::new(
            crate::ui_observability::measure_ui("pedigree_layout", || {
                circular::circular_layout(
                    arc,
                    props.root_person_id,
                    &props.data,
                    shape.ancestor_levels,
                    props.sosa_root_person_id,
                    &no_ancestor_badges,
                )
            }),
        )))
    };
    match shape.view {
        PedigreeView::Tree => ChartScene::Tree(SharedLayout(Rc::new(
            crate::ui_observability::measure_ui("pedigree_layout", || {
                compute_layout(
                    props.root_person_id,
                    &props.data,
                    props.sosa_root_person_id,
                    &sosa_ancestors,
                    PedigreeLayoutOptions::full(shape.ancestor_levels, shape.descendant_levels),
                    shape.theme,
                )
            }),
        ))),
        PedigreeView::Wheel => circular(circular::ChartArc::WHEEL),
        PedigreeView::Fan => circular(circular::ChartArc::FAN),
        PedigreeView::DescendantWheel => descendant_circular(circular::ChartArc::DESCENDANT_WHEEL),
        PedigreeView::DescendantFan => descendant_circular(circular::ChartArc::DESCENDANT_FAN),
        PedigreeView::Lineage => horizontal(&|| {
            lineage::lineage_layout(
                props.root_person_id,
                &props.data,
                shape.ancestor_levels,
                props.sosa_root_person_id,
                &no_ancestor_badges,
                shape.theme,
            )
        }),
        PedigreeView::DescendantLineage => horizontal(&|| {
            lineage::descendant_lineage_layout(
                props.root_person_id,
                &props.data,
                shape.descendant_levels,
                props.sosa_root_person_id,
                &sosa_ancestors,
                shape.theme,
            )
        }),
        PedigreeView::Hourglass => horizontal(&|| {
            lineage::hourglass_layout(
                props.root_person_id,
                &props.data,
                shape.ancestor_levels,
                shape.descendant_levels,
                props.sosa_root_person_id,
                &sosa_ancestors,
                shape.theme,
            )
        }),
        PedigreeView::Bowtie => horizontal(&|| {
            lineage::bowtie_layout(
                props.root_person_id,
                &props.data,
                shape.ancestor_levels,
                props.sosa_root_person_id,
                &no_ancestor_badges,
                shape.theme,
            )
        }),
    }
}

/// The scene for `shape`, from `cache` when nothing it depends on changed.
///
/// The chart re-renders for plenty of other reasons — the event panel, the
/// depth popover, the selection — and each of those used to recompute the
/// whole layout.
fn cached_scene(
    cache: &RefCell<Option<(LayoutKey, ChartScene)>>,
    props: &PedigreeChartProps,
    shape: SceneShape,
) -> ChartScene {
    let mut cache = cache.borrow_mut();
    if let Some((key, scene)) = &*cache
        && key.matches(props, shape)
    {
        return scene.clone();
    }
    let scene = compute_scene(props, shape);
    *cache = Some((LayoutKey::of(props, shape), scene.clone()));
    scene
}

/// Compute the RT layout for both ascending and descending trees.
///
/// Uses two independent `LayoutTreeService`-equivalent passes (one per tree) and
/// computes the SVG group transforms needed to make the root card appear at the
/// same canvas position in both trees.
/// Lay the pedigree out.
///
/// The SOSA root and its ancestor set are parameters rather than fields read
/// off `data`: the chart resolves them from its own props, and copying the
/// whole pedigree just to write two fields into it cost more than the layout.
#[derive(Clone, Copy)]
struct PedigreeLayoutOptions {
    ancestor_levels: usize,
    descendant_levels: usize,
    include_root_siblings: bool,
}

impl PedigreeLayoutOptions {
    const fn full(ancestor_levels: usize, descendant_levels: usize) -> Self {
        Self {
            ancestor_levels,
            descendant_levels,
            include_root_siblings: true,
        }
    }

    const fn mini(ancestor_levels: usize, descendant_levels: usize) -> Self {
        Self {
            ancestor_levels,
            descendant_levels,
            include_root_siblings: false,
        }
    }
}

/// The horizontal extent of the root couple, with the root drawn at
/// `root_x`: the root's card widened by its spouses from the descending tree —
/// to the right of a male root (his wife), to the left of a female one.
fn root_couple_extent(desc_arena: &[TreeNode], root_x: f64) -> (f64, f64) {
    let root = &desc_arena[0];
    let mut min_x = root_x;
    let mut max_x = root_x;
    if root.after == 0 && !root.siblings.is_empty() {
        // Male root: wife is to the right → extend maxX.
        let last_si = *root.siblings.last().unwrap();
        max_x += desc_arena[last_si].x - root.x;
    } else if !root.siblings.is_empty() {
        // Female root: husband is to the left → extend minX.
        let first_si = root.siblings[0];
        min_x -= root.x - desc_arena[first_si].x;
    }
    (min_x, max_x)
}

/// Position and depth of the root's `index`-th parent in the ascending tree.
fn root_parent_anchor(asc_arena: &[TreeNode], index: usize) -> Option<(f64, f64, i32)> {
    asc_arena[0]
        .children
        .get(index)
        .map(|&ci| (asc_arena[ci].x, asc_arena[ci].y, asc_arena[ci].depth))
}

/// The extent of a layout's cards, before its margin.
#[derive(Clone, Copy)]
struct Bounds {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

impl Bounds {
    const EMPTY: Self = Self {
        min_x: f64::INFINITY,
        max_x: f64::NEG_INFINITY,
        min_y: f64::INFINITY,
        max_y: f64::NEG_INFINITY,
    };

    /// Grows to hold a card `w` by `h` at `(x, y)`.
    fn add(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.min_x = self.min_x.min(x);
        self.max_x = self.max_x.max(x + w);
        self.min_y = self.min_y.min(y);
        self.max_y = self.max_y.max(y + h);
    }
}

/// The root's biological siblings, drawn on the root's row outside the
/// Reingold–Tilford layout: the elder to the left, linked to the father, the
/// younger to the right, linked to the mother (or the father when there is
/// no mother).
struct SiblingRow<'a> {
    data: &'a PedigreeData,
    sosa_root_id: Option<Uuid>,
    sosa_ancestors: &'a HashSet<Uuid>,
    theme: &'a PedigreeTheme,
    last_level: i32,
}

impl SiblingRow<'_> {
    /// The siblings' cards; their links are pushed onto `links`.
    fn place(
        &self,
        root_id: Uuid,
        asc_arena: &[TreeNode],
        desc_arena: &[TreeNode],
        links: &mut Vec<String>,
    ) -> Vec<LayoutNode> {
        let all_siblings = get_siblings(root_id, self.data);
        if all_siblings.len() <= 1 {
            return Vec::new();
        }
        let metrics = &self.theme.metrics;
        let root_sib_idx = all_siblings.iter().position(|&s| s == root_id).unwrap_or(0);
        let (before, after) = (
            &all_siblings[..root_sib_idx],
            &all_siblings[root_sib_idx + 1..],
        );
        let (row_y, root_x) = (asc_arena[0].y, asc_arena[0].x);
        let (sib_min_x, sib_max_x) = root_couple_extent(desc_arena, root_x);
        // Father: asc_arena[0].children[0], Mother: children[1] (if present).
        let father = root_parent_anchor(asc_arena, 0);
        let mother = root_parent_anchor(asc_arena, usize::from(asc_arena[0].children.len() > 1));
        // Elder siblings count down from the root, so the furthest is "last".
        let elder = before.iter().enumerate().map(|(i, &id)| {
            let x = sib_min_x - metrics.sibling_spacing * (before.len() - i) as f64;
            let simple = father.is_some_and(|(fx, _, _)| x >= fx);
            (id, x, father, before.len() - i - 1, before.len(), simple)
        });
        let younger = after.iter().enumerate().map(|(i, &id)| {
            let x = sib_max_x + metrics.sibling_spacing * (i + 1) as f64;
            let simple = mother.is_some_and(|(px, _, _)| x <= px);
            (id, x, mother, i, after.len(), simple)
        });
        let mut nodes = Vec::new();
        for (sib_id, sib_x, parent, index, count, simple) in elder.chain(younger) {
            let pn =
                PersonNode::from_data(sib_id, self.data, self.sosa_root_id, self.sosa_ancestors);
            nodes.push(pn.card_at(sib_id, sib_x, row_y));
            let Some((px, py, depth)) = parent else {
                continue;
            };
            links.push(link_path(
                &LinkSpec::RootSibling {
                    from: Point::new(px, py),
                    to: Point::new(sib_x, row_y),
                    from_depth: depth,
                    index,
                    count,
                    simple,
                    last_level: self.last_level,
                },
                self.theme.link_style,
                metrics,
            ));
        }
        nodes
    }
}

fn compute_layout(
    root_id: Uuid,
    data: &PedigreeData,
    sosa_root_id: Option<Uuid>,
    sosa_ancestors: &HashSet<Uuid>,
    options: PedigreeLayoutOptions,
    theme: &PedigreeTheme,
) -> PedigreeLayout {
    let metrics = &theme.metrics;
    let last_asc_level = -(options.ancestor_levels as i32);

    // ── Ascending tree ──
    let mut asc_arena = build_ascending_tree(
        root_id,
        data,
        options.ancestor_levels,
        sosa_root_id,
        sosa_ancestors,
    );
    layout_tree(&mut asc_arena, last_asc_level, metrics);
    let mut asc_links = collect_links(&asc_arena, last_asc_level, theme);

    // ── Descending tree ──
    let mut desc_arena = build_descending_tree(
        root_id,
        data,
        options.descendant_levels,
        sosa_root_id,
        sosa_ancestors,
    );
    layout_tree(&mut desc_arena, 0, metrics);
    let desc_links = collect_links(&desc_arena, 0, theme);

    // Root is always at arena index 0 in both trees.
    let asc_root_x = asc_arena[0].x;
    let asc_root_y = asc_arena[0].y;
    let desc_root_x = desc_arena[0].x;
    let desc_root_y = desc_arena[0].y;

    // Descending-group SVG transform that aligns desc root with asc root:
    //   global_x(desc_node) = desc_node.x + desc_tx + main_tx
    //   global_x(asc_node)  = asc_node.x          + main_tx
    // At root: asc_root_x = desc_root_x + desc_tx  →  desc_tx = asc_root_x - desc_root_x
    let desc_tx = asc_root_x - desc_root_x;
    let desc_ty = asc_root_y - desc_root_y; // desc_root_y is 0 after layout_tree

    // ── Root biological siblings (placed outside RT layout, same row as root).
    let extra_asc_nodes = if options.include_root_siblings {
        let row = SiblingRow {
            data,
            sosa_root_id,
            sosa_ancestors,
            theme,
            last_level: last_asc_level,
        };
        row.place(root_id, &asc_arena, &desc_arena, &mut asc_links)
    } else {
        Vec::new()
    };

    // ── Global bounding box (descending nodes shifted by desc_tx/ty) ──
    let asc_all = collect_all_nodes(&asc_arena);
    let desc_all = collect_all_nodes(&desc_arena);
    let mut bounds = Bounds::EMPTY;
    for tn in asc_all.iter().map(|&ni| &asc_arena[ni]) {
        let compact = tn.depth == last_asc_level;
        let (cw, ch) = if compact {
            (metrics.compact_w, metrics.compact_h)
        } else {
            (metrics.card_w, metrics.card_h)
        };
        bounds.add(tn.x, tn.y, cw, ch);
    }
    for tn in desc_all.iter().map(|&ni| &desc_arena[ni]) {
        let ch = if tn.depth > 0 {
            metrics.desc_h
        } else {
            metrics.card_h
        };
        bounds.add(tn.x + desc_tx, tn.y + desc_ty, metrics.card_w, ch);
    }
    // Include root biological siblings in bounding box.
    for node in &extra_asc_nodes {
        bounds.add(node.x, node.y, metrics.card_w, metrics.card_h);
    }
    let Bounds {
        min_x: gmin_x,
        max_x: gmax_x,
        min_y: gmin_y,
        max_y: gmax_y,
    } = bounds;

    let margin = metrics.layout_margin;
    // Shift so that no node has a negative coordinate inside the main group.
    let main_tx = (-gmin_x + margin).max(margin);
    let main_ty = (-gmin_y + margin).max(margin);
    let total_w = (gmax_x - gmin_x + 2.0 * margin).max(metrics.card_w);
    let total_h = (gmax_y - gmin_y + 2.0 * margin).max(metrics.card_h);
    let content_cx = (gmin_x + gmax_x) / 2.0 + main_tx;
    let content_cy = (gmin_y + gmax_y) / 2.0 + main_ty;
    let content_w = (gmax_x - gmin_x).max(metrics.card_w);
    let content_h = (gmax_y - gmin_y).max(metrics.card_h);

    // Root card centre in final SVG coordinates (used for auto-centering).
    let root_cx = asc_root_x + main_tx + metrics.card_w / 2.0;
    let root_cy = asc_root_y + main_ty + metrics.card_h / 2.0;

    // ── Build LayoutNode lists ──
    let make_node = |ni: usize, arena: &Vec<TreeNode>, is_compact: bool| LayoutNode {
        id: arena[ni].id,
        x: arena[ni].x,
        y: arena[ni].y,
        sex: arena[ni].sex,
        label_surname: arena[ni].label_surname.clone(),
        label_given: arena[ni].label_given.clone(),
        birth_year: arena[ni].birth_year,
        death_year: arena[ni].death_year,
        sosa_badge: arena[ni].sosa_badge.clone(),
        is_self: arena[ni].is_self,
        is_compact,
        child_of: arena[ni].child_of,
        is_father: arena[ni].is_father,
        is_sibling: arena[ni].is_sibling,
        has_more_relations: false,
    };

    let mut asc_nodes: Vec<LayoutNode> = asc_all
        .iter()
        .map(|&ni| make_node(ni, &asc_arena, asc_arena[ni].depth == last_asc_level))
        .collect();
    asc_nodes.extend(extra_asc_nodes);
    let mut desc_nodes: Vec<LayoutNode> = desc_all
        .iter()
        .map(|&ni| make_node(ni, &desc_arena, false))
        .collect();

    // ── Flag cards with relations not shown anywhere in this layout ──
    let rendered_ids: HashSet<Uuid> = asc_nodes
        .iter()
        .chain(desc_nodes.iter())
        .filter_map(|n| n.id)
        .collect();
    for node in asc_nodes.iter_mut().chain(desc_nodes.iter_mut()) {
        if let Some(pid) = node.id {
            node.has_more_relations = person_has_hidden_relations(pid, data, &rendered_ids);
        }
    }

    PedigreeLayout {
        asc_nodes,
        desc_nodes,
        asc_links,
        desc_links,
        main_tx,
        main_ty,
        desc_tx,
        desc_ty,
        total_w,
        total_h,
        content_cx,
        content_cy,
        content_w,
        content_h,
        root_cx,
        root_cy,
    }
}

// ── Fit to viewport ──────────────────────────────────────────────────────

/// Measures the canvas area actually free of the event panel.
///
/// Returns `(width, height, left, pageLeft, pageTop)` in viewport pixels,
/// where `left` is relative to the viewport's own free area (event-panel
/// overlap already carved out) and `pageLeft`/`pageTop` are the viewport
/// element's own position in page/client coordinates — cached by callers so
/// the wheel-zoom handler doesn't need its own round trip on every tick. The
/// panel overlaps the canvas rather than shrinking it below 900px, where it
/// is a drawer — so there the whole rect is fair game and only the wider
/// layout has to carve the panel out.
const MEASURE_PEDIGREE_VIEWPORT_JS: &str = r#"
    const viewport = document.querySelector('.pedigree-viewport');
    if (!viewport) return [800, 600, 0, 0, 0];
    const rect = viewport.getBoundingClientRect();
    const panel = document.querySelector('.ev-panel:not(.ev-panel-collapsed)');
    const isNarrow = window.innerWidth <= 900;
    let availableLeft = 0;
    let availableRight = rect.width;
    if (panel && !isNarrow) {
        const panelRect = panel.getBoundingClientRect();
        const overlapsX = panelRect.left < rect.right && panelRect.right > rect.left;
        if (overlapsX) {
            availableRight = Math.min(availableRight, panelRect.left - rect.left);
        }
    }
    return [Math.max(1, availableRight - availableLeft), rect.height, availableLeft, rect.left, rect.top];
"#;

/// A press dragged across the chart pans it; it is not a click.
///
/// Installed once per window, in the capture phase on the document: Dioxus
/// listens for clicks on its root while they bubble, so a click stopped here
/// never reaches any card, segment or root disc of any chart view — one rule
/// for every view instead of a check in each handler. A press that moved
/// less than `DRAG_CLICK_SLOP` pixels is still a click, so a trembling hand
/// can select a card.
const DRAG_IS_NOT_A_CLICK_JS: &str = r#"
    if (!window.__oxDragIsNotAClick) {
        window.__oxDragIsNotAClick = true;
        const DRAG_CLICK_SLOP = 5;
        let start = null;
        let moved = false;
        document.addEventListener('pointerdown', event => {
            start = [event.clientX, event.clientY];
            moved = false;
        }, true);
        document.addEventListener('pointermove', event => {
            if (start && event.buttons
                && Math.hypot(event.clientX - start[0], event.clientY - start[1]) > DRAG_CLICK_SLOP) {
                moved = true;
            }
        }, true);
        document.addEventListener('click', event => {
            const onChart = event.target instanceof Element
                && event.target.closest('.pedigree-viewport');
            if (moved && onChart) {
                event.stopPropagation();
                event.preventDefault();
            }
            start = null;
            moved = false;
        }, true);
    }
"#;

const WAIT_FOR_EVENT_PANEL_TRANSITION_JS: &str = r#"
    const panel = document.querySelector('.ev-panel');
    if (!panel) return;
    await new Promise(resolve => {
        let settled = false;
        const finish = () => {
            if (settled) return;
            settled = true;
            panel.removeEventListener('transitionend', onEnd);
            resolve();
        };
        const onEnd = event => {
            if (event.target === panel && (event.propertyName === 'width' || event.propertyName === 'min-width')) {
                finish();
            }
        };
        panel.addEventListener('transitionend', onEnd);
        setTimeout(finish, 250);
    });
    await new Promise(requestAnimationFrame);
"#;

/// Where the free part of the pedigree viewport sits, as last measured.
///
/// Cached rather than queried per event: a wheel gesture is a stream of small
/// updates, and an async DOM read per tick would arrive late and out of order.
/// Refreshed by every fit — which a panel resize also triggers.
#[derive(Clone, Copy, PartialEq)]
struct ViewportRect {
    /// Page coordinates of the viewport's top-left corner, which is what turns
    /// a pointer event's client coordinates into viewport ones.
    page_x: f64,
    page_y: f64,
    /// The strip left free by the events panel, in viewport coordinates.
    left: f64,
    width: f64,
    height: f64,
}

impl ViewportRect {
    /// What to assume before the first measurement lands.
    const fn assumed() -> Self {
        Self {
            page_x: 0.0,
            page_y: 0.0,
            left: 0.0,
            width: VIEWPORT_DEFAULT_W,
            height: VIEWPORT_DEFAULT_H,
        }
    }

    /// The point a zoom anchors to when it has no cursor of its own.
    ///
    /// The same point a fit centres the graph on, so zooming in and then
    /// fitting again does not slide the graph sideways.
    const fn center(self) -> (f64, f64) {
        (self.left + self.width / 2.0, self.height / 2.0)
    }
}

/// Pan and zoom move as one value so one input event produces one reactive
/// update and the renderer never observes a half-updated transform.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ViewportTransform {
    x: f64,
    y: f64,
    scale: f64,
}

/// The scale one step away, or `None` when the zoom is already at its limit.
fn zoom_step(current: f64, factor: f64, max: f64) -> Option<f64> {
    let next = (current * factor).clamp(ZOOM_MIN, max.max(ZOOM_MIN));
    ((next - current).abs() > f64::EPSILON).then_some(next)
}

/// Zooms about a fixed point, leaving whatever is there where it is.
///
/// Both zoom gestures are this operation; they differ only in the point they
/// hold still. Scaling without one lets the CSS transform scale about the
/// content's own origin, so the graph slides toward a corner as it grows —
/// which is what the zoom buttons did.
fn zoom_about(mut transform: Signal<ViewportTransform>, anchor: (f64, f64), new_scale: f64) {
    let current = transform();
    let (ax, ay) = anchor;
    transform.set(ViewportTransform {
        x: offset_holding(ax, current.x, current.scale, new_scale),
        y: offset_holding(ay, current.y, current.scale, new_scale),
        scale: new_scale,
    });
}

/// The pan offset that keeps `anchor` over the same content across a rescale.
///
/// One axis of [`zoom_about`], split out because it is the whole of the
/// behaviour and the signals around it are not.
fn offset_holding(anchor: f64, offset: f64, old_scale: f64, new_scale: f64) -> f64 {
    let content_under_anchor = (anchor - offset) / old_scale;
    anchor - content_under_anchor * new_scale
}

/// What a fit frames: the graph's extent, and the root card it keeps in view.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FitTarget {
    content_cx: f64,
    content_cy: f64,
    content_w: f64,
    content_h: f64,
    root_cx: f64,
    root_cy: f64,
    /// Whether the root is at the graph's left edge, as in the lineage view:
    /// a graph too large to frame whole then starts at the left margin
    /// rather than centring the root, which would leave half the screen
    /// empty.
    root_at_left: bool,
}

impl FitTarget {
    fn of(layout: &PedigreeLayout) -> Self {
        Self {
            content_cx: layout.content_cx,
            content_cy: layout.content_cy,
            content_w: layout.content_w,
            content_h: layout.content_h,
            root_cx: layout.root_cx,
            root_cy: layout.root_cy,
            root_at_left: false,
        }
    }
}

/// The transform that fits `target` into the free part of `rect`.
///
/// A graph that fits is framed whole. One that does not — a deep pedigree
/// already at the smallest scale — is centred on the root card instead: that
/// person is who the user asked to see, and centring the graph's middle could
/// leave them off screen entirely. A graph whose root is its left edge (the
/// lineage view) keeps the root centred vertically only: if it is wider than
/// the screen it starts at the left margin, else it is centred across.
fn fit_transform(rect: ViewportRect, target: FitTarget) -> ViewportTransform {
    let side_padding = rect.width * FIT_SIDE_PADDING_RATIO;
    let fit_w = (rect.width - 2.0 * side_padding).max(1.0);
    let whole = (fit_w / target.content_w).min(rect.height / target.content_h);
    let scale = whole.clamp(ZOOM_MIN, ZOOM_MAX);
    let (center_x, center_y) = rect.center();
    let (x, focus_y) = if whole >= ZOOM_MIN {
        (center_x - target.content_cx * scale, target.content_cy)
    } else if !target.root_at_left {
        (center_x - target.root_cx * scale, target.root_cy)
    } else if target.content_w * scale <= fit_w {
        (center_x - target.content_cx * scale, target.root_cy)
    } else {
        let content_left = target.content_cx - target.content_w / 2.0;
        (
            rect.left + side_padding - content_left * scale,
            target.root_cy,
        )
    };
    ViewportTransform {
        x,
        y: center_y - focus_y * scale,
        scale,
    }
}

/// Scales and pans the canvas so the graph sits inside the free area.
///
/// Shared by the initial/root-change fit and by the fit-screen button, which
/// held byte-identical copies of the measurement script and the arithmetic
/// below.
async fn fit_graph_in_viewport(
    mut transform: Signal<ViewportTransform>,
    mut viewport_rect: Signal<ViewportRect>,
    target: FitTarget,
) {
    let Ok(val) = document::eval(MEASURE_PEDIGREE_VIEWPORT_JS).await else {
        return;
    };
    let vw = val
        .get(0)
        .and_then(|v| v.as_f64())
        .unwrap_or(VIEWPORT_DEFAULT_W);
    let vh = val
        .get(1)
        .and_then(|v| v.as_f64())
        .unwrap_or(VIEWPORT_DEFAULT_H);
    let vx = val.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let page_x = val.get(3).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let page_y = val.get(4).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let rect = ViewportRect {
        page_x,
        page_y,
        left: vx,
        width: vw,
        height: vh,
    };
    viewport_rect.set(rect);
    transform.set(fit_transform(rect, target));
}

// ── Component ────────────────────────────────────────────────────────────

/// Preferred maximum scale for [`MiniPedigree`] — not user-adjustable.
const MINI_PEDIGREE_SCALE: f64 = 0.8;

/// Bottom padding (viewport px) kept below the root card when it's anchored
/// near the bottom of the canvas (no descendants to show underneath it).
const MINI_PEDIGREE_BOTTOM_MARGIN: f64 = 20.0;

/// Vertical target for a bottom-anchored root card.
///
/// `root_cy` is the card centre, so the theme's scaled half-height must also
/// be reserved. Treating this margin as the centre offset clips tall themes.
fn mini_pedigree_root_target_y(viewport_height: f64, scale: f64, theme: &PedigreeTheme) -> f64 {
    viewport_height - theme.metrics.card_h * scale / 2.0 - MINI_PEDIGREE_BOTTOM_MARGIN
}

/// Largest scale that keeps the whole fragment inside its static viewport.
///
/// Width is measured around the root rather than the content centre so the
/// selected person stays centered even when siblings make the graph uneven.
fn mini_pedigree_fit_scale(
    viewport_width: f64,
    viewport_height: f64,
    preferred_scale: f64,
    content_cx: f64,
    content_w: f64,
    content_h: f64,
    root_cx: f64,
) -> f64 {
    let content_left = content_cx - content_w / 2.0;
    let content_right = content_cx + content_w / 2.0;
    let half_width = (root_cx - content_left)
        .max(content_right - root_cx)
        .max(0.5);
    let available_width = (viewport_width - 2.0 * MINI_PEDIGREE_BOTTOM_MARGIN).max(1.0);
    let available_height = (viewport_height - 2.0 * MINI_PEDIGREE_BOTTOM_MARGIN).max(1.0);

    preferred_scale
        .min(available_width / (2.0 * half_width))
        .min(available_height / content_h.max(1.0))
}

/// Where a [`MiniPedigree`] fragment sits in a viewport of `width` × `height`:
/// scaled to fit (up to `preferred_scale`), the root card centred
/// horizontally, and either centred vertically or, with no descendants
/// below it, anchored near the bottom so the ancestor rows use the height.
fn mini_pedigree_transform(
    width: f64,
    height: f64,
    preferred_scale: f64,
    layout: &PedigreeLayout,
    anchor_bottom: bool,
    theme: &PedigreeTheme,
) -> MiniPedigreeTransformValue {
    let scale = mini_pedigree_fit_scale(
        width,
        height,
        preferred_scale,
        layout.content_cx,
        layout.content_w,
        layout.content_h,
        layout.root_cx,
    );
    let target_y = if anchor_bottom {
        mini_pedigree_root_target_y(height, scale, theme)
    } else {
        height / 2.0
    };
    MiniPedigreeTransformValue {
        x: width / 2.0 - layout.root_cx * scale,
        y: target_y - layout.root_cy * scale,
        scale,
    }
}

/// The only subtree that reacts to direct pan and zoom updates.
///
/// Keeping these signal reads out of [`PedigreeChart`] prevents every pointer
/// event from rebuilding the layout, cards and events panel just to change one
/// CSS transform.
#[component]
fn PedigreeTransform(
    transform: Signal<ViewportTransform>,
    animating: Signal<bool>,
    children: Element,
) -> Element {
    let current = transform();
    let transform = format!(
        "translate({}px, {}px) scale({})",
        current.x, current.y, current.scale
    );
    let class = if animating() {
        "pedigree-inner pedigree-animated"
    } else {
        "pedigree-inner"
    };

    rsx! {
        div {
            class,
            style: "transform: {transform};",
            {children}
        }
    }
}

/// Zoom readout isolated from the chart's layout and card rendering.
#[component]
fn PedigreeZoomValue(transform: Signal<ViewportTransform>) -> Element {
    let zoom_pct = (transform().scale * 100.0) as u32;
    rsx! { span { class: "isb-zoom-val", "{zoom_pct}%" } }
}

/// Stable component boundary between the reactive transform and the large SVG.
/// Its child is produced by `PedigreeChart`, which does not rerender during
/// direct manipulation, so transform updates can stop at this component node.
#[component]
fn PedigreeScene(children: Element) -> Element {
    children
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MiniPedigreeTransformValue {
    x: f64,
    y: f64,
    scale: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct MiniPedigreeTooltipValue {
    name: String,
    lifespan: String,
    pointer: Option<MiniPedigreeTooltipPointer>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MiniPedigreeTooltipPointer {
    x: f64,
    y: f64,
    opens_right: bool,
    opens_below: bool,
}

/// Isolates the mini-pedigree's one-time centering from its SVG scene.
#[component]
fn MiniPedigreeTransform(
    transform: Signal<MiniPedigreeTransformValue>,
    children: Element,
) -> Element {
    let current = transform();
    let transform = format!(
        "translate({}px, {}px) scale({})",
        current.x, current.y, current.scale
    );

    rsx! {
        div {
            class: "mini-pedigree-inner",
            style: "transform: {transform};",
            {children}
        }
    }
}

/// Screen-sized identity text for a mini-pedigree card.
///
/// This component alone subscribes to hover changes, so showing the tooltip
/// does not rebuild or diff the scaled SVG scene behind it.
#[component]
fn MiniPedigreeTooltip(hovered: Signal<Option<MiniPedigreeTooltipValue>>) -> Element {
    let Some(value) = hovered() else {
        return rsx! {};
    };
    let (class, style) = value.pointer.map_or_else(
        || ("mini-pedigree-tooltip".to_string(), String::new()),
        |pointer| {
            let horizontal = if pointer.opens_right {
                "mini-pedigree-tooltip-right"
            } else {
                "mini-pedigree-tooltip-left"
            };
            let vertical = if pointer.opens_below {
                "mini-pedigree-tooltip-below"
            } else {
                "mini-pedigree-tooltip-above"
            };
            (
                format!(
                    "mini-pedigree-tooltip mini-pedigree-tooltip-pointer {horizontal} {vertical}"
                ),
                format!(
                    "--mini-tooltip-x: {}px; --mini-tooltip-y: {}px;",
                    pointer.x, pointer.y
                ),
            )
        },
    );

    rsx! {
        div { class, style, role: "tooltip",
            div { class: "mini-pedigree-tooltip-name", "{value.name}" }
            if !value.lifespan.is_empty() {
                div { class: "mini-pedigree-tooltip-dates", "{value.lifespan}" }
            }
        }
    }
}

/// Props for [`MiniPedigree`] — a small pedigree fragment, focused and
/// centered on `root_person_id`, for embedding outside the main tree canvas
/// (e.g. on the person detail page). Its viewport is static and its scale is
/// fitted up to the preferred maximum (see [`MINI_PEDIGREE_SCALE`]).
#[derive(Props, Clone, PartialEq)]
pub struct MiniPedigreeProps {
    pub root_person_id: Uuid,
    pub data: SharedPedigree,
    pub ancestor_levels: usize,
    pub descendant_levels: usize,
    /// Called when the user clicks a person card (navigate to their page).
    /// Empty ancestor/descendant slots are not clickable.
    pub on_person_navigate: EventHandler<Uuid>,
    /// Preferred maximum scale; defaults to [`MINI_PEDIGREE_SCALE`]. The
    /// fragment reduces it as needed to fit, and embedders can request a
    /// denser maximum (e.g. search-result grid cells).
    #[props(default = MINI_PEDIGREE_SCALE)]
    pub scale: f64,
    /// Which theme to draw with, when the caller wants to decide rather than
    /// follow the viewer's preference — a settings preview showing each
    /// option as itself, for instance. `None` means the default theme.
    #[props(default)]
    pub theme: Option<&'static PedigreeTheme>,
    /// The portraits the cards draw; `None` draws silhouettes.
    #[props(default)]
    pub portraits: Option<Portraits>,
}

/// Keeps `viewport` at the element's content size, once it has one.
fn record_viewport(mut viewport: Signal<Option<(f64, f64)>>, evt: &Event<ResizeData>) {
    let Ok(size) = evt.get_content_box_size() else {
        return;
    };
    let measured = Some((size.width, size.height));
    if size.width > 0.0 && size.height > 0.0 && *viewport.peek() != measured {
        viewport.set(measured);
    }
}

/// Moves the tooltip of the hovered person, if any, to the pointer, opening
/// it towards the larger side of a screen `screen_width` wide.
fn follow_pointer(
    mut hovered: Signal<Option<MiniPedigreeTooltipValue>>,
    screen_width: f64,
    evt: &Event<MouseData>,
) {
    let coordinates = evt.client_coordinates();
    if let Some(value) = hovered.write().as_mut() {
        value.pointer = Some(MiniPedigreeTooltipPointer {
            x: coordinates.x,
            y: coordinates.y,
            opens_right: coordinates.x < screen_width / 2.0,
            opens_below: coordinates.y < 72.0,
        });
    }
}

/// The connectors of one side of a tree, keyed by `side` and their index; a
/// ruled one drawn twice, as a band with a lighter core.
fn connector_paths<'a>(
    links: impl Iterator<Item = (usize, &'a String)>,
    side: &str,
    double_ruled: bool,
) -> Element {
    rsx! {
        for (si, path) in links {
            path { key: "{side}l-{si}", d: "{path}", class: "pedigree-connector-path", fill: "none" }
            if double_ruled {
                path { key: "{side}lc-{si}", d: "{path}", class: "pedigree-connector-core", fill: "none" }
            }
        }
    }
}

/// A small static pedigree fragment (e.g. "parents & grandparents"), always
/// centered on `root_person_id` and fitted to its viewport. Reuses the same
/// layout engine and card renderer as the full interactive [`PedigreeChart`].
#[component]
pub fn MiniPedigree(props: MiniPedigreeProps) -> Element {
    let i18n = use_i18n();
    let selected_person_id = use_signal(|| props.root_person_id);
    let noop_click = EventHandler::new(|_: (Uuid, f64, f64)| {});
    let noop_empty_slot = EventHandler::new(|_: (Uuid, bool)| {});
    let preferred_scale = props.scale;
    use_chart_portraits(props.portraits.as_ref());
    let preferred = crate::prefs::use_pedigree_theme();
    let theme = props.theme.unwrap_or_else(|| preferred.theme());
    // A ruled line is drawn as a band with a lighter core, the way an
    // engraver lays one down; a Bézier one stays a single hairline.
    let double_ruled = theme.link_style == crate::components::pedigree_theme::LinkStyle::Ruled;

    let mut transform = use_signal(|| MiniPedigreeTransformValue {
        x: 0.0,
        y: 0.0,
        scale: preferred_scale,
    });
    let hovered_person = use_signal(|| None::<MiniPedigreeTooltipValue>);
    // This fragment's own viewport, measured by a resize observer: several
    // fragments can share a page, and a column can narrow or widen after
    // the first render, so neither a page-wide lookup nor a one-time
    // measurement fits them.
    let viewport = use_signal(|| None::<(f64, f64)>);
    let mut window_width = use_signal(|| 0.0_f64);

    let layout = crate::ui_observability::measure_ui("pedigree_layout", || {
        compute_layout(
            props.root_person_id,
            &props.data,
            None,
            &HashSet::new(),
            PedigreeLayoutOptions::mini(props.ancestor_levels, props.descendant_levels),
            theme,
        )
    });

    // Refit on every render that changes the answer: a new root, new data,
    // or a new viewport size. Only `MiniPedigreeTransform` reads the signal,
    // so writing it here redraws the transform, not this scene.
    // (`desc_nodes` always contains at least the root card itself, even at
    // descendant_levels == 0, so the prop decides the bottom anchoring.)
    let measured = viewport();
    let fitted = measured.map(|(width, height)| {
        mini_pedigree_transform(
            width,
            height,
            preferred_scale,
            &layout,
            props.descendant_levels == 0,
            theme,
        )
    });
    if let Some(fitted) = fitted.filter(|fitted| *transform.peek() != *fitted) {
        transform.set(fitted);
    }
    // Hidden until measured, rather than drawn once at the origin.
    let pending = if measured.is_some() {
        ""
    } else {
        "mini-pedigree-pending "
    };
    let viewport_class = format!("mini-pedigree {pending}{}", theme.viewport_class);

    rsx! {
        div {
            class: viewport_class,
            onmounted: move |_| async move {
                if let Ok(width) = document::eval("return window.innerWidth").await
                    && let Some(width) = width.as_f64()
                {
                    window_width.set(width);
                }
            },
            onresize: move |evt: Event<ResizeData>| record_viewport(viewport, &evt),
            onmousemove: move |evt: Event<MouseData>| {
                follow_pointer(hovered_person, window_width(), &evt);
            },
            MiniPedigreeTransform { transform,
                PedigreeScene {
                    svg {
                        width: "{layout.total_w}",
                        height: "{layout.total_h}",
                        "viewBox": "0 0 {layout.total_w} {layout.total_h}",
                        style: "display: block; overflow: visible;",
                        g { transform: "translate({layout.main_tx},{layout.main_ty})",
                            g {
                                {connector_paths(layout.asc_links.iter().enumerate(), "a", double_ruled)}
                                for (ni, node) in layout.asc_nodes.iter().enumerate() {
                                    {render_pedigree_card(
                                        node,
                                        ni,
                                        "an",
                                        props.root_person_id,
                                        selected_person_id,
                                        props.on_person_navigate,
                                        noop_click,
                                        noop_empty_slot,
                                        false,
                                        i18n,
                                        theme,
                                        Some(hovered_person),
                                    )}
                                }
                            }
                            if props.descendant_levels > 0 {
                                g {
                                    transform: "translate({layout.desc_tx},{layout.desc_ty})",
                                    {connector_paths(layout.desc_links.iter().enumerate(), "d", double_ruled)}
                                    for (ni, node) in layout.desc_nodes.iter().enumerate() {
                                        {render_pedigree_card(
                                            node,
                                            ni,
                                            "dn",
                                            props.root_person_id,
                                            selected_person_id,
                                            props.on_person_navigate,
                                            noop_click,
                                            noop_empty_slot,
                                            false,
                                            i18n,
                                            theme,
                                            Some(hovered_person),
                                        )}
                                    }
                                }
                            }
                        }
                    }
                }
            }
            MiniPedigreeTooltip { hovered: hovered_person }
        }
    }
}

/// A miniature of a tree view drawing, for the view picker in the settings.
///
/// The circular views draw their own rings and segments, three generations
/// deep, so the picture cannot drift from the chart; the tree is a schematic
/// of parents above the root and children below it.
#[component]
pub fn PedigreeViewSwatch(view: PedigreeView) -> Element {
    match view {
        PedigreeView::Tree => rsx! {
            svg {
                class: "ped-theme-swatch ped-view-swatch",
                "viewBox": "0 0 120 68",
                "preserveAspectRatio": "xMidYMid meet",
                "aria-hidden": "true",
                rect { x: "0", y: "0", width: "120", height: "68", style: "fill:var(--pn-swatch-bg,transparent)" }
                path { class: "pedigree-connector-path", d: "M54,14 H66 M60,14 V30 M60,42 V47 M36,47 H84 M36,47 V52 M84,47 V52" }
                for (i, (x, y)) in [(18.0, 8.0), (66.0, 8.0), (42.0, 30.0), (18.0, 52.0), (66.0, 52.0)].into_iter().enumerate() {
                    rect {
                        key: "{i}",
                        x: "{x}", y: "{y}", width: "36", height: "12", rx: "2",
                        style: if i == 2 { "fill:var(--pn-root-bg);stroke:var(--pn-border)" } else { "fill:var(--pn-bg);stroke:var(--pn-border)" },
                    }
                }
            }
        },
        PedigreeView::Wheel => circular::swatch(circular::ChartArc::WHEEL),
        PedigreeView::Fan => circular::swatch(circular::ChartArc::FAN),
        PedigreeView::DescendantWheel => circular::swatch(circular::ChartArc::DESCENDANT_WHEEL),
        PedigreeView::DescendantFan => circular::swatch(circular::ChartArc::DESCENDANT_FAN),
        PedigreeView::Lineage => lineage::swatch(),
        PedigreeView::DescendantLineage => lineage::descendant_lineage_swatch(),
        PedigreeView::Hourglass => lineage::hourglass_swatch(),
        PedigreeView::Bowtie => lineage::bowtie_swatch(),
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct PedigreeChartProps {
    pub root_person_id: Uuid,
    pub data: SharedPedigree,
    pub tree_id: String,
    /// SOSA root person ID from tree settings. When set, ancestors of this
    /// person get a small badge indicator on their card.
    #[props(default)]
    pub sosa_root_person_id: Option<Uuid>,
    /// Pre-computed set of ancestor IDs for the SOSA root (from the closure
    /// table). When provided, used instead of traversing the limited pedigree
    /// graph — ensures badges appear even when jumping to distant ancestors.
    #[props(default)]
    pub sosa_ancestor_ids: Option<AncestorSet>,
    /// The portraits the cards and the events panel draw, kept out of `data`
    /// so that their arrival redraws only the pictures (see [`Portraits`]).
    #[props(default)]
    pub portraits: Option<Portraits>,
    /// Incremented by the parent to force re-centering on the root person,
    /// even when `root_person_id` hasn't changed (e.g. navigating back from
    /// the person profile page).
    #[props(default)]
    pub center_gen: u32,
    pub on_person_click: EventHandler<(Uuid, f64, f64)>,
    pub on_person_navigate: EventHandler<Uuid>,
    pub on_empty_slot: EventHandler<(Uuid, bool)>,
    /// Called when the user clicks the empty "+" placeholder for a missing
    /// spouse on the descending side (the person needing a spouse).
    pub on_add_spouse_slot: EventHandler<Uuid>,
    #[props(default)]
    pub on_add_person: EventHandler<()>,
    #[props(default)]
    pub on_profile_view: EventHandler<Uuid>,
    /// Opens the couple view on a family.
    #[props(default)]
    pub on_couple_view: EventHandler<Uuid>,
    #[props(default)]
    pub on_settings: EventHandler<()>,
    #[props(default)]
    pub on_dictionary: EventHandler<()>,
    /// Which theme to draw with, when the caller wants to decide rather than
    /// follow the viewer's preference — a settings preview showing each
    /// option as itself, for instance. `None` means the default theme.
    #[props(default)]
    pub theme: Option<&'static PedigreeTheme>,
}

/// The widest text a card's name column can hold, in pixels.
///
/// The compact column is whatever the compact rectangle has left after the
/// text indent, so a theme that narrows its compact card narrows this with it
/// rather than letting the names run past the frame.
fn text_max_width(is_compact: bool, theme: &PedigreeTheme) -> f32 {
    if is_compact {
        theme.card.text_max_width_compact
    } else {
        theme.card.text_max_width_full
    }
}

/// Where everything inside one card goes, for the theme that is drawing it.
///
/// Pure: the same node and theme always give the same numbers and strings,
/// which is what lets a test pin a card's interior without rendering it. The
/// `rsx!` below only places what this decided, so the two themes share one
/// renderer and differ in data rather than in code.
struct CardGeometry {
    /// Drawn rectangle of the card.
    rect_w: f64,
    rect_h: f64,
    /// How the outline is drawn, and the inner rule's inset when it has one.
    frame: CardFrame,
    /// `d` of the card outline, for a theme whose frame is a shape rather
    /// than a rectangle. `None` means draw the rectangle.
    frame_d: Option<String>,
    /// `d` of the second rule inside it.
    inner_frame_d: Option<String>,
    /// `text-anchor` for the three name lines.
    text_anchor: &'static str,
    /// `d` of the sex-coded rule, for a theme that draws one beside the
    /// portrait rather than colouring the whole frame.
    gender_line: Option<String>,
    gender_line_width: f64,
    photo_x: f64,
    photo_y: f64,
    photo_w: f64,
    photo_h: f64,
    /// Corner radius of the portrait mat; half its width makes a medallion.
    photo_round: f64,
    /// Whether a ground is painted behind the portrait at all.
    photo_mat: bool,
    text_x: f64,
    sosa_cx: f64,
    sosa_cy: f64,
    sosa_r: f64,
    /// Name pieces already truncated to the column they must fit.
    given: String,
    surname: String,
    given_y: f64,
    surname_y: f64,
    date_y: f64,
    /// The lifespan as drawn, and as markup carrying its own `<title>`.
    date_text: String,
    date_html: String,
    /// Set when the lifespan has to be squeezed to fit its column.
    date_squeeze: Option<f32>,
    /// Type the theme sets its three lines in.
    surname_font: &'static str,
    body_font: &'static str,
    surname_weight: &'static str,
    surname_font_px: f32,
    given_font_px: f32,
    date_font_px: f32,
    /// Centre and radius of the edit button hanging below the focused card.
    fab_x: f64,
    fab_y: f64,
    fab_r: f64,
    /// Baseline position of the "+" marking relations outside the layout.
    more_relations_x: f64,
    more_relations_y: f64,
    /// Centre of the "+" glyph drawn in an empty slot.
    slot_plus_x: f64,
    slot_plus_y: f64,
}

fn card_geometry(node: &LayoutNode, theme: &PedigreeTheme, i18n: &I18n) -> CardGeometry {
    let metrics = &theme.metrics;
    let card = &theme.card;
    let is_compact = node.is_compact;
    let (rect_w, rect_h) = metrics.rect(is_compact);
    let (photo_x, text_x, ty, sosa_cx) = if is_compact {
        (
            card.photo_x_compact,
            card.text_x_compact,
            card.text_y_compact,
            card.sosa_cx_compact,
        )
    } else {
        (
            card.photo_x_full,
            card.text_x_full,
            card.text_y_full,
            card.sosa_cx_full,
        )
    };

    let max_width = text_max_width(is_compact, theme);
    let surname_up = node
        .label_surname
        .split(",")
        .next()
        .unwrap_or("")
        .to_uppercase();
    let surname = truncate_text_to_fit(&surname_up, max_width, card.surname_font_px);
    let label_given = node.label_given.split(",").next().unwrap_or("");
    let given = truncate_text_to_fit(label_given, max_width, card.given_font_px);
    let date_text = fit_lifespan(
        node.birth_year,
        node.death_year,
        max_width,
        card.date_font_px,
    );
    // Spelled-out form for the hover title; empty when both years are
    // exact and the marks need no explaining.
    let date_title = lifespan_tooltip(i18n, node.birth_year, node.death_year);
    // A bare `1849-1917` always fitted, so the date line was never
    // measured. The marks make it up to five characters longer, which
    // overruns a compact card's column. Squeeze rather than truncate:
    // dropping characters off a date would silently change what it
    // says, while `textLength` keeps every one of them legible.
    let date_squeeze = (crate::utils::estimate_text_width_px(&date_text, card.date_font_px)
        > max_width)
        .then_some(max_width);
    // A native SVG tooltip is a `<title>` child, and rsx cannot make
    // one: dioxus-html defines `title` as the HTML element (its SVG
    // twin is commented out), and an HTML-namespaced `<title>` inside
    // an `<svg>` is inert. Assigning innerHTML on an SVG element parses
    // the fragment in the SVG namespace, which is the only route to a
    // real tooltip here.
    //
    // Both strings are ours — translated words, integers and the fixed
    // marks — but the marks are literally `<` and `>`, so they are
    // escaped rather than trusted.
    let date_html = if date_title.is_empty() {
        escape_xml(&date_text)
    } else {
        format!(
            "<title>{}</title>{}",
            escape_xml(&date_title),
            escape_xml(&date_text)
        )
    };

    let given_y = ty;
    let surname_y = if given.is_empty() {
        ty
    } else {
        ty + card.name_line_step
    };
    let date_y = if !surname.is_empty() {
        surname_y + card.name_line_step
    } else if !given.is_empty() {
        given_y + card.name_line_step
    } else {
        ty
    };

    let gender_line = card.gender_rule.map(|rule| {
        let x = if is_compact {
            rule.x_compact
        } else {
            rule.x_full
        };
        format!("M{x},{} L{x},{}", rule.top, rule.bottom)
    });

    // The outline, and the second rule drawn inside it. Both are laid out
    // from the same box, so the inner rule is a smaller cartouche of the
    // same shape rather than an outline offset by a constant.
    let inset = match card.frame {
        CardFrame::Plain => 0.0,
        CardFrame::Cartouche { inner_inset } => inner_inset,
    };
    let frame_d = frame_path(card.frame, metrics.padding, metrics.padding, rect_w, rect_h);
    let inner_frame_d = frame_path(
        card.frame,
        metrics.padding + inset,
        metrics.padding + inset,
        rect_w - 2.0 * inset,
        rect_h - 2.0 * inset,
    );

    CardGeometry {
        rect_w,
        rect_h,
        frame: card.frame,
        frame_d,
        inner_frame_d,
        text_anchor: card.text_anchor,
        gender_line,
        gender_line_width: card.gender_rule.map_or(0.0, |rule| rule.width),
        photo_x,
        photo_y: card.photo_y,
        photo_w: card.photo_w,
        photo_h: card.photo_h,
        photo_round: card.photo_round,
        photo_mat: card.photo_mat,
        text_x,
        sosa_cx,
        sosa_cy: card.sosa_cy,
        sosa_r: card.sosa_r,
        given,
        surname,
        given_y,
        surname_y,
        date_y,
        date_text,
        date_html,
        date_squeeze,
        surname_font: card.surname_font,
        body_font: card.body_font,
        surname_weight: card.surname_weight,
        surname_font_px: card.surname_font_px,
        given_font_px: card.given_font_px,
        date_font_px: card.date_font_px,
        fab_x: metrics.padding + rect_w / 2.0,
        fab_y: metrics.padding + rect_h + card.edit_fab_gap,
        fab_r: card.edit_fab_r,
        more_relations_x: card.more_relations_x,
        more_relations_y: if is_compact {
            card.more_relations_y_compact
        } else {
            card.more_relations_y_full
        },
        slot_plus_x: metrics.padding + rect_w / 2.0,
        slot_plus_y: metrics.padding + rect_h / 2.0 + card.slot_plus_baseline,
    }
}

/// Render one card (person or empty slot) of the pedigree as an SVG `<g>`.
///
/// Used for both ascending and descending trees — pass the matching key
/// prefix (`"an"` / `"dn"`) and `allow_empty_click=true` on both sides, so
/// missing parents (ascending) and missing spouses (descending) can be
/// added inline.
#[allow(clippy::too_many_arguments)]
fn render_pedigree_card(
    node: &LayoutNode,
    ni: usize,
    key_prefix: &str,
    root_person_id: Uuid,
    selected_person_id: Signal<Uuid>,
    on_person_navigate: EventHandler<Uuid>,
    on_person_click: EventHandler<(Uuid, f64, f64)>,
    on_empty_slot: EventHandler<(Uuid, bool)>,
    allow_empty_click: bool,
    i18n: I18n,
    theme: &PedigreeTheme,
    mini_tooltip: Option<Signal<Option<MiniPedigreeTooltipValue>>>,
) -> Element {
    let geo = card_geometry(node, theme, &i18n);
    let key = format!("{key_prefix}-{ni}");

    match node.id {
        Some(pid) => render_person_card(
            node,
            pid,
            &key,
            &geo,
            theme,
            &i18n,
            PersonCardActions {
                root_person_id,
                selected_person_id,
                on_person_navigate,
                on_person_click,
                mini_tooltip,
            },
        ),
        None => render_empty_slot(
            node,
            &key,
            &geo,
            theme,
            allow_empty_click.then_some(on_empty_slot),
        ),
    }
}

/// What a person card answers to, and the state it reports into.
#[derive(Clone, Copy)]
struct PersonCardActions {
    root_person_id: Uuid,
    selected_person_id: Signal<Uuid>,
    on_person_navigate: EventHandler<Uuid>,
    on_person_click: EventHandler<(Uuid, f64, f64)>,
    mini_tooltip: Option<Signal<Option<MiniPedigreeTooltipValue>>>,
}

/// The card of the person `pid`: outline, portrait, SOSA mark, name and
/// lifespan, plus the edit button on the focus card and the "+" marking
/// relations outside the layout.
fn render_person_card(
    node: &LayoutNode,
    pid: Uuid,
    key: &str,
    geo: &CardGeometry,
    theme: &PedigreeTheme,
    i18n: &I18n,
    actions: PersonCardActions,
) -> Element {
    let PersonCardActions {
        root_person_id,
        mut selected_person_id,
        on_person_navigate,
        on_person_click,
        mini_tooltip,
    } = actions;
    let (nx, ny) = (node.x, node.y);
    let is_focus = pid == root_person_id;
    let bg = card_bg(is_focus, node.is_sibling);
    // The lifespan is secondary to the name, and every theme gives it a
    // colour of its own for that; on the root card both sit on the
    // accent and share its contrast colour.
    let (text_fill, date_fill) = if is_focus {
        ("var(--white)", "var(--white)")
    } else {
        ("var(--pn-text)", "var(--pn-text-muted)")
    };
    let stroke = gender_stroke(node.sex);
    let card_class = if is_focus {
        "ped-card ped-card-focus"
    } else {
        "ped-card"
    };
    let tooltip_value = card_tooltip(node, i18n);
    let tooltip_name = tooltip_value.name.clone();
    let enter_tooltip = tooltip_value.clone();
    let focus_tooltip = tooltip_value.clone();
    let CardGeometry {
        gender_line,
        gender_line_width,
        photo_x: ph_x,
        photo_y: ph_y,
        photo_w: ph_w,
        photo_h: ph_h,
        photo_round,
        photo_mat,
        fab_x,
        fab_y,
        fab_r,
        more_relations_x,
        more_relations_y,
        ..
    } = geo;
    rsx! {
        g {
            key: "{key}",
            class: "{card_class}",
            transform: "translate({nx},{ny})",
            style: "cursor:pointer",
            role: mini_tooltip.map(|_| "link"),
            tabindex: mini_tooltip.map(|_| "0"),
            "aria-label": mini_tooltip.map(|_| tooltip_name.clone()),
            onmouseenter: move |_| {
                if let Some(mut hovered) = mini_tooltip {
                    hovered.set(Some(enter_tooltip.clone()));
                }
            },
            onmouseleave: move |_| {
                if let Some(mut hovered) = mini_tooltip {
                    hovered.set(None);
                }
            },
            onfocus: move |_| {
                if let Some(mut hovered) = mini_tooltip {
                    hovered.set(Some(focus_tooltip.clone()));
                }
            },
            onblur: move |_| {
                if let Some(mut hovered) = mini_tooltip {
                    hovered.set(None);
                }
            },
            onclick: move |_| { selected_person_id.set(pid); on_person_navigate.call(pid); },
            oncontextmenu: move |evt: Event<MouseData>| {
                evt.prevent_default();
                evt.stop_propagation();
                selected_person_id.set(pid);
                let coords = evt.client_coordinates();
                on_person_click.call((pid, coords.x, coords.y));
            },
            {card_outline(node, geo, theme, bg)}
            if let Some(gl) = gender_line {
                path { d: "{gl}", style: "stroke:{stroke};stroke-width:{gender_line_width};fill:none" }
            }
            if *photo_mat {
                rect { class: "ped-card-mat", x: "{ph_x}", y: "{ph_y}", rx: "{photo_round}", ry: "{photo_round}", width: "{ph_w}", height: "{ph_h}", style: "fill:var(--pn-mat,var(--white))" }
            }
            CardPortrait { person: pid, sex: node.sex, x: *ph_x, y: *ph_y, width: *ph_w, height: *ph_h }
            {sosa_mark(node, geo)}
            {card_text(geo, text_fill, date_fill)}
            if is_focus {
                g {
                    class: "no-print",
                    transform: "translate({fab_x},{fab_y})",
                    style: "cursor:pointer",
                    onclick: move |evt: Event<MouseData>| {
                        evt.stop_propagation();
                        let coords = evt.client_coordinates();
                        on_person_click.call((pid, coords.x, coords.y));
                    },
                    circle { r: "{fab_r}", style: "fill:var(--pn-root-bg);stroke:var(--white);stroke-width:2" }
                    text { x: "0", y: "6", style: "fill:var(--white);font-size:16px;text-anchor:middle;font-family:serif", "\u{270E}" }
                }
            }
            if node.has_more_relations {
                g {
                    transform: "translate({more_relations_x},{more_relations_y})",
                    style: "cursor:pointer",
                    onclick: move |evt: Event<MouseData>| {
                        evt.stop_propagation();
                        selected_person_id.set(pid);
                        on_person_navigate.call(pid);
                    },
                    text { x: "0", y: "0", style: "fill:var(--blue);font-size:13px;font-weight:700;text-anchor:middle;font-family:sans-serif", "+" }
                }
            }
        }
    }
}

/// What the mini pedigree's tooltip says about a card: the name, and the
/// lifespan with its qualifiers spelled out when it has any.
fn card_tooltip(node: &LayoutNode, i18n: &I18n) -> MiniPedigreeTooltipValue {
    let name = format!("{} {}", node.label_given, node.label_surname)
        .trim()
        .to_string();
    let qualified_lifespan = lifespan_tooltip(i18n, node.birth_year, node.death_year);
    MiniPedigreeTooltipValue {
        name,
        lifespan: if qualified_lifespan.is_empty() {
            format_lifespan(node.birth_year, node.death_year)
        } else {
            qualified_lifespan
        },
        pointer: None,
    }
}

/// A person card's outline filled with `bg`, and a cartouche's inner rule.
fn card_outline(node: &LayoutNode, geo: &CardGeometry, theme: &PedigreeTheme, bg: &str) -> Element {
    let padding = theme.metrics.padding;
    let border_radius = theme.metrics.border_radius;
    let (rw, rh) = (geo.rect_w, geo.rect_h);
    // The classic card shows sex on a short rule beside the portrait and
    // keeps a neutral outline; a cartouche is heavy enough to carry the
    // colour itself, and drops the rule.
    let frame_stroke = match theme.card.frame_stroke {
        FrameStroke::Border => "var(--pn-border)",
        FrameStroke::Gender => gender_stroke(node.sex),
    };
    let frame_width = theme.card.frame_width;
    let inner = match geo.frame {
        CardFrame::Plain => None,
        CardFrame::Cartouche { inner_inset } => Some((
            padding + inner_inset,
            rw - 2.0 * inner_inset,
            rh - 2.0 * inner_inset,
        )),
    };
    rsx! {
        if let Some(d) = &geo.frame_d {
            path { class: "ped-card-rect", d: "{d}", style: "fill:{bg};stroke:{frame_stroke};stroke-width:{frame_width}" }
        } else {
            rect { class: "ped-card-rect", x: "{padding}", y: "{padding}", rx: "{border_radius}", ry: "{border_radius}", width: "{rw}", height: "{rh}", style: "fill:{bg};stroke:{frame_stroke};stroke-width:{frame_width}" }
        }
        if let Some(d) = &geo.inner_frame_d {
            path { class: "ped-card-inner-rule", d: "{d}", style: "fill:none;stroke:var(--pn-border);stroke-width:1" }
        } else if let Some((inset, iw, ih)) = inner {
            rect { class: "ped-card-inner-rule", x: "{inset}", y: "{inset}", width: "{iw}", height: "{ih}", style: "fill:none;stroke:var(--pn-border);stroke-width:1" }
        }
    }
}

/// The badge in a card's corner: the user's own mark, the SOSA root's "1",
/// or a direct ancestor's ring. Nothing for anyone else.
fn sosa_mark(node: &LayoutNode, geo: &CardGeometry) -> Element {
    let (sosa_cx, sosa_cy, sosa_r) = (geo.sosa_cx, geo.sosa_cy, geo.sosa_r);
    rsx! {
        if node.is_self {
            g {
                circle { cx: "{sosa_cx}", cy: "{sosa_cy}", r: "{sosa_r}", style: "fill:var(--pn-self)" }
                circle { cx: "{sosa_cx}", cy: "{sosa_cy}", r: "3", style: "fill:var(--white)" }
            }
        } else if matches!(node.sosa_badge, SosaBadge::Root) {
            g {
                circle { cx: "{sosa_cx}", cy: "{sosa_cy}", r: "{sosa_r}", style: "fill:var(--pn-sosa-root)" }
                text { x: "{sosa_cx}", y: "{sosa_cy+4.0}", style: "fill:var(--white);font-size:10px;font-weight:700;text-anchor:middle;font-family:Arial,sans-serif", "1" }
            }
        } else if matches!(node.sosa_badge, SosaBadge::Direct) {
            g {
                circle { cx: "{sosa_cx}", cy: "{sosa_cy}", r: "{sosa_r}", style: "fill:var(--pn-sosa)" }
                circle { cx: "{sosa_cx}", cy: "{sosa_cy}", r: "5", style: "fill:var(--white)" }
                circle { cx: "{sosa_cx}", cy: "{sosa_cy}", r: "3", style: "fill:var(--pn-sosa)" }
            }
        }
    }
}

/// A person card's given name and surname, then its lifespan.
fn card_text(geo: &CardGeometry, text_fill: &str, date_fill: &str) -> Element {
    let CardGeometry {
        text_anchor,
        text_x: tx,
        given: given_disp,
        surname: surname_disp,
        given_y,
        surname_y,
        date_y,
        date_text: date_s,
        date_html,
        date_squeeze,
        surname_font,
        body_font,
        surname_weight,
        surname_font_px,
        given_font_px,
        date_font_px,
        ..
    } = geo;
    let date_squeeze = *date_squeeze;
    rsx! {
        text {
            class: "ped-card-name-text",
            if !given_disp.is_empty() {
                tspan { x: "{tx}", y: "{given_y}", style: "font-size:{given_font_px}px;font-family:{body_font};fill:{text_fill};text-anchor:{text_anchor}", "{given_disp}" }
            }
            if !surname_disp.is_empty() {
                tspan { x: "{tx}", y: "{surname_y}", style: "font-size:{surname_font_px}px;font-weight:{surname_weight};font-family:{surname_font};fill:{text_fill};text-anchor:{text_anchor}", "{surname_disp}" }
            }
        }
        // The lifespan is its own `text` rather than a third tspan
        // so it can own a `<title>`: SVG 1.1 does not allow one
        // inside a `tspan`, and the qualifier marks are exactly the
        // part of the card that needs to be able to explain itself.
        // Absolute x/y means it lands where the tspan did.
        if !date_s.is_empty() {
            text {
                class: "ped-card-name-text",
                x: "{tx}",
                y: "{date_y}",
                style: "font-size:{date_font_px}px;font-family:{body_font};fill:{date_fill};text-anchor:{text_anchor}",
                "textLength": date_squeeze.map(|w| w.to_string()),
                "lengthAdjust": date_squeeze.map(|_| "spacingAndGlyphs"),
                dangerous_inner_html: "{date_html}",
            }
        }
    }
}

/// An empty slot: a dashed outline, with a "+" to fill it when
/// `on_empty_slot` is given and the slot knows whose parent it stands for,
/// faded otherwise.
fn render_empty_slot(
    node: &LayoutNode,
    key: &str,
    geo: &CardGeometry,
    theme: &PedigreeTheme,
    on_empty_slot: Option<EventHandler<(Uuid, bool)>>,
) -> Element {
    let (nx, ny) = (node.x, node.y);
    let is_father = node.is_father;
    let plus_x = geo.slot_plus_x;
    let plus_y = geo.slot_plus_y;
    rsx! {
        g { key: "{key}", transform: "translate({nx},{ny})",
            if let (Some(on_empty_slot), Some(cid)) = (on_empty_slot, node.child_of) {
                g {
                    style: "cursor:pointer",
                    onclick: move |_| on_empty_slot.call((cid, is_father)),
                    {empty_slot_outline(geo, theme, "fill:var(--pn-bg);stroke:var(--pn-border);stroke-width:1;stroke-dasharray:4,4")}
                    text { class: "no-print", x: "{plus_x}", y: "{plus_y}", style: "fill:var(--pn-root-bg);font-size:22px;font-weight:700;text-anchor:middle;font-family:sans-serif", "+" }
                }
            } else {
                {empty_slot_outline(geo, theme, "fill:var(--pn-bg);stroke:var(--pn-border);stroke-width:1;stroke-dasharray:4,4;opacity:0.3")}
            }
        }
    }
}

/// An empty slot's outline in `style`: the theme's frame shape, or else a
/// rounded rectangle.
fn empty_slot_outline(geo: &CardGeometry, theme: &PedigreeTheme, style: &str) -> Element {
    let padding = theme.metrics.padding;
    let border_radius = theme.metrics.border_radius;
    let (rw, rh) = (geo.rect_w, geo.rect_h);
    rsx! {
        if let Some(d) = &geo.frame_d {
            path { d: "{d}", style: "{style}" }
        } else {
            rect { x: "{padding}", y: "{padding}", rx: "{border_radius}", ry: "{border_radius}", width: "{rw}", height: "{rh}", style: "{style}" }
        }
    }
}

// ── Drawing only what can be seen ──────────────────────────────────────

/// Room around a card for what it draws past its frame: the edit button under
/// the focus card, badges on its corners, a stroke's width.
const CARD_OVERHANG: f64 = 64.0;

/// Room around a connector's points for its stroke — the ruled style draws
/// a band, not a hairline.
const LINK_OVERHANG: f64 = 8.0;

/// Viewport size to assume before the real one has been measured — as large
/// as a screen gets, so an early render never leaves part of the window
/// empty. A too-large guess only draws a few more cards for a moment.
const UNMEASURED_VIEWPORT: (f64, f64) = (3840.0, 2160.0);

/// An axis-aligned rectangle in the canvas's content coordinates — those of
/// `.pedigree-tree`, before the pan-and-zoom transform.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Area {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Area {
    fn intersects(&self, other: &Area) -> bool {
        self.x0 <= other.x1 && other.x0 <= self.x1 && self.y0 <= other.y1 && other.y0 <= self.y1
    }

    fn contains(&self, other: &Area) -> bool {
        self.x0 <= other.x0 && other.x1 <= self.x1 && self.y0 <= other.y0 && other.y1 <= self.y1
    }

    /// Grown by `factor` of its own width and height on every side.
    fn grown(&self, factor: f64) -> Area {
        let dx = (self.x1 - self.x0) * factor;
        let dy = (self.y1 - self.y0) * factor;
        Area {
            x0: self.x0 - dx,
            y0: self.y0 - dy,
            x1: self.x1 + dx,
            y1: self.y1 + dy,
        }
    }

    /// Grown by `margin` on every side.
    fn padded(&self, margin: f64) -> Area {
        Area {
            x0: self.x0 - margin,
            y0: self.y0 - margin,
            x1: self.x1 + margin,
            y1: self.y1 + margin,
        }
    }

    fn union(&self, other: &Area) -> Area {
        Area {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    fn translated(&self, dx: f64, dy: f64) -> Area {
        Area {
            x0: self.x0 + dx,
            y0: self.y0 + dy,
            x1: self.x1 + dx,
            y1: self.y1 + dy,
        }
    }
}

/// The part of the canvas the viewport shows, in content coordinates.
///
/// The transform is `translate(x, y) scale(s)` about the top-left corner, so a
/// viewport point `p` shows content point `(p - offset) / s`.
fn visible_area(transform: ViewportTransform, viewport: ViewportRect) -> Area {
    let (width, height) = if viewport == ViewportRect::assumed() {
        UNMEASURED_VIEWPORT
    } else {
        (viewport.left + viewport.width, viewport.height)
    };
    let scale = transform.scale.max(f64::EPSILON);
    Area {
        x0: -transform.x / scale,
        y0: -transform.y / scale,
        x1: (width - transform.x) / scale,
        y1: (height - transform.y) / scale,
    }
}

/// What the canvas currently draws, and what the viewport showed when it
/// was last asked.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Culling {
    region: Area,
    visible: Area,
}

/// Decide what to draw for the view now showing.
///
/// The drawn region runs a full viewport past the visible area on every
/// side, and is kept for as long as it still covers three quarters of a
/// viewport past it. A pan therefore redraws once per quarter screen, never
/// per frame, and each redraw only adds the thin strip of cards that quarter
/// uncovered — a few dozen, not the hundreds a larger step would bring in at
/// once, which is what keeps the redraw inside a frame or two. And it lands
/// while three quarters of a screen of drawn content still separate the edge
/// of the view from anything missing: nothing that should be visible is ever
/// absent.
///
/// A transform applied through the CSS transition (`animating`) jumps to its
/// end value while the screen still travels there. Every frame of that
/// travel shows a rectangle whose edges move monotonically between the start
/// and end views, so the region also keeps the view it started from.
fn cull(previous: Option<Culling>, visible: Area, animating: bool) -> Culling {
    let needed = visible.grown(0.75);
    match previous {
        Some(previous) if previous.region.contains(&needed) => Culling {
            region: previous.region,
            visible,
        },
        _ => {
            let mut region = visible.grown(1.0);
            if animating && let Some(previous) = previous {
                region = region.union(&previous.visible);
            }
            Culling { region, visible }
        }
    }
}

/// Hook: the region of the canvas a chart view draws — only what the
/// viewport can reach is in the DOM, in every view.
///
/// The memo re-runs on every pan and zoom but changes — and so redraws the
/// view that reads it — only when the view nears the edge of what is drawn;
/// see [`cull`].
fn use_culled_region(
    transform: Signal<ViewportTransform>,
    viewport: Signal<ViewportRect>,
    animating: Signal<bool>,
) -> Area {
    let culling = use_hook(|| Rc::new(Cell::new(None::<Culling>)));
    let everything = try_use_context::<crate::components::print::PrintEverything>();
    let region = use_memo(move || {
        // A chart printed over several sheets is drawn whole meanwhile.
        if everything.is_some_and(|e| (e.0)()) {
            return Area {
                x0: f64::NEG_INFINITY,
                y0: f64::NEG_INFINITY,
                x1: f64::INFINITY,
                y1: f64::INFINITY,
            };
        }
        let next = cull(
            culling.get(),
            visible_area(transform(), viewport()),
            animating(),
        );
        culling.set(Some(next));
        next.region
    });
    region()
}

/// The extent of an SVG path, from every coordinate pair in it.
///
/// Connectors are written as absolute `x,y` pairs (`M`, `L`, `C`, `S`), and a
/// Bézier curve never leaves the hull of its control points, so the pairs
/// bound the line.
fn path_extent(d: &str) -> Option<Area> {
    let mut extent: Option<Area> = None;
    for token in d.split_whitespace() {
        let token = token.trim_start_matches(|c: char| c.is_ascii_alphabetic());
        let Some((x, y)) = token.split_once(',') else {
            continue;
        };
        let (Ok(x), Ok(y)) = (x.parse::<f64>(), y.parse::<f64>()) else {
            continue;
        };
        let point = Area {
            x0: x,
            y0: y,
            x1: x,
            y1: y,
        };
        extent = Some(extent.map_or(point, |e| e.union(&point)));
    }
    extent
}

/// Where each card and connector of a layout lies, in content coordinates.
struct SceneExtents {
    asc_cards: Vec<Area>,
    desc_cards: Vec<Area>,
    asc_links: Vec<Option<Area>>,
    desc_links: Vec<Option<Area>>,
}

impl SceneExtents {
    fn of(layout: &PedigreeLayout, theme: &PedigreeTheme) -> Self {
        let metrics = &theme.metrics;
        let card_w = metrics.card_w.max(metrics.compact_w) + 2.0 * metrics.padding;
        let card_h = metrics.card_h.max(metrics.compact_h) + 2.0 * metrics.padding;
        let card = |node: &LayoutNode, dx: f64, dy: f64| Area {
            x0: node.x + dx - CARD_OVERHANG,
            y0: node.y + dy - CARD_OVERHANG,
            x1: node.x + dx + card_w + CARD_OVERHANG,
            y1: node.y + dy + card_h + CARD_OVERHANG,
        };
        let (asc_dx, asc_dy) = (layout.main_tx, layout.main_ty);
        let (desc_dx, desc_dy) = (
            layout.main_tx + layout.desc_tx,
            layout.main_ty + layout.desc_ty,
        );
        let link = |d: &String, dx: f64, dy: f64| {
            path_extent(d).map(|area| area.padded(LINK_OVERHANG).translated(dx, dy))
        };
        Self {
            asc_cards: layout
                .asc_nodes
                .iter()
                .map(|n| card(n, asc_dx, asc_dy))
                .collect(),
            desc_cards: layout
                .desc_nodes
                .iter()
                .map(|n| card(n, desc_dx, desc_dy))
                .collect(),
            asc_links: layout
                .asc_links
                .iter()
                .map(|d| link(d, asc_dx, asc_dy))
                .collect(),
            desc_links: layout
                .desc_links
                .iter()
                .map(|d| link(d, desc_dx, desc_dy))
                .collect(),
        }
    }
}

/// Whether something with this extent has to be drawn. A path whose extent
/// could not be read is always drawn: culling must never hide anything.
fn in_region(region: &Area, extent: Option<&Area>) -> bool {
    extent.is_none_or(|extent| region.intersects(extent))
}

/// Which half of a layout a card belongs to.
#[derive(Clone, Copy, PartialEq)]
enum CardSide {
    Ascending,
    Descending,
}

/// One card of a [`PedigreeCanvas`].
///
/// A component so that a card that stays in view is skipped, not rebuilt,
/// when the canvas redraws for the cards a pan brings in: its props are the
/// layout handle and a position in it, equal for as long as the layout is.
#[component]
fn PedigreeCard(
    layout: SharedLayout,
    side: CardSide,
    index: usize,
    root_person_id: Uuid,
    selected_person_id: Signal<Uuid>,
    on_person_navigate: EventHandler<Uuid>,
    on_person_click: EventHandler<(Uuid, f64, f64)>,
    on_empty_slot: EventHandler<(Uuid, bool)>,
    theme: &'static PedigreeTheme,
) -> Element {
    let i18n = use_i18n();
    let (nodes, prefix) = match side {
        CardSide::Ascending => (&layout.asc_nodes, "an"),
        CardSide::Descending => (&layout.desc_nodes, "dn"),
    };
    render_pedigree_card(
        &nodes[index],
        index,
        prefix,
        root_person_id,
        selected_person_id,
        on_person_navigate,
        on_person_click,
        on_empty_slot,
        true,
        i18n,
        theme,
        None,
    )
}

/// The cards and connectors of a laid-out pedigree.
///
/// A component of its own so that it redraws only when the layout does. The
/// chart around it re-renders for its toolbar, depth popover, event panel and
/// selection; with the cards drawn inline, each of those rebuilt and diffed
/// every card — around a hundred milliseconds at a thousand of them.
#[component]
fn PedigreeCanvas(
    layout: SharedLayout,
    root_person_id: Uuid,
    selected_person_id: Signal<Uuid>,
    on_person_navigate: EventHandler<Uuid>,
    on_person_click: EventHandler<(Uuid, f64, f64)>,
    on_empty_slot: EventHandler<(Uuid, bool)>,
    on_add_spouse_slot: EventHandler<Uuid>,
    theme: &'static PedigreeTheme,
    transform: Signal<ViewportTransform>,
    viewport: Signal<ViewportRect>,
    animating: Signal<bool>,
) -> Element {
    // A ruled line is drawn as a band with a lighter core, the way an
    // engraver lays one down; a Bézier one stays a single hairline.
    let double_ruled = theme.link_style == crate::components::pedigree_theme::LinkStyle::Ruled;
    // Adapt the descending side's empty "+" slot (missing spouse) onto the
    // dedicated add-spouse callback — the `bool` (father/mother) from
    // `on_empty_slot` doesn't apply here, only the person needing a spouse.
    let desc_empty_slot_adapter =
        use_callback(move |(pid, _): (Uuid, bool)| on_add_spouse_slot.call(pid));

    let region = use_culled_region(transform, viewport, animating);
    let extents_cache =
        use_hook(|| Rc::new(RefCell::new(None::<(SharedLayout, Rc<SceneExtents>)>)));
    let extents = {
        let mut cache = extents_cache.borrow_mut();
        match &*cache {
            Some((cached, extents)) if *cached == layout => extents.clone(),
            _ => {
                let extents = Rc::new(SceneExtents::of(&layout, theme));
                *cache = Some((layout.clone(), extents.clone()));
                extents
            }
        }
    };

    rsx! {
        div {
            class: "pedigree-tree",
            style: "position: relative; width: {layout.total_w}px; height: {layout.total_h}px;",

            svg {
                "viewBox": "0 0 {layout.total_w} {layout.total_h}",
                width: "{layout.total_w}",
                height: "{layout.total_h}",
                style: "display: block; overflow: visible;",

                g { transform: "translate({layout.main_tx},{layout.main_ty})",

                // ── Ascending tree ──
                g {
                    {connector_paths(
                        layout.asc_links.iter().enumerate().filter(|(si, _)| in_region(&region, extents.asc_links[*si].as_ref())),
                        "a",
                        double_ruled,
                    )}
                    for ni in (0..layout.asc_nodes.len()).filter(|ni| region.intersects(&extents.asc_cards[*ni])) {
                        PedigreeCard {
                            key: "an-{ni}",
                            layout: layout.clone(),
                            side: CardSide::Ascending,
                            index: ni,
                            root_person_id,
                            selected_person_id,
                            on_person_navigate,
                            on_person_click,
                            on_empty_slot: on_empty_slot,
                            theme,
                        }
                    }
                }

                // ── Descending tree ──
                g {
                    transform: "translate({layout.desc_tx},{layout.desc_ty})",
                    {connector_paths(
                        layout.desc_links.iter().enumerate().filter(|(si, _)| in_region(&region, extents.desc_links[*si].as_ref())),
                        "d",
                        double_ruled,
                    )}
                    for ni in (0..layout.desc_nodes.len()).filter(|ni| region.intersects(&extents.desc_cards[*ni])) {
                        PedigreeCard {
                            key: "dn-{ni}",
                            layout: layout.clone(),
                            side: CardSide::Descending,
                            index: ni,
                            root_person_id,
                            selected_person_id,
                            on_person_navigate,
                            on_person_click,
                            on_empty_slot: desc_empty_slot_adapter,
                            theme,
                        }
                    }
                }
                }
            }
        }
    }
}

/// Re-fit the graph when the window is actually resized.
///
/// WebKitGTK also fires resize on remapping. Check dimensions to preserve the
/// reader's pan and zoom when only window focus changes.
const RESIZE_FIT_JS: &str = r#"
    if (!window.__oxidgenePedigreeResizeFit) {
        window.__oxidgenePedigreeResizeFit = {
            timer: null,
            w: window.innerWidth,
            h: window.innerHeight,
            handler: function () {
                const state = window.__oxidgenePedigreeResizeFit;
                clearTimeout(state.timer);
                state.timer = setTimeout(function () {
                    if (window.innerWidth === state.w && window.innerHeight === state.h) {
                        return;
                    }
                    state.w = window.innerWidth;
                    state.h = window.innerHeight;
                    document.querySelector('.pedigree-resize-fit-trigger')?.click();
                }, 120);
            }
        };
        window.addEventListener('resize', window.__oxidgenePedigreeResizeFit.handler);
    }
"#;

/// Whether `now` differs from what `previous` holds, recording it if so.
fn changed<T: PartialEq + 'static>(mut previous: Signal<T>, now: T) -> bool {
    if *previous.peek() == now {
        return false;
    }
    previous.set(now);
    true
}

/// Back to scale 1, keeping the pan, before a refit.
fn reset_scale(mut transform: Signal<ViewportTransform>) {
    let current = *transform.peek();
    transform.set(ViewportTransform {
        scale: 1.0,
        ..current
    });
}

/// The levels actually drawn.
///
/// The requested depth runs ahead of the data: raising it fetches a deeper
/// pedigree, and until that arrives there is nothing more to draw. Laying the
/// old data out at the new depth drew placeholder slots for a generation
/// about to load, redrew every card to do it, and held the request back
/// behind that render.
fn drawn_levels(data: &PedigreeData, ancestors: usize, descendants: usize) -> (usize, usize) {
    (
        data.ancestor_depth_loaded
            .map_or(ancestors, |loaded| ancestors.min(loaded)),
        data.descendant_depth_loaded
            .map_or(descendants, |loaded| descendants.min(loaded)),
    )
}

/// The earliest couple of `person` with a known spouse, which the couple view
/// opens on; none for a person without one.
fn earliest_couple(data: &PedigreeData, person: Uuid) -> Option<Uuid> {
    let mut couples: Vec<(Uuid, Option<NaiveDate>)> = data
        .families_as_spouse
        .get(&person)
        .into_iter()
        .flatten()
        .filter(|fid| {
            data.spouses_by_family
                .get(fid)
                .is_some_and(|spouses| spouses.iter().any(|s| s.person_id != person))
        })
        .map(|fid| {
            let events = data.events_by_family.get(fid).into_iter().flatten();
            (*fid, union_sort_date(events))
        })
        .collect();
    sort_unions_chronologically(&mut couples, |couple| couple.1);
    couples.first().map(|couple| couple.0)
}

#[component]
pub fn PedigreeChart(props: PedigreeChartProps) -> Element {
    let view_cache = use_view_state_cache();
    let tid_parsed = props.tree_id.parse::<Uuid>().ok();
    let saved = tid_parsed.and_then(|t| view_cache.get_untracked(t));
    let defaults = use_pedigree_defaults().unwrap_or_default();
    // The saved view of this tree, else the default depths at scale 1.
    let (init_anc, init_desc, init_transform) = saved.as_ref().map_or(
        (
            defaults.ancestor_levels,
            defaults.descendant_levels,
            ViewportTransform {
                x: 0.0,
                y: 0.0,
                scale: 1.0,
            },
        ),
        |s| {
            (
                s.ancestor_levels,
                s.descendant_levels,
                ViewportTransform {
                    x: s.offset_x,
                    y: s.offset_y,
                    scale: s.scale,
                },
            )
        },
    );
    let ancestor_levels = use_signal(move || init_anc);
    let descendant_levels = use_signal(move || init_desc);
    let viewport_transform = use_signal(move || init_transform);
    // Viewport's own page position, cached from each fit measurement so
    // wheel-zoom can convert mouse coordinates without an async round trip
    // on every tick (see `controls::wheel_zoom`).
    let viewport_rect = use_signal(ViewportRect::assumed);

    // ── Selected person (drives event panel) ──
    let mut selected_person_id = use_signal(|| props.root_person_id);
    use_track_current_person(tid_parsed, Some(selected_person_id()));
    use_chart_portraits(props.portraits.as_ref());

    // ── Event panel collapse (persisted via localStorage) ──
    let last_viewport_width = use_signal(|| VIEWPORT_DEFAULT_W);
    let panel_collapsed = use_signal(|| false);
    let panel_ready = event_panel::use_restored_event_panel(panel_collapsed, last_viewport_width);

    use_effect(|| {
        document::eval(DRAG_IS_NOT_A_CLICK_JS);
    });
    use_effect(|| {
        document::eval(RESIZE_FIT_JS);
    });

    // ── Disable transition when root changes (avoid flying animation) ──
    let mut animating = use_signal(|| false);
    // Where the lineage view's list of the root's children is open, if it is.
    let mut family_menu = use_signal(|| None::<(f64, f64)>);
    // ── Fit the graph in the viewport on first load and root/depth changes ──
    // Also fit when explicitly requested via center_gen > 0 (e.g. navigation
    // from search results), even when there is saved pan/zoom state.
    let mut needs_fit = use_signal(|| true);
    let saver = controls::ViewSaver {
        cache: view_cache,
        tree_id: tid_parsed,
        root: props.root_person_id,
        transform: viewport_transform,
        ancestor_levels,
        descendant_levels,
    };

    // ── Reset pan/zoom/selection when the root person changes, or when the
    // parent increments center_gen to force re-centering ──
    let prev_root = use_signal(|| props.root_person_id);
    let root_changed = changed(prev_root, props.root_person_id);
    if root_changed {
        selected_person_id.set(props.root_person_id);
    }
    let prev_center_gen = use_signal(|| props.center_gen);
    if changed(prev_center_gen, props.center_gen) || root_changed {
        animating.set(false);
        reset_scale(viewport_transform);
        needs_fit.set(true);
    }

    // ── Fetch as soon as the requested depth changes ──
    //
    // Recording the new depth is what makes the page ask for the deeper (or
    // shallower) pedigree; it no longer waits for a re-fit, which only runs
    // once there is something new to fit.
    let requested = (ancestor_levels(), descendant_levels());
    let prev_requested = use_signal(|| requested);
    if changed(prev_requested, requested) {
        spawn(async move { saver.save() });
    }

    // ── Force re-centering when the drawn depth, the theme or the view
    // changes ──
    let (anc_now, desc_now) = drawn_levels(&props.data, requested.0, requested.1);
    let preferred = crate::prefs::use_pedigree_theme();
    let theme = props.theme.unwrap_or_else(|| preferred.theme());
    let view = crate::prefs::use_pedigree_view();
    let shape_now = ((anc_now, desc_now), *theme, view);
    let prev_shape = use_signal(|| shape_now);
    if changed(prev_shape, shape_now) {
        animating.set(false);
        needs_fit.set(true);
    }

    // ── Compute layout, only when something it depends on changes ──
    let layout_cache = use_hook(|| Rc::new(RefCell::new(None::<(LayoutKey, ChartScene)>)));
    let scene = cached_scene(
        &layout_cache,
        &props,
        SceneShape {
            view,
            ancestor_levels: anc_now,
            descendant_levels: desc_now,
            theme,
        },
    );
    let max_zoom = scene.max_zoom();
    let fit_target = scene.fit_target();

    // ── Fit graph in viewport when needed ──
    if needs_fit() && panel_ready() {
        needs_fit.set(false);
        spawn(saver.fit(viewport_rect, fit_target, animating));
    }

    rsx! {
        div { class: "pedigree-outer",
            TreeIconSidebar {
                active_view: TreeSidebarView::Pedigree,
                selected_person_id: Some(selected_person_id()),
                couple_family_id: earliest_couple(&props.data, selected_person_id()),
                on_couple_view: props.on_couple_view,
                on_profile_view: move |pid: Option<Uuid>| {
                    if let Some(pid) = pid {
                        props.on_profile_view.call(pid);
                    }
                },
                on_pedigree_view: move |_| {},
                on_add_person: props.on_add_person,
                on_settings: props.on_settings,
                on_dictionary: props.on_dictionary,
                controls::PedigreeTools { view, saver, viewport_rect, max_zoom, fit_target }
            }

            button {
                class: "pedigree-resize-fit-trigger",
                tabindex: "-1",
                onclick: move |_| {
                    spawn(controls::refit_after_resize(last_viewport_width, panel_collapsed, needs_fit));
                },
            }

            controls::PanZoomViewport {
                class: "pedigree-viewport {theme.viewport_class}",
                saver,
                viewport_rect,
                animating,
                max_zoom,
                PedigreeTransform {
                    transform: viewport_transform,
                    animating,
                    PedigreeScene {
                        match scene {
                            ChartScene::Tree(layout) => rsx! {
                                PedigreeCanvas {
                                    layout,
                                    root_person_id: props.root_person_id,
                                    selected_person_id,
                                    on_person_navigate: props.on_person_navigate,
                                    on_person_click: props.on_person_click,
                                    on_empty_slot: props.on_empty_slot,
                                    on_add_spouse_slot: props.on_add_spouse_slot,
                                    theme,
                                    transform: viewport_transform,
                                    viewport: viewport_rect,
                                    animating,
                                }
                            },
                            ChartScene::Circular(layout) => rsx! {
                                circular::CircularCanvas {
                                    layout,
                                    selected_person_id,
                                    on_person_navigate: props.on_person_navigate,
                                    on_person_click: props.on_person_click,
                                    on_empty_slot: props.on_empty_slot,
                                    theme,
                                    transform: viewport_transform,
                                    viewport: viewport_rect,
                                    animating,
                                }
                            },
                            ChartScene::Lineage(layout) => rsx! {
                                lineage::LineageCanvas {
                                    layout,
                                    root_person_id: props.root_person_id,
                                    selected_person_id,
                                    on_person_navigate: props.on_person_navigate,
                                    on_person_click: props.on_person_click,
                                    on_empty_slot: props.on_empty_slot,
                                    on_family_menu: move |at| family_menu.set(Some(at)),
                                    theme,
                                    transform: viewport_transform,
                                    viewport: viewport_rect,
                                    animating,
                                }
                            },
                        }
                    }
                }
            }

            // The lineage view's list of the root's spouses and children,
            // outside the transformed canvas so it stays where it was opened.
            if let Some((x, y)) = family_menu() {
                lineage::LineageFamilyMenu {
                    data: props.data.clone(),
                    root_person_id: props.root_person_id,
                    x,
                    y,
                    on_pick: move |person: Uuid| {
                        family_menu.set(None);
                        selected_person_id.set(person);
                        props.on_person_navigate.call(person);
                    },
                    on_close: move |_| family_menu.set(None),
                }
            }

            event_panel::EventPanel {
                data: props.data.clone(),
                selected: selected_person_id(),
                tree_id: props.tree_id.clone(),
                collapsed: panel_collapsed,
                needs_fit,
            }
        }
    }
}

#[cfg(test)]
mod mini_pedigree_tests {
    use super::*;

    /// The profile's mini-pedigree height, as its stylesheet sets it.
    const MINI_PEDIGREE_VIEWPORT_H: f64 = 280.0;

    fn layout_around_root(content_w: f64, content_h: f64) -> PedigreeLayout {
        PedigreeLayout {
            root_cx: content_w / 2.0,
            root_cy: content_h / 2.0,
            content_cx: content_w / 2.0,
            content_w,
            content_h,
            ..PedigreeLayout::default()
        }
    }

    #[test]
    fn a_narrower_viewport_shrinks_the_fragment_and_keeps_the_root_centred() {
        let theme = &PedigreeTheme::CLASSIC;
        let layout = layout_around_root(theme.metrics.card_w * 4.0, theme.metrics.card_h * 2.0);
        let wide = mini_pedigree_transform(1200.0, 280.0, 0.8, &layout, false, theme);
        let narrow = mini_pedigree_transform(300.0, 280.0, 0.8, &layout, false, theme);

        assert!(narrow.scale < wide.scale);
        for (width, fitted) in [(1200.0, wide), (300.0, narrow)] {
            let root_x = fitted.x + layout.root_cx * fitted.scale;
            assert!((root_x - width / 2.0).abs() < 1e-9, "{width}");
            assert!(layout.content_w * fitted.scale <= width, "{width}");
        }
    }

    #[test]
    fn bottom_anchored_root_keeps_the_same_margin_for_every_theme() {
        for (name, theme) in [
            ("classic", &PedigreeTheme::CLASSIC),
            ("medieval", &PedigreeTheme::MEDIEVAL),
        ] {
            let target_y =
                mini_pedigree_root_target_y(MINI_PEDIGREE_VIEWPORT_H, MINI_PEDIGREE_SCALE, theme);
            let card_bottom = target_y + theme.metrics.card_h * MINI_PEDIGREE_SCALE / 2.0;
            let margin = MINI_PEDIGREE_VIEWPORT_H - card_bottom;

            assert_eq!(margin, MINI_PEDIGREE_BOTTOM_MARGIN, "{name}");
        }
    }

    #[test]
    fn three_ancestor_rows_fit_at_the_largest_available_scale() {
        for (context, viewport_height, preferred_scale) in [
            ("profile", MINI_PEDIGREE_VIEWPORT_H, MINI_PEDIGREE_SCALE),
            ("grid", 210.0, 0.5),
        ] {
            for (theme_name, theme) in [
                ("classic", &PedigreeTheme::CLASSIC),
                ("medieval", &PedigreeTheme::MEDIEVAL),
            ] {
                let content_w = theme.metrics.card_w * 4.0;
                let content_h = theme.metrics.card_h * 3.0;
                let content_cx = content_w / 2.0;
                let scale = mini_pedigree_fit_scale(
                    1200.0,
                    viewport_height,
                    preferred_scale,
                    content_cx,
                    content_w,
                    content_h,
                    content_cx,
                );
                let available_height = viewport_height - 2.0 * MINI_PEDIGREE_BOTTOM_MARGIN;

                assert!(
                    content_h * scale <= available_height + f64::EPSILON,
                    "{context} {theme_name}"
                );
                assert!(scale <= preferred_scale, "{context} {theme_name}");
            }
        }
    }
}

#[cfg(test)]
mod culling_tests {
    use super::*;

    fn area(x0: f64, y0: f64, x1: f64, y1: f64) -> Area {
        Area { x0, y0, x1, y1 }
    }

    fn viewport(width: f64, height: f64) -> ViewportRect {
        ViewportRect {
            page_x: 10.0,
            page_y: 10.0,
            left: 0.0,
            width,
            height,
        }
    }

    #[test]
    fn a_connector_is_bounded_by_every_point_it_names() {
        assert_eq!(
            path_extent("M10,20 L10,40 30,40 S50,40 50,60 C1.5,-2 3,4 90,5"),
            Some(area(1.5, -2.0, 90.0, 60.0))
        );
        assert_eq!(path_extent("M5,5 L5,5"), Some(area(5.0, 5.0, 5.0, 5.0)));
        assert_eq!(path_extent(""), None);
    }

    #[test]
    fn a_connector_that_cannot_be_read_is_always_drawn() {
        assert!(in_region(&area(0.0, 0.0, 1.0, 1.0), None));
    }

    #[test]
    fn the_visible_area_undoes_the_pan_and_zoom() {
        let visible = visible_area(
            ViewportTransform {
                x: -200.0,
                y: 100.0,
                scale: 0.5,
            },
            viewport(1000.0, 600.0),
        );
        assert_eq!(visible, area(400.0, -200.0, 2400.0, 1000.0));
    }

    #[test]
    fn before_the_viewport_is_measured_a_whole_screen_is_assumed() {
        let visible = visible_area(
            ViewportTransform {
                x: 0.0,
                y: 0.0,
                scale: 1.0,
            },
            ViewportRect::assumed(),
        );
        assert_eq!(
            visible,
            area(0.0, 0.0, UNMEASURED_VIEWPORT.0, UNMEASURED_VIEWPORT.1)
        );
    }

    /// The promise that makes culling invisible: whatever the gesture, the
    /// drawn region covers the view — with most of a screen to spare — after
    /// every step, even steps of a third of a screen at a time.
    #[test]
    fn panning_and_zooming_never_reach_undrawn_content() {
        let vp = viewport(1400.0, 800.0);
        let mut transform = ViewportTransform {
            x: 0.0,
            y: 0.0,
            scale: 1.0,
        };
        let mut state: Option<Culling> = None;
        let mut redraws = 0;
        // A deterministic walk: drags of up to a third of the screen, wheel
        // zooms in and out, across the zoom range.
        let mut seed: u64 = 0x5eed;
        for _ in 0..5_000 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let r = (seed >> 33) as f64 / f64::from(u32::MAX >> 1);
            match seed % 3 {
                0 => transform.x += (r - 0.5) * 2.0 * vp.width / 3.0,
                1 => transform.y += (r - 0.5) * 2.0 * vp.height / 3.0,
                _ => {
                    let factor = if r < 0.5 {
                        ZOOM_FACTOR
                    } else {
                        1.0 / ZOOM_FACTOR
                    };
                    if let Some(scale) = zoom_step(transform.scale, factor, ZOOM_MAX) {
                        transform.scale = scale;
                    }
                }
            }
            let visible = visible_area(transform, vp);
            let next = cull(state, visible, false);
            if state.map(|s| s.region) != Some(next.region) {
                redraws += 1;
            }
            assert!(next.region.contains(&visible.grown(0.75)));
            state = Some(next);
        }
        assert!(redraws < 5_000, "{redraws} redraws for 5000 steps");
    }

    /// A drag moves a few pixels per pointer event; the cards are redrawn
    /// once per quarter screen, not on every one of them.
    #[test]
    fn a_drag_redraws_once_per_quarter_screen() {
        let vp = viewport(1400.0, 800.0);
        let mut transform = ViewportTransform {
            x: 0.0,
            y: 0.0,
            scale: 1.0,
        };
        let mut state: Option<Culling> = None;
        let mut redraws = 0;
        // Ten screens to the left, 20 px at a time.
        for _ in 0..700 {
            transform.x -= 20.0;
            let next = cull(state, visible_area(transform, vp), false);
            if state.map(|s| s.region) != Some(next.region) {
                redraws += 1;
            }
            state = Some(next);
        }
        // 14 000 px in steps of 20: one redraw per 360 px, plus the first draw.
        assert!((38..=41).contains(&redraws), "{redraws}");
    }

    /// A transition shows every view between its two ends; each of them has
    /// to be drawn before it is shown.
    #[test]
    fn an_animated_jump_keeps_both_ends_and_everything_between() {
        let vp = viewport(1400.0, 800.0);
        let from = ViewportTransform {
            x: -5_000.0,
            y: -300.0,
            scale: 1.6,
        };
        let to = ViewportTransform {
            x: 200.0,
            y: 50.0,
            scale: 0.3,
        };
        let start = cull(None, visible_area(from, vp), false);
        let end = cull(Some(start), visible_area(to, vp), true);
        for step in 0..=20 {
            let k = f64::from(step) / 20.0;
            let frame = ViewportTransform {
                x: from.x + (to.x - from.x) * k,
                y: from.y + (to.y - from.y) * k,
                scale: from.scale + (to.scale - from.scale) * k,
            };
            let shown = visible_area(frame, vp);
            // Allow for floating-point noise at the ends.
            assert!(end.region.padded(1e-6).contains(&shown), "frame {step}");
        }
    }
}

#[cfg(test)]
mod zoom_tests {
    use super::*;

    fn fit_rect() -> ViewportRect {
        ViewportRect {
            page_x: 0.0,
            page_y: 0.0,
            left: 0.0,
            width: 1600.0,
            height: 900.0,
        }
    }

    #[test]
    fn a_graph_that_fits_is_framed_whole() {
        let target = FitTarget {
            content_cx: 500.0,
            content_cy: 300.0,
            content_w: 1000.0,
            content_h: 600.0,
            root_cx: 900.0,
            root_cy: 550.0,
            root_at_left: false,
        };
        let fit = fit_transform(fit_rect(), target);
        let (cx, cy) = fit_rect().center();
        assert!((fit.x + target.content_cx * fit.scale - cx).abs() < 1e-9);
        assert!((fit.y + target.content_cy * fit.scale - cy).abs() < 1e-9);
    }

    /// A deep pedigree cannot be framed whole at the smallest scale; the
    /// person it is centred on — the one just searched for — must then be in
    /// the middle, not wherever the middle of the graph happens to fall.
    #[test]
    fn a_graph_too_wide_to_fit_is_centred_on_its_root() {
        let target = FitTarget {
            content_cx: 40_000.0,
            content_cy: 700.0,
            content_w: 80_000.0,
            content_h: 1_400.0,
            root_cx: 61_234.0,
            root_cy: 1_100.0,
            root_at_left: false,
        };
        let fit = fit_transform(fit_rect(), target);
        assert_eq!(fit.scale, ZOOM_MIN);
        let (cx, cy) = fit_rect().center();
        assert!((fit.x + target.root_cx * fit.scale - cx).abs() < 1e-9);
        assert!((fit.y + target.root_cy * fit.scale - cy).abs() < 1e-9);
    }

    /// A lineage too tall to frame whole keeps its root centred vertically,
    /// but its left edge — where the root is — starts at the left margin
    /// when it is wider than the screen, and is centred when it is not.
    #[test]
    fn a_graph_rooted_at_its_left_edge_starts_at_the_left_margin() {
        let rect = fit_rect();
        let wide = FitTarget {
            content_cx: 3_000.0,
            content_cy: 20_000.0,
            content_w: 6_000.0,
            content_h: 40_000.0,
            root_cx: 100.0,
            root_cy: 20_000.0,
            root_at_left: true,
        };
        let fit = fit_transform(rect, wide);
        assert_eq!(fit.scale, ZOOM_MIN);
        let margin = rect.width * FIT_SIDE_PADDING_RATIO;
        assert!(
            (fit.x - rect.left - margin).abs() < 1e-9,
            "left edge at the margin"
        );
        let (cx, cy) = rect.center();
        assert!((fit.y + wide.root_cy * fit.scale - cy).abs() < 1e-9);
        let narrow = FitTarget {
            content_w: 3_000.0,
            content_cx: 1_500.0,
            ..wide
        };
        let fit = fit_transform(rect, narrow);
        assert!(
            (fit.x + narrow.content_cx * fit.scale - cx).abs() < 1e-9,
            "centred across"
        );
    }

    /// A zoom holds one point still — that is the whole of what it promises.
    ///
    /// The wheel anchors to the cursor, the buttons to the middle of the
    /// viewport. Neither was checked, and the buttons quietly anchored to
    /// nothing at all: they set the scale and left the pan alone, so the CSS
    /// transform scaled about the content's own origin and the graph crept
    /// toward a corner one click at a time.
    #[test]
    fn a_zoom_leaves_the_content_under_its_anchor_where_it_was() {
        let cases = [
            // (anchor, offset, old scale, new scale)
            (400.0, -120.0, 1.0, ZOOM_FACTOR),
            (400.0, -120.0, 1.0, 1.0 / ZOOM_FACTOR),
            (0.0, 37.5, 0.8, 1.6),
            (1280.0, -940.0, 1.75, 0.35),
        ];
        for (anchor, offset, old_scale, new_scale) in cases {
            let moved = offset_holding(anchor, offset, old_scale, new_scale);
            let before = (anchor - offset) / old_scale;
            let after = (anchor - moved) / new_scale;
            assert!(
                (before - after).abs() < 1e-9,
                "anchor {anchor} at {old_scale}x->{new_scale}x: the content under it \
                 moved from {before} to {after}"
            );
        }
    }

    /// The buttons anchor where a fit centres, so the two agree.
    ///
    /// The free area is the viewport minus the events panel, which is why the
    /// centre is measured from `left`/`width` rather than from the element.
    #[test]
    fn the_button_anchor_is_the_middle_of_the_free_area() {
        let rect = ViewportRect {
            page_x: 46.0,
            page_y: 64.0,
            left: 0.0,
            width: 900.0,
            height: 600.0,
        };
        assert_eq!(rect.center(), (450.0, 300.0));

        // A fit puts the graph's centre on that point; zooming from there must
        // leave it there, which is what a viewer means by "zoom in".
        let content_cx = 512.0;
        let (center_x, _) = rect.center();
        let offset = center_x - content_cx * 1.0;
        let zoomed = offset_holding(center_x, offset, 1.0, ZOOM_FACTOR);
        assert!(
            (center_x - (content_cx * ZOOM_FACTOR + zoomed)).abs() < 1e-9,
            "the graph's centre left the middle of the viewport"
        );
    }

    /// Nothing happens at the ends of the range, so the pan is left alone too.
    #[test]
    fn a_zoom_at_its_limit_is_not_a_zoom() {
        assert_eq!(zoom_step(ZOOM_MAX, ZOOM_FACTOR, ZOOM_MAX), None);
        assert_eq!(zoom_step(ZOOM_MIN, 1.0 / ZOOM_FACTOR, ZOOM_MAX), None);
        assert_eq!(zoom_step(1.0, ZOOM_FACTOR, ZOOM_MAX), Some(ZOOM_FACTOR));
        assert_eq!(
            zoom_step(ZOOM_MAX / ZOOM_FACTOR * 1.5, ZOOM_FACTOR, ZOOM_MAX),
            Some(ZOOM_MAX),
            "a step past the top must land on the limit, not overshoot it"
        );
    }
}

#[cfg(test)]
mod silhouette_tests {
    use super::*;

    /// The two platforms draw the same picture: the web inlines the embedded
    /// data URL, the desktop serves these bytes. If the decode ever stopped
    /// matching, a card would render differently depending on where it ran.
    #[test]
    fn the_served_silhouette_is_the_embedded_png() {
        for sex in [Sex::Male, Sex::Female, Sex::Unknown] {
            let bytes = silhouette_png(sex);

            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{sex:?} is a PNG");
            assert!(
                default_portrait(sex).starts_with("data:image/png;base64,"),
                "{sex:?} is inlined as the same PNG"
            );
        }
    }
}

#[cfg(test)]
mod lifespan_tests {
    use super::*;

    /// The column and type size a classic full card gives its lifespan —
    /// what these cases have always been measured against.
    const CLASSIC_FULL_COLUMN: f32 = PedigreeTheme::CLASSIC.card.text_max_width_full;
    const CLASSIC_DATE_PX: f32 = PedigreeTheme::CLASSIC.card.date_font_px;

    fn y(year: i32, qualifier: DateQualifier) -> Option<QualifiedYear> {
        Some(QualifiedYear::new(year, qualifier))
    }

    /// The card for the person this feature was modelled on: born about 1849,
    /// died before 5 August 1917. Geneanet draws `ca 1849-< 1917`; so do we.
    #[test]
    fn a_card_hedges_each_year_independently() {
        assert_eq!(
            format_lifespan(
                y(1849, DateQualifier::About),
                y(1917, DateQualifier::Before)
            ),
            "ca 1849-< 1917"
        );
    }

    /// A missing year keeps its dash — the card still says "and then nothing
    /// is known", which is different from saying nothing at all.
    #[test]
    fn a_half_known_life_keeps_its_dash() {
        assert_eq!(
            format_lifespan(y(1907, DateQualifier::Before), None),
            "< 1907-"
        );
        assert_eq!(
            format_lifespan(y(1912, DateQualifier::After), None),
            "> 1912-"
        );
        assert_eq!(
            format_lifespan(None, y(1940, DateQualifier::Exact)),
            "-1940"
        );
        assert_eq!(format_lifespan(None, None), "");
    }

    /// Exact dates are the common case and must stay exactly as they were
    /// before qualifiers existed — no stray space, no mark.
    #[test]
    fn exact_years_are_unchanged() {
        assert_eq!(
            format_lifespan(y(1879, DateQualifier::Exact), y(1940, DateQualifier::Exact)),
            "1879-1940"
        );
    }

    fn range(from: i32, to: i32, qualifier: DateQualifier) -> Option<QualifiedYear> {
        Some(QualifiedYear {
            year: from,
            qualifier,
            year2: Some(to),
        })
    }

    /// A range is a fact the card can carry: "between 1691 and 1693" says more
    /// than either year alone, so both are drawn when there is room.
    #[test]
    fn a_range_shows_both_of_its_years() {
        assert_eq!(
            format_lifespan(None, range(1691, 1693, DateQualifier::Between)),
            "-1691..1693"
        );
        assert_eq!(
            format_lifespan(None, range(1691, 1693, DateQualifier::Or)),
            "-1691|1693"
        );
    }

    /// Two ranges measure 105.8px, past even the full card's 105px column, so
    /// the pair degrades to the marks rather than being squeezed unreadably.
    /// One range fits everywhere.
    #[test]
    fn a_lifespan_degrades_when_the_range_will_not_fit() {
        let both = (
            range(1691, 1693, DateQualifier::Between),
            range(1745, 1750, DateQualifier::Between),
        );
        assert_eq!(
            fit_lifespan(both.0, both.1, CLASSIC_FULL_COLUMN, CLASSIC_DATE_PX),
            ".. 1691-.. 1745",
            "two ranges do not fit the full card and lose their far ends"
        );

        // A single range is 49.8px — comfortable on both card widths.
        let one = range(1691, 1693, DateQualifier::Between);
        assert_eq!(
            fit_lifespan(None, one, CLASSIC_FULL_COLUMN, CLASSIC_DATE_PX),
            "-1691..1693"
        );
        assert_eq!(
            fit_lifespan(
                None,
                one,
                text_max_width(true, &PedigreeTheme::CLASSIC),
                CLASSIC_DATE_PX
            ),
            "-1691..1693"
        );
    }

    /// The narrow form still says *that* it is a range, so a reader is never
    /// told a hedged date is exact — only that the card ran out of room.
    #[test]
    fn the_narrow_form_keeps_the_mark() {
        let y = range(1691, 1693, DateQualifier::Between).unwrap();
        assert_eq!(y.wide(), "1691..1693");
        assert_eq!(y.narrow(), ".. 1691");
    }

    /// The tooltip is injected as markup, and the marks it sits beside are
    /// `<` and `>`. Unescaped, `< 1917` would be swallowed as a bogus tag and
    /// the card would silently lose its death year.
    #[test]
    fn the_marks_survive_being_written_as_markup() {
        assert_eq!(escape_xml("ca 1849-< 1917"), "ca 1849-&lt; 1917");
        assert_eq!(escape_xml("> 1912-"), "&gt; 1912-");
        assert_eq!(escape_xml("1879-1940"), "1879-1940");
    }

    /// The tooltip exists to explain the marks, so it stays empty when there
    /// is nothing to explain rather than restating the card.
    #[test]
    fn the_tooltip_is_silent_on_exact_dates() {
        let i18n = I18n(crate::i18n::Language::En);
        assert_eq!(
            lifespan_tooltip(
                &i18n,
                y(1879, DateQualifier::Exact),
                y(1940, DateQualifier::Exact)
            ),
            ""
        );
        assert_eq!(
            lifespan_tooltip(
                &i18n,
                y(1849, DateQualifier::About),
                y(1917, DateQualifier::Before)
            ),
            "About 1849 \u{2013} Before 1917"
        );
    }

    /// A range reads as one phrase in the tooltip, which is where the far end
    /// the card had to drop comes back.
    #[test]
    fn the_tooltip_spells_out_a_range() {
        let i18n = I18n(crate::i18n::Language::En);
        assert_eq!(
            lifespan_tooltip(&i18n, None, range(1691, 1693, DateQualifier::Between)),
            "Between 1691 and 1693"
        );
        assert_eq!(
            lifespan_tooltip(&i18n, None, range(1691, 1693, DateQualifier::Or)),
            "1691 or 1693"
        );
    }
}

#[cfg(test)]
mod layout_overlap_tests {
    use super::*;

    /// These tests assert against the geometry the chart actually ships, so
    /// they read the classic metrics rather than numbers of their own.
    const METRICS: PedigreeMetrics = PedigreeMetrics::CLASSIC;
    const CARD_W: f64 = METRICS.card_w;

    fn person(depth: i32, sex: Sex, parent2: Option<usize>) -> TreeNode {
        let mut n = TreeNode::new_real(
            Uuid::now_v7(),
            depth,
            sex,
            "Given".to_string(),
            "Surname".to_string(),
            None,
            None,
            SosaBadge::None,
            false,
            if sex == Sex::Female { 1 } else { 0 },
            false,
            false,
        );
        n.parent2 = parent2;
        n
    }

    /// Pushes `node` onto `arena` and returns its index.
    fn push(arena: &mut Vec<TreeNode>, node: TreeNode) -> usize {
        arena.push(node);
        arena.len() - 1
    }

    /// Attaches `spouse_idx` as a sibling (spouse) of `node_idx`.
    fn marry(arena: &mut [TreeNode], node_idx: usize, spouse_idx: usize) {
        arena[node_idx].siblings.push(spouse_idx);
        arena[spouse_idx].is_sibling = true;
    }

    /// Reproduces a cross-cousin overlap: depth-1 siblings A (with two
    /// depth-2 children, the second one childless-but-married) and B (with
    /// a single depth-2 child) must not have their depth-2 rows collide.
    #[test]
    fn cousin_branches_do_not_overlap_at_depth_two() {
        let mut arena: Vec<TreeNode> = Vec::new();

        // Root couple.
        let root = push(&mut arena, person(0, Sex::Male, None));
        let root_spouse = push(&mut arena, person(0, Sex::Female, None));
        marry(&mut arena, root, root_spouse);

        // Depth-1 children: a single childless sibling, then A (male) and B
        // (female) adjacent, then another single childless sibling —
        // mirrors the real family shape (Sibling 1, [Branch A + spouse],
        // [Branch B + spouse], Sibling 4).
        let sib_before = push(&mut arena, person(1, Sex::Female, Some(root_spouse)));
        let a = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        let b = push(&mut arena, person(1, Sex::Female, Some(root_spouse)));
        let sib_after1 = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        arena[root].children = vec![sib_before, a, b, sib_after1];

        let a_spouse = push(&mut arena, person(1, Sex::Female, None));
        marry(&mut arena, a, a_spouse);

        let b_spouse = push(&mut arena, person(1, Sex::Male, None));
        marry(&mut arena, b, b_spouse);

        // A's children: first child has a spouse with no children of their
        // own (the trailing, "childless spouse" case); second is the last
        // child in A's branch.
        let a_child1 = push(&mut arena, person(2, Sex::Male, Some(a_spouse)));
        let a_child1_spouse = push(&mut arena, person(2, Sex::Female, None));
        marry(&mut arena, a_child1, a_child1_spouse);

        let a_child2 = push(&mut arena, person(2, Sex::Male, Some(a_spouse)));
        let a_child2_spouse = push(&mut arena, person(2, Sex::Female, None));
        marry(&mut arena, a_child2, a_child2_spouse);
        arena[a].children = vec![a_child1, a_child2];

        // B's single child (no spouse) — the cousin branch directly to the
        // right of A's branch.
        let b_child = push(&mut arena, person(2, Sex::Female, Some(b_spouse)));
        arena[b].children = vec![b_child];

        layout_tree(&mut arena, 0, &METRICS);

        // Every pair of cards at the same depth must not horizontally
        // overlap (allowing exact edge-touch).
        let by_depth =
            |d: i32| -> Vec<f64> { arena.iter().filter(|n| n.depth == d).map(|n| n.x).collect() };

        eprintln!(
            "root={root} root_spouse={root_spouse} sib_before={sib_before} a={a} b={b} \
             sib_after1={sib_after1} a_spouse={a_spouse} \
             b_spouse={b_spouse} a_child1={a_child1} a_child1_spouse={a_child1_spouse} \
             a_child2={a_child2} a_child2_spouse={a_child2_spouse} b_child={b_child}"
        );
        for (i, n) in arena.iter().enumerate() {
            eprintln!(
                "idx={i} depth={} x={} after={} sex={:?}",
                n.depth, n.x, n.after, n.sex
            );
        }

        for depth in [0, 1, 2] {
            let mut xs = by_depth(depth);
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for w in xs.windows(2) {
                let gap = w[1] - w[0];
                assert!(
                    gap + 1e-6 >= CARD_W,
                    "depth {depth}: cards overlap (gap={gap}, need >= {CARD_W})"
                );
            }
        }
    }

    /// Reproduces a case one level shallower than the cousin-branch test: a
    /// leaf sibling (Branch A) who has both a married-in spouse AND his own
    /// children, sitting next to a childless full sibling (Branch B). The
    /// spouse's extra width must still push the childless sibling (and
    /// everything after it) over, even though Branch A's own subtree has
    /// descendants of its own.
    #[test]
    fn leaf_with_spouse_and_children_does_not_overlap_childless_sibling() {
        let mut arena: Vec<TreeNode> = Vec::new();

        let root = push(&mut arena, person(0, Sex::Male, None));
        let root_spouse = push(&mut arena, person(0, Sex::Female, None));
        marry(&mut arena, root, root_spouse);

        let branch_a = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        let branch_b = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        let branch_c = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        arena[root].children = vec![branch_a, branch_b, branch_c];

        let branch_a_spouse = push(&mut arena, person(1, Sex::Female, None));
        marry(&mut arena, branch_a, branch_a_spouse);

        let child_a = push(&mut arena, person(2, Sex::Female, Some(branch_a_spouse)));
        let child_b = push(&mut arena, person(2, Sex::Male, Some(branch_a_spouse)));
        arena[branch_a].children = vec![child_a, child_b];

        layout_tree(&mut arena, 0, &METRICS);

        let by_depth =
            |d: i32| -> Vec<f64> { arena.iter().filter(|n| n.depth == d).map(|n| n.x).collect() };

        for depth in [1, 2] {
            let mut xs = by_depth(depth);
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for w in xs.windows(2) {
                let gap = w[1] - w[0];
                assert!(
                    gap + 1e-6 >= CARD_W,
                    "depth {depth}: cards overlap (gap={gap}, need >= {CARD_W})"
                );
            }
        }
    }

    /// Reproduces the exact real-world family shape that still overlapped
    /// after the first fix: two depth-1 full siblings (both children of the
    /// root couple). The first branch has two childless children before a
    /// third child who has both a spouse and two children of his own. The
    /// second branch immediately follows with two childless children. The
    /// married-in spouse card from the first branch must not collide with the
    /// first child card from the second branch.
    #[test]
    fn cross_uncle_branch_with_grandchildren_does_not_overlap() {
        let mut arena: Vec<TreeNode> = Vec::new();

        let root = push(&mut arena, person(0, Sex::Male, None));
        let root_spouse = push(&mut arena, person(0, Sex::Female, None));
        marry(&mut arena, root, root_spouse);

        let branch_a = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        let branch_b = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        arena[root].children = vec![branch_a, branch_b];

        let branch_a_spouse = push(&mut arena, person(1, Sex::Female, None));
        marry(&mut arena, branch_a, branch_a_spouse);

        let branch_b_spouse = push(&mut arena, person(1, Sex::Female, None));
        marry(&mut arena, branch_b, branch_b_spouse);

        let child_a = push(&mut arena, person(2, Sex::Male, Some(branch_a_spouse)));
        let child_b = push(&mut arena, person(2, Sex::Male, Some(branch_a_spouse)));
        let child_c = push(&mut arena, person(2, Sex::Male, Some(branch_a_spouse)));
        arena[branch_a].children = vec![child_a, child_b, child_c];

        // Child A also has his own spouse + 3 children. Omitting these in an
        // earlier version of this test
        // masked the real bug.
        let child_a_spouse = push(&mut arena, person(2, Sex::Female, None));
        marry(&mut arena, child_a, child_a_spouse);
        let grandchild_a = push(&mut arena, person(3, Sex::Male, Some(child_a_spouse)));
        let grandchild_b = push(&mut arena, person(3, Sex::Female, Some(child_a_spouse)));
        let grandchild_c = push(&mut arena, person(3, Sex::Female, Some(child_a_spouse)));
        arena[child_a].children = vec![grandchild_a, grandchild_b, grandchild_c];

        // Child B also has his own spouse + 1 child.
        let child_b_spouse = push(&mut arena, person(2, Sex::Female, None));
        marry(&mut arena, child_b, child_b_spouse);
        let grandchild_d = push(&mut arena, person(3, Sex::Male, Some(child_b_spouse)));
        arena[child_b].children = vec![grandchild_d];

        let child_c_spouse = push(&mut arena, person(2, Sex::Female, None));
        marry(&mut arena, child_c, child_c_spouse);

        let grandchild_e = push(&mut arena, person(3, Sex::Female, Some(child_c_spouse)));
        let grandchild_f = push(&mut arena, person(3, Sex::Male, Some(child_c_spouse)));
        arena[child_c].children = vec![grandchild_e, grandchild_f];

        let child_d = push(&mut arena, person(2, Sex::Female, Some(branch_b_spouse)));
        let child_e = push(&mut arena, person(2, Sex::Male, Some(branch_b_spouse)));
        arena[branch_b].children = vec![child_d, child_e];

        layout_tree(&mut arena, 0, &METRICS);

        let by_depth =
            |d: i32| -> Vec<f64> { arena.iter().filter(|n| n.depth == d).map(|n| n.x).collect() };

        for depth in [1, 2, 3] {
            let mut xs = by_depth(depth);
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for w in xs.windows(2) {
                let gap = w[1] - w[0];
                assert!(
                    gap + 1e-6 >= CARD_W,
                    "depth {depth}: cards overlap (gap={gap}, need >= {CARD_W})"
                );
            }
        }
    }

    /// Reproduces a real family shape: the FIRST child of the root couple
    /// (`branch_a`) is married with 6 children of his own (some of those in
    /// turn married), followed by a childless second child (`branch_b`) and
    /// a female third child (`branch_c`, `after == 1`) married-in.
    /// `branch_a`'s own spouse card must stay exactly one card-width from
    /// him regardless of how wide his own descendant subtree grows or how
    /// the later `after == 1` sibling is positioned.
    #[test]
    fn first_child_spouse_gap_stays_one_card_width_with_wide_subtree() {
        let mut arena: Vec<TreeNode> = Vec::new();

        let root = push(&mut arena, person(0, Sex::Male, None));
        let root_spouse = push(&mut arena, person(0, Sex::Female, None));
        marry(&mut arena, root, root_spouse);

        let branch_a = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        let branch_b = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        let branch_c = push(&mut arena, person(1, Sex::Female, Some(root_spouse)));
        arena[root].children = vec![branch_a, branch_b, branch_c];

        let branch_a_spouse = push(&mut arena, person(1, Sex::Female, None));
        marry(&mut arena, branch_a, branch_a_spouse);

        let branch_b_spouse = push(&mut arena, person(1, Sex::Female, None));
        marry(&mut arena, branch_b, branch_b_spouse);

        let branch_c_spouse = push(&mut arena, person(1, Sex::Male, None));
        marry(&mut arena, branch_c, branch_c_spouse);

        // branch_a's 6 children, two of them married in turn.
        let child_1 = push(&mut arena, person(2, Sex::Male, Some(branch_a_spouse)));
        let child_2 = push(&mut arena, person(2, Sex::Female, Some(branch_a_spouse)));
        let child_3 = push(&mut arena, person(2, Sex::Male, Some(branch_a_spouse)));
        let child_4 = push(&mut arena, person(2, Sex::Female, Some(branch_a_spouse)));
        let child_5 = push(&mut arena, person(2, Sex::Male, Some(branch_a_spouse)));
        let child_6 = push(&mut arena, person(2, Sex::Female, Some(branch_a_spouse)));
        arena[branch_a].children = vec![child_1, child_2, child_3, child_4, child_5, child_6];

        let child_2_spouse = push(&mut arena, person(2, Sex::Male, None));
        marry(&mut arena, child_2, child_2_spouse);
        let child_3_spouse = push(&mut arena, person(2, Sex::Female, None));
        marry(&mut arena, child_3, child_3_spouse);
        // child_6 (last, female) has an unknown ("+") spouse — an empty
        // placeholder card rather than a real person.
        let child_6_spouse = push(&mut arena, TreeNode::new_empty(2, None, false));
        marry(&mut arena, child_6, child_6_spouse);

        layout_tree(&mut arena, 0, &METRICS);

        let gap = arena[branch_a_spouse].x - arena[branch_a].x;
        assert!(
            (gap - CARD_W).abs() < 1e-6,
            "branch_a spouse gap drifted: gap={gap}, expected exactly {CARD_W}"
        );
    }

    /// Builds: root couple → 3 children — `branch_a` (childless), `branch_b`
    /// (a couple of modest size, 2 children), and optionally `branch_c` (a
    /// couple with a much wider subtree: 5 children, several married in
    /// turn). Returns the gap between `branch_a` and `branch_b`.
    fn branch_a_to_branch_b_gap(with_wide_third_branch: bool) -> f64 {
        let mut arena: Vec<TreeNode> = Vec::new();

        let root = push(&mut arena, person(0, Sex::Male, None));
        let root_spouse = push(&mut arena, person(0, Sex::Female, None));
        marry(&mut arena, root, root_spouse);

        let branch_a = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        let branch_b = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        let mut root_children = vec![branch_a, branch_b];

        let branch_b_spouse = push(&mut arena, person(1, Sex::Female, None));
        marry(&mut arena, branch_b, branch_b_spouse);
        let bc_1 = push(&mut arena, person(2, Sex::Male, Some(branch_b_spouse)));
        let bc_2 = push(&mut arena, person(2, Sex::Female, Some(branch_b_spouse)));
        arena[branch_b].children = vec![bc_1, bc_2];

        if with_wide_third_branch {
            let branch_c = push(&mut arena, person(1, Sex::Female, Some(root_spouse)));
            root_children.push(branch_c);
            let branch_c_spouse = push(&mut arena, person(1, Sex::Male, None));
            marry(&mut arena, branch_c, branch_c_spouse);

            let cc_1 = push(&mut arena, person(2, Sex::Male, Some(branch_c_spouse)));
            let cc_2 = push(&mut arena, person(2, Sex::Female, Some(branch_c_spouse)));
            let cc_3 = push(&mut arena, person(2, Sex::Male, Some(branch_c_spouse)));
            let cc_4 = push(&mut arena, person(2, Sex::Female, Some(branch_c_spouse)));
            let cc_5 = push(&mut arena, person(2, Sex::Male, Some(branch_c_spouse)));
            arena[branch_c].children = vec![cc_1, cc_2, cc_3, cc_4, cc_5];

            let cc_2_spouse = push(&mut arena, person(2, Sex::Male, None));
            marry(&mut arena, cc_2, cc_2_spouse);
            let cc_4_spouse = push(&mut arena, person(2, Sex::Male, None));
            marry(&mut arena, cc_4, cc_4_spouse);
        }

        arena[root].children = root_children;

        layout_tree(&mut arena, 0, &METRICS);

        arena[branch_b].x - arena[branch_a].x
    }

    /// Reproduces the real-world bug: `apportion`/`tree_move`/`tree_shift`
    /// (a direct port of the classic Buchheim linear-time RT algorithm)
    /// deliberately distributes the extra width a wide subtree needs
    /// *backward* across its earlier siblings. That's intentional for the
    /// algorithm's "no kinks" guarantee on deep contour conflicts, but for
    /// direct siblings it means a sibling with a small/no subtree gets
    /// dragged away from its immediate neighbor just because a LATER
    /// sibling's subtree is wide — even though neither of their own
    /// subtrees needed that room. The gap between `branch_a` and `branch_b`
    /// must stay (approximately) the same whether or not a much wider third
    /// sibling exists further down the row.
    #[test]
    fn wide_later_sibling_does_not_inflate_earlier_sibling_gap() {
        let baseline_gap = branch_a_to_branch_b_gap(false);
        let gap_with_wide_sibling = branch_a_to_branch_b_gap(true);

        assert!(
            (gap_with_wide_sibling - baseline_gap).abs() < 1e-6,
            "adding a wide third sibling changed the branch_a-branch_b gap: \
             baseline={baseline_gap}, with_wide_sibling={gap_with_wide_sibling}"
        );
    }

    /// `fix_spouse_group_overlaps` shifts a child subtree rightward to kill
    /// an overlap, but if it doesn't also re-center the parent couple over
    /// the now-shifted children row, the parent drifts off-center relative
    /// to its own children — no cards overlap, but the row no longer lines
    /// up under its parent the way the Reingold-Tilford pass originally
    /// placed it. Same family shape as `cross_uncle_branch_with_grandchildren_does_not_overlap`,
    /// which is known to trigger a shift.
    #[test]
    fn parent_recenters_over_children_after_overlap_shift() {
        let mut arena: Vec<TreeNode> = Vec::new();

        let root = push(&mut arena, person(0, Sex::Male, None));
        let root_spouse = push(&mut arena, person(0, Sex::Female, None));
        marry(&mut arena, root, root_spouse);

        let branch_a = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        let branch_b = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        arena[root].children = vec![branch_a, branch_b];

        let branch_a_spouse = push(&mut arena, person(1, Sex::Female, None));
        marry(&mut arena, branch_a, branch_a_spouse);

        let branch_b_spouse = push(&mut arena, person(1, Sex::Female, None));
        marry(&mut arena, branch_b, branch_b_spouse);

        let child_a = push(&mut arena, person(2, Sex::Male, Some(branch_a_spouse)));
        let child_b = push(&mut arena, person(2, Sex::Male, Some(branch_a_spouse)));
        let child_c = push(&mut arena, person(2, Sex::Male, Some(branch_a_spouse)));
        arena[branch_a].children = vec![child_a, child_b, child_c];

        let child_a_spouse = push(&mut arena, person(2, Sex::Female, None));
        marry(&mut arena, child_a, child_a_spouse);
        let grandchild_a = push(&mut arena, person(3, Sex::Male, Some(child_a_spouse)));
        let grandchild_b = push(&mut arena, person(3, Sex::Female, Some(child_a_spouse)));
        let grandchild_c = push(&mut arena, person(3, Sex::Female, Some(child_a_spouse)));
        arena[child_a].children = vec![grandchild_a, grandchild_b, grandchild_c];

        let child_b_spouse = push(&mut arena, person(2, Sex::Female, None));
        marry(&mut arena, child_b, child_b_spouse);
        let grandchild_d = push(&mut arena, person(3, Sex::Male, Some(child_b_spouse)));
        arena[child_b].children = vec![grandchild_d];

        let child_c_spouse = push(&mut arena, person(2, Sex::Female, None));
        marry(&mut arena, child_c, child_c_spouse);
        let grandchild_e = push(&mut arena, person(3, Sex::Female, Some(child_c_spouse)));
        let grandchild_f = push(&mut arena, person(3, Sex::Male, Some(child_c_spouse)));
        arena[child_c].children = vec![grandchild_e, grandchild_f];

        let child_d = push(&mut arena, person(2, Sex::Female, Some(branch_b_spouse)));
        let child_e = push(&mut arena, person(2, Sex::Male, Some(branch_b_spouse)));
        arena[branch_b].children = vec![child_d, child_e];

        layout_tree(&mut arena, 0, &METRICS);

        // `branch_a`'s COUPLE (him + his spouse card) should sit centered
        // over the TRUE bounding box of all its children's subtrees
        // (child_a/b/c *and* their own grandchildren rows) — not just
        // wherever it landed before its children got shifted, and not just
        // the first/last child's own row (that narrower approximation
        // misses cases where a middle child's own subtree is the widest).
        // Centering the primary card alone would leave the couple half a
        // card off-center from the row.
        let mut row_min = f64::INFINITY;
        let mut row_max = f64::NEG_INFINITY;
        for &ci in &arena[branch_a].children.clone() {
            collect_min_x(&arena, ci, &mut row_min);
            collect_max_x(&arena, ci, &mut row_max);
        }
        let expected_center = (row_min + row_max) / 2.0;
        let actual = (arena[branch_a].x + arena[branch_a_spouse].x) / 2.0;
        assert!(
            (actual - expected_center).abs() < 1e-6,
            "branch_a couple not centered over its children row: couple center={actual} expected={expected_center}"
        );
    }

    /// The unconditional recenter must center the COUPLE (primary card +
    /// spouse card) over the children row — centering the primary card
    /// alone shifts every children row half a card off the couple's visual
    /// midpoint.
    #[test]
    fn couple_group_centers_over_children_row() {
        let mut arena: Vec<TreeNode> = Vec::new();

        let root = push(&mut arena, person(0, Sex::Male, None));
        let root_spouse = push(&mut arena, person(0, Sex::Female, None));
        marry(&mut arena, root, root_spouse);

        let child_a = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        let child_b = push(&mut arena, person(1, Sex::Female, Some(root_spouse)));
        arena[root].children = vec![child_a, child_b];

        layout_tree(&mut arena, 0, &METRICS);

        let row_center = (arena[child_a].x + arena[child_b].x) / 2.0;
        let couple_center = (arena[root].x + arena[root_spouse].x) / 2.0;
        assert!(
            (couple_center - row_center).abs() < 1e-6,
            "root couple not centered over children row: couple={couple_center} row={row_center}"
        );
    }

    /// A node with SEVERAL spouse cards must have the whole card group
    /// (node + every spouse) centered over the combined children row, not
    /// the primary card alone — with two spouses trailing to one side,
    /// card-alone centering leaves the visual group more than a full card
    /// off-center.
    #[test]
    fn multi_spouse_group_centers_over_combined_children() {
        let mut arena: Vec<TreeNode> = Vec::new();

        let root = push(&mut arena, person(0, Sex::Female, None));
        let husband_1 = push(&mut arena, person(0, Sex::Male, None));
        marry(&mut arena, root, husband_1);
        let husband_2 = push(&mut arena, person(0, Sex::Male, None));
        marry(&mut arena, root, husband_2);

        let child_a = push(&mut arena, person(1, Sex::Male, Some(husband_1)));
        let child_b = push(&mut arena, person(1, Sex::Female, Some(husband_1)));
        let child_c = push(&mut arena, person(1, Sex::Male, Some(husband_2)));
        arena[root].children = vec![child_a, child_b, child_c];

        layout_tree(&mut arena, 0, &METRICS);

        let row_min = arena[child_a].x.min(arena[child_b].x).min(arena[child_c].x);
        let row_max = arena[child_a].x.max(arena[child_b].x).max(arena[child_c].x);
        let group_min = arena[root]
            .x
            .min(arena[husband_1].x)
            .min(arena[husband_2].x);
        let group_max = arena[root]
            .x
            .max(arena[husband_1].x)
            .max(arena[husband_2].x);
        let row_center = (row_min + row_max) / 2.0;
        let group_center = (group_min + group_max) / 2.0;
        assert!(
            (group_center - row_center).abs() < 1e-6,
            "multi-spouse group not centered over combined children row: \
             group={group_center} row={row_center}"
        );
    }

    /// A childless sibling only ever conflicts with its neighbor on its OWN
    /// row, so it must sit exactly one card from the neighbor couple's
    /// rightmost card — not one card past the neighbor's deepest descendant
    /// row's extent, which the old whole-bounding-box gap check enforced
    /// and which pushed childless siblings several card-widths away from a
    /// neighbor whose grandchildren row is wide.
    #[test]
    fn childless_sibling_tucks_in_next_to_wide_branch() {
        let mut arena: Vec<TreeNode> = Vec::new();

        let root = push(&mut arena, person(0, Sex::Male, None));
        let root_spouse = push(&mut arena, person(0, Sex::Female, None));
        marry(&mut arena, root, root_spouse);

        let branch_a = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        let branch_b = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        arena[root].children = vec![branch_a, branch_b];

        let branch_a_spouse = push(&mut arena, person(1, Sex::Female, None));
        marry(&mut arena, branch_a, branch_a_spouse);

        // branch_a has 4 children, each married with 2 children of their
        // own — a descendant subtree far wider than the branch_a couple
        // itself.
        let mut a_children = Vec::new();
        for _ in 0..4 {
            let c = push(&mut arena, person(2, Sex::Male, Some(branch_a_spouse)));
            let c_spouse = push(&mut arena, person(2, Sex::Female, None));
            marry(&mut arena, c, c_spouse);
            let g_1 = push(&mut arena, person(3, Sex::Male, Some(c_spouse)));
            let g_2 = push(&mut arena, person(3, Sex::Female, Some(c_spouse)));
            arena[c].children = vec![g_1, g_2];
            a_children.push(c);
        }
        arena[branch_a].children = a_children;

        layout_tree(&mut arena, 0, &METRICS);

        let right_of_couple = arena[branch_a].x.max(arena[branch_a_spouse].x);
        let gap = arena[branch_b].x - right_of_couple;
        assert!(
            (gap - CARD_W).abs() < 1e-6,
            "childless sibling pushed away from neighbor couple: gap={gap}, expected {CARD_W}"
        );
    }

    /// Ascending trees never populate a node's own `siblings` (married-in
    /// spouse) field, so `fix_spouse_group_overlaps` must stay a no-op there
    /// and leave the RT-computed compact 0.5-unit separation at the deepest
    /// (last_level) row untouched — i.e. grandparent pairs must stay packed
    /// at half the normal card spacing, not be force-spread to a full
    /// CARD_W gap.
    #[test]
    fn ascending_compact_row_keeps_half_width_separation() {
        let last_level = -2;
        let mut arena: Vec<TreeNode> = Vec::new();

        let root = push(&mut arena, person(0, Sex::Male, None));

        let father = push(&mut arena, person(-1, Sex::Male, None));
        let mother = push(&mut arena, person(-1, Sex::Female, None));
        arena[root].children = vec![father, mother];

        let father_father = push(&mut arena, person(last_level, Sex::Male, None));
        let father_mother = push(&mut arena, person(last_level, Sex::Female, None));
        arena[father].children = vec![father_father, father_mother];

        let mother_father = push(&mut arena, person(last_level, Sex::Male, None));
        let mother_mother = push(&mut arena, person(last_level, Sex::Female, None));
        arena[mother].children = vec![mother_father, mother_mother];

        layout_tree(&mut arena, last_level, &METRICS);

        let by_depth =
            |d: i32| -> Vec<f64> { arena.iter().filter(|n| n.depth == d).map(|n| n.x).collect() };

        let mut compact_xs = by_depth(last_level);
        compact_xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for w in compact_xs.windows(2) {
            let gap = w[1] - w[0];
            assert!(
                gap < CARD_W - 1e-6,
                "compact row: gap widened to a full card width (gap={gap}, expected < {CARD_W})"
            );
        }
    }
}

/// Golden reference for the pedigree's absolute geometry.
///
/// Every card position, connector path and canvas transform below is derived
/// by pure functions from the layout constants at the top of this file. Those
/// constants are about to be routed through a pedigree theme, and the classic
/// theme has to stay pixel-for-pixel identical to what ships today.
///
/// Nothing else in the suite can catch that. The existing layout tests assert
/// *relations* — cards do not overlap, a parent stays centered over its
/// children — and a geometry that was uniformly wrong would satisfy every one
/// of them. This module pins the numbers themselves.
///
/// The expected block is generated, not hand-written: set `OXIDGENE_BLESS=1`
/// to have the test print the current geometry in paste-ready form instead of
/// asserting. Regenerate it only for a change you meant to make.
#[cfg(test)]
mod geometry_golden_tests {
    use super::*;
    use crate::components::pedigree_theme::CardStyle;
    use oxidgene_core::{Calendar, NameType};

    pub(super) fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn epoch() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::UNIX_EPOCH
    }

    /// Assembles a [`PedigreeData`] without going through the projection API.
    ///
    /// Ids are sequential rather than v7 so the fixture is byte-stable: the
    /// layout reads relations in insertion order, so a random id would not
    /// move a card, but it would make the golden block unreadable.
    #[derive(Default)]
    pub(super) struct Fixture {
        persons: HashMap<Uuid, Person>,
        names: HashMap<Uuid, Vec<PersonName>>,
        spouses_by_family: HashMap<Uuid, Vec<FamilySpouse>>,
        children_by_family: HashMap<Uuid, Vec<FamilyChild>>,
        families_as_child: HashMap<Uuid, Vec<Uuid>>,
        families_as_spouse: HashMap<Uuid, Vec<Uuid>>,
        events_by_person: HashMap<Uuid, Vec<DomainEvent>>,
    }

    impl Fixture {
        pub(super) fn person(
            &mut self,
            n: u128,
            sex: Sex,
            given: &str,
            surname: &str,
        ) -> &mut Self {
            let pid = id(n);
            self.persons.insert(
                pid,
                Person {
                    id: pid,
                    tree_id: id(0),
                    sex,
                    privacy: Privacy::Default,
                    portrait_media_id: None,
                    portrait_vignette_id: None,
                    created_at: epoch(),
                    updated_at: epoch(),
                    deleted_at: None,
                },
            );
            self.names.insert(
                pid,
                vec![PersonName {
                    id: id(n + 10_000),
                    person_id: pid,
                    name_type: NameType::Birth,
                    given_names: Some(given.to_string()),
                    surname: Some(surname.to_string()),
                    surname_prefix: None,
                    prefix: None,
                    suffix: None,
                    nickname: None,
                    is_primary: true,
                    sort_order: 0,
                    created_at: epoch(),
                    updated_at: epoch(),
                }],
            );
            self
        }

        /// Gives a person a birth and/or death year, with its precision mark —
        /// the card draws `ca 1849-< 1917` from exactly this.
        pub(super) fn life(
            &mut self,
            n: u128,
            birth: Option<(&str, DateQualifier)>,
            death: Option<(&str, DateQualifier)>,
        ) -> &mut Self {
            let pid = id(n);
            let mut events = Vec::new();
            for (offset, event_type, dated) in [
                (20_000, EventType::Birth, birth),
                (30_000, EventType::Death, death),
            ] {
                if let Some((value, qualifier)) = dated {
                    events.push(DomainEvent {
                        id: id(n + offset),
                        tree_id: id(0),
                        event_type,
                        date_value: Some(value.to_string()),
                        date_sort: None,
                        date_qualifier: qualifier,
                        date_value2: None,
                        calendar: Calendar::Gregorian,
                        cause: None,
                        place_id: None,
                        person_id: Some(pid),
                        family_id: None,
                        description: None,
                        created_at: epoch(),
                        updated_at: epoch(),
                        deleted_at: None,
                    });
                }
            }
            self.events_by_person.insert(pid, events);
            self
        }

        pub(super) fn family(
            &mut self,
            fam: u128,
            spouses: &[u128],
            children: &[u128],
        ) -> &mut Self {
            let fid = id(fam);
            self.spouses_by_family.insert(
                fid,
                spouses
                    .iter()
                    .enumerate()
                    .map(|(i, &n)| FamilySpouse {
                        id: id(fam + 40_000 + i as u128),
                        family_id: fid,
                        person_id: id(n),
                        role: match self.persons.get(&id(n)).map(|p| p.sex) {
                            Some(Sex::Female) => SpouseRole::Wife,
                            Some(Sex::Male) => SpouseRole::Husband,
                            _ => SpouseRole::Partner,
                        },
                        sort_order: i as i32,
                    })
                    .collect(),
            );
            self.children_by_family.insert(
                fid,
                children
                    .iter()
                    .enumerate()
                    .map(|(i, &n)| FamilyChild {
                        id: id(fam + 50_000 + i as u128),
                        family_id: fid,
                        person_id: id(n),
                        child_type: ChildType::Biological,
                        sort_order: i as i32,
                    })
                    .collect(),
            );
            for &n in spouses {
                self.families_as_spouse.entry(id(n)).or_default().push(fid);
            }
            for &n in children {
                self.families_as_child.entry(id(n)).or_default().push(fid);
            }
            self
        }

        pub(super) fn build(self) -> PedigreeData {
            PedigreeData {
                persons: self.persons,
                names: self.names,
                spouses_by_family: self.spouses_by_family,
                children_by_family: self.children_by_family,
                families_as_child: self.families_as_child,
                families_as_spouse: self.families_as_spouse,
                events_by_person: self.events_by_person,
                events_by_family: HashMap::new(),
                places: HashMap::new(),
                self_person_id: None,
                ancestor_depth_loaded: None,
                descendant_depth_loaded: None,
            }
        }
    }

    /// The person ids the golden block refers to, so a diff in it can be read
    /// back to a card. `1` is the root the chart is centered on.
    const ROOT: u128 = 1;

    /// A pedigree wide enough to exercise every geometry path at once:
    ///
    /// - three ascending generations, so the deepest row is drawn compact
    ///   (`COMPACT_W`/`COMPACT_H`) and the ones above it are not;
    /// - a missing father and a wholly unknown couple, for the empty slots;
    /// - a sibling beside the root, which is what draws the sibling connector;
    /// - two descending generations, so both `DESC_H` and `CARD_H` rows exist;
    /// - a married child with a child of their own, for the spouse link and
    ///   the multi-child fan-out.
    fn wide_pedigree() -> PedigreeData {
        let mut f = Fixture::default();

        // Root generation.
        f.person(ROOT, Sex::Male, "Root", "Branch_A")
            .life(
                ROOT,
                Some(("ABT 1849", DateQualifier::About)),
                Some(("1917", DateQualifier::Before)),
            )
            .person(2, Sex::Female, "Sibling_1", "Branch_A");

        // Parents and grandparents.
        f.person(3, Sex::Male, "Father_1", "Branch_A")
            .life(3, Some(("1820", DateQualifier::Exact)), None)
            .person(4, Sex::Female, "Mother_1", "Branch_B")
            .person(5, Sex::Male, "Grandfather_1", "Branch_A")
            .person(6, Sex::Female, "Grandmother_1", "Branch_C")
            .person(7, Sex::Female, "Grandmother_2", "Branch_D");

        // Spouse, children and a grandchild.
        f.person(8, Sex::Female, "Spouse_1", "Branch_E")
            .person(9, Sex::Male, "Child_1", "Branch_A")
            .person(10, Sex::Female, "Child_2", "Branch_A")
            .person(11, Sex::Female, "Spouse_2", "Branch_F")
            .person(12, Sex::Male, "Grandchild_1", "Branch_A");

        // Root's parental family — the sibling makes the root a middle child.
        f.family(100, &[3, 4], &[1, 2]);
        // Father's parents: both known.
        f.family(101, &[5, 6], &[3]);
        // Mother's parents: only the mother, so the father slot stays empty.
        f.family(102, &[7], &[4]);
        // Root's own family, then the married child's.
        f.family(103, &[1, 8], &[9, 10]);
        f.family(104, &[9, 11], &[12]);

        f.build()
    }

    /// Culling draws a subset of the cards and connectors; this checks it is
    /// never too small a one. Every card's real drawing — frame, edit button
    /// and all — and every connector must lie inside the extent culling tests,
    /// so anything touching the view is in the DOM.
    /// A union's marriage year keeps its precision mark, as every year shown
    /// alone does: an approximate marriage reads "ca 1870", not "1870".
    #[test]
    fn a_marriage_year_keeps_its_precision() {
        let mut data = wide_pedigree();
        let family = id(103);
        data.events_by_family.insert(
            family,
            vec![DomainEvent {
                id: id(90_000),
                tree_id: id(0),
                event_type: EventType::Marriage,
                date_value: Some("ABT 1870".to_string()),
                date_sort: None,
                date_qualifier: DateQualifier::About,
                date_value2: None,
                calendar: Calendar::Gregorian,
                cause: None,
                place_id: None,
                person_id: None,
                family_id: Some(family),
                description: None,
                created_at: epoch(),
                updated_at: epoch(),
                deleted_at: None,
            }],
        );
        let unions = data.unions_for_person(id(ROOT), &I18n(crate::i18n::Language::En));
        let (_, _, year) = unions.iter().find(|(fid, _, _)| *fid == family).unwrap();
        assert_eq!(year, "ca 1870");
    }

    #[test]
    fn culling_extents_contain_everything_a_card_or_connector_draws() {
        let data = wide_pedigree();
        let i18n = I18n(crate::i18n::Language::En);
        for theme in [&PedigreeTheme::CLASSIC, &PedigreeTheme::MEDIEVAL] {
            let layout = compute_layout(
                id(ROOT),
                &data,
                None,
                &HashSet::new(),
                PedigreeLayoutOptions::full(3, 2),
                theme,
            );
            let extents = SceneExtents::of(&layout, theme);
            let sides = [
                (
                    &layout.asc_nodes,
                    &extents.asc_cards,
                    layout.main_tx,
                    layout.main_ty,
                ),
                (
                    &layout.desc_nodes,
                    &extents.desc_cards,
                    layout.main_tx + layout.desc_tx,
                    layout.main_ty + layout.desc_ty,
                ),
            ];
            for (nodes, cards, dx, dy) in sides {
                for (node, extent) in nodes.iter().zip(cards) {
                    let geo = card_geometry(node, theme, &i18n);
                    let x = node.x + dx;
                    let y = node.y + dy;
                    let drawn = Area {
                        x0: x,
                        y0: y,
                        x1: x + 2.0 * theme.metrics.padding + geo.rect_w,
                        y1: y + geo.fab_y + 2.0 * theme.card.edit_fab_gap,
                    };
                    assert!(extent.contains(&drawn), "{:?} outside {extent:?}", node.id);
                }
            }
            for (links, extents) in [
                (&layout.asc_links, &extents.asc_links),
                (&layout.desc_links, &extents.desc_links),
            ] {
                assert!(extents.iter().all(Option::is_some));
                assert_eq!(links.len(), extents.len());
            }
        }
    }

    #[test]
    fn mini_layout_keeps_the_root_row_focused_on_the_selected_person() {
        let data = wide_pedigree();
        let full = compute_layout(
            id(ROOT),
            &data,
            None,
            &HashSet::new(),
            PedigreeLayoutOptions::full(2, 0),
            &PedigreeTheme::CLASSIC,
        );
        let mini = compute_layout(
            id(ROOT),
            &data,
            None,
            &HashSet::new(),
            PedigreeLayoutOptions::mini(2, 0),
            &PedigreeTheme::CLASSIC,
        );

        assert!(full.asc_nodes.iter().any(|node| node.id == Some(id(2))));
        assert!(!mini.asc_nodes.iter().any(|node| node.id == Some(id(2))));
        assert_eq!(
            mini.asc_nodes
                .iter()
                .filter(|node| node.y == mini.asc_nodes[0].y)
                .count(),
            1
        );
    }

    /// Renders a layout as a stable text block: one line per card, per
    /// connector and per canvas transform. Four decimals is past what a
    /// browser can resolve and still far inside `f64`'s exactness here, so a
    /// line changes only when the geometry really did.
    fn describe_layout(layout: &PedigreeLayout) -> String {
        let mut out = String::new();
        let mut card = |side: &str, i: usize, n: &LayoutNode| {
            let who = match n.id {
                Some(pid) => format!("{}", pid.as_u128()),
                None => "-".to_string(),
            };
            out.push_str(&format!(
                "{side} card[{i}] id={who} x={:.4} y={:.4} compact={} sibling={} \
                 given={:?} surname={:?} span={:?}\n",
                n.x,
                n.y,
                n.is_compact,
                n.is_sibling,
                n.label_given,
                n.label_surname,
                format_lifespan(n.birth_year, n.death_year),
            ));
        };
        for (i, n) in layout.asc_nodes.iter().enumerate() {
            card("asc", i, n);
        }
        for (i, n) in layout.desc_nodes.iter().enumerate() {
            card("desc", i, n);
        }
        for (i, p) in layout.asc_links.iter().enumerate() {
            out.push_str(&format!("asc link[{i}] {p}\n"));
        }
        for (i, p) in layout.desc_links.iter().enumerate() {
            out.push_str(&format!("desc link[{i}] {p}\n"));
        }
        out.push_str(&format!(
            "canvas main=({:.4},{:.4}) desc=({:.4},{:.4}) total=({:.4},{:.4}) \
             content=({:.4},{:.4},{:.4},{:.4}) root=({:.4},{:.4})\n",
            layout.main_tx,
            layout.main_ty,
            layout.desc_tx,
            layout.desc_ty,
            layout.total_w,
            layout.total_h,
            layout.content_cx,
            layout.content_cy,
            layout.content_w,
            layout.content_h,
            layout.root_cx,
            layout.root_cy,
        ));
        out
    }

    /// Compares against the golden block, or prints a fresh one under
    /// `OXIDGENE_BLESS=1`.
    fn assert_golden(actual: &str, expected: &str, name: &str) {
        if std::env::var_os("OXIDGENE_BLESS").is_some() {
            println!("\n===== {name} =====\n{actual}===== end {name} =====\n");
            return;
        }
        assert_eq!(
            actual.trim(),
            expected.trim(),
            "{name}: the pedigree geometry moved. If that was the point, \
             regenerate with OXIDGENE_BLESS=1."
        );
    }

    /// Renders one card's interior as a stable line.
    fn describe_card(label: &str, geo: &CardGeometry) -> String {
        format!(
            "{label} rect=({:.4},{:.4}) line={} photo_x={:.4} text_x={:.4} \
             sosa=({:.4},{:.4}) given={:?}@{:.4} surname={:?}@{:.4} \
             date={:?}@{:.4} squeeze={:?} fab=({:.4},{:.4}) plus=({:.4},{:.4})\n",
            geo.rect_w,
            geo.rect_h,
            geo.gender_line.as_deref().unwrap_or("-"),
            geo.photo_x,
            geo.text_x,
            geo.sosa_cx,
            geo.sosa_cy,
            geo.given,
            geo.given_y,
            geo.surname,
            geo.surname_y,
            geo.date_text,
            geo.date_y,
            geo.date_squeeze,
            geo.fab_x,
            geo.fab_y,
            geo.slot_plus_x,
            geo.slot_plus_y,
        )
    }

    /// The card interior is now measured by `card_geometry` rather than
    /// inline in `rsx!`, which is what makes it checkable at all — and what
    /// a themed renderer will reuse. This pins what it measures.
    ///
    /// A card carrying whatever the case under test needs.
    ///
    /// The layout fixture cannot supply every one of them: at three ancestor
    /// levels its whole compact row is empty slots, so a compact card with a
    /// name and a lifespan — the narrowest column the chart has, and the only
    /// place a date is squeezed — has to be built here.
    fn card(is_compact: bool, given: &str, surname: &str) -> LayoutNode {
        LayoutNode {
            id: Some(id(ROOT)),
            x: 0.0,
            y: 0.0,
            sex: Sex::Male,
            label_surname: surname.to_string(),
            label_given: given.to_string(),
            birth_year: None,
            death_year: None,
            sosa_badge: SosaBadge::None,
            is_self: false,
            is_compact,
            child_of: None,
            is_father: false,
            is_sibling: false,
            has_more_relations: false,
        }
    }

    /// The cards chosen are the ones with something to say: the root carries
    /// two qualified years and a hover title, a compact card has the narrow
    /// column that truncates a name and squeezes a lifespan, and an empty
    /// slot has only its "+".
    #[test]
    fn the_classic_card_interior_is_unchanged() {
        let data = wide_pedigree();
        let layout = compute_layout(
            id(ROOT),
            &data,
            None,
            &HashSet::new(),
            PedigreeLayoutOptions::full(3, 2),
            &PedigreeTheme::CLASSIC,
        );
        let i18n = I18n(crate::i18n::Language::En);

        // A name past either column, and two ranges — the pair that does not
        // fit even a full card and degrades to its marks.
        let ranged = |from: i32, to: i32| {
            Some(QualifiedYear {
                year: from,
                qualifier: DateQualifier::Between,
                year2: Some(to),
            })
        };
        let mut long_compact = card(true, "Maximilian_Alexander", "Branch_Longname");
        long_compact.birth_year = ranged(1691, 1693);
        long_compact.death_year = ranged(1745, 1750);
        let mut long_full = card(false, "Maximilian_Alexander", "Branch_Longname");
        long_full.birth_year = ranged(1691, 1693);
        long_full.death_year = ranged(1745, 1750);

        let mut out = String::new();
        let cases: [(&str, &LayoutNode); 7] = [
            ("root", &layout.asc_nodes[0]),
            ("dated-ancestor", &layout.asc_nodes[6]),
            ("empty-compact-slot", &layout.asc_nodes[3]),
            ("empty-slot", &layout.asc_nodes[5]),
            ("compact-named", &card(true, "Given_1", "Branch_A")),
            ("compact-overflowing", &long_compact),
            ("full-overflowing", &long_full),
        ];
        for (label, node) in cases {
            out.push_str(&describe_card(
                label,
                &card_geometry(node, &PedigreeTheme::CLASSIC, &i18n),
            ));
        }

        // A theme with a narrower compact card. Two things are pinned here
        // that the classic metrics cannot reach: the squeeze branch, which
        // only fires once even the narrow lifespan overruns its column, and
        // the fact that the interior follows the metrics at all rather than
        // the constants it used to read.
        let narrow = PedigreeTheme {
            metrics: PedigreeMetrics {
                compact_inner_w: 60.0,
                ..PedigreeMetrics::CLASSIC
            },
            card: CardStyle {
                // The text column is stated, not derived, so a theme that
                // narrows its card has to narrow this too — and the squeeze
                // branch below only fires because it does.
                text_max_width_compact: 50.0,
                ..PedigreeTheme::CLASSIC.card
            },
            ..PedigreeTheme::CLASSIC
        };
        out.push_str(&describe_card(
            "narrow-theme-compact",
            &card_geometry(&long_compact, &narrow, &i18n),
        ));

        assert_golden(&out, EXPECTED_CARD_INTERIOR, "card_interior");
    }

    /// A link style is a change of shape, never of position.
    ///
    /// This is the assertion the whole seam rests on: swapping the style must
    /// leave every card exactly where the classic theme put it, and must
    /// leave no curve behind. If a style ever started moving cards, the
    /// layout and the connectors would have stopped agreeing — the failure
    /// this design exists to prevent.
    #[test]
    fn the_ruled_style_reshapes_the_links_and_moves_nothing() {
        use crate::components::pedigree_theme::LinkStyle;

        let data = wide_pedigree();
        let ruled = PedigreeTheme {
            link_style: LinkStyle::Ruled,
            ..PedigreeTheme::CLASSIC
        };
        let classic = compute_layout(
            id(ROOT),
            &data,
            None,
            &HashSet::new(),
            PedigreeLayoutOptions::full(3, 2),
            &PedigreeTheme::CLASSIC,
        );
        let layout = compute_layout(
            id(ROOT),
            &data,
            None,
            &HashSet::new(),
            PedigreeLayoutOptions::full(3, 2),
            &ruled,
        );

        assert_eq!(
            layout.asc_nodes.len(),
            classic.asc_nodes.len(),
            "the two styles disagree about how many cards there are"
        );
        for (r, c) in layout
            .asc_nodes
            .iter()
            .chain(layout.desc_nodes.iter())
            .zip(classic.asc_nodes.iter().chain(classic.desc_nodes.iter()))
        {
            assert_eq!(
                (r.x, r.y),
                (c.x, c.y),
                "a card moved when the style changed"
            );
        }
        assert_eq!(
            (layout.total_w, layout.total_h),
            (classic.total_w, classic.total_h),
            "the canvas resized when only the style changed"
        );

        // `S` is the only curve command these paths ever use.
        for path in layout.asc_links.iter().chain(layout.desc_links.iter()) {
            assert!(
                !path.contains('S'),
                "a ruled connector kept a curve: {path}"
            );
        }
        // And the classic theme must still be drawing some, or the assertion
        // above would pass for the wrong reason.
        assert!(
            classic
                .asc_links
                .iter()
                .chain(classic.desc_links.iter())
                .any(|p| p.contains('S')),
            "the classic theme drew no curve at all — the fixture stopped covering them"
        );

        let mut out = String::new();
        for (i, p) in layout.asc_links.iter().enumerate() {
            out.push_str(&format!("asc link[{i}] {p}\n"));
        }
        for (i, p) in layout.desc_links.iter().enumerate() {
            out.push_str(&format!("desc link[{i}] {p}\n"));
        }
        assert_golden(&out, EXPECTED_RULED_LINKS, "ruled_links");
    }

    /// Writes an HTML preview of every theme, for a human to look at.
    ///
    /// A theme is a visual thing, and the golden blocks above cannot say
    /// whether it is any good — only whether it changed. This builds a page
    /// from the real geometry, the real connector paths and the real
    /// stylesheet, so what it shows is what the chart draws; only the mapping
    /// from measurements to SVG elements is written here rather than by
    /// `rsx!`, which needs a running Dioxus to produce anything.
    ///
    /// Opt-in, and never part of `just check`:
    ///
    /// ```text
    /// cargo test -p oxidgene-ui --lib theme_preview -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "writes a preview page for a human to look at"]
    fn theme_preview() {
        // Colours live in the theme now, not in LAYOUT_STYLES, so the preview
        // has to carry a palette of its own or every var() resolves to
        // nothing and the page renders unstyled.
        let palette = crate::theme::builtin_theme(crate::theme::DEFAULT_THEME_ID)
            .expect("default theme")
            .css();
        let data = wide_pedigree();
        let i18n = I18n(crate::i18n::Language::En);
        let out_dir = std::env::var("OXIDGENE_PREVIEW_DIR").unwrap_or_else(|_| ".".to_string());
        let write_page = |name: &str, page: String| {
            let path = format!("{out_dir}/{name}.html");
            std::fs::write(&path, page).expect("preview written");
            println!("wrote {path}");
        };
        for (name, theme) in [
            ("classic", &PedigreeTheme::CLASSIC),
            ("medieval", &PedigreeTheme::MEDIEVAL),
        ] {
            let page = chart_preview(&data, theme, &i18n, &palette);
            write_page(&format!("pedigree-{name}"), page);
        }
        write_page("pedigree-swatches", swatches_preview(&palette));
    }

    /// A page drawing `data` in `theme`, cards and connectors.
    fn chart_preview(
        data: &PedigreeData,
        theme: &PedigreeTheme,
        i18n: &I18n,
        palette: &str,
    ) -> String {
        use crate::components::layout::LAYOUT_STYLES;
        use std::fmt::Write as _;

        let layout = compute_layout(
            id(ROOT),
            data,
            None,
            &HashSet::new(),
            PedigreeLayoutOptions::full(3, 2),
            theme,
        );
        let mut svg = String::new();
        let _ = write!(
            svg,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><g transform="translate({tx},{ty})">"#,
            w = layout.total_w,
            h = layout.total_h,
            tx = layout.main_tx,
            ty = layout.main_ty,
        );
        let ruled = theme.link_style == crate::components::pedigree_theme::LinkStyle::Ruled;
        let link = |svg: &mut String, path: &str| {
            let _ = write!(svg, r#"<path class="pedigree-connector-path" d="{path}"/>"#);
            if ruled {
                let _ = write!(svg, r#"<path class="pedigree-connector-core" d="{path}"/>"#);
            }
        };
        for path in layout.asc_links.iter() {
            link(&mut svg, path);
        }
        let _ = write!(
            svg,
            r#"<g transform="translate({},{})">"#,
            layout.desc_tx, layout.desc_ty
        );
        for path in layout.desc_links.iter() {
            link(&mut svg, path);
        }
        let _ = write!(svg, "</g>");
        for node in layout.asc_nodes.iter() {
            preview_card(&mut svg, node, (0.0, 0.0), theme, i18n);
        }
        for node in layout.desc_nodes.iter() {
            preview_card(
                &mut svg,
                node,
                (layout.desc_tx, layout.desc_ty),
                theme,
                i18n,
            );
        }
        let _ = write!(svg, "</g></svg>");
        format!(
            "<!doctype html><meta charset=\"utf-8\"><style>{palette}</style>\
             <style>{LAYOUT_STYLES}\n\
             body{{margin:0}} .preview{{position:relative;overflow:visible}}</style>\
             <div class=\"pedigree-viewport preview {}\" style=\"width:{}px;height:{}px\">{svg}</div>",
            theme.viewport_class, layout.total_w, layout.total_h,
        )
    }

    /// One card of the preview, moved by `(dx, dy)`.
    fn preview_card(
        svg: &mut String,
        node: &LayoutNode,
        (dx, dy): (f64, f64),
        theme: &PedigreeTheme,
        i18n: &I18n,
    ) {
        use std::fmt::Write as _;

        let geo = card_geometry(node, theme, i18n);
        let stroke = match theme.card.frame_stroke {
            FrameStroke::Border => "var(--pn-border)",
            FrameStroke::Gender => gender_stroke(node.sex),
        };
        let is_focus = node.id == Some(id(ROOT));
        let (bg, fill) = match node.id {
            Some(_) if is_focus => (card_bg(true, node.is_sibling), "var(--white)"),
            Some(_) => (card_bg(false, node.is_sibling), "var(--pn-text)"),
            None => ("var(--pn-bg)", "var(--pn-text)"),
        };
        let dash = if node.id.is_none() {
            ";stroke-dasharray:4,4"
        } else {
            ""
        };
        let _ = write!(
            svg,
            r#"<g class="ped-card" transform="translate({},{})">"#,
            node.x + dx,
            node.y + dy,
        );
        let pad = theme.metrics.padding;
        let radius = theme.metrics.border_radius;
        let frame_width = theme.card.frame_width;
        let _ = match &geo.frame_d {
            Some(d) => write!(
                svg,
                r#"<path class="ped-card-rect" d="{d}" style="fill:{bg};stroke:{stroke};stroke-width:{frame_width}{dash}"/>"#,
            ),
            None => write!(
                svg,
                r#"<rect class="ped-card-rect" x="{pad}" y="{pad}" rx="{radius}" ry="{radius}" width="{}" height="{}" style="fill:{bg};stroke:{stroke};stroke-width:{frame_width}{dash}"/>"#,
                geo.rect_w, geo.rect_h,
            ),
        };
        if node.id.is_some() {
            preview_card_content(svg, node, &geo, fill);
        }
        let _ = write!(svg, "</g>");
    }

    /// What a card of a known person shows inside its frame, in `fill`.
    fn preview_card_content(svg: &mut String, node: &LayoutNode, geo: &CardGeometry, fill: &str) {
        use std::fmt::Write as _;

        if let Some(d) = &geo.inner_frame_d {
            let _ = write!(
                svg,
                r#"<path class="ped-card-inner-rule" d="{d}" style="fill:none;stroke:var(--pn-border);stroke-width:1"/>"#
            );
        }
        if let Some(line) = &geo.gender_line {
            let _ = write!(
                svg,
                r#"<path d="{line}" style="stroke:{};stroke-width:{};fill:none"/>"#,
                gender_stroke(node.sex),
                geo.gender_line_width,
            );
        }
        if geo.photo_mat {
            let _ = write!(
                svg,
                r#"<rect class="ped-card-mat" x="{}" y="{}" rx="{r}" ry="{r}" width="{}" height="{}" style="fill:var(--pn-mat,var(--white))"/>"#,
                geo.photo_x,
                geo.photo_y,
                geo.photo_w,
                geo.photo_h,
                r = geo.photo_round,
            );
        }
        let lines = [
            (
                geo.given_y,
                geo.given_font_px,
                "",
                geo.body_font,
                &geo.given,
            ),
            (
                geo.surname_y,
                geo.surname_font_px,
                &*format!("font-weight:{};", geo.surname_weight),
                geo.surname_font,
                &geo.surname,
            ),
            (
                geo.date_y,
                geo.date_font_px,
                "",
                geo.body_font,
                &geo.date_text,
            ),
        ];
        for (y, font_px, weight, font, text) in lines {
            let _ = write!(
                svg,
                r#"<text x="{}" y="{y}" style="font-size:{font_px}px;{weight}font-family:{font};fill:{fill};text-anchor:{}">{}</text>"#,
                geo.text_x,
                geo.text_anchor,
                escape_xml(text),
            );
        }
    }

    /// The settings swatches, on the page that shows them, so the two
    /// stylesheets fight in the order the application loads them.
    fn swatches_preview(palette: &str) -> String {
        use crate::components::layout::LAYOUT_STYLES;

        let row: String = crate::components::pedigree_theme::PedigreeThemeId::ALL
            .into_iter()
            .map(swatch_preview)
            .collect();
        format!(
            "<!doctype html><meta charset=\"utf-8\"><style>{palette}</style>\
             <style>{LAYOUT_STYLES}</style>\
             <style>{}</style><body style=\"background:var(--bg-deep);padding:24px\">\
             <div class=\"theme-picker\" style=\"max-width:560px\">{row}</div>",
            crate::pages::app_settings::SHARED_SETTINGS_STYLES,
        )
    }

    /// One theme's swatch button: two cards and the connector between them.
    fn swatch_preview(id: crate::components::pedigree_theme::PedigreeThemeId) -> String {
        use std::fmt::Write as _;

        let theme = id.theme();
        let m = theme.metrics;
        let (rw, rh) = m.rect(false);
        let link = crate::components::pedigree_theme::link_path(
            &LinkSpec::SimpleChild {
                from: Point::new(0.0, 0.0),
                to: Point::new(m.card_w * 0.5, m.card_h),
                is_edge: true,
            },
            theme.link_style,
            &m,
        );
        let child_dx = m.card_w * 0.5;
        let vb_w = m.card_w + child_dx;
        let vb_h = m.card_h + rh + 2.0 * m.padding;
        let mut cards = String::new();
        for (x, y) in [(0.0f64, 0.0f64), (child_dx, m.card_h)] {
            let _ = write!(
                cards,
                r#"<g transform="translate({x},{y})"><rect class="ped-card-rect" x="{p}" y="{p}" rx="{r}" ry="{r}" width="{rw}" height="{rh}" style="fill:var(--pn-bg);stroke:var(--pn-border);stroke-width:{fw}"/>"#,
                p = m.padding,
                r = m.border_radius,
                fw = theme.card.frame_width,
            );
            if let CardFrame::Cartouche { inner_inset } = theme.card.frame {
                let _ = write!(
                    cards,
                    r#"<rect class="ped-card-inner-rule" x="{0}" y="{0}" width="{1}" height="{2}" style="fill:none;stroke:var(--pn-border);stroke-width:1"/>"#,
                    m.padding + inner_inset,
                    rw - 2.0 * inner_inset,
                    rh - 2.0 * inner_inset,
                );
            }
            let _ = write!(cards, "</g>");
        }
        format!(
            r#"<button class="theme-picker-option"><svg class="ped-theme-swatch {cls}" viewBox="0 0 {vb_w} {vb_h}" preserveAspectRatio="xMidYMid meet"><rect x="0" y="0" width="{vb_w}" height="{vb_h}" style="fill:var(--pn-swatch-bg,transparent)"/><path d="{link}" class="pedigree-connector-path"/>{cards}</svg><span class="theme-picker-label">{id:?}</span></button>"#,
            cls = theme.viewport_class,
        )
    }

    /// Three lines of text have to fit inside the card that holds them.
    ///
    /// Caught the medieval compact card sitting its lifespan four pixels from
    /// the frame, which is the kind of thing a golden block records happily
    /// and nobody reads. Every theme answers for it, at both card sizes, with
    /// a full name and a hedged lifespan — the tallest a card ever gets.
    #[test]
    fn every_theme_leaves_its_lifespan_inside_the_card() {
        let i18n = I18n(crate::i18n::Language::En);
        for (name, theme) in [
            ("classic", &PedigreeTheme::CLASSIC),
            ("medieval", &PedigreeTheme::MEDIEVAL),
        ] {
            for is_compact in [false, true] {
                let mut node = card(is_compact, "Given_1", "Branch_A");
                node.birth_year = Some(QualifiedYear::new(1849, DateQualifier::About));
                node.death_year = Some(QualifiedYear::new(1917, DateQualifier::Before));
                let geo = card_geometry(&node, theme, &i18n);

                assert!(
                    !geo.given.is_empty() && !geo.surname.is_empty() && !geo.date_text.is_empty(),
                    "{name} compact={is_compact}: the case stopped covering all three lines"
                );
                // The baseline sits above the descender, so the glyphs need
                // roughly their own type size of room under it.
                let needed = geo.date_y + f64::from(geo.date_font_px) * 0.3;
                assert!(
                    needed <= geo.rect_h,
                    "{name} compact={is_compact}: the lifespan baseline ({}) leaves \
                     {:.1}px under it inside a {}px card — it touches the frame",
                    geo.date_y,
                    geo.rect_h - geo.date_y,
                    geo.rect_h
                );
            }
        }
    }

    /// The first name line starts below the portrait, not across it.
    ///
    /// The portrait and the baselines are separate numbers that only meet on
    /// screen, and a theme that centres its names under the portrait has no
    /// margin for error: the medieval compact card set its first baseline 2px
    /// under the medallion, so every given name on the deepest ancestor row
    /// was drawn across the bottom of the photograph.
    #[test]
    fn every_theme_starts_its_names_below_the_portrait() {
        let i18n = I18n(crate::i18n::Language::En);
        for (name, theme) in [
            ("classic", &PedigreeTheme::CLASSIC),
            ("medieval", &PedigreeTheme::MEDIEVAL),
        ] {
            for is_compact in [false, true] {
                let node = card(is_compact, "Given_1", "Branch_A");
                let geo = card_geometry(&node, theme, &i18n);
                // A card that sets its names beside the portrait is free to
                // start them level with it; only a centred column stacks.
                if geo.text_anchor != "middle" {
                    continue;
                }
                // Capitals and ascenders reach roughly the type size above
                // the baseline, which is what has to clear the portrait.
                let ascender_top = geo.given_y - f64::from(geo.given_font_px);
                let photo_bottom = geo.photo_y + geo.photo_h;
                assert!(
                    ascender_top >= photo_bottom,
                    "{name} compact={is_compact}: the given name reaches up to \
                     {ascender_top} but the portrait ends at {photo_bottom}, so \
                     the line is drawn across it"
                );
            }
        }
    }

    /// The medieval badge sits outside the cartouche, left of and level with
    /// its bottom point. Above the crown it can land on a ruled connector.
    #[test]
    fn medieval_more_relations_badge_stays_left_of_the_bottom_point() {
        let i18n = I18n(crate::i18n::Language::En);
        let theme = &PedigreeTheme::MEDIEVAL;

        for is_compact in [false, true] {
            let geo = card_geometry(&card(is_compact, "Given_1", "Branch_A"), theme, &i18n);
            assert!(geo.more_relations_x > theme.metrics.padding);
            assert!(geo.more_relations_x < geo.photo_x);
            assert_eq!(geo.more_relations_y, theme.metrics.padding + geo.rect_h);
            assert!(geo.more_relations_x < theme.metrics.padding + geo.rect_w / 2.0);
        }
    }

    /// A card has to fit the column the layout gives it.
    ///
    /// Two numbers decide this and they live far apart: `card_w` scales the
    /// Reingold-Tilford coordinates, while `padding` and the drawn rectangle
    /// decide how much of the resulting column the card actually covers. A
    /// theme can leave both looking reasonable and still have its cards eat
    /// their neighbours — which the medieval theme did, at the deepest
    /// ancestor row, where the column is the narrowest on the chart.
    ///
    /// Checked per theme rather than per render, because the overlap it
    /// catches is a property of the numbers, not of any one pedigree.
    #[test]
    fn no_theme_lets_its_cards_eat_their_neighbours() {
        for (name, theme) in [
            ("classic", &PedigreeTheme::CLASSIC),
            ("medieval", &PedigreeTheme::MEDIEVAL),
        ] {
            let m = &theme.metrics;
            for is_compact in [false, true] {
                let drawn = m.padding + m.rect(is_compact).0;
                let column = m.column(is_compact);
                assert!(
                    drawn <= column,
                    "{name} compact={is_compact}: a card drawn from {}px to {drawn}px \
                     overruns its {column}px column by {:.1}px, so neighbours overlap",
                    m.padding,
                    drawn - column
                );
            }
        }
    }

    /// The medieval theme lays out and measures its own way.
    ///
    /// Beyond pinning it, this checks the property that made the card larger
    /// in the first place: a frame and a medallion take real room, and taking
    /// it from the text column instead would leave names truncated where the
    /// classic theme shows them whole. So its column must be no narrower.
    #[test]
    fn the_medieval_theme_has_room_for_what_the_classic_one_shows() {
        let data = wide_pedigree();
        let layout = compute_layout(
            id(ROOT),
            &data,
            None,
            &HashSet::new(),
            PedigreeLayoutOptions::full(3, 2),
            &PedigreeTheme::MEDIEVAL,
        );
        let i18n = I18n(crate::i18n::Language::En);

        for is_compact in [false, true] {
            let classic = text_max_width(is_compact, &PedigreeTheme::CLASSIC);
            let medieval = text_max_width(is_compact, &PedigreeTheme::MEDIEVAL);
            assert!(
                medieval >= classic,
                "compact={is_compact}: the medieval column ({medieval}) is narrower \
                 than the classic one ({classic}), so it truncates names the classic \
                 theme shows whole"
            );
        }

        // The drawn frame, its second rule and the medallion all have to fit
        // inside the box the layout reserved.
        let m = PedigreeTheme::MEDIEVAL.metrics;
        let style = PedigreeTheme::MEDIEVAL.card;
        let CardFrame::Cartouche { inner_inset } = style.frame else {
            panic!("the medieval theme is specified to draw a cartouche");
        };
        assert!(
            style.photo_x_full + style.photo_w < m.inner_w,
            "the medallion overruns the card"
        );
        assert!(
            style.photo_y + style.photo_h < m.inner_h + m.padding,
            "the medallion overruns the card vertically"
        );
        assert!(
            inner_inset > 0.0 && inner_inset < m.inner_h / 2.0,
            "the inner rule is not inside the frame"
        );
        assert!(
            style.photo_x_full > m.padding + inner_inset,
            "the medallion sits on top of the inner rule"
        );

        let mut out = String::new();
        for (i, n) in layout.asc_nodes.iter().enumerate() {
            out.push_str(&format!(
                "asc card[{i}] x={:.4} y={:.4} compact={}\n",
                n.x, n.y, n.is_compact
            ));
        }
        for (i, p) in layout.asc_links.iter().enumerate() {
            out.push_str(&format!("asc link[{i}] {p}\n"));
        }
        out.push_str(&format!(
            "canvas total=({:.4},{:.4}) root=({:.4},{:.4})\n",
            layout.total_w, layout.total_h, layout.root_cx, layout.root_cy
        ));
        out.push_str(&describe_card(
            "root",
            &card_geometry(&layout.asc_nodes[0], &PedigreeTheme::MEDIEVAL, &i18n),
        ));
        out.push_str(&describe_card(
            "compact-named",
            &card_geometry(
                &card(true, "Given_1", "Branch_A"),
                &PedigreeTheme::MEDIEVAL,
                &i18n,
            ),
        ));
        assert_golden(&out, EXPECTED_MEDIEVAL, "medieval");
    }

    const EXPECTED_MEDIEVAL: &str = r#"
asc card[0] x=412.2500 y=568.0000 compact=false
asc card[1] x=630.5000 y=378.0000 compact=false
asc card[2] x=727.5000 y=188.0000 compact=false
asc card[3] x=824.5000 y=0.0000 compact=true
asc card[4] x=679.0000 y=0.0000 compact=true
asc card[5] x=533.5000 y=188.0000 compact=false
asc card[6] x=194.0000 y=378.0000 compact=false
asc card[7] x=339.5000 y=188.0000 compact=false
asc card[8] x=436.5000 y=0.0000 compact=true
asc card[9] x=291.0000 y=0.0000 compact=true
asc card[10] x=48.5000 y=188.0000 compact=false
asc card[11] x=145.5000 y=0.0000 compact=true
asc card[12] x=0.0000 y=0.0000 compact=true
asc card[13] x=806.2500 y=568.0000 compact=false
asc link[0] M509.25,586 L509.25,568 291,568 291,550
asc link[1] M509.25,586 L509.25,568 727.5,568
asc link[2] M727.5,396 L727.5,378 630.5,378 630.5,360
asc link[3] M727.5,396 L727.5,378 824.5,378 824.5,360
asc link[4] M824.5,206 L824.5,188 751.75,188 751.75,170
asc link[5] M824.5,206 L824.5,188 897.25,188 897.25,170
asc link[6] M291,396 L291,378 145.5,378 145.5,360
asc link[7] M291,396 L291,378 436.5,378 436.5,360
asc link[8] M436.5,206 L436.5,188 363.75,188 363.75,170
asc link[9] M436.5,206 L436.5,188 509.25,188 509.25,170
asc link[10] M145.5,206 L145.5,188 72.75,188 72.75,170
asc link[11] M145.5,206 L145.5,188 218.25,188 218.25,170
asc link[12] M727.5,550 L727.5,568 903.25,568 903.25,586
canvas total=(1140.2500,1358.0000) root=(579.2500,733.0000)
root rect=(158.0000,162.0000) line=- photo_x=64.0000 text_x=97.0000 sosa=(120.0000,86.0000) given="Root"@110.0000 surname="BRANCH_A"@128.0000 date="ca 1849-< 1917"@146.0000 squeeze=None fab=(97.0000,196.0000) plus=(97.0000,107.0000)
compact-named rect=(108.0000,160.0000) line=- photo_x=39.0000 text_x=72.0000 sosa=(95.0000,86.0000) given="Given_1"@110.0000 surname="BRANCH_A"@128.0000 date=""@146.0000 squeeze=None fab=(72.0000,194.0000) plus=(72.0000,106.0000)
"#;

    const EXPECTED_RULED_LINKS: &str = r#"
asc link[0] M370,340 L370,326.5 185,326.5 185,313
asc link[1] M370,340 L370,326.5 555,326.5
asc link[2] M555,244 L555,230.5 462.5,230.5 462.5,217
asc link[3] M555,244 L555,230.5 647.5,230.5 647.5,217
asc link[4] M647.5,148 L647.5,134.5 602.5,134.5 602.5,121
asc link[5] M647.5,148 L647.5,134.5 695,134.5 695,121
asc link[6] M185,244 L185,230.5 92.5,230.5 92.5,217
asc link[7] M185,244 L185,230.5 277.5,230.5 277.5,217
asc link[8] M277.5,148 L277.5,134.5 232.5,134.5 232.5,121
asc link[9] M277.5,148 L277.5,134.5 325,134.5 325,121
asc link[10] M92.5,148 L92.5,134.5 47.5,134.5 47.5,121
asc link[11] M92.5,148 L92.5,134.5 140,134.5 140,121
asc link[12] M555,313 L555,326.5 755,326.5 755,340
desc link[0] M262.5,38.5 L282.5,38.5
desc link[1] M277.5,38.5 L277.5,126.5 92.5,126.5 92.5,144
desc link[2] M277.5,38.5 L277.5,126.5 462.5,126.5 462.5,144
desc link[3] M170,178.5 L190,178.5
desc link[4] M185,178.5 L185,222.5 185,222.5 185,240
"#;

    const EXPECTED_CARD_INTERIOR: &str = r#"
root rect=(175.0000,67.0000) line=M9,10 L9,60 photo_x=10.0000 text_x=70.0000 sosa=(57.5000,57.5000) given="Root"@21.0000 surname="BRANCH_A"@35.0000 date="ca 1849-< 1917"@49.0000 squeeze=None fab=(92.5000,88.0000) plus=(92.5000,46.5000)
dated-ancestor rect=(175.0000,67.0000) line=M9,10 L9,60 photo_x=10.0000 text_x=70.0000 sosa=(57.5000,57.5000) given="Father_1"@21.0000 surname="BRANCH_A"@35.0000 date="1820-"@49.0000 squeeze=None fab=(92.5000,88.0000) plus=(92.5000,46.5000)
empty-compact-slot rect=(82.0000,115.0000) line=M19,10 L19,60 photo_x=20.0000 text_x=10.0000 sosa=(67.5000,57.5000) given=""@81.0000 surname=""@81.0000 date=""@81.0000 squeeze=None fab=(46.0000,136.0000) plus=(46.0000,70.5000)
empty-slot rect=(175.0000,67.0000) line=M9,10 L9,60 photo_x=10.0000 text_x=70.0000 sosa=(57.5000,57.5000) given=""@21.0000 surname=""@21.0000 date=""@21.0000 squeeze=None fab=(92.5000,88.0000) plus=(92.5000,46.5000)
compact-named rect=(82.0000,115.0000) line=M19,10 L19,60 photo_x=20.0000 text_x=10.0000 sosa=(67.5000,57.5000) given="Given_1"@81.0000 surname="BRANCH_A"@95.0000 date=""@109.0000 squeeze=None fab=(46.0000,136.0000) plus=(46.0000,70.5000)
compact-overflowing rect=(82.0000,115.0000) line=M19,10 L19,60 photo_x=20.0000 text_x=10.0000 sosa=(67.5000,57.5000) given="Maximilian_A…"@81.0000 surname="BRANCH_LO…"@95.0000 date=".. 1691-.. 1745"@109.0000 squeeze=None fab=(46.0000,136.0000) plus=(46.0000,70.5000)
full-overflowing rect=(175.0000,67.0000) line=M9,10 L9,60 photo_x=10.0000 text_x=70.0000 sosa=(57.5000,57.5000) given="Maximilian_Alexander"@21.0000 surname="BRANCH_LONGNA…"@35.0000 date=".. 1691-.. 1745"@49.0000 squeeze=None fab=(92.5000,88.0000) plus=(92.5000,46.5000)
narrow-theme-compact rect=(60.0000,115.0000) line=M19,10 L19,60 photo_x=20.0000 text_x=10.0000 sosa=(67.5000,57.5000) given="Maximili…"@81.0000 surname="BRANCH…"@95.0000 date=".. 1691-.. 1745"@109.0000 squeeze=Some(50.0) fab=(35.0000,136.0000) plus=(35.0000,70.5000)
"#;

    #[test]
    fn the_classic_geometry_is_unchanged() {
        let data = wide_pedigree();
        let layout = compute_layout(
            id(ROOT),
            &data,
            None,
            &HashSet::new(),
            PedigreeLayoutOptions::full(3, 2),
            &PedigreeTheme::CLASSIC,
        );
        assert_golden(
            &describe_layout(&layout),
            EXPECTED_WIDE_PEDIGREE,
            "wide_pedigree",
        );
    }

    const EXPECTED_WIDE_PEDIGREE: &str = r#"
asc card[0] id=1 x=277.5000 y=336.0000 compact=false sibling=false given="Root" surname="Branch_A" span="ca 1849-< 1917"
asc card[1] id=4 x=462.5000 y=240.0000 compact=false sibling=false given="Mother_1" surname="Branch_B" span=""
asc card[2] id=7 x=555.0000 y=144.0000 compact=false sibling=false given="Grandmother_2" surname="Branch_D" span=""
asc card[3] id=- x=647.5000 y=0.0000 compact=true sibling=false given="" surname="" span=""
asc card[4] id=- x=555.0000 y=0.0000 compact=true sibling=false given="" surname="" span=""
asc card[5] id=- x=370.0000 y=144.0000 compact=false sibling=false given="" surname="" span=""
asc card[6] id=3 x=92.5000 y=240.0000 compact=false sibling=false given="Father_1" surname="Branch_A" span="1820-"
asc card[7] id=6 x=185.0000 y=144.0000 compact=false sibling=false given="Grandmother_1" surname="Branch_C" span=""
asc card[8] id=- x=277.5000 y=0.0000 compact=true sibling=false given="" surname="" span=""
asc card[9] id=- x=185.0000 y=0.0000 compact=true sibling=false given="" surname="" span=""
asc card[10] id=5 x=0.0000 y=144.0000 compact=false sibling=false given="Grandfather_1" surname="Branch_A" span=""
asc card[11] id=- x=92.5000 y=0.0000 compact=true sibling=false given="" surname="" span=""
asc card[12] id=- x=0.0000 y=0.0000 compact=true sibling=false given="" surname="" span=""
asc card[13] id=2 x=662.5000 y=336.0000 compact=false sibling=false given="Sibling_1" surname="Branch_A" span=""
desc card[0] id=1 x=92.5000 y=0.0000 compact=false sibling=false given="Root" surname="Branch_A" span="ca 1849-< 1917"
desc card[1] id=8 x=277.5000 y=0.0000 compact=false sibling=true given="Spouse_1" surname="Branch_E" span=""
desc card[2] id=10 x=370.0000 y=140.0000 compact=false sibling=false given="Child_2" surname="Branch_A" span=""
desc card[3] id=9 x=0.0000 y=140.0000 compact=false sibling=false given="Child_1" surname="Branch_A" span=""
desc card[4] id=11 x=185.0000 y=140.0000 compact=false sibling=true given="Spouse_2" surname="Branch_F" span=""
desc card[5] id=12 x=92.5000 y=236.0000 compact=false sibling=false given="Grandchild_1" surname="Branch_A" span=""
asc link[0] M370,340 L370,335 S370,326.5 362,326.5 L193,326.5 S185,326.5 185,318 L185,313
asc link[1] M370,340 L370,335 S370,326.5 378,326.5 L555,326.5
asc link[2] M555,244 L555,239 S555,230.5 547,230.5 L470.5,230.5 S462.5,230.5 462.5,222 L462.5,217
asc link[3] M555,244 L555,239 S555,230.5 563,230.5 L639.5,230.5 S647.5,230.5 647.5,222 L647.5,217
asc link[4] M647.5,148 L647.5,143 S647.5,134.5 639.5,134.5 L610.5,134.5 S602.5,134.5 602.5,126 L602.5,121
asc link[5] M647.5,148 L647.5,143 S647.5,134.5 655.5,134.5 L687,134.5 S695,134.5 695,126 L695,121
asc link[6] M185,244 L185,239 S185,230.5 177,230.5 L100.5,230.5 S92.5,230.5 92.5,222 L92.5,217
asc link[7] M185,244 L185,239 S185,230.5 193,230.5 L269.5,230.5 S277.5,230.5 277.5,222 L277.5,217
asc link[8] M277.5,148 L277.5,143 S277.5,134.5 269.5,134.5 L240.5,134.5 S232.5,134.5 232.5,126 L232.5,121
asc link[9] M277.5,148 L277.5,143 S277.5,134.5 285.5,134.5 L317,134.5 S325,134.5 325,126 L325,121
asc link[10] M92.5,148 L92.5,143 S92.5,134.5 84.5,134.5 L55.5,134.5 S47.5,134.5 47.5,126 L47.5,121
asc link[11] M92.5,148 L92.5,143 S92.5,134.5 100.5,134.5 L132,134.5 S140,134.5 140,126 L140,121
asc link[12] M555,313 L555,326.5 747,326.5 S755,326.5 755,334.5 L755,340
desc link[0] M262.5,38.5 L282.5,38.5
desc link[1] M277.5,38.5 L277.5,126.5 100.5,126.5 S92.5,126.5 92.5,134.5 L92.5,144
desc link[2] M277.5,38.5 L277.5,126.5 454.5,126.5 S462.5,126.5 462.5,134.5 L462.5,144
desc link[3] M170,178.5 L190,178.5
desc link[4] M185,178.5 L185,222.5 185,222.5 185,240
canvas main=(50.0000,50.0000) desc=(185.0000,336.0000) total=(947.5000,812.0000) content=(473.7500,406.0000,847.5000,712.0000) root=(420.0000,434.0000)
"#;
}
