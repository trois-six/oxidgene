//! What a viewer who is not the tree's owner sees of it
//! (docs/privacy.md §3, §5, §6): the one policy every enforcement point
//! applies, as a [`Redaction`] built once per request and tree.
//!
//! A viewer is described by a [`Viewing`]: which of the redacting
//! permissions their access holds. The owner holds them all and sees
//! everything ([`Redaction::unrestricted`]). Editors and Guests hold
//! `read_private`, so only the owner's exclusions hide persons from them,
//! and Guests lack `read_confidential`. A Visitor holds none: the default
//! privacy, the persons' own privacy and the contemporary rule hide more,
//! shown as the tree's display of hidden persons says. The seam for a new
//! kind of viewer is the [`Viewing`] its access makes; no enforcement point
//! names a role.
//!
//! Decisions recorded in docs/privacy.md: an excluded person is absent for
//! every viewer and in every display mode — the most a stub without an
//! identifier could be is a blank, unreachable card, which the hidden mode
//! already gives (§6.3) — and a placeholder that may not be navigated
//! through carries a pseudonymous identifier, the same within one request,
//! never the person's own.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use uuid::Uuid;

use crate::enums::{HiddenDisplay, Privacy, TreeDefaultPrivacy};
use crate::privacy::{PersonVisibility, Withheld, withheld};
use crate::projection::{
    Pedigree, PedigreeFamilyMember, PedigreeNode, PersonProfile, ProfileChildLink,
    ProfileFamilyLink, ProfileName,
};
use crate::types::PrivacySettings;

/// How a person appears to a viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Appearance {
    /// Everything the record holds.
    Shown,
    /// A placeholder keeping the tree's shape: sex and family links.
    Stub,
    /// The placeholder with the initials of the first given name and the
    /// surname.
    Initials,
    /// In no list, chart, count or link; a direct read is `404`.
    Absent,
}

/// Why, and how, a person is hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HiddenPerson {
    pub appearance: Appearance,
    pub reason: Withheld,
}

/// The redacting permissions a viewer's access to a tree holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewing {
    /// `read_private`: private records and contemporaries unredacted.
    pub read_private: bool,
    /// `read_confidential`: confidential notes.
    pub read_confidential: bool,
    /// `read_tags`: the tag vocabulary and the tags of records.
    pub read_tags: bool,
    /// Whether the owner's exclusions hide persons from the viewer.
    pub exclusions_apply: bool,
}

impl Viewing {
    /// The viewing of `access`, the access the request resolved.
    #[must_use]
    pub fn of(access: &crate::access::TreeAccess) -> Self {
        use crate::access::Permission;
        Self {
            read_private: access.allows(Permission::ReadPrivate),
            read_confidential: access.allows(Permission::ReadConfidential),
            read_tags: access.allows(Permission::ReadTags),
            exclusions_apply: access.exclusions_apply,
        }
    }

    /// Whether the viewing hides nothing: the owner's.
    #[must_use]
    pub fn is_unrestricted(&self) -> bool {
        self.read_private && self.read_confidential && self.read_tags && !self.exclusions_apply
    }
}

/// What a [`Redaction`] is built from: the tree's records as the
/// enforcement points need them.
#[derive(Debug, Default)]
pub struct RedactionInputs<'a> {
    /// The persons the owner's exclusions cover.
    pub excluded: Option<&'a HashSet<Uuid>>,
    /// The stored inputs of the contemporary rule of every live person,
    /// for a viewer without `read_private`.
    pub visibility: Option<&'a HashMap<Uuid, PersonVisibility>>,
    /// Each live family with its privacy and its live spouses: those with a
    /// hidden spouse, or private to a viewer without `read_private`, are
    /// withheld. Only the families of hidden persons are needed for a viewer
    /// with `read_private`.
    pub families: &'a [(Uuid, Privacy, Vec<Uuid>)],
}

/// What one viewer sees of one tree, for one request.
#[derive(Debug, Default)]
pub struct Redaction {
    restricted: bool,
    persons: HashMap<Uuid, HiddenPerson>,
    families: HashSet<Uuid>,
    withhold_private_records: bool,
    withhold_confidential: bool,
    withhold_tags: bool,
    navigation: bool,
    contemporary_photos: bool,
    default_privacy: TreeDefaultPrivacy,
    pseudonyms: Mutex<HashMap<Uuid, Uuid>>,
}

impl Redaction {
    /// The owner's: nothing is hidden.
    #[must_use]
    pub fn unrestricted() -> Self {
        Self {
            navigation: true,
            ..Self::default()
        }
    }

    /// The redaction `viewing` makes of a tree with these settings and
    /// records, in `current_year`.
    #[must_use]
    pub fn build(
        viewing: Viewing,
        default_privacy: TreeDefaultPrivacy,
        settings: &PrivacySettings,
        inputs: &RedactionInputs<'_>,
        current_year: i32,
    ) -> Self {
        if viewing.is_unrestricted() {
            return Self::unrestricted();
        }
        let mut persons = HashMap::new();
        if viewing.exclusions_apply {
            for &id in inputs.excluded.into_iter().flatten() {
                persons.insert(
                    id,
                    HiddenPerson {
                        appearance: Appearance::Absent,
                        reason: Withheld::Excluded,
                    },
                );
            }
        }
        if !viewing.read_private {
            let shown_as = match settings.hidden_display {
                HiddenDisplay::Stub => Appearance::Stub,
                HiddenDisplay::Initials => Appearance::Initials,
                HiddenDisplay::Hidden => Appearance::Absent,
            };
            let rule = settings.contemporary_rule();
            for (&id, row) in inputs.visibility.into_iter().flatten() {
                if persons.contains_key(&id) {
                    continue;
                }
                let contemporary = row.is_contemporary(rule, current_year);
                if let Some(reason) = withheld(false, row.privacy, default_privacy, contemporary) {
                    persons.insert(
                        id,
                        HiddenPerson {
                            appearance: shown_as,
                            reason,
                        },
                    );
                }
            }
        }
        let families = inputs
            .families
            .iter()
            .filter(|(_, privacy, spouses)| {
                spouses.iter().any(|s| persons.contains_key(s))
                    || (!viewing.read_private
                        && default_privacy.resolve(*privacy) == TreeDefaultPrivacy::Private)
            })
            .map(|(id, ..)| *id)
            .collect();
        Self {
            restricted: true,
            persons,
            families,
            withhold_private_records: !viewing.read_private,
            withhold_confidential: !viewing.read_confidential,
            withhold_tags: !viewing.read_tags,
            navigation: settings.hidden_navigation,
            contemporary_photos: settings.contemporary_photos,
            default_privacy,
            pseudonyms: Mutex::default(),
        }
    }

    /// Whether the viewer sees everything: the owner.
    #[must_use]
    pub fn is_unrestricted(&self) -> bool {
        !self.restricted
    }

    /// How person `id` appears.
    #[must_use]
    pub fn person(&self, id: Uuid) -> Appearance {
        self.persons
            .get(&id)
            .map_or(Appearance::Shown, |hidden| hidden.appearance)
    }

    /// Why person `id` is hidden, if they are.
    #[must_use]
    pub fn hidden(&self, id: Uuid) -> Option<HiddenPerson> {
        self.persons.get(&id).copied()
    }

    /// Whether person `id` is shown in full.
    #[must_use]
    pub fn shows(&self, id: Uuid) -> bool {
        !self.persons.contains_key(&id)
    }

    /// Whether person `id` appears at all, in full or as a placeholder.
    #[must_use]
    pub fn appears(&self, id: Uuid) -> bool {
        self.person(id) != Appearance::Absent
    }

    /// The persons who do not appear at all.
    pub fn absent(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.persons
            .iter()
            .filter(|(_, h)| h.appearance == Appearance::Absent)
            .map(|(id, _)| *id)
    }

    /// Every hidden person, absent or a placeholder.
    pub fn hidden_persons(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.persons.keys().copied()
    }

    /// Whether the content of family `id` — its events, notes, citations
    /// and media — is withheld (§3.2): a spouse is hidden, or the family is
    /// private to a viewer without `read_private`. Its links stay.
    #[must_use]
    pub fn family_withheld(&self, id: Uuid) -> bool {
        self.families.contains(&id)
    }

    /// The families whose content is withheld ([`Self::family_withheld`]).
    pub fn withheld_families(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.families.iter().copied()
    }

    /// Whether private records are withheld: a viewer without
    /// `read_private`.
    #[must_use]
    pub fn withholds_private_records(&self) -> bool {
        self.withhold_private_records
    }

    /// Whether a record of `privacy` — a medium, a family — is private to
    /// this viewer (rule 2 of §3.1).
    #[must_use]
    pub fn private(&self, privacy: Privacy) -> bool {
        self.withhold_private_records
            && self.default_privacy.resolve(privacy) == TreeDefaultPrivacy::Private
    }

    /// Whether confidential notes are withheld.
    #[must_use]
    pub fn withholds_confidential(&self) -> bool {
        self.withhold_confidential
    }

    /// Whether tags are withheld.
    #[must_use]
    pub fn withholds_tags(&self) -> bool {
        self.withhold_tags
    }

    /// Whether a medium of `privacy` showing `persons` — linked to it, on
    /// its vignettes, through their events — and linked to `families` is
    /// withheld (§3.2). A contemporary's photograph is shown when the tree
    /// shows contemporaries' photos and that is the only reason.
    #[must_use]
    pub fn medium_withheld(
        &self,
        privacy: Privacy,
        persons: impl IntoIterator<Item = Uuid>,
        families: impl IntoIterator<Item = Uuid>,
    ) -> bool {
        if !self.restricted {
            return false;
        }
        if self.private(privacy) || families.into_iter().any(|f| self.family_withheld(f)) {
            return true;
        }
        persons.into_iter().any(|person| match self.hidden(person) {
            None => false,
            Some(hidden) => {
                !(hidden.reason == Withheld::Contemporary
                    && self.contemporary_photos
                    && hidden.appearance != Appearance::Absent)
            }
        })
    }

    /// Whether a note is withheld: confidential to a viewer without
    /// `read_confidential`, or of a record `owner_withheld` says is.
    #[must_use]
    pub fn note_withheld(&self, confidential: bool, owner_withheld: bool) -> bool {
        self.restricted && ((confidential && self.withhold_confidential) || owner_withheld)
    }

    /// The identifier person `id` is shown under: their own when shown, or
    /// a placeholder the tree lets visitors navigate through; a pseudonym,
    /// the same for the whole request, for one that may not be; none when
    /// absent.
    #[must_use]
    pub fn public_id(&self, id: Uuid) -> Option<Uuid> {
        match self.hidden(id) {
            None => Some(id),
            Some(hidden) => match hidden.appearance {
                Appearance::Absent => None,
                _ if self.navigation && hidden.reason != Withheld::Excluded => Some(id),
                _ => Some(self.pseudonym(id)),
            },
        }
    }

    fn pseudonym(&self, id: Uuid) -> Uuid {
        let mut map = self
            .pseudonyms
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Random but for its time prefix, which says when the request ran,
        // nothing of the person. The workspace's `uuid` has no `v4` on
        // WASM, which builds this crate too.
        *map.entry(id).or_insert_with(Uuid::now_v7)
    }

    // ── Projections ─────────────────────────────────────────────────────

    /// `profile` as the viewer sees it: `None` for an absent person, a
    /// placeholder for a hidden one, and for a shown one the profile with
    /// what names a hidden relative redacted and what a withheld family
    /// holds left out.
    #[must_use]
    pub fn profile(&self, profile: &PersonProfile) -> Option<PersonProfile> {
        if !self.restricted {
            return Some(profile.clone());
        }
        let id = self.public_id(profile.person_id)?;
        let appearance = self.person(profile.person_id);
        let mut out = if appearance == Appearance::Shown {
            let mut out = profile.clone();
            if self.withhold_tags {
                out.tags.clear();
                for event in out
                    .birth
                    .iter_mut()
                    .chain(out.death.iter_mut())
                    .chain(out.baptism.iter_mut())
                    .chain(out.burial.iter_mut())
                    .chain(out.other_events.iter_mut())
                {
                    event.tags.clear();
                }
            }
            out
        } else {
            PersonProfile {
                person_id: profile.person_id,
                tree_id: profile.tree_id,
                sex: profile.sex,
                primary_name: (appearance == Appearance::Initials)
                    .then(|| initials(profile.primary_name.as_ref()))
                    .flatten(),
                other_names: Vec::new(),
                birth: None,
                death: None,
                baptism: None,
                burial: None,
                other_events: Vec::new(),
                families_as_spouse: profile.families_as_spouse.clone(),
                family_as_child: profile.family_as_child.clone(),
                primary_media: None,
                media_count: 0,
                citation_count: 0,
                note_count: 0,
                tags: Vec::new(),
                updated_at: profile.updated_at,
                built_at: profile.built_at,
            }
        };
        out.person_id = id;
        out.families_as_spouse = out
            .families_as_spouse
            .iter()
            .map(|link| self.family_link(link))
            .collect();
        out.family_as_child = out
            .family_as_child
            .as_ref()
            .map(|link| self.child_link(link));
        Some(out)
    }

    fn family_link(&self, link: &ProfileFamilyLink) -> ProfileFamilyLink {
        let mut out = link.clone();
        if self.family_withheld(link.family_id) {
            out.events.clear();
        } else if self.withhold_tags {
            for event in &mut out.events {
                event.tags.clear();
            }
        }
        if let Some(spouse) = link.spouse_id {
            let appearance = self.person(spouse);
            out.spouse_id = self.public_id(spouse);
            if appearance != Appearance::Shown {
                let shown = (appearance == Appearance::Initials).then(|| {
                    initials_parts(
                        link.spouse_given_names.as_deref(),
                        link.spouse_surname.as_deref(),
                    )
                });
                let (given, surname) = shown.unwrap_or((None, None));
                out.spouse_display_name = display(given.as_deref(), surname.as_deref());
                out.spouse_given_names = given;
                out.spouse_surname = surname;
                if appearance == Appearance::Absent {
                    out.spouse_sex = None;
                }
            }
        }
        out.children_ids = link
            .children_ids
            .iter()
            .filter_map(|child| self.public_id(*child))
            .collect();
        out
    }

    fn child_link(&self, link: &ProfileChildLink) -> ProfileChildLink {
        let mut out = link.clone();
        let parent = |id: Option<Uuid>,
                      display_name: &mut Option<String>,
                      given: &mut Option<String>,
                      surname: &mut Option<String>|
         -> Option<Uuid> {
            let id = id?;
            let appearance = self.person(id);
            if appearance != Appearance::Shown {
                let (g, s) = if appearance == Appearance::Initials {
                    initials_parts(given.as_deref(), surname.as_deref())
                } else {
                    (None, None)
                };
                *display_name = display(g.as_deref(), s.as_deref());
                *given = g;
                *surname = s;
            }
            self.public_id(id)
        };
        out.father_id = parent(
            link.father_id,
            &mut out.father_display_name,
            &mut out.father_given_names,
            &mut out.father_surname,
        );
        out.mother_id = parent(
            link.mother_id,
            &mut out.mother_display_name,
            &mut out.mother_given_names,
            &mut out.mother_surname,
        );
        out
    }

    /// `pedigree` as the viewer sees it: absent persons and the links
    /// through them gone, placeholders blanked, withheld families' events
    /// left out, and every identifier the viewer may not follow replaced.
    pub fn pedigree(&self, pedigree: &mut Pedigree) {
        if !self.restricted {
            return;
        }
        let mut persons = HashMap::with_capacity(pedigree.persons.len());
        for (id, node) in pedigree.persons.drain() {
            if let Some(node) = self.node(node) {
                persons.insert(self.public_id(id).unwrap_or(id), node);
            }
        }
        pedigree.persons = persons;
        if let Some(root) = self.public_id(pedigree.root_person_id) {
            pedigree.root_person_id = root;
        }
        pedigree.edges.retain_mut(|edge| {
            match (
                self.public_id(edge.parent_id),
                self.public_id(edge.child_id),
            ) {
                (Some(parent), Some(child)) => {
                    edge.parent_id = parent;
                    edge.child_id = child;
                    true
                }
                _ => false,
            }
        });
        pedigree
            .family_events
            .retain(|family, _| !self.family_withheld(*family));
        if self.withhold_tags {
            for events in pedigree.family_events.values_mut() {
                for event in events {
                    event.tags.clear();
                }
            }
        }
        for family in pedigree.families.values_mut() {
            family.spouse_ids = family
                .spouse_ids
                .iter()
                .filter_map(|id| self.public_id(*id))
                .collect();
            family.children_ids = family
                .children_ids
                .iter()
                .filter_map(|id| self.public_id(*id))
                .collect();
            family.members = family
                .members
                .drain(..)
                .filter_map(|member| self.member(member))
                .collect();
        }
    }

    /// A pedigree card as the viewer sees it; `None` for an absent person.
    #[must_use]
    pub fn node(&self, node: PedigreeNode) -> Option<PedigreeNode> {
        if !self.restricted {
            return Some(node);
        }
        let appearance = self.person(node.person_id);
        let person_id = self.public_id(node.person_id)?;
        let mut node = node;
        if appearance == Appearance::Shown {
            if self.withhold_tags {
                node.tags.clear();
            }
            return Some(node);
        }
        let (given, surname) = if appearance == Appearance::Initials {
            initials_parts(node.given_names.as_deref(), node.surname.as_deref())
        } else {
            (None, None)
        };
        Some(PedigreeNode {
            person_id,
            sex: node.sex,
            display_name: display(given.as_deref(), surname.as_deref()).unwrap_or_default(),
            given_names: given,
            surname,
            birth: None,
            death: None,
            occupation: None,
            primary_media_path: None,
            generation: node.generation,
            sosa_number: None,
            portrait: None,
            sosa_ancestor: false,
            tags: Vec::new(),
        })
    }

    fn member(&self, member: PedigreeFamilyMember) -> Option<PedigreeFamilyMember> {
        let appearance = self.person(member.person_id);
        let person_id = self.public_id(member.person_id)?;
        let mut member = member;
        member.person_id = person_id;
        if appearance == Appearance::Shown {
            if self.withhold_tags {
                for event in member.birth.iter_mut().chain(member.death.iter_mut()) {
                    event.tags.clear();
                }
            }
            return Some(member);
        }
        let (given, surname) = if appearance == Appearance::Initials {
            initials_parts(member.given_names.as_deref(), member.surname.as_deref())
        } else {
            (None, None)
        };
        member.given_names = given;
        member.surname = surname;
        member.birth = None;
        member.death = None;
        Some(member)
    }
}

/// The first letter of `text`, capitalized, followed by a period.
fn initial(text: Option<&str>) -> Option<String> {
    let first = text?.trim().chars().next()?;
    Some(format!("{}.", first.to_uppercase()))
}

/// The initials of the first given name and of the surname.
fn initials_parts(given: Option<&str>, surname: Option<&str>) -> (Option<String>, Option<String>) {
    (
        initial(given.and_then(|g| g.split_whitespace().next())),
        initial(surname),
    )
}

fn display(given: Option<&str>, surname: Option<&str>) -> Option<String> {
    let parts: Vec<&str> = [given, surname].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// The name an `initials` placeholder carries (`J. D.`).
fn initials(name: Option<&ProfileName>) -> Option<ProfileName> {
    let name = name?;
    let (given, surname) = initials_parts(name.given_names.as_deref(), name.surname.as_deref());
    let display_name = display(given.as_deref(), surname.as_deref())?;
    Some(ProfileName {
        display_name,
        given_names: given,
        surname,
        ..name.clone()
    })
}

#[cfg(test)]
#[path = "redaction_tests.rs"]
mod tests;
