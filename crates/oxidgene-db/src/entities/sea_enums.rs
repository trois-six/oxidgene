//! SeaORM-compatible enum types that mirror `oxidgene_core::enums`.
//!
//! These enums use `DeriveActiveEnum` so SeaORM can serialize them to/from
//! string columns in the database. Conversion impls map between core and DB enums.

use sea_orm::entity::prelude::*;

use oxidgene_core::enums;

/// Biological sex — stored as a string column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(10))")]
pub enum Sex {
    #[sea_orm(string_value = "male")]
    Male,
    #[sea_orm(string_value = "female")]
    Female,
    #[sea_orm(string_value = "unknown")]
    Unknown,
}

impl From<enums::Sex> for Sex {
    fn from(v: enums::Sex) -> Self {
        match v {
            enums::Sex::Male => Self::Male,
            enums::Sex::Female => Self::Female,
            enums::Sex::Unknown => Self::Unknown,
        }
    }
}

impl From<Sex> for enums::Sex {
    fn from(v: Sex) -> Self {
        match v {
            Sex::Male => Self::Male,
            Sex::Female => Self::Female,
            Sex::Unknown => Self::Unknown,
        }
    }
}

/// Name type — stored as a string column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(20))")]
pub enum NameType {
    #[sea_orm(string_value = "birth")]
    Birth,
    #[sea_orm(string_value = "married")]
    Married,
    #[sea_orm(string_value = "also_known_as")]
    AlsoKnownAs,
    #[sea_orm(string_value = "maiden")]
    Maiden,
    #[sea_orm(string_value = "religious")]
    Religious,
    #[sea_orm(string_value = "other")]
    Other,
}

impl From<enums::NameType> for NameType {
    fn from(v: enums::NameType) -> Self {
        match v {
            enums::NameType::Birth => Self::Birth,
            enums::NameType::Married => Self::Married,
            enums::NameType::AlsoKnownAs => Self::AlsoKnownAs,
            enums::NameType::Maiden => Self::Maiden,
            enums::NameType::Religious => Self::Religious,
            enums::NameType::Other => Self::Other,
        }
    }
}

impl From<NameType> for enums::NameType {
    fn from(v: NameType) -> Self {
        match v {
            NameType::Birth => Self::Birth,
            NameType::Married => Self::Married,
            NameType::AlsoKnownAs => Self::AlsoKnownAs,
            NameType::Maiden => Self::Maiden,
            NameType::Religious => Self::Religious,
            NameType::Other => Self::Other,
        }
    }
}

/// Spouse role — stored as a string column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(10))")]
pub enum SpouseRole {
    #[sea_orm(string_value = "husband")]
    Husband,
    #[sea_orm(string_value = "wife")]
    Wife,
    #[sea_orm(string_value = "partner")]
    Partner,
}

impl From<enums::SpouseRole> for SpouseRole {
    fn from(v: enums::SpouseRole) -> Self {
        match v {
            enums::SpouseRole::Husband => Self::Husband,
            enums::SpouseRole::Wife => Self::Wife,
            enums::SpouseRole::Partner => Self::Partner,
        }
    }
}

impl From<SpouseRole> for enums::SpouseRole {
    fn from(v: SpouseRole) -> Self {
        match v {
            SpouseRole::Husband => Self::Husband,
            SpouseRole::Wife => Self::Wife,
            SpouseRole::Partner => Self::Partner,
        }
    }
}

/// Child type — stored as a string column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(15))")]
pub enum ChildType {
    #[sea_orm(string_value = "biological")]
    Biological,
    #[sea_orm(string_value = "adopted")]
    Adopted,
    #[sea_orm(string_value = "foster")]
    Foster,
    #[sea_orm(string_value = "step")]
    Step,
    #[sea_orm(string_value = "unknown")]
    Unknown,
}

impl From<enums::ChildType> for ChildType {
    fn from(v: enums::ChildType) -> Self {
        match v {
            enums::ChildType::Biological => Self::Biological,
            enums::ChildType::Adopted => Self::Adopted,
            enums::ChildType::Foster => Self::Foster,
            enums::ChildType::Step => Self::Step,
            enums::ChildType::Unknown => Self::Unknown,
        }
    }
}

impl From<ChildType> for enums::ChildType {
    fn from(v: ChildType) -> Self {
        match v {
            ChildType::Biological => Self::Biological,
            ChildType::Adopted => Self::Adopted,
            ChildType::Foster => Self::Foster,
            ChildType::Step => Self::Step,
            ChildType::Unknown => Self::Unknown,
        }
    }
}

/// Event type — stored as a string column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(25))")]
pub enum EventType {
    // Individual events
    #[sea_orm(string_value = "birth")]
    Birth,
    #[sea_orm(string_value = "death")]
    Death,
    #[sea_orm(string_value = "baptism")]
    Baptism,
    #[sea_orm(string_value = "confirmation")]
    Confirmation,
    #[sea_orm(string_value = "first_communion")]
    FirstCommunion,
    #[sea_orm(string_value = "bar_bat_mitzvah")]
    BarBatMitzvah,
    #[sea_orm(string_value = "military_service")]
    MilitaryService,
    #[sea_orm(string_value = "burial")]
    Burial,
    #[sea_orm(string_value = "cremation")]
    Cremation,
    #[sea_orm(string_value = "graduation")]
    Graduation,
    #[sea_orm(string_value = "immigration")]
    Immigration,
    #[sea_orm(string_value = "emigration")]
    Emigration,
    #[sea_orm(string_value = "naturalization")]
    Naturalization,
    #[sea_orm(string_value = "census")]
    Census,
    #[sea_orm(string_value = "occupation")]
    Occupation,
    #[sea_orm(string_value = "residence")]
    Residence,
    #[sea_orm(string_value = "retirement")]
    Retirement,
    #[sea_orm(string_value = "will")]
    Will,
    #[sea_orm(string_value = "probate")]
    Probate,
    // Family events
    #[sea_orm(string_value = "marriage")]
    Marriage,
    #[sea_orm(string_value = "divorce")]
    Divorce,
    #[sea_orm(string_value = "annulment")]
    Annulment,
    #[sea_orm(string_value = "engagement")]
    Engagement,
    #[sea_orm(string_value = "marriage_bann")]
    MarriageBann,
    #[sea_orm(string_value = "marriage_contract")]
    MarriageContract,
    #[sea_orm(string_value = "marriage_license")]
    MarriageLicense,
    #[sea_orm(string_value = "marriage_settlement")]
    MarriageSettlement,
    #[sea_orm(string_value = "adoption")]
    Adoption,
    #[sea_orm(string_value = "civil_union")]
    CivilUnion,
    #[sea_orm(string_value = "separation")]
    Separation,
    #[sea_orm(string_value = "divorce_filed")]
    DivorceFiled,
    // Generic
    #[sea_orm(string_value = "other")]
    Other,
}

impl From<enums::EventType> for EventType {
    fn from(v: enums::EventType) -> Self {
        match v {
            enums::EventType::Birth => Self::Birth,
            enums::EventType::Death => Self::Death,
            enums::EventType::Baptism => Self::Baptism,
            enums::EventType::Confirmation => Self::Confirmation,
            enums::EventType::FirstCommunion => Self::FirstCommunion,
            enums::EventType::BarBatMitzvah => Self::BarBatMitzvah,
            enums::EventType::MilitaryService => Self::MilitaryService,
            enums::EventType::Burial => Self::Burial,
            enums::EventType::Cremation => Self::Cremation,
            enums::EventType::Graduation => Self::Graduation,
            enums::EventType::Immigration => Self::Immigration,
            enums::EventType::Emigration => Self::Emigration,
            enums::EventType::Naturalization => Self::Naturalization,
            enums::EventType::Census => Self::Census,
            enums::EventType::Occupation => Self::Occupation,
            enums::EventType::Residence => Self::Residence,
            enums::EventType::Retirement => Self::Retirement,
            enums::EventType::Will => Self::Will,
            enums::EventType::Probate => Self::Probate,
            enums::EventType::Marriage => Self::Marriage,
            enums::EventType::Divorce => Self::Divorce,
            enums::EventType::Annulment => Self::Annulment,
            enums::EventType::Engagement => Self::Engagement,
            enums::EventType::MarriageBann => Self::MarriageBann,
            enums::EventType::MarriageContract => Self::MarriageContract,
            enums::EventType::MarriageLicense => Self::MarriageLicense,
            enums::EventType::MarriageSettlement => Self::MarriageSettlement,
            enums::EventType::Adoption => Self::Adoption,
            enums::EventType::CivilUnion => Self::CivilUnion,
            enums::EventType::Separation => Self::Separation,
            enums::EventType::DivorceFiled => Self::DivorceFiled,
            enums::EventType::Other => Self::Other,
        }
    }
}

impl From<EventType> for enums::EventType {
    fn from(v: EventType) -> Self {
        match v {
            EventType::Birth => Self::Birth,
            EventType::Death => Self::Death,
            EventType::Baptism => Self::Baptism,
            EventType::Confirmation => Self::Confirmation,
            EventType::FirstCommunion => Self::FirstCommunion,
            EventType::BarBatMitzvah => Self::BarBatMitzvah,
            EventType::MilitaryService => Self::MilitaryService,
            EventType::Burial => Self::Burial,
            EventType::Cremation => Self::Cremation,
            EventType::Graduation => Self::Graduation,
            EventType::Immigration => Self::Immigration,
            EventType::Emigration => Self::Emigration,
            EventType::Naturalization => Self::Naturalization,
            EventType::Census => Self::Census,
            EventType::Occupation => Self::Occupation,
            EventType::Residence => Self::Residence,
            EventType::Retirement => Self::Retirement,
            EventType::Will => Self::Will,
            EventType::Probate => Self::Probate,
            EventType::Marriage => Self::Marriage,
            EventType::Divorce => Self::Divorce,
            EventType::Annulment => Self::Annulment,
            EventType::Engagement => Self::Engagement,
            EventType::MarriageBann => Self::MarriageBann,
            EventType::MarriageContract => Self::MarriageContract,
            EventType::MarriageLicense => Self::MarriageLicense,
            EventType::MarriageSettlement => Self::MarriageSettlement,
            EventType::Adoption => Self::Adoption,
            EventType::CivilUnion => Self::CivilUnion,
            EventType::Separation => Self::Separation,
            EventType::DivorceFiled => Self::DivorceFiled,
            EventType::Other => Self::Other,
        }
    }
}

/// Per-person privacy override — stored as a string column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(10))")]
pub enum Privacy {
    #[sea_orm(string_value = "default")]
    Default,
    #[sea_orm(string_value = "public")]
    Public,
    #[sea_orm(string_value = "private")]
    Private,
}

impl From<enums::Privacy> for Privacy {
    fn from(v: enums::Privacy) -> Self {
        match v {
            enums::Privacy::Default => Self::Default,
            enums::Privacy::Public => Self::Public,
            enums::Privacy::Private => Self::Private,
        }
    }
}

impl From<Privacy> for enums::Privacy {
    fn from(v: Privacy) -> Self {
        match v {
            Privacy::Default => Self::Default,
            Privacy::Public => Self::Public,
            Privacy::Private => Self::Private,
        }
    }
}

/// Date qualifier (precision/shape of a date entry) — stored as a string column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(10))")]
pub enum DateQualifier {
    #[sea_orm(string_value = "exact")]
    Exact,
    #[sea_orm(string_value = "about")]
    About,
    #[sea_orm(string_value = "perhaps")]
    Perhaps,
    #[sea_orm(string_value = "before")]
    Before,
    #[sea_orm(string_value = "after")]
    After,
    #[sea_orm(string_value = "or")]
    Or,
    #[sea_orm(string_value = "between")]
    Between,
    #[sea_orm(string_value = "from_age")]
    FromAge,
}

impl From<enums::DateQualifier> for DateQualifier {
    fn from(v: enums::DateQualifier) -> Self {
        match v {
            enums::DateQualifier::Exact => Self::Exact,
            enums::DateQualifier::About => Self::About,
            enums::DateQualifier::Perhaps => Self::Perhaps,
            enums::DateQualifier::Before => Self::Before,
            enums::DateQualifier::After => Self::After,
            enums::DateQualifier::Or => Self::Or,
            enums::DateQualifier::Between => Self::Between,
            enums::DateQualifier::FromAge => Self::FromAge,
        }
    }
}

impl From<DateQualifier> for enums::DateQualifier {
    fn from(v: DateQualifier) -> Self {
        match v {
            DateQualifier::Exact => Self::Exact,
            DateQualifier::About => Self::About,
            DateQualifier::Perhaps => Self::Perhaps,
            DateQualifier::Before => Self::Before,
            DateQualifier::After => Self::After,
            DateQualifier::Or => Self::Or,
            DateQualifier::Between => Self::Between,
            DateQualifier::FromAge => Self::FromAge,
        }
    }
}

/// Calendar system used to record a date — stored as a string column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(20))")]
pub enum Calendar {
    #[sea_orm(string_value = "gregorian")]
    Gregorian,
    #[sea_orm(string_value = "julian")]
    Julian,
    #[sea_orm(string_value = "hebrew")]
    Hebrew,
    #[sea_orm(string_value = "french_republican")]
    FrenchRepublican,
}

impl From<enums::Calendar> for Calendar {
    fn from(v: enums::Calendar) -> Self {
        match v {
            enums::Calendar::Gregorian => Self::Gregorian,
            enums::Calendar::Julian => Self::Julian,
            enums::Calendar::Hebrew => Self::Hebrew,
            enums::Calendar::FrenchRepublican => Self::FrenchRepublican,
        }
    }
}

impl From<Calendar> for enums::Calendar {
    fn from(v: Calendar) -> Self {
        match v {
            Calendar::Gregorian => Self::Gregorian,
            Calendar::Julian => Self::Julian,
            Calendar::Hebrew => Self::Hebrew,
            Calendar::FrenchRepublican => Self::FrenchRepublican,
        }
    }
}

/// Citation confidence level — stored as a string column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(10))")]
pub enum Confidence {
    #[sea_orm(string_value = "very_low")]
    VeryLow,
    #[sea_orm(string_value = "low")]
    Low,
    #[sea_orm(string_value = "medium")]
    Medium,
    #[sea_orm(string_value = "high")]
    High,
    #[sea_orm(string_value = "very_high")]
    VeryHigh,
}

impl From<enums::Confidence> for Confidence {
    fn from(v: enums::Confidence) -> Self {
        match v {
            enums::Confidence::VeryLow => Self::VeryLow,
            enums::Confidence::Low => Self::Low,
            enums::Confidence::Medium => Self::Medium,
            enums::Confidence::High => Self::High,
            enums::Confidence::VeryHigh => Self::VeryHigh,
        }
    }
}

impl From<Confidence> for enums::Confidence {
    fn from(v: Confidence) -> Self {
        match v {
            Confidence::VeryLow => Self::VeryLow,
            Confidence::Low => Self::Low,
            Confidence::Medium => Self::Medium,
            Confidence::High => Self::High,
            Confidence::VeryHigh => Self::VeryHigh,
        }
    }
}
