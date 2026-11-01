//! The dictionary as a restricted viewer sees it (docs/privacy.md §3.2,
//! §7): values and counts from the records their [`Redaction`] shows.
//!
//! Each `*_for` read answers what its unrestricted twin answers when the
//! redaction is the owner's, with the same statements. For anyone else it
//! reads, once, the links that decide what a use is worth ([`Sight`]): an
//! event or a citation counts when the person or the family it belongs to
//! is visible, a medium when the redaction does not withhold it; a source
//! cited only by hidden records, and a place only hidden events and media
//! use, are left out altogether, and their tags go to a viewer without
//! `read_tags`.

use oxidgene_core::enums::Privacy;

use super::*;

/// Who an event, a citation or a media link belongs to: a person, a family,
/// or an event of either.
type Owner = (Option<Uuid>, Option<Uuid>);

/// A row naming a record and three optional others: an event's place,
/// person and family; a link's person, family and event; a citation's.
type Row = (Uuid, Option<Uuid>, Option<Uuid>, Option<Uuid>);

/// The links of a tree's records that decide what a restricted viewer may
/// count.
struct Sight<'a> {
    redaction: &'a Redaction,
    /// Each live event: its place and its owner.
    events: HashMap<Uuid, (Option<Uuid>, Owner)>,
    /// Each live medium with a place, and the place.
    placed_media: Vec<(Uuid, Uuid)>,
    /// The live media the redaction withholds, pages and documents together.
    withheld_media: HashSet<Uuid>,
}

impl<'a> Sight<'a> {
    /// The links of tree `tree_id`: four grouped reads, its events, media,
    /// media links and crops.
    async fn load(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        redaction: &'a Redaction,
    ) -> Result<Self, OxidGeneError> {
        let rows: Vec<Row> = event::Entity::find()
            .select_only()
            .column(event::Column::Id)
            .column(event::Column::PlaceId)
            .column(event::Column::PersonId)
            .column(event::Column::FamilyId)
            .filter(event::Column::TreeId.eq(tree_id))
            .filter(event::Column::DeletedAt.is_null())
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;
        let events = rows
            .into_iter()
            .map(|(id, place, person, family)| (id, (place, (person, family))))
            .collect();
        let media: Vec<(Uuid, Option<Uuid>, sea_enums::Privacy, Option<Uuid>)> =
            media::Entity::find()
                .select_only()
                .column(media::Column::Id)
                .column(media::Column::PlaceId)
                .column(media::Column::Privacy)
                .column(media::Column::ParentMediaId)
                .filter(media::Column::TreeId.eq(tree_id))
                .filter(media::Column::DeletedAt.is_null())
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)?;
        let links: Vec<Row> = media_link::Entity::find()
            .select_only()
            .column(media_link::Column::MediaId)
            .column(media_link::Column::PersonId)
            .column(media_link::Column::FamilyId)
            .column(media_link::Column::EventId)
            .join(JoinType::InnerJoin, media_link::Relation::Media.def())
            .filter(media::Column::TreeId.eq(tree_id))
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;
        let crops: Vec<(Uuid, Option<Uuid>, Option<Uuid>)> = vignette::Entity::find()
            .select_only()
            .column(vignette::Column::MediaId)
            .column(vignette::Column::PersonId)
            .column(vignette::Column::EventId)
            .join(JoinType::InnerJoin, vignette::Relation::Media.def())
            .filter(media::Column::TreeId.eq(tree_id))
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;
        let mut sight = Self {
            redaction,
            events,
            placed_media: media
                .iter()
                .filter_map(|(id, place, ..)| Some((*id, (*place)?)))
                .collect(),
            withheld_media: HashSet::new(),
        };
        let media: Vec<(Uuid, Privacy, Option<Uuid>)> = media
            .into_iter()
            .map(|(id, _, privacy, parent)| (id, privacy.into(), parent))
            .collect();
        let links: Vec<(Uuid, Owner, Option<Uuid>)> = links
            .into_iter()
            .map(|(media, person, family, event)| (media, (person, family), event))
            .chain(
                crops
                    .into_iter()
                    .map(|(media, person, event)| (media, (person, None), event)),
            )
            .collect();
        sight.withheld_media = sight.withheld(&media, &links);
        Ok(sight)
    }

    /// Whether what `owner` holds is visible: its person is shown in full,
    /// its family's content is not withheld.
    fn owner_visible(&self, (person, family): Owner) -> bool {
        person.is_none_or(|p| self.redaction.shows(p))
            && family.is_none_or(|f| !self.redaction.family_withheld(f))
    }

    /// Whether live event `id` is visible; a deleted one is not.
    fn event_visible(&self, id: Uuid) -> bool {
        self.events
            .get(&id)
            .is_some_and(|(_, owner)| self.owner_visible(*owner))
    }

    /// Whether a record owned by `owner`, or by `event`, is visible.
    fn record_visible(&self, owner: Owner, event: Option<Uuid>) -> bool {
        self.owner_visible(owner) && event.is_none_or(|e| self.event_visible(e))
    }

    /// The media of `media` (id, privacy, document) the redaction withholds
    /// (§3.2): each document with its pages, private, or showing a person
    /// or a family — linked, cropped, or through a linked event — the
    /// redaction hides. A page follows its document, and a document its
    /// pages.
    fn withheld(
        &self,
        media: &[(Uuid, Privacy, Option<Uuid>)],
        links: &[(Uuid, Owner, Option<Uuid>)],
    ) -> HashSet<Uuid> {
        let document: HashMap<Uuid, Uuid> = media
            .iter()
            .map(|(id, _, parent)| (*id, parent.unwrap_or(*id)))
            .collect();
        let mut shows: HashMap<Uuid, (Vec<Uuid>, Vec<Uuid>)> = HashMap::new();
        for (medium, (person, family), event) in links {
            let Some(group) = document.get(medium) else {
                continue;
            };
            let entry = shows.entry(*group).or_default();
            let through = event.and_then(|e| self.events.get(&e)).map(|(_, o)| *o);
            for (p, f) in [(*person, *family), through.unwrap_or_default()] {
                entry.0.extend(p);
                entry.1.extend(f);
            }
        }
        let withheld_groups: HashSet<Uuid> = media
            .iter()
            .filter(|(id, privacy, _)| {
                let (persons, families) = shows.get(&document[id]).cloned().unwrap_or_default();
                self.redaction.medium_withheld(*privacy, persons, families)
            })
            .map(|(id, ..)| document[id])
            .collect();
        media
            .iter()
            .filter(|(id, ..)| withheld_groups.contains(&document[id]))
            .map(|(id, ..)| *id)
            .collect()
    }

    /// Each place's visible uses — events and media — and the places with
    /// any use at all.
    fn place_uses(&self) -> (HashMap<Uuid, i64>, HashSet<Uuid>) {
        let mut visible: HashMap<Uuid, i64> = HashMap::new();
        let mut used = HashSet::new();
        for (place, owner) in self.events.values() {
            let Some(place) = place else { continue };
            used.insert(*place);
            if self.owner_visible(*owner) {
                *visible.entry(*place).or_default() += 1;
            }
        }
        for (medium, place) in &self.placed_media {
            used.insert(*place);
            if !self.withheld_media.contains(medium) {
                *visible.entry(*place).or_default() += 1;
            }
        }
        (visible, used)
    }

    /// The visible citations of each source of tree `tree_id`, and the
    /// sources cited at all.
    async fn citation_counts(
        &self,
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<(HashMap<Uuid, i64>, HashSet<Uuid>), OxidGeneError> {
        let rows: Vec<Row> = citation::Entity::find()
            .select_only()
            .column(citation::Column::SourceId)
            .column(citation::Column::PersonId)
            .column(citation::Column::FamilyId)
            .column(citation::Column::EventId)
            .join(JoinType::InnerJoin, citation::Relation::Source.def())
            .filter(source::Column::TreeId.eq(tree_id))
            .filter(source::Column::DeletedAt.is_null())
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;
        let mut visible: HashMap<Uuid, i64> = HashMap::new();
        let mut cited = HashSet::new();
        for (source, person, family, event) in rows {
            cited.insert(source);
            if self.record_visible((person, family), event) {
                *visible.entry(source).or_default() += 1;
            }
        }
        Ok((visible, cited))
    }
}

/// The sources a restricted viewer sees, with their visible citations.
struct SeenSources {
    counts: HashMap<Uuid, i64>,
    cited: HashSet<Uuid>,
}

impl SeenSources {
    async fn load(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        redaction: &Redaction,
    ) -> Result<Self, OxidGeneError> {
        let sight = Sight::load(db, tree_id, redaction).await?;
        let (counts, cited) = sight.citation_counts(db, tree_id).await?;
        Ok(Self { counts, cited })
    }

    /// Whether source `id` is visible: cited by a visible record, or by
    /// none at all.
    fn shows(&self, id: Uuid) -> bool {
        !self.cited.contains(&id) || self.counts.contains_key(&id)
    }

    fn count(&self, id: Uuid) -> i64 {
        self.counts.get(&id).copied().unwrap_or(0)
    }
}

impl DictionaryRepo {
    /// [`Self::source_titles_with_usage`] as `redaction`'s viewer sees them.
    pub async fn source_titles_with_usage_for(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        redaction: &Redaction,
    ) -> Result<Vec<(String, i64)>, OxidGeneError> {
        if redaction.is_unrestricted() {
            return Self::source_titles_with_usage(db, tree_id).await;
        }
        let seen = SeenSources::load(db, tree_id, redaction).await?;
        Ok(source_titles(db, tree_id)
            .await?
            .into_iter()
            .filter(|(id, _)| seen.shows(*id))
            .map(|(id, title)| (title, seen.count(id)))
            .collect())
    }

    /// [`Self::sources_with_usage_by_prefix`], or with `exact`
    /// [`Self::sources_with_usage_by_exact_title`], as `redaction`'s viewer
    /// sees them.
    pub async fn sources_with_usage_for(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        prefix: &str,
        exact: bool,
        redaction: &Redaction,
    ) -> Result<Vec<(Source, i64)>, OxidGeneError> {
        if redaction.is_unrestricted() {
            return if exact {
                Self::sources_with_usage_by_exact_title(db, tree_id, prefix).await
            } else {
                Self::sources_with_usage_by_prefix(db, tree_id, prefix).await
            };
        }
        let seen = SeenSources::load(db, tree_id, redaction).await?;
        let wanted = prefix.to_uppercase();
        let ids: Vec<Uuid> = source_titles(db, tree_id)
            .await?
            .into_iter()
            .filter(|(id, title)| {
                let title = title.to_uppercase();
                seen.shows(*id)
                    && if exact {
                        title == wanted
                    } else {
                        title.starts_with(&wanted)
                    }
            })
            .map(|(id, _)| id)
            .collect();
        let mut sources = sources_with_usage_of(db, &ids).await?;
        for (source, count) in &mut sources {
            *count = seen.count(source.id);
            if redaction.withholds_tags() {
                source.tags.clear();
            }
        }
        Ok(sources)
    }

    /// [`Self::resolve_source_drill_down`] over the sources `redaction`'s
    /// viewer sees.
    pub async fn resolve_source_drill_down_for(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        prefix: &str,
        threshold: i64,
        redaction: &Redaction,
    ) -> Result<(String, i64, Vec<(String, i64)>), OxidGeneError> {
        if redaction.is_unrestricted() {
            return Self::resolve_source_drill_down(db, tree_id, prefix, threshold).await;
        }
        let seen = SeenSources::load(db, tree_id, redaction).await?;
        let titles: Vec<String> = source_titles(db, tree_id)
            .await?
            .into_iter()
            .filter(|(id, _)| seen.shows(*id))
            .map(|(_, title)| title.to_uppercase())
            .collect();
        Ok(drill_down(titles, prefix, threshold))
    }

    /// [`Self::places_with_usage`] as `redaction`'s viewer sees them: each
    /// place's visible events and media, without the places only hidden
    /// ones use.
    pub async fn places_with_usage_for(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        redaction: &Redaction,
    ) -> Result<Vec<(Place, i64)>, OxidGeneError> {
        let places = Self::places_with_usage(db, tree_id).await?;
        if redaction.is_unrestricted() {
            return Ok(places);
        }
        let sight = Sight::load(db, tree_id, redaction).await?;
        let (visible, used) = sight.place_uses();
        Ok(places
            .into_iter()
            .filter(|(place, _)| !used.contains(&place.id) || visible.contains_key(&place.id))
            .map(|(mut place, _)| {
                if redaction.withholds_tags() {
                    place.tags.clear();
                }
                let count = visible.get(&place.id).copied().unwrap_or(0);
                (place, count)
            })
            .collect())
    }

    /// [`Self::source_usage_person_ids`] as `redaction`'s viewer sees them:
    /// from the visible citations only, and only the persons shown in full.
    pub async fn source_usage_person_ids_for(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        source_id: Uuid,
        redaction: &Redaction,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        if redaction.is_unrestricted() {
            return Self::source_usage_person_ids(db, source_id).await;
        }
        let sight = Sight::load(db, tree_id, redaction).await?;
        let citations = citation::Entity::find()
            .filter(citation::Column::SourceId.eq(source_id))
            .all(db)
            .await
            .map_err(db_err)?;
        let mut persons = Vec::new();
        let mut families = Vec::new();
        for c in citations {
            if !sight.record_visible((c.person_id, c.family_id), c.event_id) {
                continue;
            }
            persons.extend(c.person_id);
            families.extend(c.family_id);
            if let Some((_, (person, family))) = c.event_id.and_then(|e| sight.events.get(&e)) {
                persons.extend(*person);
                families.extend(*family);
            }
        }
        let found = live_persons_with_spouses(db, persons, families).await?;
        Ok(found.into_iter().filter(|p| redaction.shows(*p)).collect())
    }

    /// [`Self::place_usage_person_ids`] as `redaction`'s viewer sees them:
    /// from the visible events and media only, and only the persons shown
    /// in full.
    pub async fn place_usage_person_ids_for(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        place_id: Uuid,
        redaction: &Redaction,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        if redaction.is_unrestricted() {
            return Self::place_usage_person_ids(db, place_id).await;
        }
        let sight = Sight::load(db, tree_id, redaction).await?;
        let mut persons = Vec::new();
        let mut families = Vec::new();
        for (place, (person, family)) in sight.events.values() {
            if *place == Some(place_id) && sight.owner_visible((*person, *family)) {
                persons.extend(*person);
                families.extend(*family);
            }
        }
        // The visible media filed at the place, and their pages.
        let documents: HashSet<Uuid> = sight
            .placed_media
            .iter()
            .filter(|(medium, place)| *place == place_id && !sight.withheld_media.contains(medium))
            .map(|(medium, _)| *medium)
            .collect();
        let pages: Vec<Uuid> = in_chunks(
            &documents.iter().copied().collect::<Vec<_>>(),
            |chunk| async move {
                media::Entity::find()
                    .select_only()
                    .column(media::Column::Id)
                    .filter(media::Column::ParentMediaId.is_in(chunk))
                    .filter(media::Column::DeletedAt.is_null())
                    .into_tuple::<Uuid>()
                    .all(db)
                    .await
                    .map_err(db_err)
            },
        )
        .await?;
        let media_ids: Vec<Uuid> = documents.into_iter().chain(pages).collect();
        let links = in_chunks(&media_ids, |chunk| async move {
            media_link::Entity::find()
                .filter(media_link::Column::MediaId.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        let crops = in_chunks(&media_ids, |chunk| async move {
            vignette::Entity::find()
                .filter(vignette::Column::MediaId.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        let mut events: Vec<Uuid> = Vec::new();
        for link in links {
            persons.extend(link.person_id);
            families.extend(link.family_id);
            events.extend(link.event_id);
        }
        for crop in crops {
            persons.extend(crop.person_id);
            events.extend(crop.event_id);
        }
        for (_, (person, family)) in events.iter().filter_map(|e| sight.events.get(e)) {
            persons.extend(*person);
            families.extend(*family);
        }
        let found = live_persons_with_spouses(db, persons, families).await?;
        Ok(found.into_iter().filter(|p| redaction.shows(*p)).collect())
    }
}
