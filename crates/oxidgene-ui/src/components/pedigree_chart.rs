//! Vertical bidirectional pedigree chart with pan/zoom, icon sidebar, and event panel.
//!
//! Layout: `.pedigree-outer` (flex row)
//!   -> `.isb` (icon sidebar: depth/zoom controls)
//!   -> `.pedigree-viewport` (pannable/zoomable canvas)
//!   -> `.ev-panel` (selected-person event list)
//!
//! Cards are positioned using the Reingold-Tilford (Buchheim variant) algorithm,
//! connectors are drawn via SVG overlay with Bézier curves.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use dioxus::html::geometry::WheelDelta;
use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::CroppedSource;
use crate::components::cropped_image::{CroppedImage, CroppedSvgImage};
use crate::components::date_input::format_event_date;
use crate::components::pedigree_theme::{
    CardFrame, FrameStroke, LinkSpec, PedigreeMetrics, PedigreeTheme, Point, link_path,
};
use crate::components::tree_cache::{PedigreeViewState, use_view_state_cache};
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};

use oxidgene_core::projection::{Pedigree, ProfileEvent};
use oxidgene_core::types::{
    Event as DomainEvent, FamilyChild, FamilySpouse, Person, PersonName, Place, QualifiedYear,
};
use oxidgene_core::{ChildType, DateQualifier, EventType, Privacy, Sex, SpouseRole};

use crate::i18n::{I18n, use_i18n};
use crate::prefs::use_pedigree_defaults;

use crate::utils::{escape_xml, event_type_label_key, truncate_text_to_fit};

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
const ZOOM_MAX: f64 = 2.0;
// ── Year extraction ──────────────────────────────────────────────────────

const YEAR_MIN: u32 = 1000;
const YEAR_MAX: u32 = 2099;

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

/// Extract a 4-digit year from a GEDCOM date string (e.g. "ABT 1842", "1 JAN 1900").
fn fmt_year(date: &str) -> String {
    for word in date.split_whitespace() {
        if word.len() == 4
            && word
                .parse::<u32>()
                .is_ok_and(|y| (YEAR_MIN..=YEAR_MAX).contains(&y))
        {
            return word.to_string();
        }
    }
    if date.len() > 12 {
        format!("{}...", &date[..10])
    } else {
        date.to_string()
    }
}

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
fn lifespan_tooltip(
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
    /// person_id → the picture their portrait is drawn from, built by
    /// [`ApiClient::portrait_map_for_ids`]. Absent means no portrait: the card
    /// draws the silhouette rather than asking for bytes that do not exist.
    /// A portrait that arrives as a region of a larger photograph carries that
    /// region, and the card cuts it itself.
    pub photos: HashMap<Uuid, CroppedSource>,
    /// Pre-computed SOSA ancestor set (persons who are ancestors of the SOSA root).
    pub sosa_ancestors: HashSet<Uuid>,
    /// The SOSA root person ID (from tree settings).
    pub sosa_root_id: Option<Uuid>,
    /// The person this tree identifies as the current user.
    pub self_person_id: Option<Uuid>,
}

/// A [`PedigreeData`] shared between the page that assembled it, the handlers
/// that read it and the chart that draws it.
///
/// The data behind it is large — every person, name, event and place the
/// pedigree pulled in, plus a portrait picture each — and a page reads it from
/// a dozen closures. Handing each of them an owned copy meant rebuilding all of
/// that on every render, including the renders that only opened a context menu.
///
/// Equality is identity, which is what makes it a usable prop: assembling the
/// pedigree again produces a new handle and redraws the chart, while a render
/// that changed nothing about the pedigree passes the same handle and does not.
/// Comparing the contents instead would cost as much as rebuilding them.
#[derive(Clone, Debug)]
pub struct SharedPedigree(Arc<PedigreeData>);

impl SharedPedigree {
    #[must_use]
    pub fn new(data: PedigreeData) -> Self {
        Self(Arc::new(data))
    }
}

impl PartialEq for SharedPedigree {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::ops::Deref for SharedPedigree {
    type Target = PedigreeData;

    fn deref(&self) -> &Self::Target {
        &self.0
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

impl PedigreeData {
    /// Build chart data from a [`Pedigree`] returned by the projection API.
    ///
    /// Creates synthetic domain objects (Person, PersonName, Event) from the
    /// denormalized pedigree nodes for layout and rendering.
    pub fn from_pedigree(pedigree: &Pedigree) -> Self {
        use chrono::Utc;

        let now = Utc::now();
        let tree_id = pedigree.tree_id;

        // ── Persons & Names ──
        let mut persons: HashMap<Uuid, Person> = HashMap::new();
        let mut names: HashMap<Uuid, Vec<PersonName>> = HashMap::new();

        for node in pedigree.persons.values() {
            let person = Person {
                id: node.person_id,
                tree_id,
                sex: node.sex,
                privacy: Privacy::default(),
                portrait_media_id: None,
                portrait_vignette_id: None,
                created_at: now,
                updated_at: now,
                deleted_at: None,
            };
            persons.insert(node.person_id, person);

            let name = PersonName {
                id: Uuid::nil(),
                person_id: node.person_id,
                name_type: oxidgene_core::NameType::Birth,
                given_names: node.given_names.clone(),
                // Projection surnames already carry their particle, so there
                // is nothing to re-attach here.
                surname: node.surname.clone(),
                surname_prefix: None,
                prefix: None,
                suffix: None,
                nickname: None,
                is_primary: true,
                sort_order: 0,
                created_at: now,
                updated_at: now,
            };
            names.insert(node.person_id, vec![name]);
        }

        // ── Birth/death events, carried whole by the projection ──
        let mut events_by_person: HashMap<Uuid, Vec<DomainEvent>> = HashMap::new();
        for node in pedigree.persons.values() {
            let person_events: Vec<DomainEvent> = [node.birth.as_ref(), node.death.as_ref()]
                .into_iter()
                .flatten()
                .map(|pe| profile_event_to_domain(pe, tree_id, Some(node.person_id), None, now))
                .collect();
            if !person_events.is_empty() {
                events_by_person.insert(node.person_id, person_events);
            }
        }

        // ── Family relationships from PedigreeFamily + PedigreeEdge ──
        //
        // PedigreeFamily carries full family membership (spouses + children),
        // covering childless couples that produce no PedigreeEdge.
        // We supplement with edge data for child_type info.

        // Build child_type lookup from edges.
        let mut child_type_map: HashMap<(Uuid, Uuid), ChildType> = HashMap::new();
        for edge in &pedigree.edges {
            child_type_map.insert((edge.family_id, edge.child_id), edge.edge_type);
        }

        let mut spouses_by_family: HashMap<Uuid, Vec<FamilySpouse>> = HashMap::new();
        let mut children_by_family: HashMap<Uuid, Vec<FamilyChild>> = HashMap::new();
        let mut families_as_child: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
        let mut families_as_spouse: HashMap<Uuid, Vec<Uuid>> = HashMap::new();

        for (family_id, family) in &pedigree.families {
            // Build FamilySpouse entries — assign role by sex.
            for (i, &spouse_id) in family.spouse_ids.iter().enumerate() {
                let role = match persons.get(&spouse_id).map(|p| &p.sex) {
                    Some(Sex::Male) => SpouseRole::Husband,
                    Some(Sex::Female) => SpouseRole::Wife,
                    _ => {
                        if i == 0 {
                            SpouseRole::Husband
                        } else {
                            SpouseRole::Wife
                        }
                    }
                };
                let fs = FamilySpouse {
                    id: Uuid::nil(),
                    family_id: *family_id,
                    person_id: spouse_id,
                    role,
                    sort_order: i as i32,
                };
                spouses_by_family.entry(*family_id).or_default().push(fs);
                families_as_spouse
                    .entry(spouse_id)
                    .or_default()
                    .push(*family_id);
            }

            // Build FamilyChild entries.
            for (i, &child_id) in family.children_ids.iter().enumerate() {
                let child_type = child_type_map
                    .get(&(*family_id, child_id))
                    .copied()
                    .unwrap_or(ChildType::Biological);
                let fc = FamilyChild {
                    id: Uuid::nil(),
                    family_id: *family_id,
                    person_id: child_id,
                    child_type,
                    sort_order: i as i32,
                };
                children_by_family.entry(*family_id).or_default().push(fc);
                families_as_child
                    .entry(child_id)
                    .or_default()
                    .push(*family_id);
            }
        }

        // Deduplicate families_as_spouse entries.
        for fids in families_as_spouse.values_mut() {
            fids.sort();
            fids.dedup();
        }

        // Deduplicate families_as_child entries.
        for fids in families_as_child.values_mut() {
            fids.sort();
            fids.dedup();
        }

        // ── Reconstruct family events from the pedigree payload ──
        let mut events_by_family: HashMap<Uuid, Vec<DomainEvent>> = HashMap::new();
        for (family_id, events) in &pedigree.family_events {
            let domain_events: Vec<DomainEvent> = events
                .iter()
                .map(|ce| profile_event_to_domain(ce, tree_id, None, Some(*family_id), now))
                .collect();
            events_by_family.insert(*family_id, domain_events);
        }

        // ── Synthetic events + names for family members outside the pedigree window ──
        for family in pedigree.families.values() {
            for member in &family.members {
                // Skip members already in the pedigree persons map.
                if persons.contains_key(&member.person_id) {
                    continue;
                }
                // Build synthetic person + name (for display in event panel).
                let person = Person {
                    id: member.person_id,
                    tree_id,
                    sex: member.sex,
                    privacy: Privacy::default(),
                    portrait_media_id: None,
                    portrait_vignette_id: None,
                    created_at: now,
                    updated_at: now,
                    deleted_at: None,
                };
                persons.insert(member.person_id, person);
                let name = PersonName {
                    id: Uuid::nil(),
                    person_id: member.person_id,
                    name_type: oxidgene_core::NameType::Birth,
                    given_names: member.given_names.clone(),
                    surname: member.surname.clone(),
                    surname_prefix: None,
                    prefix: None,
                    suffix: None,
                    nickname: None,
                    is_primary: true,
                    sort_order: 0,
                    created_at: now,
                    updated_at: now,
                };
                names.insert(member.person_id, vec![name]);

                // Same conversion as the pedigree nodes above.
                let member_events: Vec<DomainEvent> =
                    [member.birth.as_ref(), member.death.as_ref()]
                        .into_iter()
                        .flatten()
                        .map(|pe| {
                            profile_event_to_domain(pe, tree_id, Some(member.person_id), None, now)
                        })
                        .collect();
                if !member_events.is_empty() {
                    events_by_person.insert(member.person_id, member_events);
                }
            }
        }

        Self {
            persons,
            names,
            spouses_by_family,
            children_by_family,
            families_as_child,
            families_as_spouse,
            events_by_person,
            events_by_family,
            places: HashMap::new(),
            photos: HashMap::new(),
            sosa_ancestors: HashSet::new(),
            sosa_root_id: None,
            self_person_id: None,
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

    fn parents_of(&self, person_id: Uuid) -> (Option<Uuid>, Option<Uuid>) {
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
            .and_then(|e| e.date_value.as_deref().map(fmt_year))
    }

    /// Resolve a place_id to its name.
    fn place_name(&self, place_id: Uuid) -> Option<&str> {
        self.places.get(&place_id).map(|p| p.name.as_str())
    }

    /// Get unions for a person: Vec<(family_id, partner_name, marriage_year)>.
    pub fn unions_for_person(&self, person_id: Uuid) -> Vec<(Uuid, String, String)> {
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
                            "Unknown".to_string()
                        } else {
                            format!("{} {}", gs, ss).trim().to_string()
                        }
                    })
                    .unwrap_or_else(|| "Unknown".to_string());
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
    photo_url: Option<CroppedSource>,
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
        photo_url: Option<CroppedSource>,
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
            photo_url,
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
            photo_url: None,
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
    photo_url: Option<CroppedSource>,
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

        let photo_url = data.photos.get(&id).cloned();

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
            photo_url,
            sosa_badge,
            is_self: data.self_person_id == Some(id),
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
        root_pn.photo_url,
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
                pn.photo_url,
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
                pn.photo_url,
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
        root_pn.photo_url,
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
                        spn.photo_url,
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
                        cpn.photo_url,
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
/// is ported from): the compact deepest ancestor row packs at half a card,
/// every other row at a full one.
fn tree_separation(depth: i32, last_level: i32) -> f64 {
    if depth == last_level { 0.5 } else { 1.0 }
}

#[allow(clippy::too_many_arguments)]
fn apportion(
    wrap: &mut [WrapNode],
    arena: &[TreeNode],
    v: usize,
    w: Option<usize>,
    ancestor_in: usize,
    last_level: i32,
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

        let sep = tree_separation(arena[wrap[vim_next].orig].depth, last_level);
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

fn first_walk(wrap: &mut [WrapNode], arena: &[TreeNode], root: usize, last_level: i32) {
    // Iterative post-order via explicit stack.
    let mut post_order: Vec<usize> = Vec::new();
    let mut stack = vec![root];
    while let Some(v) = stack.pop() {
        post_order.push(v);
        for &c in &wrap[v].children.clone() {
            stack.push(c);
        }
    }
    post_order.reverse();

    for v in post_order {
        let parent = wrap[v].parent;
        let siblings_in_parent = parent.map(|p| wrap[p].children.clone()).unwrap_or_default();
        let prev_sibling = if wrap[v].i > 0 {
            siblings_in_parent.get(wrap[v].i - 1).copied()
        } else {
            None
        };

        // Determine effective children: filter by first sibling's parent2 if node has siblings.
        let node_siblings = wrap[v].siblings.clone();
        let orig_children = wrap[v].children.clone();
        let effective_children: Vec<usize> = if node_siblings.is_empty() {
            orig_children.clone()
        } else {
            // Filter children belonging to first sibling (spouse). A child
            // with no recorded second parent (`parent2 == None`) belongs to
            // an empty/unknown first-sibling placeholder — without this, such
            // children match neither branch, `effective_children` comes back
            // empty, and the centering/shift logic below is skipped entirely
            // for this node, leaving its subtree adrift.
            let first_sib = node_siblings[0];
            let first_sib_orig = wrap[first_sib].orig;
            let first_sib_is_empty = arena[first_sib_orig].id.is_none();
            orig_children
                .iter()
                .copied()
                .filter(|&ci| {
                    wrap[ci].parent2.map(|p2| wrap[p2].orig) == Some(first_sib_orig)
                        || (first_sib_is_empty && wrap[ci].parent2.is_none())
                })
                .collect()
        };

        if !effective_children.is_empty() {
            tree_shift(wrap, v);

            let mut midpoint = 0.0f64;
            if !node_siblings.is_empty() && (arena[v].after != 1 || effective_children.len() == 1) {
                midpoint -= 0.5;
            }

            // Adjustment for female-first nodes (after=1) with siblings.
            let first_child = effective_children[0];
            let last_child = *effective_children.last().unwrap();
            let mut m_adj = 0.0f64;
            if arena[wrap[first_child].orig].after == 1 {
                let fc_sibs = wrap[first_child].siblings.clone();
                if let Some(&last_fc_sib) = fc_sibs.last() {
                    m_adj += wrap[last_fc_sib].z;
                }
            }
            let last_child_orig = wrap[last_child].orig;
            if arena[last_child_orig].after == 1 {
                let lc_sibs = wrap[last_child].siblings.clone();
                if let Some(&last_lc_sib) = lc_sibs.last() {
                    m_adj += wrap[last_lc_sib].z;
                }
            }

            let last_sib_z = {
                let lc_sibs = wrap[last_child].siblings.clone();
                lc_sibs.last().map(|&s| wrap[s].z).unwrap_or(0.0)
            };
            midpoint += (wrap[first_child].z + wrap[last_child].z + last_sib_z + m_adj) / 2.0;

            // Special case for 2 children at deepest level.
            if effective_children.len() == 2 && arena[wrap[first_child].orig].depth == last_level {
                midpoint -= 0.25;
            }

            match prev_sibling {
                Some(w) => {
                    let w_sib_z = wrap[w].siblings.last().map(|&s| wrap[s].z).unwrap_or(0.0);
                    // Consecutive siblings share the same parent, so they sit
                    // exactly one separation apart.
                    let sep = tree_separation(arena[wrap[v].orig].depth, last_level);
                    wrap[v].z = wrap[w].z + w_sib_z + sep;
                    wrap[v].m = wrap[v].z - midpoint;
                }
                None => {
                    wrap[v].z = midpoint;
                }
            }
        } else if let Some(w) = prev_sibling {
            let w_sib_z = wrap[w].siblings.last().map(|&s| wrap[s].z).unwrap_or(0.0);
            let sep = tree_separation(arena[wrap[v].orig].depth, last_level);
            wrap[v].z = wrap[w].z + w_sib_z + sep;
        }

        // Multi-spouse positioning (simplified port).
        let mut last_z = 0.0f64;
        let node_siblings_clone = wrap[v].siblings.clone();
        let orig_children_clone = wrap[v].children.clone();

        if !node_siblings_clone.is_empty() && arena[v].after == 1 && effective_children.len() != 1 {
            wrap[v].m -= 0.5;
            // Port of JS `firstSibWithChild` correction:
            // if the FIRST sibling (index 0) is the parent2 of the children,
            // firstSibWithChild = 0 - 1 = -1 → node.m -= (-1) → m += 1.
            // Net result for the common case (first spouse has the children): m += 0.5.
            let mut first_sib_with_child = 0i32;
            if !orig_children_clone.is_empty() {
                let first_child_p2 = wrap[orig_children_clone[0]].parent2;
                for (index, &sib_wi) in node_siblings_clone.iter().enumerate() {
                    let sib_is_empty = arena[wrap[sib_wi].orig].id.is_none();
                    if first_child_p2 == Some(sib_wi) || (first_child_p2.is_none() && sib_is_empty)
                    {
                        first_sib_with_child = index as i32 - 1;
                    }
                }
            }
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

        // Apportion.
        let _new_ancestor = apportion(
            wrap,
            arena,
            v,
            prev_sibling,
            siblings_in_parent.first().copied().unwrap_or(v),
            last_level,
        );
    }
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
                let last_sib_z = wrap[v].siblings.last().map(|&s| wrap[s].z).unwrap_or(0.0);
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

            // Tightest depth wins: after shifting, every depth the two sides
            // share must keep a 1.0-unit gap; depths only one side occupies
            // are unconstrained.
            let mut shift = f64::NEG_INFINITY;
            for (d, cmin) in &curr_min {
                if let Some(pmax) = contour_max.get(d) {
                    shift = shift.max(1.0 - (cmin - pmax));
                }
            }
            if !shift.is_finite() {
                shift = 0.0;
            }

            if shift.abs() > 1e-9 {
                for &ci in &children[i..] {
                    shift_subtree(arena, ci, shift);
                }
                // If `curr` starts a new parent2 (half-sibling) group, shift
                // the matching spouse sibling of `node` along with it.
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
    first_walk(&mut wrap, arena, 0, last_level);

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
    photo_url: Option<CroppedSource>,
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
fn compute_layout(
    root_id: Uuid,
    data: &PedigreeData,
    sosa_root_id: Option<Uuid>,
    sosa_ancestors: &HashSet<Uuid>,
    ancestor_levels: usize,
    descendant_levels: usize,
    theme: &PedigreeTheme,
) -> PedigreeLayout {
    let metrics = &theme.metrics;
    let last_asc_level = -(ancestor_levels as i32);

    // ── Ascending tree ──
    let mut asc_arena =
        build_ascending_tree(root_id, data, ancestor_levels, sosa_root_id, sosa_ancestors);
    layout_tree(&mut asc_arena, last_asc_level, metrics);
    let mut asc_links = collect_links(&asc_arena, last_asc_level, theme);

    // ── Descending tree ──
    let mut desc_arena = build_descending_tree(
        root_id,
        data,
        descendant_levels,
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
    let mut extra_asc_nodes: Vec<LayoutNode> = Vec::new();
    {
        let all_siblings = get_siblings(root_id, data);
        if all_siblings.len() > 1 {
            let root_sib_idx = all_siblings.iter().position(|&s| s == root_id).unwrap_or(0);
            let sibs_before = &all_siblings[..root_sib_idx];
            let sibs_after = &all_siblings[root_sib_idx + 1..];

            // Extend minX/maxX based on desc root's spouses.
            let mut sib_min_x = asc_root_x;
            let mut sib_max_x = asc_root_x;
            if desc_arena[0].after == 0 && !desc_arena[0].siblings.is_empty() {
                // Male root: wife is to the right → extend maxX.
                let last_si = *desc_arena[0].siblings.last().unwrap();
                sib_max_x += desc_arena[last_si].x - desc_root_x;
            } else if !desc_arena[0].siblings.is_empty() {
                // Female root: husband is to the left → extend minX.
                let first_si = desc_arena[0].siblings[0];
                sib_min_x -= desc_root_x - desc_arena[first_si].x;
            }

            // Father: asc_arena[0].children[0], Mother: children[1] (if present).
            let father_data = asc_arena[0]
                .children
                .first()
                .map(|&ci| (asc_arena[ci].x, asc_arena[ci].y, asc_arena[ci].depth));
            let parent_idx = if asc_arena[0].children.len() > 1 {
                1
            } else {
                0
            };
            let mother_data = asc_arena[0]
                .children
                .get(parent_idx)
                .map(|&ci| (asc_arena[ci].x, asc_arena[ci].y, asc_arena[ci].depth));

            let len_before = sibs_before.len();
            let len_after = sibs_after.len();

            for (i, &sib_id) in sibs_before.iter().enumerate() {
                let sib_x = sib_min_x - metrics.sibling_spacing * (len_before - i) as f64;
                let sib_y = asc_root_y;
                let pn = PersonNode::from_data(sib_id, data, sosa_root_id, sosa_ancestors);
                extra_asc_nodes.push(LayoutNode {
                    id: Some(sib_id),
                    x: sib_x,
                    y: sib_y,
                    sex: pn.sex,
                    label_surname: pn.surname,
                    label_given: pn.given,
                    birth_year: pn.birth_year,
                    death_year: pn.death_year,
                    photo_url: pn.photo_url,
                    sosa_badge: pn.sosa_badge,
                    is_self: pn.is_self,
                    is_compact: false,
                    child_of: None,
                    is_father: false,
                    is_sibling: false,
                    has_more_relations: false,
                });
                // Link from father node (index reversed so furthest sibling is "last").
                if let Some((fx, fy, fd)) = father_data {
                    let rev_idx = len_before - i - 1;
                    let simple = sib_x >= fx;
                    asc_links.push(link_path(
                        &LinkSpec::RootSibling {
                            from: Point::new(fx, fy),
                            to: Point::new(sib_x, sib_y),
                            from_depth: fd,
                            index: rev_idx,
                            count: len_before,
                            simple,
                            last_level: last_asc_level,
                        },
                        theme.link_style,
                        metrics,
                    ));
                }
            }

            for (i, &sib_id) in sibs_after.iter().enumerate() {
                let sib_x = sib_max_x + metrics.sibling_spacing * (i + 1) as f64;
                let sib_y = asc_root_y;
                let pn = PersonNode::from_data(sib_id, data, sosa_root_id, sosa_ancestors);
                extra_asc_nodes.push(LayoutNode {
                    id: Some(sib_id),
                    x: sib_x,
                    y: sib_y,
                    sex: pn.sex,
                    label_surname: pn.surname,
                    label_given: pn.given,
                    birth_year: pn.birth_year,
                    death_year: pn.death_year,
                    photo_url: pn.photo_url,
                    sosa_badge: pn.sosa_badge,
                    is_self: pn.is_self,
                    is_compact: false,
                    child_of: None,
                    is_father: false,
                    is_sibling: false,
                    has_more_relations: false,
                });
                // Link from mother (or father if no mother).
                if let Some((px, py, pd)) = mother_data {
                    let simple = sib_x <= px;
                    asc_links.push(link_path(
                        &LinkSpec::RootSibling {
                            from: Point::new(px, py),
                            to: Point::new(sib_x, sib_y),
                            from_depth: pd,
                            index: i,
                            count: len_after,
                            simple,
                            last_level: last_asc_level,
                        },
                        theme.link_style,
                        metrics,
                    ));
                }
            }
        }
    }

    // ── Global bounding box (descending nodes shifted by desc_tx/ty) ──
    let asc_all = collect_all_nodes(&asc_arena);
    let desc_all = collect_all_nodes(&desc_arena);

    let mut gmin_x = f64::INFINITY;
    let mut gmax_x = f64::NEG_INFINITY;
    let mut gmin_y = f64::INFINITY;
    let mut gmax_y = f64::NEG_INFINITY;

    for &ni in &asc_all {
        let tn = &asc_arena[ni];
        let cw = if tn.depth == last_asc_level {
            metrics.compact_w
        } else {
            metrics.card_w
        };
        let ch = if tn.depth == last_asc_level {
            metrics.compact_h
        } else {
            metrics.card_h
        };
        gmin_x = gmin_x.min(tn.x);
        gmax_x = gmax_x.max(tn.x + cw);
        gmin_y = gmin_y.min(tn.y);
        gmax_y = gmax_y.max(tn.y + ch);
    }
    for &ni in &desc_all {
        let tn = &desc_arena[ni];
        let ch = if tn.depth > 0 {
            metrics.desc_h
        } else {
            metrics.card_h
        };
        let gx = tn.x + desc_tx;
        let gy = tn.y + desc_ty;
        gmin_x = gmin_x.min(gx);
        gmax_x = gmax_x.max(gx + metrics.card_w);
        gmin_y = gmin_y.min(gy);
        gmax_y = gmax_y.max(gy + ch);
    }
    // Include root biological siblings in bounding box.
    for node in &extra_asc_nodes {
        gmin_x = gmin_x.min(node.x);
        gmax_x = gmax_x.max(node.x + metrics.card_w);
        gmin_y = gmin_y.min(node.y);
        gmax_y = gmax_y.max(node.y + metrics.card_h);
    }

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
        photo_url: arena[ni].photo_url.clone(),
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

/// Scales and pans the canvas so the whole graph sits inside the free area.
///
/// Shared by the initial/root-change fit and by the fit-screen button, which
/// held byte-identical copies of the measurement script and the arithmetic
/// below.
async fn fit_graph_in_viewport(
    mut scale: Signal<f64>,
    mut offset_x: Signal<f64>,
    mut offset_y: Signal<f64>,
    mut viewport_page_pos: Signal<(f64, f64)>,
    // (center x, center y, width, height) of the graph content, in content
    // units — grouped into one tuple to keep the argument count clippy-clean.
    content: (f64, f64, f64, f64),
) {
    let (content_cx, content_cy, content_w, content_h) = content;
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
    viewport_page_pos.set((page_x, page_y));
    let side_padding = vw * FIT_SIDE_PADDING_RATIO;
    let fit_w = (vw - 2.0 * side_padding).max(1.0);
    let fit_scale = (fit_w / content_w)
        .min(vh / content_h)
        .clamp(ZOOM_MIN, ZOOM_MAX);
    scale.set(fit_scale);
    offset_x.set(vx + vw / 2.0 - content_cx * fit_scale);
    offset_y.set(vh / 2.0 - content_cy * fit_scale);
}

// ── Component ────────────────────────────────────────────────────────────

/// Fixed zoom level for [`MiniPedigree`] — not user-adjustable.
const MINI_PEDIGREE_SCALE: f64 = 0.8;

/// Default viewport size for [`MiniPedigree`] before the actual DOM element
/// has been measured (see `needs_center` below).
const MINI_PEDIGREE_VIEWPORT_W: f64 = 400.0;
const MINI_PEDIGREE_VIEWPORT_H: f64 = 280.0;

/// Bottom padding (viewport px) kept below the root card when it's anchored
/// near the bottom of the canvas (no descendants to show underneath it).
const MINI_PEDIGREE_BOTTOM_MARGIN: f64 = 60.0;

/// Props for [`MiniPedigree`] — a small pedigree fragment, focused and
/// centered on `root_person_id`, for embedding outside the main tree canvas
/// (e.g. on the person detail page). Panning is enabled but the zoom level
/// is fixed (see [`MINI_PEDIGREE_SCALE`]) — there is no zoom control.
#[derive(Props, Clone, PartialEq)]
pub struct MiniPedigreeProps {
    pub root_person_id: Uuid,
    pub data: SharedPedigree,
    pub ancestor_levels: usize,
    pub descendant_levels: usize,
    /// Called when the user clicks a person card (navigate to their page).
    /// Empty ancestor/descendant slots are not clickable.
    pub on_person_navigate: EventHandler<Uuid>,
    /// Fixed zoom level; defaults to [`MINI_PEDIGREE_SCALE`]. Still not
    /// user-adjustable — this only lets embedders pick a denser scale
    /// (e.g. search-result grid cells).
    #[props(default = MINI_PEDIGREE_SCALE)]
    pub scale: f64,
    /// Which theme to draw with, when the caller wants to decide rather than
    /// follow the viewer's preference — a settings preview showing each
    /// option as itself, for instance. `None` means the default theme.
    #[props(default)]
    pub theme: Option<&'static PedigreeTheme>,
}

/// A small, pannable (but not zoomable) pedigree fragment (e.g. "parents &
/// grandparents"), always centered on `root_person_id` at a fixed zoom
/// level. Reuses the same layout engine and card renderer as the full
/// interactive [`PedigreeChart`].
#[component]
pub fn MiniPedigree(props: MiniPedigreeProps) -> Element {
    let i18n = use_i18n();
    let selected_person_id = use_signal(|| props.root_person_id);
    let noop_click = EventHandler::new(|_: (Uuid, f64, f64)| {});
    let noop_empty_slot = EventHandler::new(|_: (Uuid, bool)| {});
    let scale = props.scale;
    let preferred = crate::prefs::use_pedigree_theme();
    let theme = props.theme.unwrap_or_else(|| preferred.theme());

    // ── Pan state (no zoom signal — the scale is the fixed constant above) ──
    let mut offset_x = use_signal(|| 0.0f64);
    let mut offset_y = use_signal(|| 0.0f64);
    let mut dragging = use_signal(|| false);
    let mut drag_start_x = use_signal(|| 0.0f64);
    let mut drag_start_y = use_signal(|| 0.0f64);
    let mut drag_origin_x = use_signal(|| 0.0f64);
    let mut drag_origin_y = use_signal(|| 0.0f64);

    let layout = crate::ui_observability::measure_ui("pedigree_layout", || {
        compute_layout(
            props.root_person_id,
            &props.data,
            props.data.sosa_root_id,
            &props.data.sosa_ancestors,
            props.ancestor_levels,
            props.descendant_levels,
            theme,
        )
    });

    // ── Center the root person in the viewport on first render and root change ──
    let mut prev_root = use_signal(|| props.root_person_id);
    let mut needs_center = use_signal(|| true);
    if prev_root() != props.root_person_id {
        prev_root.set(props.root_person_id);
        needs_center.set(true);
    }
    if needs_center() {
        needs_center.set(false);
        let root_cx = layout.root_cx;
        let root_cy = layout.root_cy;
        // When there are no descendants to show below the root, anchor it
        // near the bottom of the viewport instead of the vertical middle —
        // otherwise the ancestor rows above waste half the canvas.
        // (`desc_nodes` always contains at least the root card itself, even
        // at descendant_levels == 0, so check the prop directly instead.)
        let anchor_bottom = props.descendant_levels == 0;
        spawn(async move {
            // Small delay so the DOM has rendered the viewport element.
            crate::utils::sleep_ms(30).await;
            if let Ok(val) = document::eval(
                "var el = document.querySelector('.mini-pedigree'); return el ? [el.clientWidth, el.clientHeight] : [400, 280]"
            ).await {
                let vw = val.get(0).and_then(|v| v.as_f64()).unwrap_or(MINI_PEDIGREE_VIEWPORT_W);
                let vh = val.get(1).and_then(|v| v.as_f64()).unwrap_or(MINI_PEDIGREE_VIEWPORT_H);
                offset_x.set(vw / 2.0 - root_cx * scale);
                let target_y = if anchor_bottom {
                    vh - MINI_PEDIGREE_BOTTOM_MARGIN
                } else {
                    vh / 2.0
                };
                offset_y.set(target_y - root_cy * scale);
            }
        });
    }

    let transform = format!(
        "translate({}px, {}px) scale({scale})",
        offset_x(),
        offset_y(),
    );

    rsx! {
        div {
            class: "mini-pedigree {theme.viewport_class}",
            onpointerdown: move |evt| {
                let coords = evt.client_coordinates();
                drag_start_x.set(coords.x);
                drag_start_y.set(coords.y);
                drag_origin_x.set(offset_x());
                drag_origin_y.set(offset_y());
                dragging.set(true);
            },
            onpointermove: move |evt| {
                if dragging() {
                    let coords = evt.client_coordinates();
                    offset_x.set(drag_origin_x() + coords.x - drag_start_x());
                    offset_y.set(drag_origin_y() + coords.y - drag_start_y());
                }
            },
            onpointerup: move |_| dragging.set(false),
            onpointerleave: move |_| dragging.set(false),

            div {
                class: "mini-pedigree-inner",
                style: "transform: {transform};",
                svg {
                    width: "{layout.total_w}",
                    height: "{layout.total_h}",
                    "viewBox": "0 0 {layout.total_w} {layout.total_h}",
                    style: "display: block; overflow: visible;",
                    g { transform: "translate({layout.main_tx},{layout.main_ty})",
                        g {
                            for (si, path) in layout.asc_links.iter().enumerate() {
                                path { key: "al-{si}", d: "{path}", class: "pedigree-connector-path", fill: "none" }
                            }
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
                                )}
                            }
                        }
                        g {
                            transform: "translate({layout.desc_tx},{layout.desc_ty})",
                            for (si, path) in layout.desc_links.iter().enumerate() {
                                path { key: "dl-{si}", d: "{path}", class: "pedigree-connector-path", fill: "none" }
                            }
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
                                )}
                            }
                        }
                    }
                }
            }
        }
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
    pub sosa_ancestor_ids: Option<std::collections::HashSet<Uuid>>,
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
        (theme.metrics.compact_inner_w - theme.card.text_x_compact) as f32
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

    CardGeometry {
        rect_w,
        rect_h,
        frame: card.frame,
        gender_line,
        gender_line_width: card.gender_rule.map_or(0.0, |rule| rule.width),
        photo_x,
        photo_y: card.photo_y,
        photo_w: card.photo_w,
        photo_h: card.photo_h,
        photo_round: card.photo_round,
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
    mut selected_person_id: Signal<Uuid>,
    on_person_navigate: EventHandler<Uuid>,
    on_person_click: EventHandler<(Uuid, f64, f64)>,
    on_empty_slot: EventHandler<(Uuid, bool)>,
    allow_empty_click: bool,
    i18n: I18n,
    theme: &PedigreeTheme,
) -> Element {
    let geo = card_geometry(node, theme, &i18n);
    let CardGeometry {
        rect_w: rw,
        rect_h: rh,
        frame,
        gender_line: gl_path,
        gender_line_width,
        photo_x: ph_x,
        photo_y: ph_y,
        photo_w: ph_w,
        photo_h: ph_h,
        photo_round,
        text_x: tx,
        sosa_cx,
        sosa_cy,
        sosa_r,
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
        fab_x,
        fab_y,
        fab_r,
        slot_plus_x,
        slot_plus_y,
    } = geo;
    let padding = theme.metrics.padding;
    let border_radius = theme.metrics.border_radius;
    // A cartouche's second rule, measured once for every branch that draws
    // an outline — the person card, and both empty-slot forms.
    // The classic card shows sex on a short rule beside the portrait and
    // keeps a neutral outline; a cartouche is heavy enough to carry the
    // colour itself, and drops the rule.
    let frame_stroke = match theme.card.frame_stroke {
        FrameStroke::Border => "var(--pn-border)",
        FrameStroke::Gender => gender_stroke(node.sex),
    };
    let frame_width = theme.card.frame_width;
    let inner = match frame {
        CardFrame::Plain => None,
        CardFrame::Cartouche { inner_inset } => Some((
            padding + inner_inset,
            rw - 2.0 * inner_inset,
            rh - 2.0 * inner_inset,
        )),
    };
    let key = format!("{key_prefix}-{ni}");
    let nx = node.x;
    let ny = node.y;

    match node.id {
        Some(pid) => {
            let is_focus = pid == root_person_id;
            let bg = card_bg(is_focus, node.is_sibling);
            let text_fill = if is_focus {
                "var(--white)"
            } else {
                "var(--pn-text)"
            };
            let stroke = gender_stroke(node.sex);
            let has_surname = !surname_disp.is_empty();
            let has_given = !given_disp.is_empty();
            let has_date = !date_s.is_empty();
            let portrait = node
                .photo_url
                .clone()
                .unwrap_or_else(|| CroppedSource::silhouette(node.sex));
            let is_sosa_root = matches!(node.sosa_badge, SosaBadge::Root);
            let is_sosa_direct = matches!(node.sosa_badge, SosaBadge::Direct);
            let is_self = node.is_self;
            let card_class = if is_focus {
                "ped-card ped-card-focus"
            } else {
                "ped-card"
            };
            rsx! {
                g {
                    key: "{key}",
                    class: "{card_class}",
                    transform: "translate({nx},{ny})",
                    style: "cursor:pointer",
                    onclick: move |_| { selected_person_id.set(pid); on_person_navigate.call(pid); },
                    oncontextmenu: move |evt: Event<MouseData>| {
                        evt.prevent_default();
                        evt.stop_propagation();
                        selected_person_id.set(pid);
                        let coords = evt.client_coordinates();
                        on_person_click.call((pid, coords.x, coords.y));
                    },
                    rect { class: "ped-card-rect", x: "{padding}", y: "{padding}", rx: "{border_radius}", ry: "{border_radius}", width: "{rw}", height: "{rh}", style: "fill:{bg};stroke:{frame_stroke};stroke-width:{frame_width}" }
                    if let Some((inset, iw, ih)) = inner {
                        rect { class: "ped-card-inner-rule", x: "{inset}", y: "{inset}", width: "{iw}", height: "{ih}", style: "fill:none;stroke:var(--pn-border);stroke-width:1" }
                    }
                    if let Some(gl) = gl_path {
                        path { d: "{gl}", style: "stroke:{stroke};stroke-width:{gender_line_width};fill:none" }
                    }
                    rect { class: "ped-card-mat", x: "{ph_x}", y: "{ph_y}", rx: "{photo_round}", ry: "{photo_round}", width: "{ph_w}", height: "{ph_h}", style: "fill:var(--pn-mat,var(--white))" }
                    CroppedSvgImage { image: portrait, x: ph_x, y: ph_y, width: ph_w, height: ph_h, fallback: CroppedSource::silhouette(node.sex) }
                    if is_self {
                        g {
                            circle { cx: "{sosa_cx}", cy: "{sosa_cy}", r: "{sosa_r}", style: "fill:var(--pn-self)" }
                            circle { cx: "{sosa_cx}", cy: "{sosa_cy}", r: "3", style: "fill:var(--white)" }
                        }
                    } else if is_sosa_root {
                        g {
                            circle { cx: "{sosa_cx}", cy: "{sosa_cy}", r: "{sosa_r}", style: "fill:var(--pn-sosa-root)" }
                            text { x: "{sosa_cx}", y: "{sosa_cy+4.0}", style: "fill:var(--white);font-size:10px;font-weight:700;text-anchor:middle;font-family:Arial,sans-serif", "1" }
                        }
                    } else if is_sosa_direct {
                        g {
                            circle { cx: "{sosa_cx}", cy: "{sosa_cy}", r: "{sosa_r}", style: "fill:var(--pn-sosa)" }
                            circle { cx: "{sosa_cx}", cy: "{sosa_cy}", r: "5", style: "fill:var(--white)" }
                            circle { cx: "{sosa_cx}", cy: "{sosa_cy}", r: "3", style: "fill:var(--pn-sosa)" }
                        }
                    }
                    text {
                        class: "ped-card-name-text",
                        if has_given {
                            tspan { x: "{tx}", y: "{given_y}", style: "font-size:{given_font_px}px;font-family:{body_font};fill:{text_fill}", "{given_disp}" }
                        }
                        if has_surname {
                            tspan { x: "{tx}", y: "{surname_y}", style: "font-size:{surname_font_px}px;font-weight:{surname_weight};font-family:{surname_font};fill:{text_fill}", "{surname_disp}" }
                        }
                    }
                    // The lifespan is its own `text` rather than a third tspan
                    // so it can own a `<title>`: SVG 1.1 does not allow one
                    // inside a `tspan`, and the qualifier marks are exactly the
                    // part of the card that needs to be able to explain itself.
                    // Absolute x/y means it lands where the tspan did.
                    if has_date {
                        text {
                            class: "ped-card-name-text",
                            x: "{tx}",
                            y: "{date_y}",
                            style: "font-size:{date_font_px}px;font-family:{body_font};fill:{text_fill}",
                            "textLength": date_squeeze.map(|w| w.to_string()),
                            "lengthAdjust": date_squeeze.map(|_| "spacingAndGlyphs"),
                            dangerous_inner_html: "{date_html}",
                        }
                    }
                    if is_focus {
                        g {
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
                            transform: "translate({padding},{padding})",
                            style: "cursor:pointer",
                            onclick: move |evt: Event<MouseData>| {
                                evt.stop_propagation();
                                selected_person_id.set(pid);
                                on_person_navigate.call(pid);
                            },
                            text { x: "5", y: "-2", style: "fill:var(--blue);font-size:13px;font-weight:700;text-anchor:middle;font-family:sans-serif", "+" }
                        }
                    }
                }
            }
        }
        None => {
            let child_id = node.child_of;
            let is_father = node.is_father;
            let plus_x = slot_plus_x;
            let plus_y = slot_plus_y;
            rsx! {
                g { key: "{key}", transform: "translate({nx},{ny})",
                    if let (true, Some(cid)) = (allow_empty_click, child_id) {
                        g {
                            style: "cursor:pointer",
                            onclick: move |_| on_empty_slot.call((cid, is_father)),
                            rect { x: "{padding}", y: "{padding}", rx: "{border_radius}", ry: "{border_radius}", width: "{rw}", height: "{rh}", style: "fill:var(--pn-bg);stroke:var(--pn-border);stroke-width:1;stroke-dasharray:4,4" }
                            text { x: "{plus_x}", y: "{plus_y}", style: "fill:var(--pn-root-bg);font-size:22px;font-weight:700;text-anchor:middle;font-family:sans-serif", "+" }
                        }
                    } else {
                        rect { x: "{padding}", y: "{padding}", rx: "{border_radius}", ry: "{border_radius}", width: "{rw}", height: "{rh}", style: "fill:var(--pn-bg);stroke:var(--pn-border);stroke-width:1;stroke-dasharray:4,4;opacity:0.3" }
                    }
                }
            }
        }
    }
}

#[component]
pub fn PedigreeChart(props: PedigreeChartProps) -> Element {
    let i18n = use_i18n();
    let view_cache = use_view_state_cache();
    let tid_parsed = props.tree_id.parse::<Uuid>().ok();
    let saved = tid_parsed.and_then(|t| view_cache.get_untracked(t));
    let defaults = use_pedigree_defaults().unwrap_or_default();

    // Extract initial values from saved state (or defaults)
    let init_anc = saved
        .as_ref()
        .map_or(defaults.ancestor_levels, |s| s.ancestor_levels);
    let init_desc = saved
        .as_ref()
        .map_or(defaults.descendant_levels, |s| s.descendant_levels);
    let init_ox = saved.as_ref().map_or(0.0, |s| s.offset_x);
    let init_oy = saved.as_ref().map_or(0.0, |s| s.offset_y);
    let init_sc = saved.as_ref().map_or(1.0, |s| s.scale);

    // ── Depth controls (max 10) ──
    let mut ancestor_levels = use_signal(move || init_anc);
    let mut descendant_levels = use_signal(move || init_desc);
    let mut depth_hover = use_signal(|| false);
    let mut depth_hover_gen = use_signal(|| 0u32);

    // ── Pan state ──
    let mut offset_x = use_signal(move || init_ox);
    let mut offset_y = use_signal(move || init_oy);
    let mut dragging = use_signal(|| false);
    let mut drag_start_x = use_signal(|| 0.0f64);
    let mut drag_start_y = use_signal(|| 0.0f64);
    let mut drag_origin_x = use_signal(|| 0.0f64);
    let mut drag_origin_y = use_signal(|| 0.0f64);

    // ── Zoom state ──
    let mut scale = use_signal(move || init_sc);
    // Viewport's own page position, cached from each fit measurement so
    // wheel-zoom can convert mouse coordinates without an async round trip
    // on every tick (see `onwheel` below).
    let viewport_page_pos = use_signal(|| (0.0f64, 0.0f64));

    // ── Selected person (drives event panel) ──
    let mut selected_person_id = use_signal(|| props.root_person_id);

    let mut last_viewport_width = use_signal(|| VIEWPORT_DEFAULT_W);

    // ── Event panel collapse (persisted via localStorage) ──
    let mut panel_collapsed = use_signal(|| false);
    let mut panel_init = use_signal(|| false);
    if !panel_init() {
        panel_init.set(true);
        spawn(async move {
            if let Ok(val) = document::eval(&format!(
                r#"
                localStorage.removeItem('oxidgene-ev-panel');
                const storedRatio = Number.parseFloat(localStorage.getItem('{EVENT_PANEL_RATIO_STORAGE_KEY}'));
                if (Number.isFinite(storedRatio) && storedRatio > 0) {{
                    // Only a panel the reader has dragged is proportional; the
                    // untouched default stays at the fixed width from the CSS.
                    const ratio = Math.min({EVENT_PANEL_MAX_RATIO}, storedRatio);
                    const sidebarWidth = document.querySelector('.pedigree-outer > .isb')?.getBoundingClientRect().width || 46;
                    document.documentElement.style.setProperty(
                        '--evw',
                        `calc(${{ratio * 100}}% - ${{ratio * sidebarWidth}}px)`,
                    );
                }}
                const width = window.innerWidth || document.documentElement.clientWidth || 1024;
                return [localStorage.getItem('{EVENT_PANEL_MANUAL_STORAGE_KEY}') === 'collapsed', width];
                "#,
            ))
            .await
            {
                let manual_collapsed = val.get(0).and_then(|value| value.as_bool()).unwrap_or(false);
                let width = val
                    .get(1)
                    .and_then(|value| value.as_f64())
                    .unwrap_or(VIEWPORT_DEFAULT_W);
                last_viewport_width.set(width);
                panel_collapsed.set(manual_collapsed || width <= EVENT_PANEL_AUTO_COLLAPSE_WIDTH);
            }
        });
    }

    // Re-fit the graph when the window is actually resized.
    //
    // WebKitGTK also fires resize on remapping. Check dimensions to preserve
    // the reader's pan and zoom when only window focus changes.
    use_effect(move || {
        document::eval(
            r#"
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
            "#,
        );
    });

    // ── Disable transition when root changes (avoid flying animation) ──
    let mut animating = use_signal(|| false);

    // ── Fit the graph in the viewport on first load and root/depth changes ──
    // Also fit when explicitly requested via center_gen > 0 (e.g. navigation
    // from search results), even when there is saved pan/zoom state.
    let mut needs_fit = use_signal(|| true);

    // ── Reset pan/zoom/selection when the root person changes ──
    let mut prev_root = use_signal(|| props.root_person_id);
    if prev_root() != props.root_person_id {
        prev_root.set(props.root_person_id);
        animating.set(false);
        scale.set(1.0);
        selected_person_id.set(props.root_person_id);
        needs_fit.set(true);
    }

    // ── Force re-centering when parent increments center_gen ──
    let mut prev_center_gen = use_signal(|| props.center_gen);
    if prev_center_gen() != props.center_gen {
        prev_center_gen.set(props.center_gen);
        animating.set(false);
        scale.set(1.0);
        needs_fit.set(true);
    }

    // ── Force re-centering when depth levels change ──
    let anc_now = ancestor_levels();
    let desc_now = descendant_levels();
    let mut prev_anc = use_signal(|| anc_now);
    let mut prev_desc = use_signal(|| desc_now);
    if prev_anc() != anc_now || prev_desc() != desc_now {
        prev_anc.set(anc_now);
        prev_desc.set(desc_now);
        animating.set(false);
        needs_fit.set(true);
    }

    // ── Compute SOSA ancestor set (persons who are ancestors of the SOSA root) ──
    // Use the server-provided SOSA ancestor set when available,
    // falling back to local graph traversal (which only works within the pedigree window).
    let sosa_ancestors: HashSet<Uuid> = props
        .sosa_ancestor_ids
        .clone()
        .or_else(|| {
            props
                .sosa_root_person_id
                .map(|sosa_id| props.data.ancestor_set(sosa_id))
        })
        .unwrap_or_default();

    // ── Compute layout ──
    let preferred = crate::prefs::use_pedigree_theme();
    let theme = props.theme.unwrap_or_else(|| preferred.theme());
    let layout = crate::ui_observability::measure_ui("pedigree_layout", || {
        compute_layout(
            props.root_person_id,
            &props.data,
            props.sosa_root_person_id,
            &sosa_ancestors,
            ancestor_levels(),
            descendant_levels(),
            theme,
        )
    });

    // ── Fit graph in viewport when needed ──
    if needs_fit() {
        let fit_content_cx = layout.content_cx;
        let fit_content_cy = layout.content_cy;
        let fit_content_w = layout.content_w;
        let fit_content_h = layout.content_h;
        needs_fit.set(false);
        spawn(async move {
            // Small delay so the DOM has rendered the viewport element.
            crate::utils::sleep_ms(30).await;
            fit_graph_in_viewport(
                scale,
                offset_x,
                offset_y,
                viewport_page_pos,
                (fit_content_cx, fit_content_cy, fit_content_w, fit_content_h),
            )
            .await;
            // Re-enable animation after fitting.
            crate::utils::sleep_ms(20).await;
            animating.set(true);
        });
    }

    // ── Persist view state into global cache so it survives navigation ──
    {
        let ox = offset_x();
        let oy = offset_y();
        let sc = scale();
        let anc = ancestor_levels();
        let desc = descendant_levels();
        let root = props.root_person_id;
        if let Some(tid) = tid_parsed {
            view_cache.save(PedigreeViewState {
                tree_id: tid,
                offset_x: ox,
                offset_y: oy,
                scale: sc,
                ancestor_levels: anc,
                descendant_levels: desc,
                selected_root: Some(root),
            });
        }
    }

    let transform = format!(
        "translate({}px, {}px) scale({})",
        offset_x(),
        offset_y(),
        scale()
    );
    let zoom_pct = (scale() * 100.0) as u32;

    let inner_class = if animating() {
        "pedigree-inner pedigree-animated"
    } else {
        "pedigree-inner"
    };

    // ── Event panel data (selected person) ──
    let sel_pid = selected_person_id();
    // The same resolver every other surface uses, so the no-name fallback is
    // the translated one rather than a hardcoded "Unknown".
    let sel_full_name = props.data.display_name(sel_pid, &i18n);
    let sel_portrait = props
        .data
        .photos
        .get(&sel_pid)
        .cloned()
        .unwrap_or_else(|| CroppedSource::silhouette(props.data.sex_of(sel_pid)));
    // The same lifespan the card draws, rather than the old "n. 1620" / "d.
    // 1691" abbreviations: the panel sits beside the card showing the very
    // same person, and two spellings of one life read as two different facts.
    // The events below keep their own full-text dates.
    // Always the wide form here: this is HTML that wraps, so unlike the card
    // it never has to give up a range's far end.
    let sel_dates = format_lifespan(
        props.data.qualified_birth_year(sel_pid),
        props.data.qualified_death_year(sel_pid),
    );

    // Family IDs where the selected person is a spouse — used both to pull in
    // conjugal-family events below and to flag which rendered events are
    // "direct" (on the person or their own conjugal family) vs. narrative
    // context (children, parents, siblings).
    let spouse_family_ids: Vec<Uuid> = props
        .data
        .families_as_spouse
        .get(&sel_pid)
        .cloned()
        .unwrap_or_default();

    // Collect all events relevant to this person:
    // 1. Individual events (birth, death, occupation, etc.)
    let mut sel_events: Vec<DomainEvent> = props
        .data
        .events_by_person
        .get(&sel_pid)
        .cloned()
        .unwrap_or_default();
    // 2. Conjugal family events (marriage, divorce, etc.)
    if let Some(fam_ids) = props.data.families_as_spouse.get(&sel_pid) {
        for fid in fam_ids {
            if let Some(fam_events) = props.data.events_by_family.get(fid) {
                sel_events.extend(fam_events.iter().cloned());
            }
            // Also include major life events of children (birth, death, baptism, burial).
            if let Some(children) = props.data.children_by_family.get(fid) {
                for child in children {
                    if let Some(child_events) = props.data.events_by_person.get(&child.person_id) {
                        for ce in child_events {
                            if ce.event_type == EventType::Birth
                                || ce.event_type == EventType::Death
                                || ce.event_type == EventType::Baptism
                                || ce.event_type == EventType::Burial
                            {
                                sel_events.push(ce.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    // 3. Parental family events (sibling birth, parent death, etc.)
    if let Some(fam_ids) = props.data.families_as_child.get(&sel_pid) {
        for fid in fam_ids {
            if let Some(fam_events) = props.data.events_by_family.get(fid) {
                sel_events.extend(fam_events.iter().cloned());
            }
            // Also include individual events of family members (parents, siblings).
            if let Some(spouses) = props.data.spouses_by_family.get(fid) {
                for spouse in spouses {
                    if let Some(parent_events) = props.data.events_by_person.get(&spouse.person_id)
                    {
                        for pe in parent_events {
                            // Include major life events of parents (death, burial).
                            if pe.event_type == EventType::Death
                                || pe.event_type == EventType::Burial
                            {
                                sel_events.push(pe.clone());
                            }
                        }
                    }
                }
            }
            if let Some(children) = props.data.children_by_family.get(fid) {
                for child in children {
                    if child.person_id == sel_pid {
                        continue; // Skip self.
                    }
                    if let Some(sib_events) = props.data.events_by_person.get(&child.person_id) {
                        for se in sib_events {
                            // Include major life events of siblings (birth, death).
                            if se.event_type == EventType::Birth
                                || se.event_type == EventType::Death
                                || se.event_type == EventType::Baptism
                                || se.event_type == EventType::Burial
                            {
                                sel_events.push(se.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    // Deduplicate by event ID and sort by date.
    sel_events.sort_by_key(|a| a.id);
    sel_events.dedup_by_key(|e| e.id);
    sel_events.sort_by_key(|a| a.date_sort);

    // Group events by year for display.
    let mut event_groups: Vec<(String, Vec<DomainEvent>)> = Vec::new();
    for evt in &sel_events {
        let year = evt
            .date_value
            .as_deref()
            .map(fmt_year)
            .unwrap_or_else(|| "Unknown".to_string());
        if let Some(last) = event_groups.last_mut()
            && last.0 == year
        {
            last.1.push(evt.clone());
            continue;
        }
        event_groups.push((year, vec![evt.clone()]));
    }

    // ── Fit-to-content zoom calculation ──
    let fit_content_cx = layout.content_cx;
    let fit_content_cy = layout.content_cy;
    let fit_content_w = layout.content_w;
    let fit_content_h = layout.content_h;

    // Adapt the descending side's empty "+" slot (missing spouse) onto the
    // dedicated add-spouse callback — the `bool` (father/mother) from
    // `on_empty_slot` doesn't apply here, only the person needing a spouse.
    let on_add_spouse_slot = props.on_add_spouse_slot;
    let desc_empty_slot_adapter =
        EventHandler::new(move |(pid, _): (Uuid, bool)| on_add_spouse_slot.call(pid));

    rsx! {
        div { class: "pedigree-outer",

            // ══════════════════════════════════
            // ICON SIDEBAR
            // ══════════════════════════════════
            TreeIconSidebar {
                active_view: TreeSidebarView::Pedigree,
                selected_person_id: Some(selected_person_id()),
                on_profile_view: move |pid| {
                    if let Some(pid) = pid {
                        props.on_profile_view.call(pid);
                    }
                },
                on_pedigree_view: move |_| {},
                on_add_person: props.on_add_person,
                on_settings: props.on_settings,
                on_dictionary: props.on_dictionary,

                // Depth selector (hover popover)
                div {
                    class: "isb-depth-wrap",
                    onmouseenter: move |_| {
                        // Bump generation to cancel any pending close task.
                        depth_hover_gen += 1;
                        depth_hover.set(true);
                    },
                    onmouseleave: move |_| {
                        // Close after 200ms unless mouse re-enters (generation changes).
                        let leave_gen = depth_hover_gen();
                        spawn(async move {
                            crate::utils::sleep_ms(200).await;
                            if depth_hover_gen() == leave_gen {
                                depth_hover.set(false);
                            }
                        });
                    },
                    button {
                        class: "isb-btn",
                        title: "{i18n.t(\"pedigree.depth\")}",
                        svg {
                            width: "16",
                            height: "16",
                            fill: "none",
                            "viewBox": "0 0 24 24",
                            stroke: "currentColor",
                            "strokeWidth": "2",
                            // Layers/depth icon
                            path { d: "M12 2 2 7l10 5 10-5-10-5z" }
                            path { d: "M2 17l10 5 10-5" }
                            path { d: "M2 12l10 5 10-5" }
                        }
                    }
                    if depth_hover() {
                        div { class: "pedigree-depth-popover",
                            div { class: "pedigree-depth-row",
                                span { class: "pedigree-depth-arrow", "\u{2191}" }
                                button {
                                    class: "pedigree-depth-btn",
                                    onclick: move |_| { if ancestor_levels() > 0 { ancestor_levels -= 1; } },
                                    "\u{2212}" // −
                                }
                                span { class: "pedigree-depth-val", "{ancestor_levels()}" }
                                button {
                                    class: "pedigree-depth-btn",
                                    onclick: move |_| { if ancestor_levels() < 10 { ancestor_levels += 1; } },
                                    "+"
                                }
                            }
                            div { class: "pedigree-depth-row",
                                span { class: "pedigree-depth-arrow", "\u{2193}" }
                                button {
                                    class: "pedigree-depth-btn",
                                    onclick: move |_| { if descendant_levels() > 0 { descendant_levels -= 1; } },
                                    "\u{2212}"
                                }
                                span { class: "pedigree-depth-val", "{descendant_levels()}" }
                                button {
                                    class: "pedigree-depth-btn",
                                    onclick: move |_| { if descendant_levels() < 10 { descendant_levels += 1; } },
                                    "+"
                                }
                            }
                        }
                    }
                }

                div { class: "isb-hr" }

                button {
                    class: "isb-btn",
                    title: "{i18n.t(\"pedigree.zoom_in\")}",
                    onclick: move |_| scale.set((scale() * ZOOM_FACTOR).clamp(ZOOM_MIN, ZOOM_MAX)),
                    svg {
                        width: "16",
                        height: "16",
                        fill: "none",
                        "viewBox": "0 0 24 24",
                        stroke: "currentColor",
                        "strokeWidth": "2",
                        circle { cx: "11", cy: "11", r: "8" }
                        line { x1: "21", y1: "21", x2: "16.65", y2: "16.65" }
                        line { x1: "11", y1: "8", x2: "11", y2: "14" }
                        line { x1: "8", y1: "11", x2: "14", y2: "11" }
                    }
                }
                button {
                    class: "isb-btn",
                    title: "{i18n.t(\"pedigree.zoom_out\")}",
                    onclick: move |_| scale.set((scale() / ZOOM_FACTOR).clamp(ZOOM_MIN, ZOOM_MAX)),
                    svg {
                        width: "16",
                        height: "16",
                        fill: "none",
                        "viewBox": "0 0 24 24",
                        stroke: "currentColor",
                        "strokeWidth": "2",
                        circle { cx: "11", cy: "11", r: "8" }
                        line { x1: "21", y1: "21", x2: "16.65", y2: "16.65" }
                        line { x1: "8", y1: "11", x2: "14", y2: "11" }
                    }
                }
                span { class: "isb-zoom-val", "{zoom_pct}%" }
                button {
                    class: "isb-btn",
                    title: "{i18n.t(\"pedigree.fit_screen\")}",
                    onclick: move |_| {
                        spawn(fit_graph_in_viewport(
                            scale,
                            offset_x,
                            offset_y,
                            viewport_page_pos,
                            (fit_content_cx, fit_content_cy, fit_content_w, fit_content_h),
                        ));
                    },
                    svg {
                        width: "16",
                        height: "16",
                        fill: "none",
                        "viewBox": "0 0 24 24",
                        stroke: "currentColor",
                        "strokeWidth": "2",
                        // Maximize/fit-screen icon (four corners)
                        path { d: "M3 8V5a2 2 0 0 1 2-2h3" }
                        path { d: "M16 3h3a2 2 0 0 1 2 2v3" }
                        path { d: "M21 16v3a2 2 0 0 1-2 2h-3" }
                        path { d: "M8 21H5a2 2 0 0 1-2-2v-3" }
                    }
                }

                div { class: "isb-hr" }
            }

            button {
                class: "pedigree-resize-fit-trigger",
                tabindex: "-1",
                onclick: move |_| {
                    spawn(async move {
                        if let Ok(val) = document::eval("return window.innerWidth || document.documentElement.clientWidth || 1024").await {
                            let width = val.as_f64().unwrap_or(VIEWPORT_DEFAULT_W);
                            let previous_width = last_viewport_width();
                            if previous_width > EVENT_PANEL_AUTO_COLLAPSE_WIDTH
                                && width <= EVENT_PANEL_AUTO_COLLAPSE_WIDTH
                                && !panel_collapsed()
                            {
                                panel_collapsed.set(true);
                            }
                            last_viewport_width.set(width);
                        }
                        needs_fit.set(true);
                    });
                },
            }

            // ══════════════════════════════════
            // CANVAS VIEWPORT
            // ══════════════════════════════════
            div {
                class: "pedigree-viewport {theme.viewport_class}",

                onpointerdown: move |evt| {
                    // Direct manipulation tracks the pointer 1:1 — the CSS
                    // transition is only for programmatic jumps (fit/center).
                    animating.set(false);
                    let coords = evt.client_coordinates();
                    drag_start_x.set(coords.x);
                    drag_start_y.set(coords.y);
                    drag_origin_x.set(offset_x());
                    drag_origin_y.set(offset_y());
                    dragging.set(true);
                },
                onpointermove: move |evt| {
                    if dragging() {
                        let coords = evt.client_coordinates();
                        offset_x.set(drag_origin_x() + coords.x - drag_start_x());
                        offset_y.set(drag_origin_y() + coords.y - drag_start_y());
                    }
                },
                onpointerup: move |_| { dragging.set(false); },
                onpointerleave: move |_| { dragging.set(false); },
                onwheel: move |evt| {
                    let delta_y = match evt.delta() {
                        WheelDelta::Lines(l) => l.y * 20.0,
                        WheelDelta::Pixels(p) => p.y,
                        WheelDelta::Pages(p) => p.y * 400.0,
                    };
                    let factor = if delta_y > 0.0 { 0.9 } else { 1.0 / 0.9 };
                    let old_scale = scale();
                    let new_scale = (old_scale * factor).clamp(ZOOM_MIN, ZOOM_MAX);
                    if (new_scale - old_scale).abs() > f64::EPSILON {
                        // Same reasoning as onpointerdown: a wheel gesture is
                        // a stream of many small updates, each of which must
                        // land instantly or they visibly fight the CSS
                        // transition and the zoom feels laggy. Reading the
                        // cached viewport position (refreshed on each fit)
                        // instead of an async DOM query per tick keeps this
                        // handler fully synchronous, so there is no
                        // round-trip latency and no risk of updates applying
                        // out of order.
                        animating.set(false);
                        let coords = evt.client_coordinates();
                        let (vx, vy) = viewport_page_pos();
                        let mouse_x = coords.x - vx;
                        let mouse_y = coords.y - vy;
                        let world_x = (mouse_x - offset_x()) / old_scale;
                        let world_y = (mouse_y - offset_y()) / old_scale;
                        scale.set(new_scale);
                        offset_x.set(mouse_x - world_x * new_scale);
                        offset_y.set(mouse_y - world_y * new_scale);
                    }
                },

                div {
                    class: inner_class,
                    style: "transform: {transform};",

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
                                    for (si, path) in layout.asc_links.iter().enumerate() {
                                        path { key: "al-{si}", d: "{path}", class: "pedigree-connector-path", fill: "none" }
                                    }
                                    for (ni, node) in layout.asc_nodes.iter().enumerate() {
                                        {render_pedigree_card(
                                            node,
                                            ni,
                                            "an",
                                            props.root_person_id,
                                            selected_person_id,
                                            props.on_person_navigate,
                                            props.on_person_click,
                                            props.on_empty_slot,
                                            true,
                                            i18n,
                                            theme,
                                        )}
                                    }
                                }

                                // ── Descending tree ──
                                g {
                                    transform: "translate({layout.desc_tx},{layout.desc_ty})",
                                    for (si, path) in layout.desc_links.iter().enumerate() {
                                        path { key: "dl-{si}", d: "{path}", class: "pedigree-connector-path", fill: "none" }
                                    }
                                    for (ni, node) in layout.desc_nodes.iter().enumerate() {
                                        {render_pedigree_card(
                                            node,
                                            ni,
                                            "dn",
                                            props.root_person_id,
                                            selected_person_id,
                                            props.on_person_navigate,
                                            props.on_person_click,
                                            desc_empty_slot_adapter,
                                            true,
                                            i18n,
                                            theme,
                                        )}
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ══════════════════════════════════
            // EVENT PANEL
            // ══════════════════════════════════
            if !panel_collapsed() {
                div {
                    class: "evp-resize-handle",
                    role: "separator",
                    tabindex: "0",
                    "aria-orientation": "vertical",
                    "aria-label": i18n.t("pedigree.resize_events"),
                    title: i18n.t("pedigree.resize_events"),
                    onpointerdown: move |evt| {
                        let start_x = evt.client_coordinates().x;
                        document::eval(&format!(
                            r#"
                            const outer = document.querySelector('.pedigree-outer');
                            const panel = document.querySelector('.ev-panel:not(.ev-panel-collapsed)');
                            if (!outer || !panel || window.innerWidth <= {EVENT_PANEL_AUTO_COLLAPSE_WIDTH}) return;

                            const sidebarWidth = outer.querySelector(':scope > .isb')?.getBoundingClientRect().width || 46;
                            const availableWidth = Math.max(1, outer.getBoundingClientRect().width - sidebarWidth);
                            const maxWidth = Math.max(
                                {EVENT_PANEL_MIN_WIDTH},
                                Math.min({EVENT_PANEL_MAX_WIDTH}, availableWidth * {EVENT_PANEL_MAX_RATIO}),
                            );
                            const startWidth = panel.getBoundingClientRect().width;
                            const startX = {start_x};

                            let width = startWidth;
                            const move = (event) => {{
                                const requested = startWidth + startX - event.clientX;
                                width = Math.min(maxWidth, Math.max({EVENT_PANEL_MIN_WIDTH}, requested));
                                document.documentElement.style.setProperty('--evw', `${{width}}px`);
                            }};
                            const finish = () => {{
                                window.removeEventListener('pointermove', move);
                                window.removeEventListener('pointerup', finish);
                                window.removeEventListener('pointercancel', finish);
                                outer.classList.remove('pedigree-is-resizing');
                                document.body.style.removeProperty('cursor');
                                document.body.style.removeProperty('user-select');

                                // Store and re-apply the width as a ratio so it
                                // follows later window resizes.
                                const ratio = width / availableWidth;
                                document.documentElement.style.setProperty(
                                    '--evw',
                                    `calc(${{ratio * 100}}% - ${{ratio * sidebarWidth}}px)`,
                                );
                                localStorage.setItem('{EVENT_PANEL_RATIO_STORAGE_KEY}', String(ratio));
                                document.querySelector('.pedigree-resize-fit-trigger')?.click();
                            }};

                            outer.classList.add('pedigree-is-resizing');
                            document.body.style.cursor = 'col-resize';
                            document.body.style.userSelect = 'none';
                            window.addEventListener('pointermove', move);
                            window.addEventListener('pointerup', finish);
                            window.addEventListener('pointercancel', finish);
                            "#,
                        ));
                    },
                    onkeydown: move |evt| {
                        let delta = match evt.key() {
                            Key::ArrowLeft => EVENT_PANEL_KEYBOARD_STEP,
                            Key::ArrowRight => -EVENT_PANEL_KEYBOARD_STEP,
                            _ => return,
                        };
                        evt.prevent_default();
                        document::eval(&format!(
                            r#"
                            const outer = document.querySelector('.pedigree-outer');
                            const panel = document.querySelector('.ev-panel:not(.ev-panel-collapsed)');
                            if (!outer || !panel || window.innerWidth <= {EVENT_PANEL_AUTO_COLLAPSE_WIDTH}) return;
                            const sidebarWidth = outer.querySelector(':scope > .isb')?.getBoundingClientRect().width || 46;
                            const availableWidth = Math.max(1, outer.getBoundingClientRect().width - sidebarWidth);
                            const maxWidth = Math.max(
                                {EVENT_PANEL_MIN_WIDTH},
                                Math.min({EVENT_PANEL_MAX_WIDTH}, availableWidth * {EVENT_PANEL_MAX_RATIO}),
                            );
                            const width = Math.min(
                                maxWidth,
                                Math.max({EVENT_PANEL_MIN_WIDTH}, panel.getBoundingClientRect().width + {delta}),
                            );
                            const ratio = width / availableWidth;
                            document.documentElement.style.setProperty(
                                '--evw',
                                `calc(${{ratio * 100}}% - ${{ratio * sidebarWidth}}px)`,
                            );
                            localStorage.setItem('{EVENT_PANEL_RATIO_STORAGE_KEY}', String(ratio));
                            document.querySelector('.pedigree-resize-fit-trigger')?.click();
                            "#,
                        ));
                    },
                }
            }
            div {
                class: if panel_collapsed() { "ev-panel ev-panel-collapsed" } else { "ev-panel" },
                button {
                    class: "evp-toggle",
                    title: if panel_collapsed() { i18n.t("pedigree.events") } else { i18n.t("pedigree.hide_events") },
                    onclick: move |_| {
                        let new_val = !panel_collapsed();
                        panel_collapsed.set(new_val);
                        let val = if new_val { "collapsed" } else { "open" };
                        document::eval(&format!(
                            "localStorage.setItem('{EVENT_PANEL_MANUAL_STORAGE_KEY}', '{}')",
                            val,
                        ));
                    },
                    if panel_collapsed() { "\u{203A}" } else { "\u{2039}" }
                }
                if !panel_collapsed() {
                    div { class: "evp-hd", {i18n.t("pedigree.events")} }
                    div { class: "evp-person",
                        div { class: "evp-av",
                            CroppedImage {
                                image: sel_portrait,
                                alt: String::new(),
                                fallback: CroppedSource::silhouette(props.data.sex_of(sel_pid)),
                            }
                        }
                        div { class: "evp-name",
                            strong { "{sel_full_name}" }
                            if !sel_dates.is_empty() {
                                span { "{sel_dates}" }
                            }
                        }
                    }
                    div { class: "evp-list",
                        if sel_events.is_empty() {
                            div { class: "evp-empty", {i18n.t("person_form.no_other_events")} }
                        } else {
                            for (gi, (year, events)) in event_groups.iter().enumerate() {
                                {
                                    let year = year.clone();
                                    let events = events.clone();
                                    let tree_id = props.tree_id.clone();
                                    rsx! {
                                        div { key: "evg-{gi}", class: "ev-year-group",
                                            div { class: "ev-year-header", "{year}" }
                                            for (ei, evt) in events.iter().enumerate() {
                                                {
                                                    let (icon, ic_class, label_key) = event_ui(evt.event_type);
                                                    let label = i18n.t(label_key);
                                                    let date_s = format_event_date(&i18n, evt);
                                                    let place_s = evt.place_id
                                                        .and_then(|pid| props.data.place_name(pid).map(String::from))
                                                        .or_else(|| evt.description.clone())
                                                        .unwrap_or_default();
                                                    // Build context label for events from related persons.
                                                    let context_name: Option<String> = if evt.person_id.is_some() && evt.person_id != Some(sel_pid) {
                                                        evt.person_id.map(|pid| props.data.display_name(pid, &i18n))
                                                    } else if evt.family_id.is_some() && evt.person_id.is_none() {
                                                        // Family event (marriage, divorce…) — show partner name.
                                                        evt.family_id.and_then(|fid| {
                                                            props.data.spouses_by_family.get(&fid).and_then(|spouses| {
                                                                spouses.iter()
                                                                    .find(|s| s.person_id != sel_pid)
                                                                    .map(|s| props.data.display_name(s.person_id, &i18n))
                                                            })
                                                        })
                                                    } else {
                                                        None
                                                    };
                                                    let full_label = if let Some(ref ctx) = context_name {
                                                        format!("{label} ({ctx})")
                                                    } else {
                                                        label
                                                    };
                                                    let is_direct = evt.person_id == Some(sel_pid)
                                                        || evt.family_id.is_some_and(|fid| spouse_family_ids.contains(&fid));
                                                    let item_class = if is_direct {
                                                        "ev-item ev-item-clickable ev-item-direct"
                                                    } else {
                                                        "ev-item ev-item-clickable"
                                                    };
                                                    let sel = sel_pid;
                                                    let tid = tree_id.clone();
                                                    let nav = use_navigator();
                                                    rsx! {
                                                        div {
                                                            key: "ev-{gi}-{ei}",
                                                            class: "{item_class}",
                                                            onclick: move |_| {
                                                                nav.push(crate::router::Route::PersonDetail {
                                                                    tree_id: tid.clone(),
                                                                    person_id: sel.to_string(),
                                                                });
                                                            },
                                                            div { class: ic_class, "{icon}" }
                                                            div { class: "ev-info",
                                                                div { class: "ev-type", "{full_label}" }
                                                                if !date_s.is_empty() {
                                                                    div { class: "ev-date", "{date_s}" }
                                                                }
                                                                if !place_s.is_empty() {
                                                                    div { class: "ev-place", "{place_s}" }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
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
    use oxidgene_core::{Calendar, NameType};

    fn id(n: u128) -> Uuid {
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
    struct Fixture {
        persons: HashMap<Uuid, Person>,
        names: HashMap<Uuid, Vec<PersonName>>,
        spouses_by_family: HashMap<Uuid, Vec<FamilySpouse>>,
        children_by_family: HashMap<Uuid, Vec<FamilyChild>>,
        families_as_child: HashMap<Uuid, Vec<Uuid>>,
        families_as_spouse: HashMap<Uuid, Vec<Uuid>>,
        events_by_person: HashMap<Uuid, Vec<DomainEvent>>,
    }

    impl Fixture {
        fn person(&mut self, n: u128, sex: Sex, given: &str, surname: &str) -> &mut Self {
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
        fn life(
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

        fn family(&mut self, fam: u128, spouses: &[u128], children: &[u128]) -> &mut Self {
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

        fn build(self) -> PedigreeData {
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
                photos: HashMap::new(),
                sosa_ancestors: HashSet::new(),
                sosa_root_id: None,
                self_person_id: None,
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
            photo_url: None,
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
            3,
            2,
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
            3,
            2,
            &PedigreeTheme::CLASSIC,
        );
        let layout = compute_layout(id(ROOT), &data, None, &HashSet::new(), 3, 2, &ruled);

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
        use crate::components::layout::LAYOUT_STYLES;
        use std::fmt::Write as _;

        let data = wide_pedigree();
        let i18n = I18n(crate::i18n::Language::En);
        let out_dir = std::env::var("OXIDGENE_PREVIEW_DIR").unwrap_or_else(|_| ".".to_string());

        for (name, theme) in [
            ("classic", &PedigreeTheme::CLASSIC),
            ("medieval", &PedigreeTheme::MEDIEVAL),
        ] {
            let layout = compute_layout(id(ROOT), &data, None, &HashSet::new(), 3, 2, theme);
            let mut svg = String::new();
            let _ = write!(
                svg,
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><g transform="translate({tx},{ty})">"#,
                w = layout.total_w,
                h = layout.total_h,
                tx = layout.main_tx,
                ty = layout.main_ty,
            );
            for path in layout.asc_links.iter() {
                let _ = write!(svg, r#"<path class="pedigree-connector-path" d="{path}"/>"#);
            }
            let _ = write!(
                svg,
                r#"<g transform="translate({},{})">"#,
                layout.desc_tx, layout.desc_ty
            );
            for path in layout.desc_links.iter() {
                let _ = write!(svg, r#"<path class="pedigree-connector-path" d="{path}"/>"#);
            }
            let _ = write!(svg, "</g>");

            let pad = theme.metrics.padding;
            let radius = theme.metrics.border_radius;
            let mut card_svg = |node: &LayoutNode, dx: f64, dy: f64| {
                let geo = card_geometry(node, theme, &i18n);
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
                    r#"<g class="ped-card" transform="translate({},{})"><rect class="ped-card-rect" x="{pad}" y="{pad}" rx="{radius}" ry="{radius}" width="{}" height="{}" style="fill:{bg};stroke:{stroke};stroke-width:{}{dash}"/>"#,
                    node.x + dx,
                    node.y + dy,
                    geo.rect_w,
                    geo.rect_h,
                    theme.card.frame_width,
                );
                if let (CardFrame::Cartouche { inner_inset }, true) = (geo.frame, node.id.is_some())
                {
                    let _ = write!(
                        svg,
                        r#"<rect class="ped-card-inner-rule" x="{0}" y="{0}" width="{1}" height="{2}" style="fill:none;stroke:var(--pn-border);stroke-width:1"/>"#,
                        pad + inner_inset,
                        geo.rect_w - 2.0 * inner_inset,
                        geo.rect_h - 2.0 * inner_inset,
                    );
                }
                if node.id.is_some() {
                    if let Some(line) = &geo.gender_line {
                        let _ = write!(
                            svg,
                            r#"<path d="{line}" style="stroke:{};stroke-width:{};fill:none"/>"#,
                            gender_stroke(node.sex),
                            geo.gender_line_width,
                        );
                    }
                    let _ = write!(
                        svg,
                        r#"<rect class="ped-card-mat" x="{}" y="{}" rx="{r}" ry="{r}" width="{}" height="{}" style="fill:var(--pn-mat,var(--white))"/>"#,
                        geo.photo_x,
                        geo.photo_y,
                        geo.photo_w,
                        geo.photo_h,
                        r = geo.photo_round,
                    );
                    let _ = write!(
                        svg,
                        r#"<text x="{}" y="{}" style="font-size:{}px;font-family:{};fill:{fill}">{}</text>"#,
                        geo.text_x,
                        geo.given_y,
                        geo.given_font_px,
                        geo.body_font,
                        escape_xml(&geo.given),
                    );
                    let _ = write!(
                        svg,
                        r#"<text x="{}" y="{}" style="font-size:{}px;font-weight:{};font-family:{};fill:{fill}">{}</text>"#,
                        geo.text_x,
                        geo.surname_y,
                        geo.surname_font_px,
                        geo.surname_weight,
                        geo.surname_font,
                        escape_xml(&geo.surname),
                    );
                    let _ = write!(
                        svg,
                        r#"<text x="{}" y="{}" style="font-size:{}px;font-family:{};fill:{fill}">{}</text>"#,
                        geo.text_x,
                        geo.date_y,
                        geo.date_font_px,
                        geo.body_font,
                        escape_xml(&geo.date_text),
                    );
                }
                let _ = write!(svg, "</g>");
            };

            for node in layout.asc_nodes.iter() {
                card_svg(node, 0.0, 0.0);
            }
            for node in layout.desc_nodes.iter() {
                card_svg(node, layout.desc_tx, layout.desc_ty);
            }
            let _ = write!(svg, "</g></svg>");

            let page = format!(
                "<!doctype html><meta charset=\"utf-8\"><style>{LAYOUT_STYLES}\n\
                 body{{margin:0}} .preview{{position:relative;overflow:visible}}</style>\
                 <div class=\"pedigree-viewport preview {}\" style=\"width:{}px;height:{}px\">{svg}</div>",
                theme.viewport_class, layout.total_w, layout.total_h,
            );
            let path = format!("{out_dir}/pedigree-{name}.html");
            std::fs::write(&path, page).expect("preview written");
            println!("wrote {path}");
        }

        // The settings swatches, on the page that shows them, so the two
        // stylesheets fight in the order the application loads them.
        let mut row = String::new();
        for id in crate::components::pedigree_theme::PedigreeThemeId::ALL {
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
            let _ = write!(
                row,
                r#"<button class="ped-theme-option"><svg class="ped-theme-swatch {cls}" viewBox="0 0 {vb_w} {vb_h}" preserveAspectRatio="xMidYMid meet"><rect x="0" y="0" width="{vb_w}" height="{vb_h}" style="fill:var(--pn-swatch-bg,transparent)"/><path d="{link}" class="pedigree-connector-path"/>{cards}</svg><span class="ped-theme-option-label">{id:?}</span></button>"#,
                cls = theme.viewport_class,
            );
        }
        let page = format!(
            "<!doctype html><meta charset=\"utf-8\"><style>{LAYOUT_STYLES}</style>\
             <style>{}</style><body style=\"background:var(--bg-deep);padding:24px\">\
             <div class=\"ped-theme-options\" style=\"max-width:420px\">{row}</div>",
            crate::pages::app_settings::SHARED_SETTINGS_STYLES,
        );
        let path = format!("{out_dir}/pedigree-swatches.html");
        std::fs::write(&path, page).expect("preview written");
        println!("wrote {path}");
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
            3,
            2,
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
asc card[0] x=315.0000 y=388.0000 compact=false
asc card[1] x=525.0000 y=276.0000 compact=false
asc card[2] x=630.0000 y=164.0000 compact=false
asc card[3] x=735.0000 y=0.0000 compact=true
asc card[4] x=630.0000 y=0.0000 compact=true
asc card[5] x=420.0000 y=164.0000 compact=false
asc card[6] x=105.0000 y=276.0000 compact=false
asc card[7] x=210.0000 y=164.0000 compact=false
asc card[8] x=315.0000 y=0.0000 compact=true
asc card[9] x=210.0000 y=0.0000 compact=true
asc card[10] x=0.0000 y=164.0000 compact=false
asc card[11] x=105.0000 y=0.0000 compact=true
asc card[12] x=0.0000 y=0.0000 compact=true
asc card[13] x=750.0000 y=388.0000 compact=false
asc link[0] M420,392 L420,377 210,377 210,362
asc link[1] M420,392 L420,377 630,377
asc link[2] M630,280 L630,265 525,265 525,250
asc link[3] M630,280 L630,265 735,265 735,250
asc link[4] M735,168 L735,153 685,153 685,138
asc link[5] M735,168 L735,153 790,153 790,138
asc link[6] M210,280 L210,265 105,265 105,250
asc link[7] M210,280 L210,265 315,265 315,250
asc link[8] M315,168 L315,153 265,153 265,138
asc link[9] M315,168 L315,153 370,153 370,138
asc link[10] M105,168 L105,153 55,153 55,138
asc link[11] M105,168 L105,153 160,153 160,138
asc link[12] M630,362 L630,377 855,377 855,392
canvas total=(1080.0000,932.0000) root=(480.0000,504.0000)
root rect=(200.0000,82.0000) line=- photo_x=14.0000 text_x=82.0000 sosa=(64.0000,63.0000) given="Root"@30.0000 surname="BRANCH_A"@47.0000 date="ca 1849-< 1917"@64.0000 squeeze=None fab=(105.0000,103.0000) plus=(105.0000,54.0000)
compact-named rect=(97.0000,134.0000) line=- photo_x=25.5000 text_x=12.0000 sosa=(75.5000,63.0000) given="Given_1"@94.0000 surname="BRANCH_A"@111.0000 date=""@128.0000 squeeze=None fab=(53.5000,155.0000) plus=(53.5000,80.0000)
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
            3,
            2,
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
