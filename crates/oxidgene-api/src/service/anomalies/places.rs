//! The rules about places (`docs/ui-tools.md` §3.3): one place written in
//! several ways, and two events of a person on one day too far apart.
//!
//! Both read the places as the projections' events name them, so a place no
//! event uses is never compared.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::NaiveDate;
use oxidgene_core::DateQualifier;
use oxidgene_core::projection::{PersonProfile, ProfileEvent};
use oxidgene_core::search::fold_words;
use uuid::Uuid;

use super::{Anomaly, AnomalyPlace, Findings, Life};

/// The mean radius of the Earth, in kilometres.
const EARTH_RADIUS_KM: f64 = 6371.0;

/// A person's own events and those of their unions.
pub(super) fn events_of(p: &PersonProfile) -> impl Iterator<Item = &ProfileEvent> {
    [&p.birth, &p.baptism, &p.death, &p.burial]
        .into_iter()
        .flatten()
        .chain(&p.other_events)
        .chain(
            p.families_as_spouse
                .iter()
                .flat_map(|l| l.events.iter().chain(l.marriage.as_ref())),
        )
}

/// The day an event is dated to, when its date is a single complete one.
fn day_of(event: &ProfileEvent) -> Option<NaiveDate> {
    let complete = event
        .date_value
        .as_deref()
        .is_some_and(|v| v.split_whitespace().count() >= 3);
    (event.date_qualifier == DateQualifier::Exact && complete)
        .then_some(event.date_sort)
        .flatten()
}

/// The pairs of a person's events, their own or their unions', dated to one
/// complete day and placed in two different places: each pair once, a family
/// event once though both spouses' links hold it.
pub(super) fn same_day_pairs(p: &PersonProfile) -> Vec<(&ProfileEvent, &ProfileEvent)> {
    let mut by_day: BTreeMap<NaiveDate, Vec<&ProfileEvent>> = BTreeMap::new();
    let mut seen = HashSet::new();
    for event in events_of(p) {
        if let (Some(day), Some(_)) = (day_of(event), event.place_id)
            && seen.insert(event.event_id)
        {
            by_day.entry(day).or_default().push(event);
        }
    }
    let mut pairs = Vec::new();
    for events in by_day.values() {
        for (i, a) in events.iter().enumerate() {
            for b in &events[i + 1..] {
                if a.place_id != b.place_id {
                    pairs.push((*a, *b));
                }
            }
        }
    }
    pairs
}

/// The great-circle distance between two points, in kilometres.
fn distance_km((lat1, lon1): (f64, f64), (lat2, lon2): (f64, f64)) -> f64 {
    let (phi1, phi2) = (lat1.to_radians(), lat2.to_radians());
    let half_dphi = (lat2 - lat1).to_radians() / 2.0;
    let half_dlambda = (lon2 - lon1).to_radians() / 2.0;
    let h = half_dphi.sin().powi(2) + phi1.cos() * phi2.cos() * half_dlambda.sin().powi(2);
    2.0 * EARTH_RADIUS_KM * h.sqrt().min(1.0).asin()
}

fn place_of(event: &ProfileEvent) -> Option<AnomalyPlace> {
    Some(AnomalyPlace {
        place_id: event.place_id?.to_string(),
        name: event.place_name.clone().unwrap_or_default(),
    })
}

/// Two events of a person on one day in places further apart than
/// the thresholds' distance: the farthest pair of each day, each pair
/// once (the first spouse reports a pair of their union's events).
pub(super) fn distant_places(
    me: &Life<'_>,
    coordinates: &HashMap<Uuid, (f64, f64)>,
    found: &mut Findings,
) {
    let spot = |e: &ProfileEvent| e.place_id.and_then(|id| coordinates.get(&id)).copied();
    let mut farthest: BTreeMap<NaiveDate, (f64, &ProfileEvent, &ProfileEvent)> = BTreeMap::new();
    for (a, b) in same_day_pairs(me.profile) {
        let (Some(x), Some(y), Some(day)) = (spot(a), spot(b), day_of(a)) else {
            continue;
        };
        let km = distance_km(x, y);
        if km > me.limits.same_day_km && farthest.get(&day).is_none_or(|(most, ..)| km > *most) {
            farthest.insert(day, (km, a, b));
        }
    }
    for (km, a, b) in farthest.into_values() {
        // A pair of one union's events is the union's: reported by the
        // spouse whose links come first, the other's being the same.
        let shared = me.profile.families_as_spouse.iter().find(|l| {
            [a, b].iter().all(|e| {
                l.events
                    .iter()
                    .chain(l.marriage.as_ref())
                    .any(|f| f.event_id == e.event_id)
            })
        });
        if let Some(link) = shared
            && link.spouse_id.is_some_and(|s| s < me.profile.person_id)
        {
            continue;
        }
        let mut anomaly = Anomaly::of(vec![me.at()])
            .with_event(a.event_type)
            .with_other_event(b.event_type)
            .with_value(km);
        anomaly.places = [a, b].into_iter().filter_map(place_of).collect();
        if let Some(link) = shared {
            anomaly = anomaly.with_family(link.family_id);
        }
        found.add("distant_places_same_day", anomaly);
    }
}

/// A place label's parts, comma-separated, each folded (case, accents and
/// punctuation aside), empty parts dropped.
fn parts(name: &str) -> Vec<String> {
    name.split(',')
        .map(fold_words)
        .map(|part| part.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|part| !part.is_empty())
        .collect()
}

/// Whether `short` is `long` with some parts missing, in the same order.
fn missing_parts(short: &[String], long: &[String]) -> bool {
    if short.len() >= long.len() {
        return false;
    }
    let mut rest = long.iter();
    short.iter().all(|part| rest.any(|p| p == part))
}

/// Places that may be one: labels equal once folded — differing only by
/// case, accents or punctuation — or one the other with parts missing,
/// whose first part, the locality, is the same (`Lyon` and `Lyon, Rhône`).
/// Each group of such places once, the places by name.
pub(super) fn place_spellings(sorted: &[&PersonProfile], found: &mut Findings) {
    let mut names: BTreeMap<Uuid, &str> = BTreeMap::new();
    for profile in sorted {
        for event in events_of(profile) {
            if let (Some(id), Some(name)) = (event.place_id, event.place_name.as_deref()) {
                names.entry(id).or_insert(name);
            }
        }
    }
    // Places by locality: only those sharing their first part are compared.
    let mut localities: BTreeMap<String, Vec<(Uuid, Vec<String>)>> = BTreeMap::new();
    for (id, name) in &names {
        let parts = parts(name);
        if let Some(first) = parts.first() {
            localities
                .entry(first.clone())
                .or_default()
                .push((*id, parts));
        }
    }
    for places in localities.values().filter(|places| places.len() > 1) {
        for group in groups(places) {
            let mut named: Vec<AnomalyPlace> = group
                .iter()
                .map(|id| AnomalyPlace {
                    place_id: id.to_string(),
                    name: names[id].to_string(),
                })
                .collect();
            named.sort_by(|a, b| (&a.name, &a.place_id).cmp(&(&b.name, &b.place_id)));
            let mut anomaly = Anomaly::of(Vec::new());
            anomaly.places = named;
            found.add("place_spellings", anomaly);
        }
    }
}

/// The places of one locality joined when one may be the other, in groups of
/// more than one: a union-find over the pairs.
fn groups(places: &[(Uuid, Vec<String>)]) -> Vec<Vec<Uuid>> {
    let mut root: Vec<usize> = (0..places.len()).collect();
    fn find(root: &mut [usize], mut i: usize) -> usize {
        while root[i] != i {
            root[i] = root[root[i]];
            i = root[i];
        }
        i
    }
    for (i, (_, a)) in places.iter().enumerate() {
        for (j, (_, b)) in places.iter().enumerate().skip(i + 1) {
            if a == b || missing_parts(a, b) || missing_parts(b, a) {
                let (x, y) = (find(&mut root, i), find(&mut root, j));
                root[x.max(y)] = x.min(y);
            }
        }
    }
    let mut by_root: BTreeMap<usize, Vec<Uuid>> = BTreeMap::new();
    for (i, (id, _)) in places.iter().enumerate() {
        let r = find(&mut root, i);
        by_root.entry(r).or_default().push(*id);
    }
    by_root.into_values().filter(|g| g.len() > 1).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_reads_by_its_folded_parts() {
        assert_eq!(
            parts("Saint-Étienne,  Loire ,France"),
            vec!["saint etienne", "loire", "france"]
        );
        let short = parts("Lyon, France");
        let long = parts("Lyon, Rhône, France");
        assert!(missing_parts(&short, &long));
        assert!(!missing_parts(&long, &short));
        assert!(!missing_parts(&parts("Lyon, Isère"), &long));
    }

    #[test]
    fn distances_are_great_circle_kilometres() {
        // Paris to Lyon, about 392 km.
        let km = distance_km((48.8566, 2.3522), (45.7640, 4.8357));
        assert!((385.0..400.0).contains(&km), "{km}");
        assert!(distance_km((45.0, 5.0), (45.0, 5.0)) < 1e-9);
    }
}
