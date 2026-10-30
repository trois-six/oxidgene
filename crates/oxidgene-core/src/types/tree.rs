use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::TreeDefaultPrivacy;

/// A genealogical tree (project).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tree {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub sosa_root_person_id: Option<Uuid>,
    /// The person in this tree who represents the current user.
    pub self_person_id: Option<Uuid>,
    /// What `Privacy::Default` resolves to for every person, couple and
    /// document in this tree. Enforced by nothing yet — see the roadmap.
    #[serde(default)]
    pub default_privacy: TreeDefaultPrivacy,
    /// Who the tree's GEDCOM exports say they are from (`SUBM.NAME`); when
    /// unset, the "Who am I?" person's name.
    #[serde(default)]
    pub submitter_name: Option<String>,
    /// The submitter's email (`SUBM.EMAIL`).
    #[serde(default)]
    pub submitter_email: Option<String>,
    /// The submitter's postal address, over several lines (`SUBM.ADDR`).
    #[serde(default)]
    pub submitter_address: Option<String>,
    /// The thresholds the anomaly rules of this tree apply.
    #[serde(default)]
    pub anomaly_thresholds: AnomalyThresholds,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// The thresholds of the anomaly rules (`docs/ui-tools.md` §3.3), a tree
/// setting: each in whole years, days, kilometres, or a year for
/// `centenarian_before_year`. A field a stored or sent value leaves out
/// keeps its default, so a threshold added later reads as its default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnomalyThresholds {
    /// A life longer than this is flagged whenever it was lived.
    pub max_lifespan_years: i32,
    /// A life longer than this is flagged when it began before
    /// `centenarian_before_year`, when centenarians were rarer still.
    pub old_centenarian_years: i32,
    pub centenarian_before_year: i32,
    /// A parent younger than this at a child's birth.
    pub min_parent_age_years: i32,
    /// A father older than this at a child's birth.
    pub max_father_age_years: i32,
    /// A mother older than this at a child's birth.
    pub max_mother_age_years: i32,
    /// A child born more than this many days after the father's death.
    pub max_posthumous_birth_days: i32,
    /// Two children of a union born fewer days apart than this are twins.
    pub twin_days: i32,
    /// Two children of a union born fewer days apart than this, twins
    /// aside, are too close.
    pub min_sibling_gap_days: i32,
    /// Two consecutive children of a union born further apart than this.
    pub max_sibling_gap_years: i32,
    /// A spouse younger than this at the union.
    pub min_union_age_years: i32,
    /// A spouse older than this at the union.
    pub max_union_age_years: i32,
    /// Spouses born further apart than this.
    pub max_spouse_gap_years: i32,
    /// How far an age a record gives may stray from the dates, each way.
    pub recorded_age_tolerance_years: i32,
    /// How far a date about, calculated or estimated may stray, each way.
    pub approximate_slack_years: i32,
    /// A person with no death recorded born longer ago than this.
    pub max_age_alive_years: i32,
    /// Two events of one person on one day further apart than this.
    pub max_same_day_distance_km: i32,
}

impl Default for AnomalyThresholds {
    fn default() -> Self {
        Self {
            max_lifespan_years: 105,
            old_centenarian_years: 100,
            centenarian_before_year: 1900,
            min_parent_age_years: 11,
            max_father_age_years: 70,
            max_mother_age_years: 55,
            max_posthumous_birth_days: 300,
            twin_days: 11,
            min_sibling_gap_days: 213,
            max_sibling_gap_years: 50,
            min_union_age_years: 12,
            max_union_age_years: 100,
            max_spouse_gap_years: 50,
            recorded_age_tolerance_years: 2,
            approximate_slack_years: 2,
            max_age_alive_years: 110,
            max_same_day_distance_km: 100,
        }
    }
}

impl AnomalyThresholds {
    /// Each threshold with the range it must lie in, inclusive.
    pub fn ranges(&self) -> [(&'static str, i32, (i32, i32)); 17] {
        [
            ("max_lifespan_years", self.max_lifespan_years, (80, 150)),
            (
                "old_centenarian_years",
                self.old_centenarian_years,
                (80, 150),
            ),
            (
                "centenarian_before_year",
                self.centenarian_before_year,
                (1000, 2100),
            ),
            ("min_parent_age_years", self.min_parent_age_years, (1, 20)),
            ("max_father_age_years", self.max_father_age_years, (30, 120)),
            ("max_mother_age_years", self.max_mother_age_years, (30, 80)),
            (
                "max_posthumous_birth_days",
                self.max_posthumous_birth_days,
                (180, 400),
            ),
            ("twin_days", self.twin_days, (1, 60)),
            ("min_sibling_gap_days", self.min_sibling_gap_days, (2, 365)),
            (
                "max_sibling_gap_years",
                self.max_sibling_gap_years,
                (10, 80),
            ),
            ("min_union_age_years", self.min_union_age_years, (1, 25)),
            ("max_union_age_years", self.max_union_age_years, (50, 150)),
            ("max_spouse_gap_years", self.max_spouse_gap_years, (5, 100)),
            (
                "recorded_age_tolerance_years",
                self.recorded_age_tolerance_years,
                (0, 20),
            ),
            (
                "approximate_slack_years",
                self.approximate_slack_years,
                (0, 20),
            ),
            ("max_age_alive_years", self.max_age_alive_years, (80, 150)),
            (
                "max_same_day_distance_km",
                self.max_same_day_distance_km,
                (1, 20000),
            ),
        ]
    }

    /// Each threshold by name, to be set: the names and order of
    /// [`Self::ranges`].
    pub fn fields_mut(&mut self) -> [(&'static str, &mut i32); 17] {
        [
            ("max_lifespan_years", &mut self.max_lifespan_years),
            ("old_centenarian_years", &mut self.old_centenarian_years),
            ("centenarian_before_year", &mut self.centenarian_before_year),
            ("min_parent_age_years", &mut self.min_parent_age_years),
            ("max_father_age_years", &mut self.max_father_age_years),
            ("max_mother_age_years", &mut self.max_mother_age_years),
            (
                "max_posthumous_birth_days",
                &mut self.max_posthumous_birth_days,
            ),
            ("twin_days", &mut self.twin_days),
            ("min_sibling_gap_days", &mut self.min_sibling_gap_days),
            ("max_sibling_gap_years", &mut self.max_sibling_gap_years),
            ("min_union_age_years", &mut self.min_union_age_years),
            ("max_union_age_years", &mut self.max_union_age_years),
            ("max_spouse_gap_years", &mut self.max_spouse_gap_years),
            (
                "recorded_age_tolerance_years",
                &mut self.recorded_age_tolerance_years,
            ),
            ("approximate_slack_years", &mut self.approximate_slack_years),
            ("max_age_alive_years", &mut self.max_age_alive_years),
            (
                "max_same_day_distance_km",
                &mut self.max_same_day_distance_km,
            ),
        ]
    }

    /// The threshold named `name`, if there is one.
    pub fn get(&self, name: &str) -> Option<i32> {
        self.ranges()
            .into_iter()
            .find(|(field, ..)| *field == name)
            .map(|(_, value, _)| value)
    }

    /// Why these thresholds cannot be applied, if they cannot: a value out
    /// of its range, the centenarian age above the longest life, or twins
    /// born as far apart as siblings too close.
    pub fn check(&self) -> Result<(), String> {
        for (name, value, (min, max)) in self.ranges() {
            if !(min..=max).contains(&value) {
                return Err(format!(
                    "anomaly_thresholds.{name} must be between {min} and {max}"
                ));
            }
        }
        if self.old_centenarian_years > self.max_lifespan_years {
            return Err(
                "anomaly_thresholds.old_centenarian_years must not exceed max_lifespan_years"
                    .to_string(),
            );
        }
        if self.twin_days >= self.min_sibling_gap_days {
            return Err(
                "anomaly_thresholds.twin_days must be below min_sibling_gap_days".to_string(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_thresholds_are_valid_and_a_partial_value_keeps_the_rest() {
        assert_eq!(AnomalyThresholds::default().check(), Ok(()));
        let partial: AnomalyThresholds =
            serde_json::from_str(r#"{"max_age_alive_years": 120}"#).unwrap();
        assert_eq!(partial.max_age_alive_years, 120);
        assert_eq!(partial.max_lifespan_years, 105);
        assert!(serde_json::from_str::<AnomalyThresholds>(r#"{"unknown": 1}"#).is_err());
    }

    #[test]
    fn every_threshold_is_named_once_in_the_same_order() {
        let mut thresholds = AnomalyThresholds::default();
        let ranged: Vec<&str> = thresholds.ranges().iter().map(|(n, ..)| *n).collect();
        let named: Vec<&str> = thresholds.fields_mut().iter().map(|(n, _)| *n).collect();
        assert_eq!(ranged, named);
        let json = serde_json::to_value(thresholds).unwrap();
        assert_eq!(json.as_object().unwrap().len(), named.len());
        for (name, value) in thresholds.fields_mut() {
            *value += 1;
            assert!(json.get(name).is_some(), "{name}");
        }
        assert_eq!(thresholds.get("twin_days"), Some(12));
        assert_eq!(thresholds.get("unknown"), None);
    }

    #[test]
    fn thresholds_out_of_range_or_inconsistent_are_refused() {
        let out = AnomalyThresholds {
            max_father_age_years: 10,
            ..AnomalyThresholds::default()
        };
        assert!(out.check().unwrap_err().contains("max_father_age_years"));
        let centenarian = AnomalyThresholds {
            old_centenarian_years: 110,
            ..AnomalyThresholds::default()
        };
        assert!(centenarian.check().is_err());
        let twins = AnomalyThresholds {
            twin_days: 30,
            min_sibling_gap_days: 30,
            ..AnomalyThresholds::default()
        };
        assert!(twins.check().unwrap_err().contains("twin_days"));
    }
}
