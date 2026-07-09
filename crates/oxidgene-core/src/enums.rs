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
            Self::Birth => write!(f, "Birth name"),
            Self::Married => write!(f, "Married name"),
            Self::AlsoKnownAs => write!(f, "Also known as"),
            Self::Maiden => write!(f, "Maiden name"),
            Self::Religious => write!(f, "Religious name"),
            Self::Other => write!(f, "Other"),
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

/// Per-person privacy override (§7 of the person edit modal spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Privacy {
    /// Follows the tree-level privacy settings.
    #[default]
    Default,
    /// Always visible regardless of tree settings.
    Public,
    /// Always hidden regardless of tree settings.
    Private,
}

impl std::fmt::Display for Privacy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::Public => write!(f, "public"),
            Self::Private => write!(f, "private"),
        }
    }
}

/// Qualifier describing the precision/shape of a date entry (§5 of the
/// person edit modal spec). `Or` and `Between` use two date values; the
/// rest use a single one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateQualifier {
    #[default]
    Exact,
    About,
    Perhaps,
    Before,
    After,
    Or,
    Between,
    FromAge,
}

impl std::fmt::Display for DateQualifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exact => write!(f, "exact"),
            Self::About => write!(f, "about"),
            Self::Perhaps => write!(f, "perhaps"),
            Self::Before => write!(f, "before"),
            Self::After => write!(f, "after"),
            Self::Or => write!(f, "or"),
            Self::Between => write!(f, "between"),
            Self::FromAge => write!(f, "from_age"),
        }
    }
}

impl DateQualifier {
    /// Returns `true` if this qualifier requires two date fields (`Or`, `Between`).
    pub fn needs_second_date(&self) -> bool {
        matches!(self, Self::Or | Self::Between)
    }
}

/// Calendar system used to record a date (§8 of the person edit modal spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Calendar {
    #[default]
    Gregorian,
    Julian,
    Hebrew,
    FrenchRepublican,
}

impl std::fmt::Display for Calendar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gregorian => write!(f, "gregorian"),
            Self::Julian => write!(f, "julian"),
            Self::Hebrew => write!(f, "hebrew"),
            Self::FrenchRepublican => write!(f, "french_republican"),
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
    Confirmation,
    FirstCommunion,
    BarBatMitzvah,
    MilitaryService,
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
    /// Adoption (GEDCOM `ADOP`) — an individual-level event, not a family
    /// event: it may reference the adoptive family via a nested `FAMC`.
    Adoption,
    /// Caste name (GEDCOM `CAST`).
    CasteName,
    /// Physical description (GEDCOM `DSCR`).
    PhysicalDescription,
    /// Education / scholastic achievement (GEDCOM `EDUC`).
    Education,
    /// National ID number (GEDCOM `IDNO`).
    NationalId,
    /// National or tribal origin (GEDCOM `NATI`).
    NationalOrigin,
    /// Count of children (GEDCOM `NCHI`).
    ChildrenCount,
    /// Count of marriages (GEDCOM `NMR`).
    MarriagesCount,
    /// Possessions / property (GEDCOM `PROP`).
    Property,
    /// Religious affiliation (GEDCOM `RELI`).
    Religion,
    /// Social security number (GEDCOM `SSN`).
    SocialSecurityNumber,
    /// Title of nobility (GEDCOM `TITL` as an individual attribute).
    NobilityTitle,
    /// Generic fact (GEDCOM `FACT`).
    Fact,
    // Family events
    Marriage,
    Divorce,
    Annulment,
    Engagement,
    MarriageBann,
    MarriageContract,
    MarriageLicense,
    MarriageSettlement,
    /// Civil union / PACS / cohabitation — an unmarried partnership recorded
    /// via GEDCOM's generic `EVEN` family tag (no dedicated tag exists).
    CivilUnion,
    /// Legal separation, not yet a divorce (GEDCOM 7.0 `SEP` tag).
    Separation,
    /// Divorce petition filed but not finalized (GEDCOM `DIVF` tag).
    DivorceFiled,
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
                | Self::Confirmation
                | Self::FirstCommunion
                | Self::BarBatMitzvah
                | Self::MilitaryService
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
                | Self::Adoption
                | Self::CasteName
                | Self::PhysicalDescription
                | Self::Education
                | Self::NationalId
                | Self::NationalOrigin
                | Self::ChildrenCount
                | Self::MarriagesCount
                | Self::Property
                | Self::Religion
                | Self::SocialSecurityNumber
                | Self::NobilityTitle
                | Self::Fact
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
                | Self::CivilUnion
                | Self::Separation
                | Self::DivorceFiled
        )
    }
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Birth => write!(f, "birth"),
            Self::Death => write!(f, "death"),
            Self::Baptism => write!(f, "baptism"),
            Self::Confirmation => write!(f, "confirmation"),
            Self::FirstCommunion => write!(f, "first_communion"),
            Self::BarBatMitzvah => write!(f, "bar_bat_mitzvah"),
            Self::MilitaryService => write!(f, "military_service"),
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
            Self::Adoption => write!(f, "adoption"),
            Self::CasteName => write!(f, "caste_name"),
            Self::PhysicalDescription => write!(f, "physical_description"),
            Self::Education => write!(f, "education"),
            Self::NationalId => write!(f, "national_id"),
            Self::NationalOrigin => write!(f, "national_origin"),
            Self::ChildrenCount => write!(f, "children_count"),
            Self::MarriagesCount => write!(f, "marriages_count"),
            Self::Property => write!(f, "property"),
            Self::Religion => write!(f, "religion"),
            Self::SocialSecurityNumber => write!(f, "social_security_number"),
            Self::NobilityTitle => write!(f, "nobility_title"),
            Self::Fact => write!(f, "fact"),
            Self::Marriage => write!(f, "marriage"),
            Self::Divorce => write!(f, "divorce"),
            Self::Annulment => write!(f, "annulment"),
            Self::Engagement => write!(f, "engagement"),
            Self::MarriageBann => write!(f, "marriage_bann"),
            Self::MarriageContract => write!(f, "marriage_contract"),
            Self::MarriageLicense => write!(f, "marriage_license"),
            Self::MarriageSettlement => write!(f, "marriage_settlement"),
            Self::CivilUnion => write!(f, "civil_union"),
            Self::Separation => write!(f, "separation"),
            Self::DivorceFiled => write!(f, "divorce_filed"),
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
    fn test_adoption_is_individual_not_family() {
        // GEDCOM 5.5.1 `ADOP` is an individual-level event that may
        // reference the adoptive family via a nested `FAMC`.
        assert!(EventType::Adoption.is_individual());
        assert!(!EventType::Adoption.is_family());
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
