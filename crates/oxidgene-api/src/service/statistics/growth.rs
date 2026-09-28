//! The Growth tab of the Statistics page (`docs/ui-statistics.md` §10): how
//! many persons the tree held over the calendar days it was worked on.
//!
//! The person table answers exactly: every person keeps its `created_at`,
//! and a deletion or a merge only sets `deleted_at`, so the count at any
//! instant is the persons created by then less those deleted by then. An
//! import stamps everyone it brings with one time, so it is one step. The
//! history baseline versions existing persons without touching them, so it
//! adds nothing. Only a restore rewrites the table — it clears `deleted_at`
//! — and the spells it erases are read back from the person's versions.
//! The audit log otherwise only marks the imports on the chart.
//!
//! Both reads are grouped in SQL and bucketed here in one pass: a large
//! import is a single row, and no version snapshot is ever loaded.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use oxidgene_core::history::AuditDetails;
use oxidgene_db::repo::{HistoryRepo, PersonRepo, TreeRepo};
use serde::Serialize;
use uuid::Uuid;

/// How the number of persons in a tree changed, day by day.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct TreeGrowth {
    /// Each day (UTC) the number of persons changed, oldest first. Their
    /// running total of `added - removed` is the number of persons at the end
    /// of each day, and the last one the tree's persons now.
    pub days: Vec<GrowthDay>,
    /// The tree's recorded imports, oldest first.
    pub imports: Vec<GrowthImport>,
}

/// One day's changes to the number of persons.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct GrowthDay {
    pub date: NaiveDate,
    /// Persons created, imported or restored that day.
    pub added: i64,
    /// Persons deleted or merged into another that day.
    pub removed: i64,
}

/// A completed import, to mark on the chart.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct GrowthImport {
    pub occurred_at: DateTime<Utc>,
    /// `gedcom`, `gedzip`, `geneweb`, `geneanet`, or `duplicate`.
    pub format: Option<String>,
    /// The imported file's name, or the source tree's for a duplication.
    pub file_name: Option<String>,
    /// Persons the import brought.
    pub persons: i64,
}

/// Loads a tree's growth: 404 for an unknown tree.
pub async fn load(
    db: &sea_orm::DatabaseConnection,
    tree_id: Uuid,
) -> Result<TreeGrowth, oxidgene_core::OxidGeneError> {
    TreeRepo::get(db, tree_id).await?;
    let created = PersonRepo::count_by_creation(db, tree_id).await?;
    let deleted = PersonRepo::count_by_deletion(db, tree_id).await?;
    let restores = HistoryRepo::person_restores(db, tree_id).await?;
    let imports = HistoryRepo::imports(db, tree_id).await?;
    Ok(compute(&created, &deleted, &restores, imports))
}

/// Files the creations, deletions and restores by day.
///
/// `created` and `deleted` count the persons stamped with each time;
/// `restores` are the `(deleted, restored)` spells the person table no
/// longer shows. A spell counts as a removal on its first day and an
/// addition on its last, so the running total stays the true count.
pub fn compute(
    created: &[(DateTime<Utc>, i64)],
    deleted: &[(DateTime<Utc>, i64)],
    restores: &[(DateTime<Utc>, DateTime<Utc>)],
    imports: Vec<(DateTime<Utc>, AuditDetails)>,
) -> TreeGrowth {
    let mut days: BTreeMap<NaiveDate, (i64, i64)> = BTreeMap::new();
    for (at, count) in created {
        days.entry(at.date_naive()).or_default().0 += count;
    }
    for (at, count) in deleted {
        days.entry(at.date_naive()).or_default().1 += count;
    }
    for (removed_at, restored_at) in restores {
        days.entry(removed_at.date_naive()).or_default().1 += 1;
        days.entry(restored_at.date_naive()).or_default().0 += 1;
    }
    TreeGrowth {
        days: days
            .into_iter()
            .map(|(date, (added, removed))| GrowthDay {
                date,
                added,
                removed,
            })
            .collect(),
        imports: imports
            .into_iter()
            .map(|(occurred_at, details)| GrowthImport {
                occurred_at,
                format: details.format,
                file_name: details.file_name,
                persons: details
                    .count
                    .map_or(0, |n| i64::try_from(n).unwrap_or(i64::MAX)),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, day, hour, 0, 0).unwrap()
    }

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 3, day).unwrap()
    }

    #[test]
    fn an_empty_tree_has_no_day() {
        assert_eq!(
            compute(&[], &[], &[], Vec::new()),
            TreeGrowth {
                days: Vec::new(),
                imports: Vec::new(),
            }
        );
    }

    #[test]
    fn changes_are_filed_by_day_and_add_up_to_the_count_now() {
        // An import of 500 on the 2nd, two persons created by hand on the
        // 2nd and the 5th, one merged away on the 5th, and one deleted on
        // the 9th then restored on the 12th — which cleared their
        // `deleted_at`, so only the spell remembers it.
        let created = [(at(2, 9), 500), (at(2, 17), 1), (at(5, 8), 1)];
        let deleted = [(at(5, 10), 1)];
        let restores = [(at(9, 12), at(12, 7))];
        let growth = compute(&created, &deleted, &restores, Vec::new());
        let days: Vec<(NaiveDate, i64, i64)> = growth
            .days
            .iter()
            .map(|d| (d.date, d.added, d.removed))
            .collect();
        assert_eq!(
            days,
            [
                (date(2), 501, 0),
                (date(5), 1, 1),
                (date(9), 0, 1),
                (date(12), 1, 0),
            ]
        );
        let now: i64 = growth.days.iter().map(|d| d.added - d.removed).sum();
        assert_eq!(now, 501);
    }

    #[test]
    fn imports_carry_their_details() {
        let details = AuditDetails {
            format: Some("gedcom".to_string()),
            file_name: Some("fixture.ged".to_string()),
            count: Some(42),
            ..AuditDetails::default()
        };
        let growth = compute(&[(at(3, 10), 42)], &[], &[], vec![(at(3, 10), details)]);
        assert_eq!(
            growth.imports,
            [GrowthImport {
                occurred_at: at(3, 10),
                format: Some("gedcom".to_string()),
                file_name: Some("fixture.ged".to_string()),
                persons: 42,
            }]
        );
    }
}
