//! Projection shape guard: the JSON shape of a stored `PersonProfile` is
//! pinned to `PROJECTION_SCHEMA_VERSION`.
//!
//! Drift it prevents: a field added to `PersonProfile` or a nested type
//! without a version bump. Stored projections deserialize with
//! `#[serde(default)]`, so an old payload looks complete and the new field
//! stays empty on every existing install until something rebuilds it
//! (AGENTS.md).
//!
//! The test serializes a profile with every field and nested type set and
//! compares its shape — keys and JSON types, not values — with
//! `tests/projection_shape.json`, which records the version it belongs to.
//!
//! Fixing a failure: bump `PROJECTION_SCHEMA_VERSION` (with a line saying
//! why), set every new field in `full_profile` below, then rewrite the
//! snapshot with `OXIDGENE_BLESS=1 cargo nextest run -p oxidgene-core --test
//! projection_shape_test` and commit it.

use chrono::{NaiveDate, TimeZone, Utc};
use oxidgene_core::enums::{
    Calendar, ChildType, DateQualifier, EventType, NameType, Sex, SpouseRole,
};
use oxidgene_core::projection::{
    PROJECTION_SCHEMA_VERSION, PersonProfile, ProfileChildLink, ProfileEvent, ProfileFamilyLink,
    ProfileMediaRef, ProfileName,
};
use serde_json::{Map, Value, json};
use uuid::Uuid;

const SNAPSHOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/projection_shape.json");

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn name() -> ProfileName {
    ProfileName {
        name_id: id(1),
        name_type: NameType::Birth,
        display_name: "Anchor Ashdown".into(),
        given_names: Some("Anchor".into()),
        surname: Some("Ashdown".into()),
    }
}

fn event() -> ProfileEvent {
    ProfileEvent {
        event_id: id(2),
        event_type: EventType::Birth,
        date_value: Some("BET 1 JAN 1900 AND 2 FEB 1901".into()),
        date_sort: NaiveDate::from_ymd_opt(1900, 1, 1),
        date_qualifier: DateQualifier::Between,
        date_value2: Some("2 FEB 1901".into()),
        calendar: Calendar::Julian,
        place_name: Some("Fictland".into()),
        place_id: Some(id(3)),
        description: Some("A fictitious event".into()),
        age: Some("30y".into()),
    }
}

/// A profile with every field and every nested type set.
fn full_profile() -> PersonProfile {
    let at = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
    PersonProfile {
        person_id: id(4),
        tree_id: id(5),
        sex: Sex::Female,
        primary_name: Some(name()),
        other_names: vec![name()],
        birth: Some(event()),
        death: Some(event()),
        baptism: Some(event()),
        burial: Some(event()),
        occupation: Some("Weaver".into()),
        other_events: vec![event()],
        families_as_spouse: vec![ProfileFamilyLink {
            family_id: id(6),
            role: SpouseRole::Wife,
            spouse_id: Some(id(7)),
            spouse_display_name: Some("Spouse Ashdown".into()),
            spouse_surname: Some("Ashdown".into()),
            spouse_given_names: Some("Spouse".into()),
            spouse_sex: Some(Sex::Male),
            marriage: Some(event()),
            events: vec![event()],
            children_ids: vec![id(8)],
            children_count: 1,
        }],
        family_as_child: Some(ProfileChildLink {
            family_id: id(9),
            child_type: ChildType::Biological,
            father_id: Some(id(10)),
            father_display_name: Some("Father Ashdown".into()),
            father_surname: Some("Ashdown".into()),
            father_given_names: Some("Father".into()),
            mother_id: Some(id(11)),
            mother_display_name: Some("Mother Ashdown".into()),
            mother_surname: Some("Ashdown".into()),
            mother_given_names: Some("Mother".into()),
        }),
        primary_media: Some(ProfileMediaRef {
            media_id: id(12),
            vignette_id: Some(id(13)),
            file_path: "media/portrait.jpg".into(),
            mime_type: "image/jpeg".into(),
            title: Some("Portrait".into()),
        }),
        media_count: 1,
        citation_count: 1,
        note_count: 1,
        updated_at: at,
        built_at: at,
    }
}

/// The JSON shape of `value`: objects keep their keys, arrays the shape of
/// their first element, and every scalar becomes its type's name.
fn shape(value: &Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, field)| (key.clone(), shape(field)))
                .collect::<Map<_, _>>(),
        ),
        Value::Array(items) => Value::Array(items.first().map(shape).into_iter().collect()),
        Value::String(_) => json!("string"),
        Value::Number(_) => json!("number"),
        Value::Bool(_) => json!("bool"),
        Value::Null => json!("null"),
    }
}

#[test]
fn the_profile_shape_is_pinned_to_the_schema_version() {
    let profile = serde_json::to_value(full_profile()).unwrap();
    let current = json!({
        "schema_version": PROJECTION_SCHEMA_VERSION,
        "person_profile": shape(&profile),
    });
    if std::env::var_os("OXIDGENE_BLESS").is_some() {
        let text = serde_json::to_string_pretty(&current).unwrap() + "\n";
        std::fs::write(SNAPSHOT, text).unwrap();
        return;
    }
    let committed: Value = serde_json::from_str(&std::fs::read_to_string(SNAPSHOT).unwrap())
        .expect("tests/projection_shape.json");
    let null_fields = profile.to_string().matches(":null").count();
    assert_eq!(null_fields, 0, "full_profile leaves a field unset");
    if committed["schema_version"] == current["schema_version"] {
        assert!(
            committed["person_profile"] == current["person_profile"],
            "PersonProfile changed shape without a PROJECTION_SCHEMA_VERSION bump: bump it, then rewrite the snapshot (see this file's docs)"
        );
    } else {
        panic!(
            "PROJECTION_SCHEMA_VERSION is {} but the snapshot records {}: rewrite the snapshot (see this file's docs)",
            PROJECTION_SCHEMA_VERSION, committed["schema_version"]
        );
    }
}
