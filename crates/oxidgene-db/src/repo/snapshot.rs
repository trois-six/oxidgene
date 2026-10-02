//! Reading versioned records into snapshots, and writing a snapshot back.
//!
//! A person's snapshot is everything their profile shows but their media:
//! names, own events, notes, citations, the families they are a child of, and
//! the families they are a spouse in with those families' events, notes,
//! citations, spouses, and children. Places, sources, and the tree's settings
//! are snapshots of their own.
//!
//! Collections are sorted so that an unchanged record always builds an equal
//! snapshot: the history service compares a record's state before a write
//! with its state after it to decide whether the write changed it at all.
//!
//! Restoring writes a snapshot's state over the live rows, keeping every ID.
//! Rows the snapshot lacks are removed the way the API removes them — soft
//! deletion where the table has it — and rows it has come back, undeleted or
//! re-inserted. References to persons that no longer exist are dropped rather
//! than resurrected: restoring one person must not bring back another.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use oxidgene_core::collections::sorted_unique;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::history::{
    ChildLinkSnapshot, CitationSnapshot, EventSnapshot, NameSnapshot, NoteSnapshot, PersonSnapshot,
    PlaceSnapshot, RecordLabel, RecordSnapshot, RecordType, RepositorySnapshot,
    SourceRepositorySnapshot, SourceSnapshot, SpouseAgeSnapshot, SpouseLinkSnapshot, TreeSnapshot,
    UnionSnapshot, WitnessSnapshot,
};
use oxidgene_core::types::join_surname_particle;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::Expr;
use sea_orm::{ConnectionTrait, IntoActiveModel, QueryFilter, QueryOrder, QuerySelect, Set};
use uuid::Uuid;

use super::HistoryRepo;
use super::batch::in_chunks;
use crate::entities::{
    citation, event, event_spouse_age, event_witness, family, family_child, family_spouse, note,
    person, person_name, place, repository, source, source_repository, tree,
};
use crate::html::sanitize_note_html;
use crate::repo::NoteFilter;
use crate::repo::db_err;

/// A record's state as its rows hold it now, ready to compare and store.
#[derive(Debug, Clone)]
pub struct BuiltSnapshot {
    pub record_id: Uuid,
    /// The record is soft-deleted.
    pub deleted: bool,
    pub snapshot: RecordSnapshot,
    pub labels: Vec<RecordLabel>,
}

/// Snapshot building and restoring.
pub struct SnapshotRepo;

impl SnapshotRepo {
    /// Snapshot records of one type of the tree, deleted or not. IDs that do
    /// not exist are skipped; the tree's settings are its own ID's.
    pub async fn build(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        record_type: RecordType,
        ids: &[Uuid],
    ) -> Result<Vec<BuiltSnapshot>, OxidGeneError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        match record_type {
            RecordType::Person => Self::persons(db, tree_id, ids).await,
            RecordType::Place => Self::places(db, tree_id, ids).await,
            RecordType::Source => Self::sources(db, tree_id, ids).await,
            RecordType::Repository => Self::repositories(db, tree_id, ids).await,
            RecordType::Tree if ids.contains(&tree_id) => Self::tree(db, tree_id).await,
            RecordType::Tree => Ok(Vec::new()),
        }
    }

    async fn persons(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        ids: &[Uuid],
    ) -> Result<Vec<BuiltSnapshot>, OxidGeneError> {
        let persons: Vec<person::Model> = in_chunks(ids, |chunk| async move {
            person::Entity::find()
                .filter(person::Column::TreeId.eq(tree_id))
                .filter(person::Column::Id.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        if persons.is_empty() {
            return Ok(Vec::new());
        }
        let person_ids: Vec<Uuid> = persons.iter().map(|p| p.id).collect();
        let rows = PersonRows::load(db, &person_ids).await?;
        Ok(persons
            .into_iter()
            .map(|person| rows.snapshot(person))
            .collect())
    }

    async fn places(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        ids: &[Uuid],
    ) -> Result<Vec<BuiltSnapshot>, OxidGeneError> {
        let rows: Vec<place::Model> = in_chunks(ids, |chunk| async move {
            place::Entity::find()
                .filter(place::Column::TreeId.eq(tree_id))
                .filter(place::Column::Id.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| BuiltSnapshot {
                record_id: row.id,
                deleted: false,
                snapshot: RecordSnapshot::Place(PlaceSnapshot {
                    name: row.name,
                    latitude: row.latitude,
                    longitude: row.longitude,
                }),
                labels: Vec::new(),
            })
            .collect())
    }

    async fn sources(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        ids: &[Uuid],
    ) -> Result<Vec<BuiltSnapshot>, OxidGeneError> {
        let rows: Vec<source::Model> = in_chunks(ids, |chunk| async move {
            source::Entity::find()
                .filter(source::Column::TreeId.eq(tree_id))
                .filter(source::Column::Id.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
        let notes = in_chunks(&ids, |chunk| async move {
            note::Entity::find()
                .filter(note::Column::SourceId.is_in(chunk))
                .filter(note::Column::DeletedAt.is_null())
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        let notes_by_source = group(notes, |n| n.source_id.unwrap_or_default());
        // Every link, the deleted repositories' included: they are the
        // source's state, read while the repository is deleted.
        let links = in_chunks(&ids, |chunk| async move {
            source_repository::Entity::find()
                .filter(source_repository::Column::SourceId.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        let repository_ids = sorted_unique(links.iter().map(|l| l.repository_id));
        let repository_labels = repository_names(db, &repository_ids).await?;
        let links_by_source = group(links, |l| l.source_id);
        Ok(rows
            .into_iter()
            .map(|row| {
                let links = links_by_source.get(&row.id);
                let labels: HashMap<Uuid, String> = links
                    .into_iter()
                    .flatten()
                    .filter_map(|l| {
                        let label = repository_labels.get(&l.repository_id)?;
                        Some((l.repository_id, label.clone()))
                    })
                    .collect();
                BuiltSnapshot {
                    record_id: row.id,
                    deleted: row.deleted_at.is_some(),
                    snapshot: RecordSnapshot::Source(SourceSnapshot {
                        notes: sorted(
                            notes_by_source
                                .get(&row.id)
                                .into_iter()
                                .flatten()
                                .map(note_snapshot)
                                .collect(),
                            |n: &NoteSnapshot| n.id,
                        ),
                        repositories: sorted(
                            links
                                .into_iter()
                                .flatten()
                                .map(|l| SourceRepositorySnapshot {
                                    id: l.id,
                                    repository_id: l.repository_id,
                                    call_number: l.call_number.clone(),
                                    media_type: l.media_type.map(Into::into),
                                    sort_order: l.sort_order,
                                })
                                .collect(),
                            |l: &SourceRepositorySnapshot| (l.sort_order, l.id),
                        ),
                        title: row.title,
                        author: row.author,
                        publisher: row.publisher,
                        abbreviation: row.abbreviation,
                        agency: row.agency,
                    }),
                    labels: into_labels(labels),
                }
            })
            .collect())
    }

    async fn repositories(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        ids: &[Uuid],
    ) -> Result<Vec<BuiltSnapshot>, OxidGeneError> {
        let rows: Vec<repository::Model> = in_chunks(ids, |chunk| async move {
            repository::Entity::find()
                .filter(repository::Column::TreeId.eq(tree_id))
                .filter(repository::Column::Id.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
        let notes = in_chunks(&ids, |chunk| async move {
            note::Entity::find()
                .filter(note::Column::RepositoryId.is_in(chunk))
                .filter(note::Column::DeletedAt.is_null())
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        let notes_by_repository = group(notes, |n| n.repository_id.unwrap_or_default());
        Ok(rows
            .into_iter()
            .map(|row| BuiltSnapshot {
                record_id: row.id,
                deleted: row.deleted_at.is_some(),
                snapshot: RecordSnapshot::Repository(RepositorySnapshot {
                    notes: sorted(
                        notes_by_repository
                            .get(&row.id)
                            .into_iter()
                            .flatten()
                            .map(note_snapshot)
                            .collect(),
                        |n: &NoteSnapshot| n.id,
                    ),
                    name: row.name,
                    address: row.address,
                    phone: row.phone,
                    email: row.email,
                    website: row.website,
                }),
                labels: Vec::new(),
            })
            .collect())
    }

    async fn tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<BuiltSnapshot>, OxidGeneError> {
        let Some(row) = tree::Entity::find_by_id(tree_id)
            .one(db)
            .await
            .map_err(db_err)?
        else {
            return Ok(Vec::new());
        };
        let named: Vec<Uuid> = row
            .sosa_root_person_id
            .into_iter()
            .chain(row.self_person_id)
            .collect();
        let labels = into_labels(display_names(db, &named).await?);
        Ok(vec![BuiltSnapshot {
            record_id: row.id,
            deleted: row.deleted_at.is_some(),
            snapshot: RecordSnapshot::Tree(TreeSnapshot {
                name: row.name,
                description: row.description,
                default_privacy: row.default_privacy.into(),
                entry_suggestions: row.entry_suggestions,
                sosa_root_person_id: row.sosa_root_person_id,
                self_person_id: row.self_person_id,
            }),
            labels,
        }])
    }

    // ── Restoring ───────────────────────────────────────────────────────

    /// Write a person snapshot over the person's live rows, and return every
    /// person whose links the restore may have changed.
    pub async fn restore_person(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        person_id: Uuid,
        snapshot: &PersonSnapshot,
        labels: &[RecordLabel],
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        let existing = person::Entity::find_by_id(person_id)
            .filter(person::Column::TreeId.eq(tree_id))
            .one(db)
            .await
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound {
                entity: "Person",
                id: person_id,
            })?;
        let mut active = existing.into_active_model();
        active.sex = Set(snapshot.sex.into());
        active.privacy = Set(snapshot.privacy.into());
        active.deleted_at = Set(None);
        active.updated_at = Set(Utc::now());
        active.update(db).await.map_err(db_err)?;

        let mut touched: HashSet<Uuid> = HashSet::from([person_id]);
        let mut restorer = Restorer::new(db, tree_id, labels);
        restorer.names(person_id, &snapshot.names).await?;
        restorer
            .events(EventOwner::Person(person_id), &snapshot.events)
            .await?;
        restorer
            .notes(NoteOwner::Person(person_id), &snapshot.notes)
            .await?;
        restorer
            .citations(CitationOwner::Person(person_id), &snapshot.citations)
            .await?;
        restorer
            .parents(person_id, &snapshot.parents, &mut touched)
            .await?;
        restorer
            .unions(person_id, &snapshot.unions, &mut touched)
            .await?;

        let mut touched: Vec<Uuid> = touched.into_iter().collect();
        touched.sort();
        Ok(touched)
    }

    /// Write a place snapshot back, re-creating the place if it was deleted.
    pub async fn restore_place(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        place_id: Uuid,
        snapshot: &PlaceSnapshot,
    ) -> Result<(), OxidGeneError> {
        let now = Utc::now();
        match place::Entity::find_by_id(place_id)
            .filter(place::Column::TreeId.eq(tree_id))
            .one(db)
            .await
            .map_err(db_err)?
        {
            Some(existing) => {
                let mut active = existing.into_active_model();
                active.name = Set(snapshot.name.clone());
                active.latitude = Set(snapshot.latitude);
                active.longitude = Set(snapshot.longitude);
                active.updated_at = Set(now);
                active.update(db).await.map_err(db_err)?;
            }
            None => {
                place::ActiveModel {
                    id: Set(place_id),
                    tree_id: Set(tree_id),
                    name: Set(snapshot.name.clone()),
                    latitude: Set(snapshot.latitude),
                    longitude: Set(snapshot.longitude),
                    created_at: Set(now),
                    updated_at: Set(now),
                }
                .insert(db)
                .await
                .map_err(db_err)?;
            }
        }
        Ok(())
    }

    /// Write a source snapshot back, undeleting the source.
    pub async fn restore_source(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        source_id: Uuid,
        snapshot: &SourceSnapshot,
    ) -> Result<(), OxidGeneError> {
        let now = Utc::now();
        let existing = source::Entity::find_by_id(source_id)
            .filter(source::Column::TreeId.eq(tree_id))
            .one(db)
            .await
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound {
                entity: "Source",
                id: source_id,
            })?;
        let mut active = existing.into_active_model();
        active.title = Set(snapshot.title.clone());
        active.author = Set(snapshot.author.clone());
        active.publisher = Set(snapshot.publisher.clone());
        active.abbreviation = Set(snapshot.abbreviation.clone());
        active.agency = Set(snapshot.agency.clone());
        active.deleted_at = Set(None);
        active.updated_at = Set(now);
        active.update(db).await.map_err(db_err)?;
        let mut restorer = Restorer::new(db, tree_id, &[]);
        restorer
            .notes(NoteOwner::Source(source_id), &snapshot.notes)
            .await?;
        restorer
            .source_repositories(source_id, &snapshot.repositories)
            .await
    }

    /// Write a repository snapshot back, undeleting the repository.
    pub async fn restore_repository(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        repository_id: Uuid,
        snapshot: &RepositorySnapshot,
    ) -> Result<(), OxidGeneError> {
        let existing = repository::Entity::find_by_id(repository_id)
            .filter(repository::Column::TreeId.eq(tree_id))
            .one(db)
            .await
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound {
                entity: "Repository",
                id: repository_id,
            })?;
        let mut active = existing.into_active_model();
        active.name = Set(snapshot.name.clone());
        active.address = Set(snapshot.address.clone());
        active.phone = Set(snapshot.phone.clone());
        active.email = Set(snapshot.email.clone());
        active.website = Set(snapshot.website.clone());
        active.deleted_at = Set(None);
        active.updated_at = Set(Utc::now());
        active.update(db).await.map_err(db_err)?;
        Restorer::new(db, tree_id, &[])
            .notes(NoteOwner::Repository(repository_id), &snapshot.notes)
            .await
    }

    /// Write the tree's settings back. A root or "self" person who no longer
    /// exists is cleared rather than pointed at.
    pub async fn restore_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        snapshot: &TreeSnapshot,
    ) -> Result<(), OxidGeneError> {
        let existing = tree::Entity::find_by_id(tree_id)
            .filter(tree::Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound {
                entity: "Tree",
                id: tree_id,
            })?;
        let live = live_persons(
            db,
            &snapshot
                .sosa_root_person_id
                .into_iter()
                .chain(snapshot.self_person_id)
                .collect::<Vec<_>>(),
        )
        .await?;
        let mut active = existing.into_active_model();
        active.name = Set(snapshot.name.clone());
        active.description = Set(snapshot.description.clone());
        active.default_privacy = Set(snapshot.default_privacy.into());
        active.entry_suggestions = Set(snapshot.entry_suggestions);
        active.sosa_root_person_id =
            Set(snapshot.sosa_root_person_id.filter(|id| live.contains(id)));
        active.self_person_id = Set(snapshot.self_person_id.filter(|id| live.contains(id)));
        active.updated_at = Set(Utc::now());
        active.update(db).await.map_err(db_err)?;
        Ok(())
    }
}

// ── Building helpers ────────────────────────────────────────────────────

/// Everything a batch of person snapshots is assembled from: the rows each
/// person's profile shows, grouped by the record they belong to, and the
/// labels of every place, source, and person those rows name.
struct PersonRows {
    families: HashMap<Uuid, family::Model>,
    as_child_by_person: HashMap<Uuid, Vec<family_child::Model>>,
    as_spouse_by_person: HashMap<Uuid, Vec<family_spouse::Model>>,
    spouses_by_family: HashMap<Uuid, Vec<family_spouse::Model>>,
    children_by_family: HashMap<Uuid, Vec<family_child::Model>>,
    names_by_person: HashMap<Uuid, Vec<person_name::Model>>,
    events_by_person: HashMap<Uuid, Vec<event::Model>>,
    events_by_family: HashMap<Uuid, Vec<event::Model>>,
    witnesses_by_event: HashMap<Uuid, Vec<event_witness::Model>>,
    /// A family event's spouse ages, with the person each membership names.
    spouse_ages_by_event: HashMap<Uuid, Vec<(event_spouse_age::Model, Uuid)>>,
    notes: Attached<note::Model>,
    citations: Attached<citation::Model>,
    place_labels: HashMap<Uuid, String>,
    source_labels: HashMap<Uuid, String>,
    person_labels: HashMap<Uuid, String>,
}

impl PersonRows {
    /// Read the rows the snapshots of these persons are built from.
    async fn load(db: &impl ConnectionTrait, person_ids: &[Uuid]) -> Result<Self, OxidGeneError> {
        // Memberships, and the live families they lead to.
        let (as_child, as_spouse) = memberships(db, person_ids).await?;
        let family_ids = sorted_unique(
            as_child
                .iter()
                .map(|c| c.family_id)
                .chain(as_spouse.iter().map(|s| s.family_id)),
        );
        let families: HashMap<Uuid, family::Model> = live_families(db, &family_ids)
            .await?
            .into_iter()
            .map(|f| (f.id, f))
            .collect();
        let family_ids: Vec<Uuid> = families.keys().copied().collect();

        // Everything the families hold.
        let (family_spouses, family_children) = family_links(db, &family_ids).await?;

        let names = in_chunks(person_ids, |chunk| async move {
            person_name::Entity::find()
                .filter(person_name::Column::PersonId.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        let (person_events, family_events) = live_events(db, person_ids, &family_ids).await?;
        let event_ids: Vec<Uuid> = person_events
            .iter()
            .chain(family_events.iter())
            .map(|e| e.id)
            .collect();
        let witnesses = in_chunks(&event_ids, |chunk| async move {
            event_witness::Entity::find()
                .filter(event_witness::Column::EventId.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        let spouse_ages = spouse_ages(db, &family_events, &family_spouses).await?;
        let notes = Attached::notes(db, person_ids, &event_ids, &family_ids).await?;
        let citations = Attached::citations(db, person_ids, &event_ids, &family_ids).await?;

        // Labels: places, sources, and every person or family named by ID.
        let place_ids = sorted_unique(
            person_events
                .iter()
                .chain(family_events.iter())
                .filter_map(|e| e.place_id),
        );
        let source_ids = sorted_unique(citations.rows.iter().map(|c| c.source_id));
        let named = sorted_unique(
            witnesses
                .iter()
                .map(|w| w.person_id)
                .chain(family_spouses.iter().map(|s| s.person_id))
                .chain(family_children.iter().map(|c| c.person_id)),
        );
        let place_labels = place_names(db, &place_ids).await?;
        let source_labels = source_titles(db, &source_ids).await?;
        let person_labels = display_names(db, &named).await?;

        Ok(Self {
            families,
            as_child_by_person: group(as_child, |c| c.person_id),
            as_spouse_by_person: group(as_spouse, |s| s.person_id),
            spouses_by_family: group(family_spouses, |s| s.family_id),
            children_by_family: group(family_children, |c| c.family_id),
            names_by_person: group(names, |n| n.person_id),
            events_by_person: group(person_events, |e| e.person_id.unwrap_or_default()),
            events_by_family: group(family_events, |e| e.family_id.unwrap_or_default()),
            witnesses_by_event: group(witnesses, |w| w.event_id),
            spouse_ages_by_event: group(spouse_ages, |(a, _)| a.event_id),
            notes,
            citations,
            place_labels,
            source_labels,
            person_labels,
        })
    }

    /// A person's snapshot, with the labels of everything it names.
    fn snapshot(&self, person: person::Model) -> BuiltSnapshot {
        let mut labels: HashMap<Uuid, String> = HashMap::new();
        let names = sorted(
            self.names_by_person
                .get(&person.id)
                .into_iter()
                .flatten()
                .map(name_snapshot)
                .collect(),
            |n: &NameSnapshot| (!n.is_primary, n.sort_order, n.id),
        );
        let events = self.events(self.events_by_person.get(&person.id));
        let parents = sorted(
            self.as_child_by_person
                .get(&person.id)
                .into_iter()
                .flatten()
                .filter(|c| self.families.contains_key(&c.family_id))
                .map(child_snapshot)
                .collect(),
            |c: &ChildLinkSnapshot| (c.sort_order, c.id),
        );
        for parent in &parents {
            self.label_parent_family(parent.family_id, &mut labels);
        }
        let unions = sorted(
            self.as_spouse_by_person
                .get(&person.id)
                .into_iter()
                .flatten()
                .filter_map(|link| self.families.get(&link.family_id))
                .map(|family| self.union(family))
                .collect(),
            |u: &UnionSnapshot| u.family_id,
        );
        let snapshot = PersonSnapshot {
            sex: person.sex.into(),
            privacy: person.privacy.into(),
            names,
            events,
            notes: self.notes.of(|n| n.person_id == Some(person.id)),
            citations: self.citations.of(|c| c.person_id == Some(person.id)),
            parents,
            unions,
        };
        collect_person_labels(
            &snapshot,
            &mut labels,
            &self.place_labels,
            &self.source_labels,
            &self.person_labels,
        );
        BuiltSnapshot {
            record_id: person.id,
            deleted: person.deleted_at.is_some(),
            snapshot: RecordSnapshot::Person(snapshot),
            labels: into_labels(labels),
        }
    }

    /// Label a parent family with its spouses' names joined, and each spouse
    /// with their own.
    fn label_parent_family(&self, family_id: Uuid, labels: &mut HashMap<Uuid, String>) {
        let spouse_names: Vec<&str> = self
            .spouses_by_family
            .get(&family_id)
            .into_iter()
            .flatten()
            .filter_map(|s| self.person_labels.get(&s.person_id).map(String::as_str))
            .collect();
        if !spouse_names.is_empty() {
            labels.insert(family_id, spouse_names.join(" & "));
        }
        labels.extend(
            self.spouses_by_family
                .get(&family_id)
                .into_iter()
                .flatten()
                .filter_map(|s| {
                    self.person_labels
                        .get(&s.person_id)
                        .map(|name| (s.person_id, name.clone()))
                }),
        );
    }

    /// A family the person is a spouse in, with everything it holds.
    fn union(&self, family: &family::Model) -> UnionSnapshot {
        UnionSnapshot {
            family_id: family.id,
            privacy: family.privacy.into(),
            spouses: sorted(
                self.spouses_by_family
                    .get(&family.id)
                    .into_iter()
                    .flatten()
                    .map(spouse_snapshot)
                    .collect(),
                |s: &SpouseLinkSnapshot| (s.sort_order, s.id),
            ),
            children: sorted(
                self.children_by_family
                    .get(&family.id)
                    .into_iter()
                    .flatten()
                    .map(child_snapshot)
                    .collect(),
                |c: &ChildLinkSnapshot| (c.sort_order, c.id),
            ),
            events: self.events(self.events_by_family.get(&family.id)),
            notes: self.notes.of(|n| n.family_id == Some(family.id)),
            citations: self.citations.of(|c| c.family_id == Some(family.id)),
        }
    }

    /// The snapshots of some events, in ID order.
    fn events(&self, events: Option<&Vec<event::Model>>) -> Vec<EventSnapshot> {
        sorted(
            events
                .into_iter()
                .flatten()
                .map(|e| self.event(e))
                .collect(),
            |e: &EventSnapshot| e.id,
        )
    }

    /// An event's snapshot, with its witnesses, notes, and citations.
    fn event(&self, e: &event::Model) -> EventSnapshot {
        EventSnapshot {
            id: e.id,
            event_type: e.event_type.into(),
            date_value: e.date_value.clone(),
            date_sort: e.date_sort,
            date_qualifier: e.date_qualifier.into(),
            date_value2: e.date_value2.clone(),
            calendar: e.calendar.into(),
            cause: e.cause.clone(),
            age: e.age.clone(),
            agency: e.agency.clone(),
            place_id: e.place_id,
            description: e.description.clone(),
            witnesses: sorted(
                self.witnesses_by_event
                    .get(&e.id)
                    .into_iter()
                    .flatten()
                    .map(|w| WitnessSnapshot {
                        id: w.id,
                        person_id: w.person_id,
                        relation: w.relation.clone(),
                        sort_order: w.sort_order,
                    })
                    .collect(),
                |w| (w.sort_order, w.id),
            ),
            spouse_ages: sorted(
                self.spouse_ages_by_event
                    .get(&e.id)
                    .into_iter()
                    .flatten()
                    .map(|(a, person_id)| SpouseAgeSnapshot {
                        id: a.id,
                        family_spouse_id: a.family_spouse_id,
                        person_id: *person_id,
                        age: a.age.clone(),
                    })
                    .collect(),
                |a| a.id,
            ),
            notes: self.notes.of(|n| n.event_id == Some(e.id)),
            citations: self.citations.of(|c| c.event_id == Some(e.id)),
        }
    }
}

/// The spouse ages of `family_events`, each with the person its membership
/// (one of `family_spouses`) names.
async fn spouse_ages(
    db: &impl ConnectionTrait,
    family_events: &[event::Model],
    family_spouses: &[family_spouse::Model],
) -> Result<Vec<(event_spouse_age::Model, Uuid)>, OxidGeneError> {
    let event_ids: Vec<Uuid> = family_events.iter().map(|e| e.id).collect();
    let person_of: HashMap<Uuid, Uuid> =
        family_spouses.iter().map(|s| (s.id, s.person_id)).collect();
    let rows = in_chunks(&event_ids, |chunk| async move {
        event_spouse_age::Entity::find()
            .filter(event_spouse_age::Column::EventId.is_in(chunk))
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let person_id = *person_of.get(&row.family_spouse_id)?;
            Some((row, person_id))
        })
        .collect())
}

/// The links making these persons a child and a spouse, in that order.
async fn memberships(
    db: &impl ConnectionTrait,
    person_ids: &[Uuid],
) -> Result<(Vec<family_child::Model>, Vec<family_spouse::Model>), OxidGeneError> {
    let as_child = in_chunks(person_ids, |chunk| async move {
        family_child::Entity::find()
            .filter(family_child::Column::PersonId.is_in(chunk))
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?;
    let as_spouse = in_chunks(person_ids, |chunk| async move {
        family_spouse::Entity::find()
            .filter(family_spouse::Column::PersonId.is_in(chunk))
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?;
    Ok((as_child, as_spouse))
}

/// The spouse and child links of these families, in that order.
async fn family_links(
    db: &impl ConnectionTrait,
    family_ids: &[Uuid],
) -> Result<(Vec<family_spouse::Model>, Vec<family_child::Model>), OxidGeneError> {
    let spouses = in_chunks(family_ids, |chunk| async move {
        family_spouse::Entity::find()
            .filter(family_spouse::Column::FamilyId.is_in(chunk))
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?;
    let children = in_chunks(family_ids, |chunk| async move {
        family_child::Entity::find()
            .filter(family_child::Column::FamilyId.is_in(chunk))
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?;
    Ok((spouses, children))
}

/// The live events of these persons, then those of these families.
async fn live_events(
    db: &impl ConnectionTrait,
    person_ids: &[Uuid],
    family_ids: &[Uuid],
) -> Result<(Vec<event::Model>, Vec<event::Model>), OxidGeneError> {
    let person_events = in_chunks(person_ids, |chunk| async move {
        event::Entity::find()
            .filter(event::Column::PersonId.is_in(chunk))
            .filter(event::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?;
    let family_events = in_chunks(family_ids, |chunk| async move {
        event::Entity::find()
            .filter(event::Column::FamilyId.is_in(chunk))
            .filter(event::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?;
    Ok((person_events, family_events))
}

/// Notes and citations attached to persons, events, and families.
struct Attached<T> {
    rows: Vec<T>,
}

impl Attached<note::Model> {
    async fn notes(
        db: &impl ConnectionTrait,
        person_ids: &[Uuid],
        event_ids: &[Uuid],
        family_ids: &[Uuid],
    ) -> Result<Self, OxidGeneError> {
        let mut rows = Vec::new();
        for (column, ids) in [
            (note::Column::PersonId, person_ids),
            (note::Column::EventId, event_ids),
            (note::Column::FamilyId, family_ids),
        ] {
            rows.extend(
                in_chunks(ids, |chunk| async move {
                    note::Entity::find()
                        .filter(column.is_in(chunk))
                        .filter(note::Column::DeletedAt.is_null())
                        // A note about a media belongs to the media.
                        .filter(note::Column::MediaId.is_null())
                        .all(db)
                        .await
                        .map_err(db_err)
                })
                .await?,
            );
        }
        rows.sort_by_key(|n| n.id);
        rows.dedup_by_key(|n| n.id);
        Ok(Self { rows })
    }

    fn of(&self, belongs: impl Fn(&note::Model) -> bool) -> Vec<NoteSnapshot> {
        self.rows
            .iter()
            .filter(|n| belongs(n))
            .map(note_snapshot)
            .collect()
    }
}

impl Attached<citation::Model> {
    async fn citations(
        db: &impl ConnectionTrait,
        person_ids: &[Uuid],
        event_ids: &[Uuid],
        family_ids: &[Uuid],
    ) -> Result<Self, OxidGeneError> {
        let mut rows = Vec::new();
        for (column, ids) in [
            (citation::Column::PersonId, person_ids),
            (citation::Column::EventId, event_ids),
            (citation::Column::FamilyId, family_ids),
        ] {
            rows.extend(
                in_chunks(ids, |chunk| async move {
                    citation::Entity::find()
                        .filter(column.is_in(chunk))
                        .all(db)
                        .await
                        .map_err(db_err)
                })
                .await?,
            );
        }
        rows.sort_by_key(|c| c.id);
        rows.dedup_by_key(|c| c.id);
        Ok(Self { rows })
    }

    fn of(&self, belongs: impl Fn(&citation::Model) -> bool) -> Vec<CitationSnapshot> {
        self.rows
            .iter()
            .filter(|c| belongs(c))
            .map(|c| CitationSnapshot {
                id: c.id,
                source_id: c.source_id,
                page: c.page.clone(),
                confidence: c.confidence.map(Into::into),
                text: c.text.clone(),
            })
            .collect()
    }
}

fn name_snapshot(n: &person_name::Model) -> NameSnapshot {
    NameSnapshot {
        id: n.id,
        name_type: n.name_type.into(),
        given_names: n.given_names.clone(),
        surname: n.surname.clone(),
        surname_prefix: n.surname_prefix.clone(),
        prefix: n.prefix.clone(),
        suffix: n.suffix.clone(),
        nickname: n.nickname.clone(),
        is_primary: n.is_primary,
        sort_order: n.sort_order,
    }
}

fn note_snapshot(n: &note::Model) -> NoteSnapshot {
    NoteSnapshot {
        id: n.id,
        text: n.text.clone(),
    }
}

fn child_snapshot(c: &family_child::Model) -> ChildLinkSnapshot {
    ChildLinkSnapshot {
        id: c.id,
        family_id: c.family_id,
        person_id: c.person_id,
        child_type: c.child_type.into(),
        sort_order: c.sort_order,
    }
}

fn spouse_snapshot(s: &family_spouse::Model) -> SpouseLinkSnapshot {
    SpouseLinkSnapshot {
        id: s.id,
        person_id: s.person_id,
        role: s.role.into(),
        sort_order: s.sort_order,
    }
}

/// Record the labels of every place, source, and person a snapshot names.
fn collect_person_labels(
    snapshot: &PersonSnapshot,
    labels: &mut HashMap<Uuid, String>,
    places: &HashMap<Uuid, String>,
    sources: &HashMap<Uuid, String>,
    persons: &HashMap<Uuid, String>,
) {
    let mut add = |id: Uuid, from: &HashMap<Uuid, String>| {
        if let Some(label) = from.get(&id) {
            labels.insert(id, label.clone());
        }
    };
    let mut events: Vec<&EventSnapshot> = snapshot.events.iter().collect();
    let mut citations: Vec<&CitationSnapshot> = snapshot.citations.iter().collect();
    for union in &snapshot.unions {
        events.extend(&union.events);
        citations.extend(&union.citations);
        for spouse in &union.spouses {
            add(spouse.person_id, persons);
        }
        for child in &union.children {
            add(child.person_id, persons);
        }
    }
    for event in &events {
        citations.extend(&event.citations);
        if let Some(place_id) = event.place_id {
            add(place_id, places);
        }
        for witness in &event.witnesses {
            add(witness.person_id, persons);
        }
    }
    for citation in citations {
        add(citation.source_id, sources);
    }
}

fn into_labels(labels: HashMap<Uuid, String>) -> Vec<RecordLabel> {
    let mut labels: Vec<RecordLabel> = labels
        .into_iter()
        .map(|(id, label)| RecordLabel { id, label })
        .collect();
    labels.sort_by_key(|label| label.id);
    labels
}

fn group<T, K: std::hash::Hash + Eq>(rows: Vec<T>, key: impl Fn(&T) -> K) -> HashMap<K, Vec<T>> {
    let mut grouped: HashMap<K, Vec<T>> = HashMap::new();
    for row in rows {
        grouped.entry(key(&row)).or_default().push(row);
    }
    grouped
}

fn sorted<T, K: Ord>(mut items: Vec<T>, key: impl Fn(&T) -> K) -> Vec<T> {
    items.sort_by_key(|item| key(item));
    items
}

async fn live_families(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
) -> Result<Vec<family::Model>, OxidGeneError> {
    in_chunks(ids, |chunk| async move {
        family::Entity::find()
            .filter(family::Column::Id.is_in(chunk))
            .filter(family::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(db_err)
    })
    .await
}

async fn live_persons(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
) -> Result<HashSet<Uuid>, OxidGeneError> {
    Ok(in_chunks(ids, |chunk| async move {
        person::Entity::find()
            .select_only()
            .column(person::Column::Id)
            .filter(person::Column::Id.is_in(chunk))
            .filter(person::Column::DeletedAt.is_null())
            .into_tuple::<Uuid>()
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?
    .into_iter()
    .collect())
}

async fn repository_names(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, OxidGeneError> {
    Ok(in_chunks(ids, |chunk| async move {
        repository::Entity::find()
            .select_only()
            .column(repository::Column::Id)
            .column(repository::Column::Name)
            .filter(repository::Column::Id.is_in(chunk))
            .into_tuple::<(Uuid, String)>()
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?
    .into_iter()
    .collect())
}

async fn place_names(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, OxidGeneError> {
    Ok(in_chunks(ids, |chunk| async move {
        place::Entity::find()
            .select_only()
            .column(place::Column::Id)
            .column(place::Column::Name)
            .filter(place::Column::Id.is_in(chunk))
            .into_tuple::<(Uuid, String)>()
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?
    .into_iter()
    .collect())
}

async fn source_titles(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, OxidGeneError> {
    Ok(in_chunks(ids, |chunk| async move {
        source::Entity::find()
            .select_only()
            .column(source::Column::Id)
            .column(source::Column::Title)
            .filter(source::Column::Id.is_in(chunk))
            .into_tuple::<(Uuid, String)>()
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?
    .into_iter()
    .collect())
}

/// The display name of each person, from their primary name — or their first
/// name when none is marked primary. A person without any name has none.
pub async fn display_names(
    db: &impl ConnectionTrait,
    person_ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, OxidGeneError> {
    let mut names = in_chunks(person_ids, |chunk| async move {
        person_name::Entity::find()
            .filter(person_name::Column::PersonId.is_in(chunk))
            .order_by_asc(person_name::Column::SortOrder)
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?;
    names.sort_by_key(|n| (n.person_id, !n.is_primary, n.sort_order, n.id));
    let mut labels = HashMap::new();
    for name in names {
        labels.entry(name.person_id).or_insert_with(|| {
            [
                name.prefix.clone(),
                name.given_names.clone(),
                name.surname
                    .as_deref()
                    .map(|root| join_surname_particle(name.surname_prefix.as_deref(), root)),
                name.suffix.clone(),
            ]
            .into_iter()
            .flatten()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
        });
    }
    labels.retain(|_, label| !label.is_empty());
    Ok(labels)
}

/// Every spouse and child of a family.
async fn family_members(
    db: &impl ConnectionTrait,
    family_id: Uuid,
) -> Result<Vec<Uuid>, OxidGeneError> {
    let spouses = family_spouse::Entity::find()
        .select_only()
        .column(family_spouse::Column::PersonId)
        .filter(family_spouse::Column::FamilyId.eq(family_id))
        .into_tuple::<Uuid>()
        .all(db)
        .await
        .map_err(db_err)?;
    let children = family_child::Entity::find()
        .select_only()
        .column(family_child::Column::PersonId)
        .filter(family_child::Column::FamilyId.eq(family_id))
        .into_tuple::<Uuid>()
        .all(db)
        .await
        .map_err(db_err)?;
    Ok(spouses.into_iter().chain(children).collect())
}

// ── Restoring helpers ───────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum EventOwner {
    Person(Uuid),
    Family(Uuid),
}

#[derive(Clone, Copy)]
enum NoteOwner {
    Person(Uuid),
    Event(Uuid),
    Family(Uuid),
    Source(Uuid),
    Repository(Uuid),
}

#[derive(Clone, Copy)]
enum CitationOwner {
    Person(Uuid),
    Event(Uuid),
    Family(Uuid),
}

/// Writes snapshot rows back for one restore, remembering which places,
/// persons, and families it has already checked.
struct Restorer<'a, C> {
    db: &'a C,
    tree_id: Uuid,
    labels: &'a [RecordLabel],
    places: HashMap<Uuid, bool>,
    persons: HashMap<Uuid, bool>,
    sources: HashMap<Uuid, bool>,
}

impl<'a, C: ConnectionTrait> Restorer<'a, C> {
    fn new(db: &'a C, tree_id: Uuid, labels: &'a [RecordLabel]) -> Self {
        Self {
            db,
            tree_id,
            labels,
            places: HashMap::new(),
            persons: HashMap::new(),
            sources: HashMap::new(),
        }
    }

    /// Replace a person's names with the snapshot's. Names have no soft
    /// deletion, so the ones it lacks are removed.
    async fn names(
        &mut self,
        person_id: Uuid,
        names: &[NameSnapshot],
    ) -> Result<(), OxidGeneError> {
        let wanted: HashSet<Uuid> = names.iter().map(|n| n.id).collect();
        person_name::Entity::delete_many()
            .filter(person_name::Column::PersonId.eq(person_id))
            .filter(person_name::Column::Id.is_not_in(wanted))
            .exec(self.db)
            .await
            .map_err(db_err)?;
        for name in names {
            self.name(person_id, name).await?;
        }
        Ok(())
    }

    async fn name(&mut self, person_id: Uuid, name: &NameSnapshot) -> Result<(), OxidGeneError> {
        let now = Utc::now();
        let row = person_name::ActiveModel {
            id: Set(name.id),
            person_id: Set(person_id),
            name_type: Set(name.name_type.into()),
            given_names: Set(name.given_names.clone()),
            surname: Set(name.surname.clone()),
            surname_prefix: Set(name.surname_prefix.clone()),
            prefix: Set(name.prefix.clone()),
            suffix: Set(name.suffix.clone()),
            nickname: Set(name.nickname.clone()),
            is_primary: Set(name.is_primary),
            sort_order: Set(name.sort_order),
            created_at: Set(now),
            updated_at: Set(now),
        };
        upsert(
            self.db,
            row,
            person_name::Entity::find_by_id(name.id),
            &[person_name::Column::CreatedAt],
        )
        .await
    }

    async fn events(
        &mut self,
        owner: EventOwner,
        events: &[EventSnapshot],
    ) -> Result<(), OxidGeneError> {
        let now = Utc::now();
        let owner_filter = match owner {
            EventOwner::Person(id) => event::Column::PersonId.eq(id),
            EventOwner::Family(id) => event::Column::FamilyId.eq(id),
        };
        let wanted: Vec<Uuid> = events.iter().map(|e| e.id).collect();
        event::Entity::update_many()
            .col_expr(event::Column::DeletedAt, Expr::value(Some(now)))
            .col_expr(event::Column::UpdatedAt, Expr::value(now))
            .filter(owner_filter)
            .filter(event::Column::DeletedAt.is_null())
            .filter(event::Column::Id.is_not_in(wanted))
            .exec(self.db)
            .await
            .map_err(db_err)?;

        for snapshot in events {
            let place_id = match snapshot.place_id {
                Some(place_id) if self.ensure_place(place_id).await? => Some(place_id),
                _ => None,
            };
            let (person_id, family_id) = match owner {
                EventOwner::Person(id) => (Some(id), None),
                EventOwner::Family(id) => (None, Some(id)),
            };
            let row = event::ActiveModel {
                id: Set(snapshot.id),
                tree_id: Set(self.tree_id),
                event_type: Set(snapshot.event_type.into()),
                date_value: Set(snapshot.date_value.clone()),
                date_sort: Set(snapshot.date_sort),
                date_qualifier: Set(snapshot.date_qualifier.into()),
                date_value2: Set(snapshot.date_value2.clone()),
                calendar: Set(snapshot.calendar.into()),
                cause: Set(snapshot.cause.clone()),
                age: Set(snapshot.age.clone()),
                agency: Set(snapshot.agency.clone()),
                place_id: Set(place_id),
                person_id: Set(person_id),
                family_id: Set(family_id),
                description: Set(snapshot.description.clone()),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
            };
            upsert(
                self.db,
                row,
                event::Entity::find_by_id(snapshot.id),
                &[event::Column::CreatedAt],
            )
            .await?;
            self.witnesses(snapshot.id, &snapshot.witnesses).await?;
            self.spouse_ages(snapshot.id, &snapshot.spouse_ages).await?;
            self.notes(NoteOwner::Event(snapshot.id), &snapshot.notes)
                .await?;
            self.citations(CitationOwner::Event(snapshot.id), &snapshot.citations)
                .await?;
        }
        Ok(())
    }

    async fn witnesses(
        &mut self,
        event_id: Uuid,
        witnesses: &[WitnessSnapshot],
    ) -> Result<(), OxidGeneError> {
        let wanted: Vec<Uuid> = witnesses.iter().map(|w| w.id).collect();
        event_witness::Entity::delete_many()
            .filter(event_witness::Column::EventId.eq(event_id))
            .filter(event_witness::Column::Id.is_not_in(wanted))
            .exec(self.db)
            .await
            .map_err(db_err)?;
        for witness in witnesses {
            if !self.person_is_live(witness.person_id).await? {
                continue;
            }
            let row = event_witness::ActiveModel {
                id: Set(witness.id),
                event_id: Set(event_id),
                person_id: Set(witness.person_id),
                relation: Set(witness.relation.clone()),
                sort_order: Set(witness.sort_order),
            };
            upsert(
                self.db,
                row,
                event_witness::Entity::find_by_id(witness.id),
                &[],
            )
            .await?;
        }
        Ok(())
    }

    /// Replace an event's spouse ages with the snapshot's, skipping those
    /// whose spouse is no longer a member of the family.
    async fn spouse_ages(
        &mut self,
        event_id: Uuid,
        ages: &[SpouseAgeSnapshot],
    ) -> Result<(), OxidGeneError> {
        let wanted: Vec<Uuid> = ages.iter().map(|a| a.id).collect();
        event_spouse_age::Entity::delete_many()
            .filter(event_spouse_age::Column::EventId.eq(event_id))
            .filter(event_spouse_age::Column::Id.is_not_in(wanted))
            .exec(self.db)
            .await
            .map_err(db_err)?;
        for age in ages {
            let member = family_spouse::Entity::find_by_id(age.family_spouse_id)
                .one(self.db)
                .await
                .map_err(db_err)?;
            if member.is_none() {
                continue;
            }
            let row = event_spouse_age::ActiveModel {
                id: Set(age.id),
                event_id: Set(event_id),
                family_spouse_id: Set(age.family_spouse_id),
                age: Set(age.age.clone()),
            };
            upsert(
                self.db,
                row,
                event_spouse_age::Entity::find_by_id(age.id),
                &[],
            )
            .await?;
        }
        Ok(())
    }

    async fn notes(
        &mut self,
        owner: NoteOwner,
        notes: &[NoteSnapshot],
    ) -> Result<(), OxidGeneError> {
        let now = Utc::now();
        let mut target = NoteFilter::default();
        let owner_filter = match owner {
            NoteOwner::Person(id) => {
                target.person_id = Some(id);
                note::Column::PersonId.eq(id)
            }
            NoteOwner::Event(id) => {
                target.event_id = Some(id);
                note::Column::EventId.eq(id)
            }
            NoteOwner::Family(id) => {
                target.family_id = Some(id);
                note::Column::FamilyId.eq(id)
            }
            NoteOwner::Source(id) => {
                target.source_id = Some(id);
                note::Column::SourceId.eq(id)
            }
            NoteOwner::Repository(id) => {
                target.repository_id = Some(id);
                note::Column::RepositoryId.eq(id)
            }
        };
        let wanted: Vec<Uuid> = notes.iter().map(|n| n.id).collect();
        note::Entity::update_many()
            .col_expr(note::Column::DeletedAt, Expr::value(Some(now)))
            .col_expr(note::Column::UpdatedAt, Expr::value(now))
            .filter(owner_filter)
            .filter(note::Column::MediaId.is_null())
            .filter(note::Column::DeletedAt.is_null())
            .filter(note::Column::Id.is_not_in(wanted))
            .exec(self.db)
            .await
            .map_err(db_err)?;
        for snapshot in notes {
            let row = note::ActiveModel {
                id: Set(snapshot.id),
                tree_id: Set(self.tree_id),
                // A version may predate a sanitizer rule, and the history
                // table is not a trusted source either: a restore is a note
                // write like any other and goes through the same filter.
                text: Set(sanitize_note_html(&snapshot.text)),
                person_id: Set(target.person_id),
                event_id: Set(target.event_id),
                family_id: Set(target.family_id),
                source_id: Set(target.source_id),
                media_id: Set(None),
                repository_id: Set(target.repository_id),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
            };
            upsert(
                self.db,
                row,
                note::Entity::find_by_id(snapshot.id),
                &[note::Column::CreatedAt],
            )
            .await?;
        }
        Ok(())
    }

    /// Replace a source's repository links with the snapshot's, skipping
    /// those whose repository no longer exists at all.
    async fn source_repositories(
        &mut self,
        source_id: Uuid,
        links: &[SourceRepositorySnapshot],
    ) -> Result<(), OxidGeneError> {
        let wanted: Vec<Uuid> = links.iter().map(|l| l.id).collect();
        source_repository::Entity::delete_many()
            .filter(source_repository::Column::SourceId.eq(source_id))
            .filter(source_repository::Column::Id.is_not_in(wanted))
            .exec(self.db)
            .await
            .map_err(db_err)?;
        for link in links {
            let exists = repository::Entity::find_by_id(link.repository_id)
                .filter(repository::Column::TreeId.eq(self.tree_id))
                .one(self.db)
                .await
                .map_err(db_err)?
                .is_some();
            if !exists {
                continue;
            }
            let row = source_repository::ActiveModel {
                id: Set(link.id),
                source_id: Set(source_id),
                repository_id: Set(link.repository_id),
                call_number: Set(link.call_number.clone()),
                media_type: Set(link.media_type.map(Into::into)),
                sort_order: Set(link.sort_order),
            };
            upsert(
                self.db,
                row,
                source_repository::Entity::find_by_id(link.id),
                &[],
            )
            .await?;
        }
        Ok(())
    }

    async fn citations(
        &mut self,
        owner: CitationOwner,
        citations: &[CitationSnapshot],
    ) -> Result<(), OxidGeneError> {
        let now = Utc::now();
        let (person_id, event_id, family_id) = match owner {
            CitationOwner::Person(id) => (Some(id), None, None),
            CitationOwner::Event(id) => (None, Some(id), None),
            CitationOwner::Family(id) => (None, None, Some(id)),
        };
        let owner_filter = match owner {
            CitationOwner::Person(id) => citation::Column::PersonId.eq(id),
            CitationOwner::Event(id) => citation::Column::EventId.eq(id),
            CitationOwner::Family(id) => citation::Column::FamilyId.eq(id),
        };
        let wanted: Vec<Uuid> = citations.iter().map(|c| c.id).collect();
        citation::Entity::delete_many()
            .filter(owner_filter)
            .filter(citation::Column::Id.is_not_in(wanted))
            .exec(self.db)
            .await
            .map_err(db_err)?;
        for snapshot in citations {
            if !self.ensure_source(snapshot.source_id).await? {
                continue;
            }
            let row = citation::ActiveModel {
                id: Set(snapshot.id),
                source_id: Set(snapshot.source_id),
                person_id: Set(person_id),
                event_id: Set(event_id),
                family_id: Set(family_id),
                page: Set(snapshot.page.clone()),
                confidence: Set(snapshot.confidence.map(Into::into)),
                text: Set(snapshot.text.clone()),
                created_at: Set(now),
                updated_at: Set(now),
            };
            upsert(
                self.db,
                row,
                citation::Entity::find_by_id(snapshot.id),
                &[citation::Column::CreatedAt],
            )
            .await?;
        }
        Ok(())
    }

    async fn child_link(&mut self, link: &ChildLinkSnapshot) -> Result<(), OxidGeneError> {
        let row = family_child::ActiveModel {
            id: Set(link.id),
            family_id: Set(link.family_id),
            person_id: Set(link.person_id),
            child_type: Set(link.child_type.into()),
            sort_order: Set(link.sort_order),
        };
        upsert(self.db, row, family_child::Entity::find_by_id(link.id), &[]).await
    }

    /// Restore the links making a person a child, to the families still
    /// there, and remove the links the snapshot lacks. The members of every
    /// family whose links change are added to `touched`.
    async fn parents(
        &mut self,
        person_id: Uuid,
        parents: &[ChildLinkSnapshot],
        touched: &mut HashSet<Uuid>,
    ) -> Result<(), OxidGeneError> {
        let current_parents = family_child::Entity::find()
            .filter(family_child::Column::PersonId.eq(person_id))
            .all(self.db)
            .await
            .map_err(db_err)?;
        let wanted: HashSet<Uuid> = parents.iter().map(|c| c.id).collect();
        for link in current_parents.iter().filter(|c| !wanted.contains(&c.id)) {
            touched.extend(family_members(self.db, link.family_id).await?);
            family_child::Entity::delete_by_id(link.id)
                .exec(self.db)
                .await
                .map_err(db_err)?;
        }
        for link in parents {
            if self.family_is_live(link.family_id).await? {
                self.child_link(link).await?;
                touched.extend(family_members(self.db, link.family_id).await?);
            }
        }
        Ok(())
    }

    /// Leave the families the snapshot lacks, and restore the others. The
    /// members of every family whose links change are added to `touched`.
    async fn unions(
        &mut self,
        person_id: Uuid,
        unions: &[UnionSnapshot],
        touched: &mut HashSet<Uuid>,
    ) -> Result<(), OxidGeneError> {
        let current_unions = family_spouse::Entity::find()
            .filter(family_spouse::Column::PersonId.eq(person_id))
            .all(self.db)
            .await
            .map_err(db_err)?;
        let kept: HashSet<Uuid> = unions.iter().map(|u| u.family_id).collect();
        for link in current_unions
            .iter()
            .filter(|s| !kept.contains(&s.family_id))
        {
            touched.extend(family_members(self.db, link.family_id).await?);
            family_spouse::Entity::delete_by_id(link.id)
                .exec(self.db)
                .await
                .map_err(db_err)?;
        }
        for union in unions {
            touched.extend(family_members(self.db, union.family_id).await?);
            self.union(union).await?;
            touched.extend(family_members(self.db, union.family_id).await?);
        }
        Ok(())
    }

    async fn union(&mut self, union: &UnionSnapshot) -> Result<(), OxidGeneError> {
        self.family(union).await?;
        self.spouses(union.family_id, &union.spouses).await?;
        self.children(union.family_id, &union.children).await?;
        self.events(EventOwner::Family(union.family_id), &union.events)
            .await?;
        self.notes(NoteOwner::Family(union.family_id), &union.notes)
            .await?;
        self.citations(CitationOwner::Family(union.family_id), &union.citations)
            .await
    }

    /// Undelete a union's family with the snapshot's privacy, re-creating it
    /// if it is gone.
    async fn family(&mut self, union: &UnionSnapshot) -> Result<(), OxidGeneError> {
        let now = Utc::now();
        match family::Entity::find_by_id(union.family_id)
            .filter(family::Column::TreeId.eq(self.tree_id))
            .one(self.db)
            .await
            .map_err(db_err)?
        {
            Some(existing) => {
                let mut active = existing.into_active_model();
                active.privacy = Set(union.privacy.into());
                active.deleted_at = Set(None);
                active.updated_at = Set(now);
                active.update(self.db).await.map_err(db_err)?;
            }
            None => {
                family::ActiveModel {
                    id: Set(union.family_id),
                    tree_id: Set(self.tree_id),
                    privacy: Set(union.privacy.into()),
                    created_at: Set(now),
                    updated_at: Set(now),
                    deleted_at: Set(None),
                }
                .insert(self.db)
                .await
                .map_err(db_err)?;
            }
        }
        Ok(())
    }

    /// Replace a family's spouse links with the snapshot's, skipping persons
    /// who no longer exist.
    async fn spouses(
        &mut self,
        family_id: Uuid,
        spouses: &[SpouseLinkSnapshot],
    ) -> Result<(), OxidGeneError> {
        let wanted: Vec<Uuid> = spouses.iter().map(|s| s.id).collect();
        family_spouse::Entity::delete_many()
            .filter(family_spouse::Column::FamilyId.eq(family_id))
            .filter(family_spouse::Column::Id.is_not_in(wanted))
            .exec(self.db)
            .await
            .map_err(db_err)?;
        for spouse in spouses {
            if !self.person_is_live(spouse.person_id).await? {
                continue;
            }
            let row = family_spouse::ActiveModel {
                id: Set(spouse.id),
                family_id: Set(family_id),
                person_id: Set(spouse.person_id),
                role: Set(spouse.role.into()),
                sort_order: Set(spouse.sort_order),
            };
            upsert(
                self.db,
                row,
                family_spouse::Entity::find_by_id(spouse.id),
                &[],
            )
            .await?;
        }
        Ok(())
    }

    /// Replace a family's child links with the snapshot's, skipping persons
    /// who no longer exist.
    async fn children(
        &mut self,
        family_id: Uuid,
        children: &[ChildLinkSnapshot],
    ) -> Result<(), OxidGeneError> {
        let wanted: Vec<Uuid> = children.iter().map(|c| c.id).collect();
        family_child::Entity::delete_many()
            .filter(family_child::Column::FamilyId.eq(family_id))
            .filter(family_child::Column::Id.is_not_in(wanted))
            .exec(self.db)
            .await
            .map_err(db_err)?;
        for child in children {
            if self.person_is_live(child.person_id).await? {
                self.child_link(child).await?;
            }
        }
        Ok(())
    }

    async fn family_is_live(&mut self, family_id: Uuid) -> Result<bool, OxidGeneError> {
        Ok(!live_families(self.db, &[family_id]).await?.is_empty())
    }

    async fn person_is_live(&mut self, person_id: Uuid) -> Result<bool, OxidGeneError> {
        if let Some(live) = self.persons.get(&person_id) {
            return Ok(*live);
        }
        let live = live_persons(self.db, &[person_id])
            .await?
            .contains(&person_id);
        self.persons.insert(person_id, live);
        Ok(live)
    }

    /// Make sure an event's place exists, re-creating a deleted one from the
    /// last state its history stored, or from the label recorded with the
    /// snapshot.
    async fn ensure_place(&mut self, place_id: Uuid) -> Result<bool, OxidGeneError> {
        if let Some(present) = self.places.get(&place_id) {
            return Ok(*present);
        }
        let exists = place::Entity::find_by_id(place_id)
            .one(self.db)
            .await
            .map_err(db_err)?
            .is_some();
        let present = if exists {
            true
        } else {
            let latest = HistoryRepo::last_stored_snapshot(self.db, RecordType::Place, place_id)
                .await?
                .and_then(|latest| match latest {
                    RecordSnapshot::Place(place) => Some(place),
                    _ => None,
                });
            let restored = latest.or_else(|| {
                self.labels
                    .iter()
                    .find(|label| label.id == place_id)
                    .map(|label| PlaceSnapshot {
                        name: label.label.clone(),
                        latitude: None,
                        longitude: None,
                    })
            });
            match restored {
                Some(place) => {
                    SnapshotRepo::restore_place(self.db, self.tree_id, place_id, &place).await?;
                    true
                }
                None => false,
            }
        };
        self.places.insert(place_id, present);
        Ok(present)
    }

    /// Make sure a cited source exists, undeleting it if it was deleted.
    async fn ensure_source(&mut self, source_id: Uuid) -> Result<bool, OxidGeneError> {
        if let Some(present) = self.sources.get(&source_id) {
            return Ok(*present);
        }
        let present = match source::Entity::find_by_id(source_id)
            .filter(source::Column::TreeId.eq(self.tree_id))
            .one(self.db)
            .await
            .map_err(db_err)?
        {
            Some(existing) if existing.deleted_at.is_some() => {
                let mut active = existing.into_active_model();
                active.deleted_at = Set(None);
                active.updated_at = Set(Utc::now());
                active.update(self.db).await.map_err(db_err)?;
                true
            }
            Some(_) => true,
            None => false,
        };
        self.sources.insert(source_id, present);
        Ok(present)
    }
}

/// Update a row that exists, keeping its stored value of the `kept`
/// columns; insert it otherwise.
async fn upsert<E, A>(
    db: &impl ConnectionTrait,
    mut row: A,
    existing: sea_orm::Select<E>,
    kept: &[E::Column],
) -> Result<(), OxidGeneError>
where
    E: EntityTrait,
    A: ActiveModelTrait<Entity = E> + ActiveModelBehavior + Send,
    E::Model: IntoActiveModel<A>,
{
    if let Some(existing) = existing.one(db).await.map_err(db_err)? {
        for column in kept {
            row.set(*column, existing.get(*column));
        }
        row.update(db).await.map_err(db_err)?;
    } else {
        row.insert(db).await.map_err(db_err)?;
    }
    Ok(())
}
