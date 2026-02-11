//! Domain enums for OxidGene.
//!
//! All enums are serializable and use string representations for database storage.

use serde::{Deserialize, Serialize};

/// Biological sex of a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sex {
    Male,
    Female,
    Unknown,
}

impl std::fmt::Display for Sex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Male => write!(f, "male"),
            Self::Female => write!(f, "female"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Type of a person's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NameType {
    Birth,
    Married,
    AlsoKnownAs,
    Maiden,
    Religious,
    Other,
}

impl std::fmt::Display for NameType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Birth => write!(f, "birth"),
            Self::Married => write!(f, "married"),
            Self::AlsoKnownAs => write!(f, "also_known_as"),
            Self::Maiden => write!(f, "maiden"),
            Self::Religious => write!(f, "religious"),
            Self::Other => write!(f, "other"),
        }
    }
}

/// Role of a spouse in a family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpouseRole {
    Husband,
    Wife,
    Partner,
}

impl std::fmt::Display for SpouseRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Husband => write!(f, "husband"),
            Self::Wife => write!(f, "wife"),
            Self::Partner => write!(f, "partner"),
        }
    }
}

/// Type of relationship between a child and a family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildType {
    Biological,
    Adopted,
    Foster,
    Step,
    Unknown,
}

impl std::fmt::Display for ChildType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Biological => write!(f, "biological"),
            Self::Adopted => write!(f, "adopted"),
            Self::Foster => write!(f, "foster"),
            Self::Step => write!(f, "step"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Type of genealogical event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    // Individual events
    Birth,
    Death,
    Baptism,
    Burial,
    Cremation,
    Graduation,
    Immigration,
    Emigration,
    Naturalization,
    Census,
    Occupation,
    Residence,
    Retirement,
    Will,
    Probate,
    // Family events
    Marriage,
    Divorce,
    Annulment,
    Engagement,
    MarriageBann,
    MarriageContract,
    MarriageLicense,
    MarriageSettlement,
    // Generic
    Other,
}

impl EventType {
    /// Returns `true` if this event type applies to an individual person.
    pub fn is_individual(&self) -> bool {
        matches!(
            self,
            Self::Birth
                | Self::Death
                | Self::Baptism
                | Self::Burial
                | Self::Cremation
                | Self::Graduation
                | Self::Immigration
                | Self::Emigration
                | Self::Naturalization
                | Self::Census
                | Self::Occupation
                | Self::Residence
                | Self::Retirement
                | Self::Will
                | Self::Probate
        )
    }

    /// Returns `true` if this event type applies to a family.
    pub fn is_family(&self) -> bool {
        matches!(
            self,
            Self::Marriage
                | Self::Divorce
                | Self::Annulment
                | Self::Engagement
                | Self::MarriageBann
                | Self::MarriageContract
                | Self::MarriageLicense
                | Self::MarriageSettlement
        )
    }
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Birth => write!(f, "birth"),
            Self::Death => write!(f, "death"),
            Self::Baptism => write!(f, "baptism"),
            Self::Burial => write!(f, "burial"),
            Self::Cremation => write!(f, "cremation"),
            Self::Graduation => write!(f, "graduation"),
            Self::Immigration => write!(f, "immigration"),
            Self::Emigration => write!(f, "emigration"),
            Self::Naturalization => write!(f, "naturalization"),
            Self::Census => write!(f, "census"),
            Self::Occupation => write!(f, "occupation"),
            Self::Residence => write!(f, "residence"),
            Self::Retirement => write!(f, "retirement"),
            Self::Will => write!(f, "will"),
            Self::Probate => write!(f, "probate"),
            Self::Marriage => write!(f, "marriage"),
            Self::Divorce => write!(f, "divorce"),
            Self::Annulment => write!(f, "annulment"),
            Self::Engagement => write!(f, "engagement"),
            Self::MarriageBann => write!(f, "marriage_bann"),
            Self::MarriageContract => write!(f, "marriage_contract"),
            Self::MarriageLicense => write!(f, "marriage_license"),
            Self::MarriageSettlement => write!(f, "marriage_settlement"),
            Self::Other => write!(f, "other"),
        }
    }
}

/// Confidence level for a citation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    VeryLow,
    Low,
    Medium,
    High,
    VeryHigh,
}

impl std::fmt::Display for Confidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VeryLow => write!(f, "very_low"),
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::VeryHigh => write!(f, "very_high"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_individual() {
        assert!(EventType::Birth.is_individual());
        assert!(EventType::Death.is_individual());
        assert!(!EventType::Marriage.is_individual());
        assert!(!EventType::Other.is_individual());
    }

    #[test]
    fn test_event_type_family() {
        assert!(EventType::Marriage.is_family());
        assert!(EventType::Divorce.is_family());
        assert!(!EventType::Birth.is_family());
        assert!(!EventType::Other.is_family());
    }

    #[test]
    fn test_sex_display() {
        assert_eq!(Sex::Male.to_string(), "male");
        assert_eq!(Sex::Female.to_string(), "female");
        assert_eq!(Sex::Unknown.to_string(), "unknown");
    }

    #[test]
    fn test_sex_serde_roundtrip() {
        let json = serde_json::to_string(&Sex::Male).unwrap();
        assert_eq!(json, r#""male""#);
        let deserialized: Sex = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, Sex::Male);
    }

    #[test]
    fn test_event_type_serde_roundtrip() {
        let json = serde_json::to_string(&EventType::MarriageBann).unwrap();
        assert_eq!(json, r#""marriage_bann""#);
        let deserialized: EventType = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, EventType::MarriageBann);
    }

    #[test]
    fn test_confidence_serde_roundtrip() {
        let json = serde_json::to_string(&Confidence::VeryHigh).unwrap();
        assert_eq!(json, r#""very_high""#);
        let deserialized: Confidence = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, Confidence::VeryHigh);
    }
}
