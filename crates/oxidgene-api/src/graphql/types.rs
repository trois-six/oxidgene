//! GraphQL object types for OxidGene.
//!
//! Each domain type is wrapped in a GraphQL object with resolvers for
//! nested relationships (e.g., Person -> names, events, families).

use crate::media::MediaStore;
use crate::profile::ProfileService;
use crate::service::purge::PurgeQueue;
use async_graphql::{ComplexObject, Context, Enum, ID, Result, SimpleObject};
use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use uuid::Uuid;

use oxidgene_db::repo::{
    CitationRepo, EventFilter, EventRepo, EventWitnessRepo, FamilyChildRepo, FamilySpouseRepo,
    MediaLinkRepo, MediaRepo, NoteRepo, PaginationParams, PersonNameRepo, PersonRepo, PlaceRepo,
    PortraitRow,
};

// ── GraphQL Enums ────────────────────────────────────────────────────

/// Biological sex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlSex {
    Male,
    Female,
    Unknown,
}

impl From<oxidgene_core::Sex> for GqlSex {
    fn from(s: oxidgene_core::Sex) -> Self {
        match s {
            oxidgene_core::Sex::Male => Self::Male,
            oxidgene_core::Sex::Female => Self::Female,
            oxidgene_core::Sex::Unknown => Self::Unknown,
        }
    }
}

impl From<GqlSex> for oxidgene_core::Sex {
    fn from(s: GqlSex) -> Self {
        match s {
            GqlSex::Male => Self::Male,
            GqlSex::Female => Self::Female,
            GqlSex::Unknown => Self::Unknown,
        }
    }
}

/// Per-person privacy override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlPrivacy {
    Default,
    Public,
    Private,
}

impl From<oxidgene_core::Privacy> for GqlPrivacy {
    fn from(p: oxidgene_core::Privacy) -> Self {
        match p {
            oxidgene_core::Privacy::Default => Self::Default,
            oxidgene_core::Privacy::Public => Self::Public,
            oxidgene_core::Privacy::Private => Self::Private,
        }
    }
}

impl From<GqlPrivacy> for oxidgene_core::Privacy {
    fn from(p: GqlPrivacy) -> Self {
        match p {
            GqlPrivacy::Default => Self::Default,
            GqlPrivacy::Public => Self::Public,
            GqlPrivacy::Private => Self::Private,
        }
    }
}

/// Name type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlNameType {
    Birth,
    Married,
    AlsoKnownAs,
    Maiden,
    Religious,
    GivenName,
    Alias,
    Byname,
    Sobriquet,
    Other,
}

impl From<oxidgene_core::NameType> for GqlNameType {
    fn from(n: oxidgene_core::NameType) -> Self {
        match n {
            oxidgene_core::NameType::Birth => Self::Birth,
            oxidgene_core::NameType::Married => Self::Married,
            oxidgene_core::NameType::AlsoKnownAs => Self::AlsoKnownAs,
            oxidgene_core::NameType::Maiden => Self::Maiden,
            oxidgene_core::NameType::Religious => Self::Religious,
            oxidgene_core::NameType::GivenName => Self::GivenName,
            oxidgene_core::NameType::Alias => Self::Alias,
            oxidgene_core::NameType::Byname => Self::Byname,
            oxidgene_core::NameType::Sobriquet => Self::Sobriquet,
            oxidgene_core::NameType::Other => Self::Other,
        }
    }
}

impl From<GqlNameType> for oxidgene_core::NameType {
    fn from(n: GqlNameType) -> Self {
        match n {
            GqlNameType::Birth => Self::Birth,
            GqlNameType::Married => Self::Married,
            GqlNameType::AlsoKnownAs => Self::AlsoKnownAs,
            GqlNameType::Maiden => Self::Maiden,
            GqlNameType::Religious => Self::Religious,
            GqlNameType::GivenName => Self::GivenName,
            GqlNameType::Alias => Self::Alias,
            GqlNameType::Byname => Self::Byname,
            GqlNameType::Sobriquet => Self::Sobriquet,
            GqlNameType::Other => Self::Other,
        }
    }
}

/// Spouse role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlSpouseRole {
    Husband,
    Wife,
    Partner,
}

impl From<oxidgene_core::SpouseRole> for GqlSpouseRole {
    fn from(r: oxidgene_core::SpouseRole) -> Self {
        match r {
            oxidgene_core::SpouseRole::Husband => Self::Husband,
            oxidgene_core::SpouseRole::Wife => Self::Wife,
            oxidgene_core::SpouseRole::Partner => Self::Partner,
        }
    }
}

impl From<GqlSpouseRole> for oxidgene_core::SpouseRole {
    fn from(r: GqlSpouseRole) -> Self {
        match r {
            GqlSpouseRole::Husband => Self::Husband,
            GqlSpouseRole::Wife => Self::Wife,
            GqlSpouseRole::Partner => Self::Partner,
        }
    }
}

/// Child type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlChildType {
    Biological,
    Adopted,
    Foster,
    Step,
    Unknown,
}

impl From<oxidgene_core::ChildType> for GqlChildType {
    fn from(c: oxidgene_core::ChildType) -> Self {
        match c {
            oxidgene_core::ChildType::Biological => Self::Biological,
            oxidgene_core::ChildType::Adopted => Self::Adopted,
            oxidgene_core::ChildType::Foster => Self::Foster,
            oxidgene_core::ChildType::Step => Self::Step,
            oxidgene_core::ChildType::Unknown => Self::Unknown,
        }
    }
}

impl From<GqlChildType> for oxidgene_core::ChildType {
    fn from(c: GqlChildType) -> Self {
        match c {
            GqlChildType::Biological => Self::Biological,
            GqlChildType::Adopted => Self::Adopted,
            GqlChildType::Foster => Self::Foster,
            GqlChildType::Step => Self::Step,
            GqlChildType::Unknown => Self::Unknown,
        }
    }
}

/// What `Default` privacy resolves to, for one tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlTreeDefaultPrivacy {
    Public,
    Private,
}

impl From<oxidgene_core::enums::TreeDefaultPrivacy> for GqlTreeDefaultPrivacy {
    fn from(v: oxidgene_core::enums::TreeDefaultPrivacy) -> Self {
        match v {
            oxidgene_core::enums::TreeDefaultPrivacy::Public => Self::Public,
            oxidgene_core::enums::TreeDefaultPrivacy::Private => Self::Private,
        }
    }
}

impl From<GqlTreeDefaultPrivacy> for oxidgene_core::enums::TreeDefaultPrivacy {
    fn from(v: GqlTreeDefaultPrivacy) -> Self {
        match v {
            GqlTreeDefaultPrivacy::Public => Self::Public,
            GqlTreeDefaultPrivacy::Private => Self::Private,
        }
    }
}

/// What a medium physically is — GEDCOM's `SOURCE_MEDIA_TYPE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlSourceMediaType {
    Audio,
    Book,
    Card,
    Electronic,
    Fiche,
    Film,
    Magazine,
    Manuscript,
    Map,
    Newspaper,
    Photo,
    Tombstone,
    Video,
    Other,
}

impl From<oxidgene_core::enums::SourceMediaType> for GqlSourceMediaType {
    fn from(m: oxidgene_core::enums::SourceMediaType) -> Self {
        use oxidgene_core::enums::SourceMediaType as S;
        match m {
            S::Audio => Self::Audio,
            S::Book => Self::Book,
            S::Card => Self::Card,
            S::Electronic => Self::Electronic,
            S::Fiche => Self::Fiche,
            S::Film => Self::Film,
            S::Magazine => Self::Magazine,
            S::Manuscript => Self::Manuscript,
            S::Map => Self::Map,
            S::Newspaper => Self::Newspaper,
            S::Photo => Self::Photo,
            S::Tombstone => Self::Tombstone,
            S::Video => Self::Video,
            S::Other => Self::Other,
        }
    }
}

impl From<GqlSourceMediaType> for oxidgene_core::enums::SourceMediaType {
    fn from(m: GqlSourceMediaType) -> Self {
        match m {
            GqlSourceMediaType::Audio => Self::Audio,
            GqlSourceMediaType::Book => Self::Book,
            GqlSourceMediaType::Card => Self::Card,
            GqlSourceMediaType::Electronic => Self::Electronic,
            GqlSourceMediaType::Fiche => Self::Fiche,
            GqlSourceMediaType::Film => Self::Film,
            GqlSourceMediaType::Magazine => Self::Magazine,
            GqlSourceMediaType::Manuscript => Self::Manuscript,
            GqlSourceMediaType::Map => Self::Map,
            GqlSourceMediaType::Newspaper => Self::Newspaper,
            GqlSourceMediaType::Photo => Self::Photo,
            GqlSourceMediaType::Tombstone => Self::Tombstone,
            GqlSourceMediaType::Video => Self::Video,
            GqlSourceMediaType::Other => Self::Other,
        }
    }
}

/// What kind of record a medium is — the distinction GEDCOM cannot draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlDocumentCategory {
    Portrait,
    GroupPhoto,
    FamilyDocument,
    CivilRecord,
    ParishRecord,
    NotarialArchive,
    MilitaryArchive,
    Census,
    CoatOfArms,
    Grave,
    Other,
}

impl From<oxidgene_core::enums::DocumentCategory> for GqlDocumentCategory {
    fn from(c: oxidgene_core::enums::DocumentCategory) -> Self {
        use oxidgene_core::enums::DocumentCategory as D;
        match c {
            D::Portrait => Self::Portrait,
            D::GroupPhoto => Self::GroupPhoto,
            D::FamilyDocument => Self::FamilyDocument,
            D::CivilRecord => Self::CivilRecord,
            D::ParishRecord => Self::ParishRecord,
            D::NotarialArchive => Self::NotarialArchive,
            D::MilitaryArchive => Self::MilitaryArchive,
            D::Census => Self::Census,
            D::CoatOfArms => Self::CoatOfArms,
            D::Grave => Self::Grave,
            D::Other => Self::Other,
        }
    }
}

impl From<GqlDocumentCategory> for oxidgene_core::enums::DocumentCategory {
    fn from(c: GqlDocumentCategory) -> Self {
        match c {
            GqlDocumentCategory::Portrait => Self::Portrait,
            GqlDocumentCategory::GroupPhoto => Self::GroupPhoto,
            GqlDocumentCategory::FamilyDocument => Self::FamilyDocument,
            GqlDocumentCategory::CivilRecord => Self::CivilRecord,
            GqlDocumentCategory::ParishRecord => Self::ParishRecord,
            GqlDocumentCategory::NotarialArchive => Self::NotarialArchive,
            GqlDocumentCategory::MilitaryArchive => Self::MilitaryArchive,
            GqlDocumentCategory::Census => Self::Census,
            GqlDocumentCategory::CoatOfArms => Self::CoatOfArms,
            GqlDocumentCategory::Grave => Self::Grave,
            GqlDocumentCategory::Other => Self::Other,
        }
    }
}

/// Date qualifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlDateQualifier {
    Exact,
    About,
    Calculated,
    Estimated,
    Perhaps,
    Before,
    After,
    Or,
    Between,
    FromAge,
}

impl From<oxidgene_core::DateQualifier> for GqlDateQualifier {
    fn from(d: oxidgene_core::DateQualifier) -> Self {
        match d {
            oxidgene_core::DateQualifier::Exact => Self::Exact,
            oxidgene_core::DateQualifier::About => Self::About,
            oxidgene_core::DateQualifier::Calculated => Self::Calculated,
            oxidgene_core::DateQualifier::Estimated => Self::Estimated,
            oxidgene_core::DateQualifier::Perhaps => Self::Perhaps,
            oxidgene_core::DateQualifier::Before => Self::Before,
            oxidgene_core::DateQualifier::After => Self::After,
            oxidgene_core::DateQualifier::Or => Self::Or,
            oxidgene_core::DateQualifier::Between => Self::Between,
            oxidgene_core::DateQualifier::FromAge => Self::FromAge,
        }
    }
}

impl From<GqlDateQualifier> for oxidgene_core::DateQualifier {
    fn from(d: GqlDateQualifier) -> Self {
        match d {
            GqlDateQualifier::Exact => Self::Exact,
            GqlDateQualifier::About => Self::About,
            GqlDateQualifier::Calculated => Self::Calculated,
            GqlDateQualifier::Estimated => Self::Estimated,
            GqlDateQualifier::Perhaps => Self::Perhaps,
            GqlDateQualifier::Before => Self::Before,
            GqlDateQualifier::After => Self::After,
            GqlDateQualifier::Or => Self::Or,
            GqlDateQualifier::Between => Self::Between,
            GqlDateQualifier::FromAge => Self::FromAge,
        }
    }
}

/// Calendar system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlCalendar {
    Gregorian,
    Julian,
    Hebrew,
    FrenchRepublican,
}

impl From<oxidgene_core::Calendar> for GqlCalendar {
    fn from(c: oxidgene_core::Calendar) -> Self {
        match c {
            oxidgene_core::Calendar::Gregorian => Self::Gregorian,
            oxidgene_core::Calendar::Julian => Self::Julian,
            oxidgene_core::Calendar::Hebrew => Self::Hebrew,
            oxidgene_core::Calendar::FrenchRepublican => Self::FrenchRepublican,
        }
    }
}

impl From<GqlCalendar> for oxidgene_core::Calendar {
    fn from(c: GqlCalendar) -> Self {
        match c {
            GqlCalendar::Gregorian => Self::Gregorian,
            GqlCalendar::Julian => Self::Julian,
            GqlCalendar::Hebrew => Self::Hebrew,
            GqlCalendar::FrenchRepublican => Self::FrenchRepublican,
        }
    }
}

/// Event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlEventType {
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
    Adoption,
    CasteName,
    PhysicalDescription,
    Education,
    NationalId,
    NationalOrigin,
    ChildrenCount,
    MarriagesCount,
    Property,
    Religion,
    SocialSecurityNumber,
    NobilityTitle,
    Fact,
    LdsBaptism,
    LdsConfirmation,
    Blessing,
    Ordination,
    Christening,
    AdultChristening,
    Accomplishment,
    Acquisition,
    Membership,
    ChangeName,
    Circumcision,
    Award,
    MilitaryDischarge,
    Degree,
    Distinction,
    Election,
    Excommunication,
    Funeral,
    Hospitalization,
    Illness,
    PassengerList,
    MilitaryDistinction,
    MilitaryPromotion,
    MilitaryMobilization,
    PropertySale,
    Endowment,
    LdsDotation,
    SealingChild,
    SealingSpouse,
    SealingParent,
    FamilyLinkLds,
    NoMarriage,
    NoMention,
    Marriage,
    Divorce,
    Annulment,
    Engagement,
    MarriageBann,
    MarriageContract,
    MarriageLicense,
    MarriageSettlement,
    CivilUnion,
    Separation,
    DivorceFiled,
    Other,
}

impl From<oxidgene_core::EventType> for GqlEventType {
    fn from(e: oxidgene_core::EventType) -> Self {
        match e {
            oxidgene_core::EventType::Birth => Self::Birth,
            oxidgene_core::EventType::Death => Self::Death,
            oxidgene_core::EventType::Baptism => Self::Baptism,
            oxidgene_core::EventType::Confirmation => Self::Confirmation,
            oxidgene_core::EventType::FirstCommunion => Self::FirstCommunion,
            oxidgene_core::EventType::BarBatMitzvah => Self::BarBatMitzvah,
            oxidgene_core::EventType::MilitaryService => Self::MilitaryService,
            oxidgene_core::EventType::Burial => Self::Burial,
            oxidgene_core::EventType::Cremation => Self::Cremation,
            oxidgene_core::EventType::Graduation => Self::Graduation,
            oxidgene_core::EventType::Immigration => Self::Immigration,
            oxidgene_core::EventType::Emigration => Self::Emigration,
            oxidgene_core::EventType::Naturalization => Self::Naturalization,
            oxidgene_core::EventType::Census => Self::Census,
            oxidgene_core::EventType::Occupation => Self::Occupation,
            oxidgene_core::EventType::Residence => Self::Residence,
            oxidgene_core::EventType::Retirement => Self::Retirement,
            oxidgene_core::EventType::Will => Self::Will,
            oxidgene_core::EventType::Probate => Self::Probate,
            oxidgene_core::EventType::Adoption => Self::Adoption,
            oxidgene_core::EventType::CasteName => Self::CasteName,
            oxidgene_core::EventType::PhysicalDescription => Self::PhysicalDescription,
            oxidgene_core::EventType::Education => Self::Education,
            oxidgene_core::EventType::NationalId => Self::NationalId,
            oxidgene_core::EventType::NationalOrigin => Self::NationalOrigin,
            oxidgene_core::EventType::ChildrenCount => Self::ChildrenCount,
            oxidgene_core::EventType::MarriagesCount => Self::MarriagesCount,
            oxidgene_core::EventType::Property => Self::Property,
            oxidgene_core::EventType::Religion => Self::Religion,
            oxidgene_core::EventType::SocialSecurityNumber => Self::SocialSecurityNumber,
            oxidgene_core::EventType::NobilityTitle => Self::NobilityTitle,
            oxidgene_core::EventType::Fact => Self::Fact,
            oxidgene_core::EventType::LdsBaptism => Self::LdsBaptism,
            oxidgene_core::EventType::LdsConfirmation => Self::LdsConfirmation,
            oxidgene_core::EventType::Blessing => Self::Blessing,
            oxidgene_core::EventType::Ordination => Self::Ordination,
            oxidgene_core::EventType::Christening => Self::Christening,
            oxidgene_core::EventType::AdultChristening => Self::AdultChristening,
            oxidgene_core::EventType::Accomplishment => Self::Accomplishment,
            oxidgene_core::EventType::Acquisition => Self::Acquisition,
            oxidgene_core::EventType::Membership => Self::Membership,
            oxidgene_core::EventType::ChangeName => Self::ChangeName,
            oxidgene_core::EventType::Circumcision => Self::Circumcision,
            oxidgene_core::EventType::Award => Self::Award,
            oxidgene_core::EventType::MilitaryDischarge => Self::MilitaryDischarge,
            oxidgene_core::EventType::Degree => Self::Degree,
            oxidgene_core::EventType::Distinction => Self::Distinction,
            oxidgene_core::EventType::Election => Self::Election,
            oxidgene_core::EventType::Excommunication => Self::Excommunication,
            oxidgene_core::EventType::Funeral => Self::Funeral,
            oxidgene_core::EventType::Hospitalization => Self::Hospitalization,
            oxidgene_core::EventType::Illness => Self::Illness,
            oxidgene_core::EventType::PassengerList => Self::PassengerList,
            oxidgene_core::EventType::MilitaryDistinction => Self::MilitaryDistinction,
            oxidgene_core::EventType::MilitaryPromotion => Self::MilitaryPromotion,
            oxidgene_core::EventType::MilitaryMobilization => Self::MilitaryMobilization,
            oxidgene_core::EventType::PropertySale => Self::PropertySale,
            oxidgene_core::EventType::Endowment => Self::Endowment,
            oxidgene_core::EventType::LdsDotation => Self::LdsDotation,
            oxidgene_core::EventType::SealingChild => Self::SealingChild,
            oxidgene_core::EventType::SealingSpouse => Self::SealingSpouse,
            oxidgene_core::EventType::SealingParent => Self::SealingParent,
            oxidgene_core::EventType::FamilyLinkLds => Self::FamilyLinkLds,
            oxidgene_core::EventType::NoMarriage => Self::NoMarriage,
            oxidgene_core::EventType::NoMention => Self::NoMention,
            oxidgene_core::EventType::Marriage => Self::Marriage,
            oxidgene_core::EventType::Divorce => Self::Divorce,
            oxidgene_core::EventType::Annulment => Self::Annulment,
            oxidgene_core::EventType::Engagement => Self::Engagement,
            oxidgene_core::EventType::MarriageBann => Self::MarriageBann,
            oxidgene_core::EventType::MarriageContract => Self::MarriageContract,
            oxidgene_core::EventType::MarriageLicense => Self::MarriageLicense,
            oxidgene_core::EventType::MarriageSettlement => Self::MarriageSettlement,
            oxidgene_core::EventType::CivilUnion => Self::CivilUnion,
            oxidgene_core::EventType::Separation => Self::Separation,
            oxidgene_core::EventType::DivorceFiled => Self::DivorceFiled,
            oxidgene_core::EventType::Other => Self::Other,
        }
    }
}

impl From<GqlEventType> for oxidgene_core::EventType {
    fn from(e: GqlEventType) -> Self {
        match e {
            GqlEventType::Birth => Self::Birth,
            GqlEventType::Death => Self::Death,
            GqlEventType::Baptism => Self::Baptism,
            GqlEventType::Confirmation => Self::Confirmation,
            GqlEventType::FirstCommunion => Self::FirstCommunion,
            GqlEventType::BarBatMitzvah => Self::BarBatMitzvah,
            GqlEventType::MilitaryService => Self::MilitaryService,
            GqlEventType::Burial => Self::Burial,
            GqlEventType::Cremation => Self::Cremation,
            GqlEventType::Graduation => Self::Graduation,
            GqlEventType::Immigration => Self::Immigration,
            GqlEventType::Emigration => Self::Emigration,
            GqlEventType::Naturalization => Self::Naturalization,
            GqlEventType::Census => Self::Census,
            GqlEventType::Occupation => Self::Occupation,
            GqlEventType::Residence => Self::Residence,
            GqlEventType::Retirement => Self::Retirement,
            GqlEventType::Will => Self::Will,
            GqlEventType::Probate => Self::Probate,
            GqlEventType::Adoption => Self::Adoption,
            GqlEventType::CasteName => Self::CasteName,
            GqlEventType::PhysicalDescription => Self::PhysicalDescription,
            GqlEventType::Education => Self::Education,
            GqlEventType::NationalId => Self::NationalId,
            GqlEventType::NationalOrigin => Self::NationalOrigin,
            GqlEventType::ChildrenCount => Self::ChildrenCount,
            GqlEventType::MarriagesCount => Self::MarriagesCount,
            GqlEventType::Property => Self::Property,
            GqlEventType::Religion => Self::Religion,
            GqlEventType::SocialSecurityNumber => Self::SocialSecurityNumber,
            GqlEventType::NobilityTitle => Self::NobilityTitle,
            GqlEventType::Fact => Self::Fact,
            GqlEventType::LdsBaptism => Self::LdsBaptism,
            GqlEventType::LdsConfirmation => Self::LdsConfirmation,
            GqlEventType::Blessing => Self::Blessing,
            GqlEventType::Ordination => Self::Ordination,
            GqlEventType::Christening => Self::Christening,
            GqlEventType::AdultChristening => Self::AdultChristening,
            GqlEventType::Accomplishment => Self::Accomplishment,
            GqlEventType::Acquisition => Self::Acquisition,
            GqlEventType::Membership => Self::Membership,
            GqlEventType::ChangeName => Self::ChangeName,
            GqlEventType::Circumcision => Self::Circumcision,
            GqlEventType::Award => Self::Award,
            GqlEventType::MilitaryDischarge => Self::MilitaryDischarge,
            GqlEventType::Degree => Self::Degree,
            GqlEventType::Distinction => Self::Distinction,
            GqlEventType::Election => Self::Election,
            GqlEventType::Excommunication => Self::Excommunication,
            GqlEventType::Funeral => Self::Funeral,
            GqlEventType::Hospitalization => Self::Hospitalization,
            GqlEventType::Illness => Self::Illness,
            GqlEventType::PassengerList => Self::PassengerList,
            GqlEventType::MilitaryDistinction => Self::MilitaryDistinction,
            GqlEventType::MilitaryPromotion => Self::MilitaryPromotion,
            GqlEventType::MilitaryMobilization => Self::MilitaryMobilization,
            GqlEventType::PropertySale => Self::PropertySale,
            GqlEventType::Endowment => Self::Endowment,
            GqlEventType::LdsDotation => Self::LdsDotation,
            GqlEventType::SealingChild => Self::SealingChild,
            GqlEventType::SealingSpouse => Self::SealingSpouse,
            GqlEventType::SealingParent => Self::SealingParent,
            GqlEventType::FamilyLinkLds => Self::FamilyLinkLds,
            GqlEventType::NoMarriage => Self::NoMarriage,
            GqlEventType::NoMention => Self::NoMention,
            GqlEventType::Marriage => Self::Marriage,
            GqlEventType::Divorce => Self::Divorce,
            GqlEventType::Annulment => Self::Annulment,
            GqlEventType::Engagement => Self::Engagement,
            GqlEventType::MarriageBann => Self::MarriageBann,
            GqlEventType::MarriageContract => Self::MarriageContract,
            GqlEventType::MarriageLicense => Self::MarriageLicense,
            GqlEventType::MarriageSettlement => Self::MarriageSettlement,
            GqlEventType::CivilUnion => Self::CivilUnion,
            GqlEventType::Separation => Self::Separation,
            GqlEventType::DivorceFiled => Self::DivorceFiled,
            GqlEventType::Other => Self::Other,
        }
    }
}

/// Confidence level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlConfidence {
    VeryLow,
    Low,
    Medium,
    High,
    VeryHigh,
}

impl From<oxidgene_core::Confidence> for GqlConfidence {
    fn from(c: oxidgene_core::Confidence) -> Self {
        match c {
            oxidgene_core::Confidence::VeryLow => Self::VeryLow,
            oxidgene_core::Confidence::Low => Self::Low,
            oxidgene_core::Confidence::Medium => Self::Medium,
            oxidgene_core::Confidence::High => Self::High,
            oxidgene_core::Confidence::VeryHigh => Self::VeryHigh,
        }
    }
}

impl From<GqlConfidence> for oxidgene_core::Confidence {
    fn from(c: GqlConfidence) -> Self {
        match c {
            GqlConfidence::VeryLow => Self::VeryLow,
            GqlConfidence::Low => Self::Low,
            GqlConfidence::Medium => Self::Medium,
            GqlConfidence::High => Self::High,
            GqlConfidence::VeryHigh => Self::VeryHigh,
        }
    }
}

// ── Helper ───────────────────────────────────────────────────────────

pub(crate) fn db_from_ctx<'a>(ctx: &'a Context<'_>) -> &'a DatabaseConnection {
    ctx.data_unchecked::<DatabaseConnection>()
}

pub(crate) fn profiles_from_ctx<'a>(ctx: &'a Context<'_>) -> &'a Arc<ProfileService> {
    ctx.data_unchecked::<Arc<ProfileService>>()
}

pub(crate) fn purge_from_ctx<'a>(ctx: &'a Context<'_>) -> &'a PurgeQueue {
    ctx.data_unchecked::<PurgeQueue>()
}

pub(crate) fn media_from_ctx<'a>(ctx: &'a Context<'_>) -> &'a Arc<dyn MediaStore> {
    ctx.data_unchecked::<Arc<dyn MediaStore>>()
}

pub(crate) fn require_local_file_access(ctx: &Context<'_>) -> async_graphql::Result<()> {
    ctx.data_unchecked::<crate::rest::state::LocalFileAccess>()
        .require()
        .map_err(Into::into)
}

// ── PageInfo ─────────────────────────────────────────────────────────

/// Relay-style pagination info.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPageInfo {
    pub has_next_page: bool,
    pub end_cursor: Option<String>,
}

// ── Tree ─────────────────────────────────────────────────────────────

/// A genealogical tree.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct GqlTree {
    pub id: ID,
    pub name: String,
    pub description: Option<String>,
    pub sosa_root_person_id: Option<ID>,
    pub self_person_id: Option<ID>,
    /// What `Default` privacy resolves to for everything in this tree.
    pub default_privacy: GqlTreeDefaultPrivacy,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[ComplexObject]
impl GqlTree {
    /// Count of persons in this tree.
    async fn person_count(&self, ctx: &Context<'_>) -> Result<i64> {
        let db = db_from_ctx(ctx);
        let tree_id = Uuid::parse_str(self.id.as_str())?;
        let params = PaginationParams {
            first: 0,
            after: None,
        };
        let conn = PersonRepo::list(db, tree_id, &params).await?;
        Ok(conn.total_count)
    }

    /// Count of families in this tree.
    async fn family_count(&self, ctx: &Context<'_>) -> Result<i64> {
        let db = db_from_ctx(ctx);
        let tree_id = Uuid::parse_str(self.id.as_str())?;
        let params = PaginationParams {
            first: 0,
            after: None,
        };
        let conn = oxidgene_db::repo::FamilyRepo::list(db, tree_id, &params).await?;
        Ok(conn.total_count)
    }
}

impl From<oxidgene_core::types::Tree> for GqlTree {
    fn from(t: oxidgene_core::types::Tree) -> Self {
        Self {
            id: ID(t.id.to_string()),
            name: t.name,
            description: t.description,
            sosa_root_person_id: t.sosa_root_person_id.map(|id| ID(id.to_string())),
            self_person_id: t.self_person_id.map(|id| ID(id.to_string())),
            default_privacy: t.default_privacy.into(),
            created_at: t.created_at,
            updated_at: t.updated_at,
        }
    }
}

// ── Tree Connection ──────────────────────────────────────────────────

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlTreeEdge {
    pub cursor: String,
    pub node: GqlTree,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlTreeConnection {
    pub edges: Vec<GqlTreeEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

impl From<oxidgene_core::types::Connection<oxidgene_core::types::Tree>> for GqlTreeConnection {
    fn from(c: oxidgene_core::types::Connection<oxidgene_core::types::Tree>) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|e| GqlTreeEdge {
                    cursor: e.cursor,
                    node: e.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: c.page_info.has_next_page,
                end_cursor: c.page_info.end_cursor,
            },
            total_count: c.total_count,
        }
    }
}

// ── Person ───────────────────────────────────────────────────────────

/// A person in a genealogical tree.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct GqlPerson {
    pub id: ID,
    pub tree_id: ID,
    pub sex: GqlSex,
    pub privacy: GqlPrivacy,
    /// The whole media representing this person, if their portrait is one.
    pub portrait_media_id: Option<ID>,
    /// The region of a larger image representing them, if it is a crop.
    pub portrait_vignette_id: Option<ID>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[ComplexObject]
impl GqlPerson {
    /// All names for this person.
    async fn names(&self, ctx: &Context<'_>) -> Result<Vec<GqlPersonName>> {
        let db = db_from_ctx(ctx);
        let id = Uuid::parse_str(self.id.as_str())?;
        let names = PersonNameRepo::list_by_person(db, id).await?;
        Ok(names.into_iter().map(GqlPersonName::from).collect())
    }

    /// Primary name of this person.
    async fn primary_name(&self, ctx: &Context<'_>) -> Result<Option<GqlPersonName>> {
        let db = db_from_ctx(ctx);
        let id = Uuid::parse_str(self.id.as_str())?;
        let names = PersonNameRepo::list_by_person(db, id).await?;
        Ok(names
            .into_iter()
            .find(|n| n.is_primary)
            .map(GqlPersonName::from))
    }

    /// Events associated with this person.
    async fn events(&self, ctx: &Context<'_>) -> Result<Vec<GqlEvent>> {
        let db = db_from_ctx(ctx);
        let person_id = Uuid::parse_str(self.id.as_str())?;
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        let filter = EventFilter {
            event_type: None,
            person_id: Some(person_id),
            family_id: None,
        };
        let params = PaginationParams {
            first: 100,
            after: None,
        };
        let conn = EventRepo::list(db, tree_id, &filter, &params).await?;
        Ok(conn
            .edges
            .into_iter()
            .map(|e| GqlEvent::from(e.node))
            .collect())
    }

    /// Families this person belongs to (as spouse).
    async fn families(&self, ctx: &Context<'_>) -> Result<Vec<GqlFamily>> {
        let db = db_from_ctx(ctx);
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        // Get all families for this tree and filter where person is a spouse
        let person_id = Uuid::parse_str(self.id.as_str())?;
        let params = PaginationParams {
            first: 100,
            after: None,
        };
        let families = oxidgene_db::repo::FamilyRepo::list(db, tree_id, &params).await?;
        let mut result = Vec::new();
        for edge in families.edges {
            let spouses = FamilySpouseRepo::list_by_family(db, edge.node.id).await?;
            if spouses.iter().any(|s| s.person_id == person_id) {
                result.push(GqlFamily::from(edge.node));
            }
        }
        Ok(result)
    }

    /// Citations referencing this person.
    async fn citations(&self, ctx: &Context<'_>) -> Result<Vec<GqlCitation>> {
        let db = db_from_ctx(ctx);
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        let person_id = Uuid::parse_str(self.id.as_str())?;
        // Use note repo pattern — list all sources and filter citations by person_id
        // For now, iterate sources. This is acceptable for MVP.
        let source_params = PaginationParams {
            first: 100,
            after: None,
        };
        let sources = oxidgene_db::repo::SourceRepo::list(db, tree_id, &source_params).await?;
        let mut citations = Vec::new();
        for se in sources.edges {
            let cits = CitationRepo::list_by_source(db, se.node.id).await?;
            for c in cits {
                if c.person_id == Some(person_id) {
                    citations.push(GqlCitation::from(c));
                }
            }
        }
        Ok(citations)
    }

    /// Media linked to this person.
    async fn media(&self, ctx: &Context<'_>) -> Result<Vec<GqlMedia>> {
        let db = db_from_ctx(ctx);
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        let person_id = Uuid::parse_str(self.id.as_str())?;
        let media_params = PaginationParams {
            first: 100,
            after: None,
        };
        let media_list = MediaRepo::list(db, tree_id, &media_params).await?;
        let mut result = Vec::new();
        for me in media_list.edges {
            let links = MediaLinkRepo::list_by_media(db, me.node.id).await?;
            if links.iter().any(|l| l.person_id == Some(person_id)) {
                result.push(GqlMedia::from(me.node));
            }
        }
        Ok(result)
    }

    /// Notes attached to this person.
    async fn notes(&self, ctx: &Context<'_>) -> Result<Vec<GqlNote>> {
        let db = db_from_ctx(ctx);
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        let person_id = Uuid::parse_str(self.id.as_str())?;
        let notes =
            NoteRepo::list_by_entity(db, tree_id, Some(person_id), None, None, None, None).await?;
        Ok(notes.into_iter().map(GqlNote::from).collect())
    }
}

impl From<oxidgene_core::types::Person> for GqlPerson {
    fn from(p: oxidgene_core::types::Person) -> Self {
        Self {
            id: ID(p.id.to_string()),
            tree_id: ID(p.tree_id.to_string()),
            sex: p.sex.into(),
            privacy: p.privacy.into(),
            portrait_media_id: p.portrait_media_id.map(|id| ID(id.to_string())),
            portrait_vignette_id: p.portrait_vignette_id.map(|id| ID(id.to_string())),
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

// ── Person Connection ────────────────────────────────────────────────

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPersonEdge {
    pub cursor: String,
    pub node: GqlPerson,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPersonConnection {
    pub edges: Vec<GqlPersonEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

impl From<oxidgene_core::types::Connection<oxidgene_core::types::Person>> for GqlPersonConnection {
    fn from(c: oxidgene_core::types::Connection<oxidgene_core::types::Person>) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|e| GqlPersonEdge {
                    cursor: e.cursor,
                    node: e.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: c.page_info.has_next_page,
                end_cursor: c.page_info.end_cursor,
            },
            total_count: c.total_count,
        }
    }
}

// ── PersonWithDepth ──────────────────────────────────────────────────

/// A person with ancestry depth info.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPersonWithDepth {
    pub person: GqlPerson,
    pub depth: i32,
}

// ── PersonName ───────────────────────────────────────────────────────

/// A person name.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPersonName {
    pub id: ID,
    pub person_id: ID,
    pub name_type: GqlNameType,
    pub given_names: Option<String>,
    /// Surname root, particle excluded — see `surnamePrefix`.
    pub surname: Option<String>,
    /// The surname particle, GEDCOM `SPFX` ("de la", "van der").
    pub surname_prefix: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub is_primary: bool,
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<oxidgene_core::types::PersonName> for GqlPersonName {
    fn from(n: oxidgene_core::types::PersonName) -> Self {
        Self {
            id: ID(n.id.to_string()),
            person_id: ID(n.person_id.to_string()),
            name_type: n.name_type.into(),
            given_names: n.given_names,
            surname: n.surname,
            surname_prefix: n.surname_prefix,
            prefix: n.prefix,
            suffix: n.suffix,
            nickname: n.nickname,
            is_primary: n.is_primary,
            sort_order: n.sort_order,
            created_at: n.created_at,
            updated_at: n.updated_at,
        }
    }
}

// ── Family ───────────────────────────────────────────────────────────

/// A family unit.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct GqlFamily {
    pub id: ID,
    pub tree_id: ID,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[ComplexObject]
impl GqlFamily {
    /// Spouses in this family.
    async fn spouses(&self, ctx: &Context<'_>) -> Result<Vec<GqlFamilySpouseDetail>> {
        let db = db_from_ctx(ctx);
        let family_id = Uuid::parse_str(self.id.as_str())?;
        let spouses = FamilySpouseRepo::list_by_family(db, family_id).await?;
        let mut result = Vec::new();
        for s in spouses {
            let person = PersonRepo::get(db, s.person_id).await?;
            result.push(GqlFamilySpouseDetail {
                id: ID(s.id.to_string()),
                person: GqlPerson::from(person),
                role: s.role.into(),
                sort_order: s.sort_order,
            });
        }
        Ok(result)
    }

    /// Children in this family.
    async fn children(&self, ctx: &Context<'_>) -> Result<Vec<GqlFamilyChildDetail>> {
        let db = db_from_ctx(ctx);
        let family_id = Uuid::parse_str(self.id.as_str())?;
        let children = FamilyChildRepo::list_by_family(db, family_id).await?;
        let mut result = Vec::new();
        for c in children {
            let person = PersonRepo::get(db, c.person_id).await?;
            result.push(GqlFamilyChildDetail {
                id: ID(c.id.to_string()),
                person: GqlPerson::from(person),
                child_type: c.child_type.into(),
                sort_order: c.sort_order,
            });
        }
        Ok(result)
    }

    /// Events associated with this family.
    async fn events(&self, ctx: &Context<'_>) -> Result<Vec<GqlEvent>> {
        let db = db_from_ctx(ctx);
        let family_id = Uuid::parse_str(self.id.as_str())?;
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        let filter = EventFilter {
            event_type: None,
            person_id: None,
            family_id: Some(family_id),
        };
        let params = PaginationParams {
            first: 100,
            after: None,
        };
        let conn = EventRepo::list(db, tree_id, &filter, &params).await?;
        Ok(conn
            .edges
            .into_iter()
            .map(|e| GqlEvent::from(e.node))
            .collect())
    }
}

impl From<oxidgene_core::types::Family> for GqlFamily {
    fn from(f: oxidgene_core::types::Family) -> Self {
        Self {
            id: ID(f.id.to_string()),
            tree_id: ID(f.tree_id.to_string()),
            created_at: f.created_at,
            updated_at: f.updated_at,
        }
    }
}

// ── Family Connection ────────────────────────────────────────────────

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlFamilyEdge {
    pub cursor: String,
    pub node: GqlFamily,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlFamilyConnection {
    pub edges: Vec<GqlFamilyEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

impl From<oxidgene_core::types::Connection<oxidgene_core::types::Family>> for GqlFamilyConnection {
    fn from(c: oxidgene_core::types::Connection<oxidgene_core::types::Family>) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|e| GqlFamilyEdge {
                    cursor: e.cursor,
                    node: e.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: c.page_info.has_next_page,
                end_cursor: c.page_info.end_cursor,
            },
            total_count: c.total_count,
        }
    }
}

// ── FamilySpouseDetail ───────────────────────────────────────────────

/// A spouse with resolved person data.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlFamilySpouseDetail {
    pub id: ID,
    pub person: GqlPerson,
    pub role: GqlSpouseRole,
    pub sort_order: i32,
}

// ── FamilyChildDetail ────────────────────────────────────────────────

/// A child with resolved person data.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlFamilyChildDetail {
    pub id: ID,
    pub person: GqlPerson,
    pub child_type: GqlChildType,
    pub sort_order: i32,
}

// ── Event ────────────────────────────────────────────────────────────

/// A genealogical event.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct GqlEvent {
    pub id: ID,
    pub tree_id: ID,
    pub event_type: GqlEventType,
    pub date_value: Option<String>,
    pub date_sort: Option<String>,
    pub date_qualifier: GqlDateQualifier,
    pub date_value2: Option<String>,
    pub calendar: GqlCalendar,
    pub cause: Option<String>,
    pub place_id: Option<ID>,
    pub person_id: Option<ID>,
    pub family_id: Option<ID>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[ComplexObject]
impl GqlEvent {
    /// Resolved place for this event.
    async fn place(&self, ctx: &Context<'_>) -> Result<Option<GqlPlace>> {
        let Some(ref pid) = self.place_id else {
            return Ok(None);
        };
        let db = db_from_ctx(ctx);
        let id = Uuid::parse_str(pid.as_str())?;
        match PlaceRepo::get(db, id).await {
            Ok(p) => Ok(Some(GqlPlace::from(p))),
            Err(_) => Ok(None),
        }
    }

    /// Resolved person for this event.
    async fn person(&self, ctx: &Context<'_>) -> Result<Option<GqlPerson>> {
        let Some(ref pid) = self.person_id else {
            return Ok(None);
        };
        let db = db_from_ctx(ctx);
        let id = Uuid::parse_str(pid.as_str())?;
        match PersonRepo::get(db, id).await {
            Ok(p) => Ok(Some(GqlPerson::from(p))),
            Err(_) => Ok(None),
        }
    }

    /// Resolved family for this event.
    async fn family(&self, ctx: &Context<'_>) -> Result<Option<GqlFamily>> {
        let Some(ref fid) = self.family_id else {
            return Ok(None);
        };
        let db = db_from_ctx(ctx);
        let id = Uuid::parse_str(fid.as_str())?;
        match oxidgene_db::repo::FamilyRepo::get(db, id).await {
            Ok(f) => Ok(Some(GqlFamily::from(f))),
            Err(_) => Ok(None),
        }
    }

    /// Citations for this event.
    async fn citations(&self, ctx: &Context<'_>) -> Result<Vec<GqlCitation>> {
        let db = db_from_ctx(ctx);
        let event_id = Uuid::parse_str(self.id.as_str())?;
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        let source_params = PaginationParams {
            first: 100,
            after: None,
        };
        let sources = oxidgene_db::repo::SourceRepo::list(db, tree_id, &source_params).await?;
        let mut citations = Vec::new();
        for se in sources.edges {
            let cits = CitationRepo::list_by_source(db, se.node.id).await?;
            for c in cits {
                if c.event_id == Some(event_id) {
                    citations.push(GqlCitation::from(c));
                }
            }
        }
        Ok(citations)
    }

    /// Media linked to this event.
    async fn media(&self, ctx: &Context<'_>) -> Result<Vec<GqlMedia>> {
        let db = db_from_ctx(ctx);
        let event_id = Uuid::parse_str(self.id.as_str())?;
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        let media_params = PaginationParams {
            first: 100,
            after: None,
        };
        let media_list = MediaRepo::list(db, tree_id, &media_params).await?;
        let mut result = Vec::new();
        for me in media_list.edges {
            let links = MediaLinkRepo::list_by_media(db, me.node.id).await?;
            if links.iter().any(|l| l.event_id == Some(event_id)) {
                result.push(GqlMedia::from(me.node));
            }
        }
        Ok(result)
    }

    /// Notes for this event.
    async fn notes(&self, ctx: &Context<'_>) -> Result<Vec<GqlNote>> {
        let db = db_from_ctx(ctx);
        let tree_id = Uuid::parse_str(self.tree_id.as_str())?;
        let event_id = Uuid::parse_str(self.id.as_str())?;
        let notes =
            NoteRepo::list_by_entity(db, tree_id, None, Some(event_id), None, None, None).await?;
        Ok(notes.into_iter().map(GqlNote::from).collect())
    }

    /// Witnesses (godparents, etc.) linked to this event.
    async fn witnesses(&self, ctx: &Context<'_>) -> Result<Vec<GqlEventWitness>> {
        let db = db_from_ctx(ctx);
        let event_id = Uuid::parse_str(self.id.as_str())?;
        let witnesses = EventWitnessRepo::list_by_event(db, event_id).await?;
        Ok(witnesses.into_iter().map(GqlEventWitness::from).collect())
    }
}

impl From<oxidgene_core::types::Event> for GqlEvent {
    fn from(e: oxidgene_core::types::Event) -> Self {
        Self {
            id: ID(e.id.to_string()),
            tree_id: ID(e.tree_id.to_string()),
            event_type: e.event_type.into(),
            date_value: e.date_value,
            date_sort: e.date_sort.map(|d| d.to_string()),
            date_qualifier: e.date_qualifier.into(),
            date_value2: e.date_value2,
            calendar: e.calendar.into(),
            cause: e.cause,
            place_id: e.place_id.map(|id| ID(id.to_string())),
            person_id: e.person_id.map(|id| ID(id.to_string())),
            family_id: e.family_id.map(|id| ID(id.to_string())),
            description: e.description,
            created_at: e.created_at,
            updated_at: e.updated_at,
        }
    }
}

// ── Event Witness ────────────────────────────────────────────────────

/// A witness (or godparent, etc.) linked to an event — a pointer to another
/// person in the tree, mirroring GEDCOM's `ASSO`/`RELA` association.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct GqlEventWitness {
    pub id: ID,
    pub event_id: ID,
    pub person_id: ID,
    pub relation: Option<String>,
    pub sort_order: i32,
}

#[ComplexObject]
impl GqlEventWitness {
    /// Resolved person for this witness.
    async fn person(&self, ctx: &Context<'_>) -> Result<Option<GqlPerson>> {
        let db = db_from_ctx(ctx);
        let id = Uuid::parse_str(self.person_id.as_str())?;
        match PersonRepo::get(db, id).await {
            Ok(p) => Ok(Some(GqlPerson::from(p))),
            Err(_) => Ok(None),
        }
    }
}

impl From<oxidgene_core::types::EventWitness> for GqlEventWitness {
    fn from(w: oxidgene_core::types::EventWitness) -> Self {
        Self {
            id: ID(w.id.to_string()),
            event_id: ID(w.event_id.to_string()),
            person_id: ID(w.person_id.to_string()),
            relation: w.relation,
            sort_order: w.sort_order,
        }
    }
}

// ── Event Connection ─────────────────────────────────────────────────

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlEventEdge {
    pub cursor: String,
    pub node: GqlEvent,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlEventConnection {
    pub edges: Vec<GqlEventEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

impl From<oxidgene_core::types::Connection<oxidgene_core::types::Event>> for GqlEventConnection {
    fn from(c: oxidgene_core::types::Connection<oxidgene_core::types::Event>) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|e| GqlEventEdge {
                    cursor: e.cursor,
                    node: e.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: c.page_info.has_next_page,
                end_cursor: c.page_info.end_cursor,
            },
            total_count: c.total_count,
        }
    }
}

// ── Place ────────────────────────────────────────────────────────────

/// A geographic place.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPlace {
    pub id: ID,
    pub tree_id: ID,
    pub name: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<oxidgene_core::types::Place> for GqlPlace {
    fn from(p: oxidgene_core::types::Place) -> Self {
        Self {
            id: ID(p.id.to_string()),
            tree_id: ID(p.tree_id.to_string()),
            name: p.name,
            latitude: p.latitude,
            longitude: p.longitude,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

// ── Place Connection ─────────────────────────────────────────────────

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPlaceEdge {
    pub cursor: String,
    pub node: GqlPlace,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPlaceConnection {
    pub edges: Vec<GqlPlaceEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

impl From<oxidgene_core::types::Connection<oxidgene_core::types::Place>> for GqlPlaceConnection {
    fn from(c: oxidgene_core::types::Connection<oxidgene_core::types::Place>) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|e| GqlPlaceEdge {
                    cursor: e.cursor,
                    node: e.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: c.page_info.has_next_page,
                end_cursor: c.page_info.end_cursor,
            },
            total_count: c.total_count,
        }
    }
}

// ── Source ────────────────────────────────────────────────────────────

/// A bibliographic source.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct GqlSource {
    pub id: ID,
    pub tree_id: ID,
    pub title: String,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub abbreviation: Option<String>,
    pub repository_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[ComplexObject]
impl GqlSource {
    /// Citations from this source.
    async fn citations(&self, ctx: &Context<'_>) -> Result<Vec<GqlCitation>> {
        let db = db_from_ctx(ctx);
        let id = Uuid::parse_str(self.id.as_str())?;
        let cits = CitationRepo::list_by_source(db, id).await?;
        Ok(cits.into_iter().map(GqlCitation::from).collect())
    }
}

impl From<oxidgene_core::types::Source> for GqlSource {
    fn from(s: oxidgene_core::types::Source) -> Self {
        Self {
            id: ID(s.id.to_string()),
            tree_id: ID(s.tree_id.to_string()),
            title: s.title,
            author: s.author,
            publisher: s.publisher,
            abbreviation: s.abbreviation,
            repository_name: s.repository_name,
            created_at: s.created_at,
            updated_at: s.updated_at,
        }
    }
}

// ── Source Connection ────────────────────────────────────────────────

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlSourceEdge {
    pub cursor: String,
    pub node: GqlSource,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlSourceConnection {
    pub edges: Vec<GqlSourceEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

impl From<oxidgene_core::types::Connection<oxidgene_core::types::Source>> for GqlSourceConnection {
    fn from(c: oxidgene_core::types::Connection<oxidgene_core::types::Source>) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|e| GqlSourceEdge {
                    cursor: e.cursor,
                    node: e.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: c.page_info.has_next_page,
                end_cursor: c.page_info.end_cursor,
            },
            total_count: c.total_count,
        }
    }
}

// ── Citation ─────────────────────────────────────────────────────────

/// A citation linking a source to an entity.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlCitation {
    pub id: ID,
    pub source_id: ID,
    pub person_id: Option<ID>,
    pub event_id: Option<ID>,
    pub family_id: Option<ID>,
    pub page: Option<String>,
    pub confidence: GqlConfidence,
    pub text: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<oxidgene_core::types::Citation> for GqlCitation {
    fn from(c: oxidgene_core::types::Citation) -> Self {
        Self {
            id: ID(c.id.to_string()),
            source_id: ID(c.source_id.to_string()),
            person_id: c.person_id.map(|id| ID(id.to_string())),
            event_id: c.event_id.map(|id| ID(id.to_string())),
            family_id: c.family_id.map(|id| ID(id.to_string())),
            page: c.page,
            confidence: c.confidence.into(),
            text: c.text,
            created_at: c.created_at,
            updated_at: c.updated_at,
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlCitationEdge {
    pub cursor: String,
    pub node: GqlCitation,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlCitationConnection {
    pub edges: Vec<GqlCitationEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

impl From<oxidgene_core::types::Connection<oxidgene_core::types::Citation>>
    for GqlCitationConnection
{
    fn from(c: oxidgene_core::types::Connection<oxidgene_core::types::Citation>) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|edge| GqlCitationEdge {
                    cursor: edge.cursor,
                    node: edge.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: c.page_info.has_next_page,
                end_cursor: c.page_info.end_cursor,
            },
            total_count: c.total_count,
        }
    }
}

// ── Media ────────────────────────────────────────────────────────────

/// A media file.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMedia {
    pub id: ID,
    pub tree_id: ID,
    pub file_name: String,
    pub mime_type: String,
    /// Path as it appears in GEDCOM. Not a URL and not where our copy lives —
    /// fetch the bytes from `/api/v1/trees/{treeId}/media/{id}/file`.
    pub file_path: String,
    /// Key of the stored bytes, or null when the record names a file we have
    /// never received (every GEDCOM-imported row starts that way).
    pub storage_key: Option<String>,
    /// Hex SHA-256 of the stored bytes.
    pub sha256: Option<String>,
    /// Key of the generated thumbnail; null for PDFs and byte-less records.
    pub thumbnail_key: Option<String>,
    /// Intrinsic pixel size, after applying any EXIF orientation.
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// Pages in the document; 1 for photos and single-page files. For an
    /// `isDocument` row it is the number of page images assembled into it.
    pub page_count: i32,
    /// The document this is a page of, if it is one.
    pub parent_media_id: Option<ID>,
    /// Zero-based position within that document.
    pub page_index: i32,
    /// True when this row *is* a multi-page document rather than a file. Such
    /// a row carries the title, date, place, description and note that
    /// describe the document as a whole, and holds no bytes.
    pub is_document: bool,
    pub file_size: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub date_value: Option<String>,
    pub date_sort: Option<String>,
    /// Whether this is shown when the tree is published.
    pub privacy: GqlPrivacy,
    /// What the medium physically is, in GEDCOM's own vocabulary.
    pub source_media_type: GqlSourceMediaType,
    /// What kind of record it is; null when unclassified.
    pub document_category: Option<GqlDocumentCategory>,
    /// Free-form labels for this media or document.
    pub tags: Vec<String>,
    pub place_id: Option<ID>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<oxidgene_core::types::Media> for GqlMedia {
    fn from(m: oxidgene_core::types::Media) -> Self {
        Self {
            id: ID(m.id.to_string()),
            tree_id: ID(m.tree_id.to_string()),
            file_name: m.file_name,
            mime_type: m.mime_type,
            file_path: m.file_path,
            storage_key: m.storage_key,
            sha256: m.sha256,
            thumbnail_key: m.thumbnail_key,
            width: m.width,
            height: m.height,
            page_count: m.page_count,
            parent_media_id: m.parent_media_id.map(|id| ID(id.to_string())),
            page_index: m.page_index,
            is_document: m.is_document,
            file_size: m.file_size,
            title: m.title,
            description: m.description,
            date_value: m.date_value,
            date_sort: m.date_sort.map(|d| d.to_string()),
            privacy: m.privacy.into(),
            source_media_type: m.source_media_type.into(),
            document_category: m.document_category.map(Into::into),
            tags: m.tags,
            place_id: m.place_id.map(|id| ID(id.to_string())),
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

/// A media together with the link that attached it — one gallery tile.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMediaWithLink {
    pub link_id: ID,
    pub sort_order: i32,
    pub media: GqlMedia,
}

/// A flat media link with the display fields needed by tree-wide consumers.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlTreeMediaLink {
    pub link_id: ID,
    pub entity_id: ID,
    pub entity_type: String,
    pub media_id: ID,
    pub file_path: String,
    pub file_name: String,
    pub mime_type: String,
    pub has_thumbnail: bool,
}

impl From<oxidgene_db::repo::MediaLinkRow> for GqlTreeMediaLink {
    fn from(link: oxidgene_db::repo::MediaLinkRow) -> Self {
        Self {
            link_id: ID(link.link_id.to_string()),
            entity_id: ID(link.entity_id.to_string()),
            entity_type: link.entity_type,
            media_id: ID(link.media_id.to_string()),
            file_path: link.file_path,
            file_name: link.file_name,
            mime_type: link.mime_type,
            has_thumbnail: link.has_thumbnail,
        }
    }
}

// ── Vignette ─────────────────────────────────────────────────────────

/// A rectangular region of a media file, kept as coordinates rather than as a
/// second copy of the pixels.
///
/// Fetch the cropped image itself from
/// `/api/v1/trees/{treeId}/vignettes/{id}/image`.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlVignette {
    pub id: ID,
    pub media_id: ID,
    /// Zero-based page of a multi-page document; 0 for a photo.
    pub page: i32,
    /// Crop rectangle, in the source image's own pixel coordinates.
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub person_id: Option<ID>,
    pub event_id: Option<ID>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<oxidgene_core::types::Vignette> for GqlVignette {
    fn from(v: oxidgene_core::types::Vignette) -> Self {
        Self {
            id: ID(v.id.to_string()),
            media_id: ID(v.media_id.to_string()),
            page: v.page,
            x: v.x,
            y: v.y,
            width: v.width,
            height: v.height,
            person_id: v.person_id.map(|id| ID(id.to_string())),
            event_id: v.event_id.map(|id| ID(id.to_string())),
            created_at: v.created_at,
            updated_at: v.updated_at,
        }
    }
}

// ── Media Connection ─────────────────────────────────────────────────

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMediaEdge {
    pub cursor: String,
    pub node: GqlMedia,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMediaConnection {
    pub edges: Vec<GqlMediaEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

impl From<oxidgene_core::types::Connection<oxidgene_core::types::Media>> for GqlMediaConnection {
    fn from(c: oxidgene_core::types::Connection<oxidgene_core::types::Media>) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|e| GqlMediaEdge {
                    cursor: e.cursor,
                    node: e.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: c.page_info.has_next_page,
                end_cursor: c.page_info.end_cursor,
            },
            total_count: c.total_count,
        }
    }
}

// ── MediaLink ────────────────────────────────────────────────────────

/// A link between media and an entity.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMediaLink {
    pub id: ID,
    pub media_id: ID,
    pub person_id: Option<ID>,
    pub event_id: Option<ID>,
    pub source_id: Option<ID>,
    pub family_id: Option<ID>,
    pub sort_order: i32,
}

impl From<oxidgene_core::types::MediaLink> for GqlMediaLink {
    fn from(l: oxidgene_core::types::MediaLink) -> Self {
        Self {
            id: ID(l.id.to_string()),
            media_id: ID(l.media_id.to_string()),
            person_id: l.person_id.map(|id| ID(id.to_string())),
            event_id: l.event_id.map(|id| ID(id.to_string())),
            source_id: l.source_id.map(|id| ID(id.to_string())),
            family_id: l.family_id.map(|id| ID(id.to_string())),
            sort_order: l.sort_order,
        }
    }
}

// ── FamilySpouse (raw) ──────────────────────────────────────────────

/// Raw family spouse record (returned from mutations).
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlFamilySpouse {
    pub id: ID,
    pub family_id: ID,
    pub person_id: ID,
    pub role: GqlSpouseRole,
    pub sort_order: i32,
}

impl From<oxidgene_core::types::FamilySpouse> for GqlFamilySpouse {
    fn from(s: oxidgene_core::types::FamilySpouse) -> Self {
        Self {
            id: ID(s.id.to_string()),
            family_id: ID(s.family_id.to_string()),
            person_id: ID(s.person_id.to_string()),
            role: s.role.into(),
            sort_order: s.sort_order,
        }
    }
}

// ── FamilyChild (raw) ───────────────────────────────────────────────

/// Raw family child record (returned from mutations).
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlFamilyChild {
    pub id: ID,
    pub family_id: ID,
    pub person_id: ID,
    pub child_type: GqlChildType,
    pub sort_order: i32,
}

impl From<oxidgene_core::types::FamilyChild> for GqlFamilyChild {
    fn from(c: oxidgene_core::types::FamilyChild) -> Self {
        Self {
            id: ID(c.id.to_string()),
            family_id: ID(c.family_id.to_string()),
            person_id: ID(c.person_id.to_string()),
            child_type: c.child_type.into(),
            sort_order: c.sort_order,
        }
    }
}

// ── Note ─────────────────────────────────────────────────────────────

/// A textual note.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlNote {
    pub id: ID,
    pub tree_id: ID,
    pub text: String,
    pub person_id: Option<ID>,
    pub event_id: Option<ID>,
    pub family_id: Option<ID>,
    pub source_id: Option<ID>,
    pub media_id: Option<ID>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<oxidgene_core::types::Note> for GqlNote {
    fn from(n: oxidgene_core::types::Note) -> Self {
        Self {
            id: ID(n.id.to_string()),
            tree_id: ID(n.tree_id.to_string()),
            text: n.text,
            person_id: n.person_id.map(|id| ID(id.to_string())),
            event_id: n.event_id.map(|id| ID(id.to_string())),
            family_id: n.family_id.map(|id| ID(id.to_string())),
            source_id: n.source_id.map(|id| ID(id.to_string())),
            media_id: n.media_id.map(|id| ID(id.to_string())),
            created_at: n.created_at,
            updated_at: n.updated_at,
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlNoteEdge {
    pub cursor: String,
    pub node: GqlNote,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlNoteConnection {
    pub edges: Vec<GqlNoteEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

impl From<oxidgene_core::types::Connection<oxidgene_core::types::Note>> for GqlNoteConnection {
    fn from(c: oxidgene_core::types::Connection<oxidgene_core::types::Note>) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|edge| GqlNoteEdge {
                    cursor: edge.cursor,
                    node: edge.node.into(),
                })
                .collect(),
            page_info: GqlPageInfo {
                has_next_page: c.page_info.has_next_page,
                end_cursor: c.page_info.end_cursor,
            },
            total_count: c.total_count,
        }
    }
}

// ── Import/Export Results ─────────────────────────────────────────────

/// Result of an import operation, whatever the source format.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlImportResult {
    pub persons_count: i32,
    pub families_count: i32,
    pub events_count: i32,
    pub sources_count: i32,
    pub media_count: i32,
    pub places_count: i32,
    pub notes_count: i32,
    pub warnings: Vec<String>,
}

/// Result of a GEDCOM export operation.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlExportGedcomResult {
    pub gedcom: String,
    pub warnings: Vec<String>,
}

/// Identifier returned after queuing a background job.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlBackgroundJobStarted {
    pub job_id: ID,
}

/// Pollable state of a durable GEDZIP export.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlExportJobStatus {
    pub phase: String,
    pub done: i64,
    pub total: i64,
    pub download_url: Option<String>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

/// Pollable state of a durable genealogy file import.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlImportJobStatus {
    pub phase: String,
    pub done: i64,
    pub total: i64,
    pub result: Option<GqlImportResult>,
    pub geneanet_result: Option<GqlGeneanetImportResult>,
    pub error: Option<String>,
}

// ── Geneanet import wizard ──────────────────────────────────────────

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGeneanetInspection {
    pub person_count: i64,
    pub family_count: i64,
    pub skipped_blocks: i64,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGeneanetIndexedArchive {
    pub path: String,
    pub file_name: String,
    pub file_count: i64,
    pub image_count: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGeneanetArchiveIndex {
    pub archives: Vec<GqlGeneanetIndexedArchive>,
    pub file_count: i64,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGeneanetPreview {
    pub person_count: i64,
    pub photo_count: i64,
    pub persons_with_photo: i64,
    pub attachment_count: i64,
    pub in_archives: i64,
    pub to_match: i64,
    pub to_download: i64,
    pub group_photos: i64,
    pub unlinked_views: i64,
    pub documents: i64,
    pub document_pages: i64,
    pub unlinked_names: i64,
    pub outside_tree: i64,
    pub ambiguous: i64,
    pub unlinked_names_sample: Vec<String>,
    pub outside_tree_names: Vec<String>,
    pub ambiguous_names: Vec<String>,
    pub mismatch: bool,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGeneanetNeededMedia {
    pub deposit_id: i64,
    pub view_id: i64,
    pub page: Option<i64>,
    pub url: String,
    pub original: bool,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGeneanetDepositSize {
    pub deposit_id: i64,
    pub size: i64,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGeneanetMediaPath {
    pub url: String,
    pub path: String,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGeneanetSession {
    pub collection: String,
    pub deposit_sizes: Vec<GqlGeneanetDepositSize>,
    pub account: Option<String>,
    pub photo_count: i64,
    pub media: Vec<GqlGeneanetMediaPath>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGeneanetSessionArchive {
    pub archive_base64: String,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGeneanetImportResult {
    pub persons_count: i64,
    pub families_count: i64,
    pub events_count: i64,
    pub sources_count: i64,
    pub places_count: i64,
    pub notes_count: i64,
    pub media_count: i64,
    pub links_count: i64,
    pub portraits_count: i64,
    pub isolated_count: i64,
    pub vignettes_count: i64,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

impl From<crate::service::geneanet::Preview> for GqlGeneanetPreview {
    fn from(preview: crate::service::geneanet::Preview) -> Self {
        Self {
            person_count: preview.person_count as i64,
            photo_count: preview.photo_count as i64,
            persons_with_photo: preview.persons_with_photo as i64,
            attachment_count: preview.attachment_count as i64,
            in_archives: preview.in_archives as i64,
            to_match: preview.to_match as i64,
            to_download: preview.to_download as i64,
            group_photos: preview.group_photos as i64,
            unlinked_views: preview.unlinked_views as i64,
            documents: preview.documents as i64,
            document_pages: preview.document_pages as i64,
            unlinked_names: preview.unlinked_names as i64,
            outside_tree: preview.outside_tree as i64,
            ambiguous: preview.ambiguous as i64,
            unlinked_names_sample: preview.unlinked_names_sample,
            outside_tree_names: preview.outside_tree_names,
            ambiguous_names: preview.ambiguous_names,
            mismatch: preview.mismatch,
        }
    }
}

impl From<crate::service::geneanet::NeededMedia> for GqlGeneanetNeededMedia {
    fn from(needed: crate::service::geneanet::NeededMedia) -> Self {
        Self {
            deposit_id: needed.deposit_id,
            view_id: needed.view_id,
            page: needed.page,
            url: needed.url,
            original: needed.original,
        }
    }
}

// ── Projection GraphQL types ────────────────────────────────────────────────

/// A denormalized person profile — everything needed for card/detail
/// display in a single object.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPersonProfile {
    pub person_id: ID,
    pub tree_id: ID,
    pub sex: GqlSex,
    pub primary_name: Option<GqlProfileName>,
    pub other_names: Vec<GqlProfileName>,
    pub birth: Option<GqlProfileEvent>,
    pub death: Option<GqlProfileEvent>,
    pub baptism: Option<GqlProfileEvent>,
    pub burial: Option<GqlProfileEvent>,
    pub occupation: Option<String>,
    pub other_events: Vec<GqlProfileEvent>,
    pub families_as_spouse: Vec<GqlProfileFamilyLink>,
    pub family_as_child: Option<GqlProfileChildLink>,
    pub primary_media: Option<GqlProfileMediaRef>,
    pub media_count: i32,
    pub citation_count: i32,
    pub note_count: i32,
    pub updated_at: DateTime<Utc>,
    pub built_at: DateTime<Utc>,
}

/// A name entry, pre-computed for display.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlProfileName {
    pub name_id: ID,
    pub name_type: GqlNameType,
    pub display_name: String,
    pub given_names: Option<String>,
    pub surname: Option<String>,
}

/// An event summary with its place name resolved (birth, death, etc.).
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlProfileEvent {
    pub event_id: ID,
    pub event_type: GqlEventType,
    pub date_value: Option<String>,
    /// How precise `date_value` is — without it a client cannot tell
    /// "1849" from "about 1849".
    pub date_qualifier: GqlDateQualifier,
    pub place_name: Option<String>,
    pub place_id: Option<ID>,
    pub description: Option<String>,
}

/// A family link (spouse relationship).
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlProfileFamilyLink {
    pub family_id: ID,
    pub role: GqlSpouseRole,
    pub spouse_id: Option<ID>,
    pub spouse_display_name: Option<String>,
    pub spouse_sex: Option<GqlSex>,
    pub marriage: Option<GqlProfileEvent>,
    pub children_ids: Vec<ID>,
    pub children_count: i32,
}

/// A child link (child's relationship to parents).
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlProfileChildLink {
    pub family_id: ID,
    pub child_type: GqlChildType,
    pub father_id: Option<ID>,
    pub father_display_name: Option<String>,
    pub mother_id: Option<ID>,
    pub mother_display_name: Option<String>,
}

/// A media reference (portrait / primary photo).
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlProfileMediaRef {
    pub media_id: ID,
    pub file_path: String,
    pub mime_type: String,
    pub title: Option<String>,
}

/// A single search result entry.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlSearchEntry {
    pub person_id: ID,
    pub sex: GqlSex,
    pub display_name: String,
    pub surname: String,
    pub given_names: String,
    pub birth_year: Option<String>,
    pub birth_place: Option<String>,
    pub death_year: Option<String>,
}

/// Paginated search results.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlSearchResult {
    pub entries: Vec<GqlSearchEntry>,
    pub total_count: i32,
}

/// Server-side ordering for person search results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlPersonSearchSort {
    Relevance,
    NameAsc,
    NameDesc,
    BirthAsc,
    BirthDesc,
}

impl From<GqlPersonSearchSort> for oxidgene_db::repo::PersonSearchSort {
    fn from(value: GqlPersonSearchSort) -> Self {
        match value {
            GqlPersonSearchSort::Relevance => Self::Relevance,
            GqlPersonSearchSort::NameAsc => Self::NameAsc,
            GqlPersonSearchSort::NameDesc => Self::NameDesc,
            GqlPersonSearchSort::BirthAsc => Self::BirthAsc,
            GqlPersonSearchSort::BirthDesc => Self::BirthDesc,
        }
    }
}

/// A distinct dictionary value with the number of people who use it.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlDictionaryEntry {
    pub value: String,
    pub sort_key: String,
    pub count: i64,
}

impl From<oxidgene_db::repo::DictionaryValueEntry> for GqlDictionaryEntry {
    fn from(entry: oxidgene_db::repo::DictionaryValueEntry) -> Self {
        Self {
            value: entry.value,
            sort_key: entry.sort_key,
            count: entry.count,
        }
    }
}

/// A person reached from a dictionary usage drill-down.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPersonUsageEntry {
    pub person_id: ID,
    pub given_names: Option<String>,
    pub surname: Option<String>,
    pub birth_year: Option<i32>,
    pub birth_qualifier: GqlDateQualifier,
    pub death_year: Option<i32>,
    pub death_qualifier: GqlDateQualifier,
}

impl From<oxidgene_db::repo::PersonUsageEntry> for GqlPersonUsageEntry {
    fn from(entry: oxidgene_db::repo::PersonUsageEntry) -> Self {
        Self {
            person_id: ID(entry.person_id.to_string()),
            given_names: entry.given_names,
            surname: entry.surname,
            birth_year: entry.birth_year,
            birth_qualifier: entry.birth_qualifier.into(),
            death_year: entry.death_year,
            death_qualifier: entry.death_qualifier.into(),
        }
    }
}

/// A source paired with the number of citations that use it.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlSourceDictionaryEntry {
    pub source: GqlSource,
    pub count: i64,
}

/// A place paired with its event and media usage count.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPlaceDictionaryEntry {
    pub place: GqlPlace,
    pub count: i64,
}

/// One selectable prefix in the source dictionary drill-down.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlSourceDictionaryGroup {
    pub label: String,
    pub count: i64,
}

/// The next level of the source dictionary drill-down.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlSourceDictionaryDrill {
    pub prefix: String,
    pub total: i64,
    pub groups: Vec<GqlSourceDictionaryGroup>,
}

/// Static reference information for one occupation label.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlOccupationReference {
    pub label: String,
    pub summary: String,
    pub text: String,
}

impl From<crate::reference::OccupationEntry> for GqlOccupationReference {
    fn from(entry: crate::reference::OccupationEntry) -> Self {
        Self {
            label: entry.label,
            summary: entry.summary,
            text: entry.text,
        }
    }
}

/// Static reference information for one given name.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGivenNameReference {
    pub label: String,
    pub origin: String,
    pub meaning: String,
    pub text: String,
    pub feast_day: Option<String>,
}

impl From<crate::reference::GivenNameEntry> for GqlGivenNameReference {
    fn from(entry: crate::reference::GivenNameEntry) -> Self {
        Self {
            label: entry.label,
            origin: entry.origin,
            meaning: entry.meaning,
            text: entry.text,
            feast_day: entry.feast_day,
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGivenNameReferenceMatch {
    pub term: String,
    pub reference: GqlGivenNameReference,
}

impl From<crate::reference::GivenNameMatch> for GqlGivenNameReferenceMatch {
    fn from(result: crate::reference::GivenNameMatch) -> Self {
        Self {
            term: result.term,
            reference: result.entry.into(),
        }
    }
}

/// The legacy all-at-once tree view, kept in GraphQL for REST parity.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlTreeSnapshot {
    pub persons: Vec<GqlPerson>,
    pub names: Vec<GqlPersonName>,
    pub events: Vec<GqlEvent>,
    pub places: Vec<GqlPlace>,
    pub spouses: Vec<GqlFamilySpouse>,
    pub children: Vec<GqlFamilyChild>,
}

/// The media or vignette selected to represent one person.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPortrait {
    pub person_id: ID,
    pub media_id: Option<ID>,
    pub vignette_id: Option<ID>,
    pub file_path: String,
    pub has_thumbnail: bool,
}

impl From<PortraitRow> for GqlPortrait {
    fn from(portrait: PortraitRow) -> Self {
        Self {
            person_id: ID(portrait.person_id.to_string()),
            media_id: portrait.media_id.map(|id| ID(id.to_string())),
            vignette_id: portrait.vignette_id.map(|id| ID(id.to_string())),
            file_path: portrait.file_path,
            has_thumbnail: portrait.has_thumbnail,
        }
    }
}

/// One display-ready portrait returned by the batched image query.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPortraitImage {
    pub person_id: ID,
    pub source: String,
}

impl From<crate::service::portrait::PortraitImage> for GqlPortraitImage {
    fn from(image: crate::service::portrait::PortraitImage) -> Self {
        Self {
            person_id: ID(image.person_id.to_string()),
            source: image.source,
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGalleryBundle {
    pub media: Vec<GqlGalleryMedia>,
    pub vignettes: Vec<GqlGalleryVignette>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGalleryMedia {
    pub media_id: ID,
    pub source: Option<String>,
    pub event_ids: Vec<ID>,
    pub document_previews: Vec<String>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGalleryVignette {
    pub vignette_id: ID,
    pub source: String,
}

impl From<crate::service::gallery::GalleryBundle> for GqlGalleryBundle {
    fn from(bundle: crate::service::gallery::GalleryBundle) -> Self {
        Self {
            media: bundle
                .media
                .into_iter()
                .map(|item| GqlGalleryMedia {
                    media_id: ID(item.media_id.to_string()),
                    source: item.source,
                    event_ids: item
                        .event_ids
                        .into_iter()
                        .map(|id| ID(id.to_string()))
                        .collect(),
                    document_previews: item.document_previews,
                })
                .collect(),
            vignettes: bundle
                .vignettes
                .into_iter()
                .map(|item| GqlGalleryVignette {
                    vignette_id: ID(item.vignette_id.to_string()),
                    source: item.source,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPersonDetailBundle {
    pub sosa_number: Option<u64>,
    pub persons: Vec<GqlPerson>,
    pub names: Vec<GqlPersonName>,
    pub events: Vec<GqlEvent>,
    pub places: Vec<GqlPlace>,
    pub spouses: Vec<GqlFamilySpouse>,
    pub children: Vec<GqlFamilyChild>,
    pub citations: Vec<GqlCitation>,
    pub sources: Vec<GqlSource>,
    pub profile_media: Vec<GqlProfileMediaTile>,
    pub profile_vignettes: Vec<GqlVignette>,
    pub event_media: Vec<GqlEventMediaTile>,
    pub gallery: GqlGalleryBundle,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlRelationLabels {
    pub names: Vec<GqlPersonName>,
    pub spouses: Vec<GqlFamilySpouse>,
}

impl From<crate::service::relation_labels::RelationLabels> for GqlRelationLabels {
    fn from(labels: crate::service::relation_labels::RelationLabels) -> Self {
        Self {
            names: labels.names.into_iter().map(Into::into).collect(),
            spouses: labels.spouses.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlEventMediaTile {
    pub event_id: ID,
    pub link_id: ID,
    pub sort_order: i32,
    pub media: GqlMedia,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlProfileMediaTile {
    pub link_id: ID,
    pub sort_order: i32,
    pub media: GqlMedia,
}

impl From<crate::service::person_detail::PersonDetailBundle> for GqlPersonDetailBundle {
    fn from(bundle: crate::service::person_detail::PersonDetailBundle) -> Self {
        Self {
            sosa_number: bundle.sosa_number,
            persons: bundle.persons.into_iter().map(Into::into).collect(),
            names: bundle.names.into_iter().map(Into::into).collect(),
            events: bundle.events.into_iter().map(Into::into).collect(),
            places: bundle.places.into_iter().map(Into::into).collect(),
            spouses: bundle.spouses.into_iter().map(Into::into).collect(),
            children: bundle.children.into_iter().map(Into::into).collect(),
            citations: bundle.citations.into_iter().map(Into::into).collect(),
            sources: bundle.sources.into_iter().map(Into::into).collect(),
            profile_media: bundle
                .profile_media
                .into_iter()
                .map(|item| GqlProfileMediaTile {
                    link_id: ID(item.link_id.to_string()),
                    sort_order: item.sort_order,
                    media: item.media.into(),
                })
                .collect(),
            profile_vignettes: bundle
                .profile_vignettes
                .into_iter()
                .map(Into::into)
                .collect(),
            event_media: bundle
                .event_media
                .into_iter()
                .map(|item| GqlEventMediaTile {
                    event_id: ID(item.event_id.to_string()),
                    link_id: ID(item.link_id.to_string()),
                    sort_order: item.sort_order,
                    media: item.media.into(),
                })
                .collect(),
            gallery: bundle.gallery.into(),
        }
    }
}

/// Result of a projection rebuild operation.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlProfileRebuildResult {
    pub rebuilt: bool,
    pub persons_count: i32,
}

/// Result of the dictionary's bulk surname-particle edit.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlFamilyNameParticleUpdate {
    /// The surname as it will still be listed — re-cutting moves where the
    /// name files, not the text itself.
    pub value: String,
    pub surname_prefix: Option<String>,
    pub surname: String,
    pub names_updated: i32,
    pub persons_updated: i32,
}

impl From<oxidgene_db::repo::FamilyNameParticleUpdate> for GqlFamilyNameParticleUpdate {
    fn from(u: oxidgene_db::repo::FamilyNameParticleUpdate) -> Self {
        Self {
            value: u.value,
            surname_prefix: u.surname_prefix,
            surname: u.surname,
            names_updated: u.names_updated as i32,
            persons_updated: u.persons_updated as i32,
        }
    }
}

// ── From impls for projection types ─────────────────────────────────────────

impl From<oxidgene_core::projection::PersonProfile> for GqlPersonProfile {
    fn from(p: oxidgene_core::projection::PersonProfile) -> Self {
        Self {
            person_id: ID(p.person_id.to_string()),
            tree_id: ID(p.tree_id.to_string()),
            sex: p.sex.into(),
            primary_name: p.primary_name.map(Into::into),
            other_names: p.other_names.into_iter().map(Into::into).collect(),
            birth: p.birth.map(Into::into),
            death: p.death.map(Into::into),
            baptism: p.baptism.map(Into::into),
            burial: p.burial.map(Into::into),
            occupation: p.occupation,
            other_events: p.other_events.into_iter().map(Into::into).collect(),
            families_as_spouse: p.families_as_spouse.into_iter().map(Into::into).collect(),
            family_as_child: p.family_as_child.map(Into::into),
            primary_media: p.primary_media.map(Into::into),
            media_count: p.media_count as i32,
            citation_count: p.citation_count as i32,
            note_count: p.note_count as i32,
            updated_at: p.updated_at,
            built_at: p.built_at,
        }
    }
}

impl From<oxidgene_core::projection::ProfileName> for GqlProfileName {
    fn from(n: oxidgene_core::projection::ProfileName) -> Self {
        Self {
            name_id: ID(n.name_id.to_string()),
            name_type: n.name_type.into(),
            display_name: n.display_name,
            given_names: n.given_names,
            surname: n.surname,
        }
    }
}

impl From<oxidgene_core::projection::ProfileEvent> for GqlProfileEvent {
    fn from(e: oxidgene_core::projection::ProfileEvent) -> Self {
        Self {
            event_id: ID(e.event_id.to_string()),
            event_type: e.event_type.into(),
            date_value: e.date_value,
            date_qualifier: e.date_qualifier.into(),
            place_name: e.place_name,
            place_id: e.place_id.map(|id| ID(id.to_string())),
            description: e.description,
        }
    }
}

impl From<oxidgene_core::projection::ProfileFamilyLink> for GqlProfileFamilyLink {
    fn from(f: oxidgene_core::projection::ProfileFamilyLink) -> Self {
        Self {
            family_id: ID(f.family_id.to_string()),
            role: f.role.into(),
            spouse_id: f.spouse_id.map(|id| ID(id.to_string())),
            spouse_display_name: f.spouse_display_name,
            spouse_sex: f.spouse_sex.map(Into::into),
            marriage: f.marriage.map(Into::into),
            children_ids: f
                .children_ids
                .into_iter()
                .map(|id| ID(id.to_string()))
                .collect(),
            children_count: f.children_count as i32,
        }
    }
}

impl From<oxidgene_core::projection::ProfileChildLink> for GqlProfileChildLink {
    fn from(c: oxidgene_core::projection::ProfileChildLink) -> Self {
        Self {
            family_id: ID(c.family_id.to_string()),
            child_type: c.child_type.into(),
            father_id: c.father_id.map(|id| ID(id.to_string())),
            father_display_name: c.father_display_name,
            mother_id: c.mother_id.map(|id| ID(id.to_string())),
            mother_display_name: c.mother_display_name,
        }
    }
}

impl From<oxidgene_core::projection::ProfileMediaRef> for GqlProfileMediaRef {
    fn from(m: oxidgene_core::projection::ProfileMediaRef) -> Self {
        Self {
            media_id: ID(m.media_id.to_string()),
            file_path: m.file_path,
            mime_type: m.mime_type,
            title: m.title,
        }
    }
}

impl From<oxidgene_core::projection::SearchEntry> for GqlSearchEntry {
    fn from(e: oxidgene_core::projection::SearchEntry) -> Self {
        Self {
            person_id: ID(e.person_id.to_string()),
            sex: e.sex.into(),
            display_name: e.display_name,
            surname: e.surname,
            given_names: e.given_names,
            birth_year: e.birth_year,
            birth_place: e.birth_place,
            death_year: e.death_year,
        }
    }
}

impl From<oxidgene_core::projection::SearchResult> for GqlSearchResult {
    fn from(r: oxidgene_core::projection::SearchResult) -> Self {
        Self {
            entries: r.entries.into_iter().map(Into::into).collect(),
            total_count: r.total_count as i32,
        }
    }
}

// ── Pedigree GraphQL types ────────────────────────────────────────────

/// Direction for pedigree expansion.
#[derive(async_graphql::Enum, Copy, Clone, Eq, PartialEq, Debug)]
pub enum GqlPedigreeDirection {
    Ancestors,
    Descendants,
}

impl From<GqlPedigreeDirection> for oxidgene_core::projection::PedigreeDirection {
    fn from(d: GqlPedigreeDirection) -> Self {
        match d {
            GqlPedigreeDirection::Ancestors => Self::Ancestors,
            GqlPedigreeDirection::Descendants => Self::Descendants,
        }
    }
}

/// A single node in the pedigree tree (minimal data for card display).
#[derive(SimpleObject, Debug, Clone)]
pub struct GqlPedigreeNode {
    pub person_id: ID,
    pub sex: GqlSex,
    pub display_name: String,
    /// The whole birth event — its date, precision, second date, calendar and
    /// place — rather than a year and a place name pulled out of it. Falls
    /// back to the baptism when no birth was recorded.
    pub birth: Option<GqlProfileEvent>,
    /// The whole death event, falling back to the burial. See `birth`.
    pub death: Option<GqlProfileEvent>,
    pub occupation: Option<String>,
    pub primary_media_path: Option<String>,
    /// Relative to root: 0 = root, -1 = parent, +1 = child.
    pub generation: i32,
    /// Sosa-Stradonitz number if on ancestor path.
    pub sosa_number: Option<String>,
}

/// An edge connecting a parent to a child within a family.
#[derive(SimpleObject, Debug, Clone)]
pub struct GqlPedigreeEdge {
    pub parent_id: ID,
    pub child_id: ID,
    pub family_id: ID,
    pub edge_type: GqlChildType,
}

/// Full windowed pedigree for a root person.
#[derive(SimpleObject, Debug, Clone)]
pub struct GqlPedigree {
    pub tree_id: ID,
    pub root_person_id: ID,
    pub nodes: Vec<GqlPedigreeNode>,
    pub edges: Vec<GqlPedigreeEdge>,
    pub ancestor_depth_loaded: i32,
    pub descendant_depth_loaded: i32,
}

/// Delta returned by expand operations (only the new nodes and edges).
#[derive(SimpleObject, Debug, Clone)]
pub struct GqlPedigreeDelta {
    pub new_nodes: Vec<GqlPedigreeNode>,
    pub new_edges: Vec<GqlPedigreeEdge>,
    pub ancestor_depth_loaded: i32,
    pub descendant_depth_loaded: i32,
}

// ── From impls for pedigree types ─────────────────────────────────────

impl From<oxidgene_core::projection::PedigreeNode> for GqlPedigreeNode {
    fn from(n: oxidgene_core::projection::PedigreeNode) -> Self {
        Self {
            person_id: ID(n.person_id.to_string()),
            sex: n.sex.into(),
            display_name: n.display_name,
            birth: n.birth.map(Into::into),
            death: n.death.map(Into::into),
            occupation: n.occupation,
            primary_media_path: n.primary_media_path,
            generation: n.generation,
            sosa_number: n.sosa_number.map(|s| s.to_string()),
        }
    }
}

impl From<oxidgene_core::projection::PedigreeEdge> for GqlPedigreeEdge {
    fn from(e: oxidgene_core::projection::PedigreeEdge) -> Self {
        Self {
            parent_id: ID(e.parent_id.to_string()),
            child_id: ID(e.child_id.to_string()),
            family_id: ID(e.family_id.to_string()),
            edge_type: e.edge_type.into(),
        }
    }
}

impl From<oxidgene_core::projection::Pedigree> for GqlPedigree {
    fn from(p: oxidgene_core::projection::Pedigree) -> Self {
        Self {
            tree_id: ID(p.tree_id.to_string()),
            root_person_id: ID(p.root_person_id.to_string()),
            nodes: p.persons.into_values().map(Into::into).collect(),
            edges: p.edges.into_iter().map(Into::into).collect(),
            ancestor_depth_loaded: p.ancestor_depth_loaded as i32,
            descendant_depth_loaded: p.descendant_depth_loaded as i32,
        }
    }
}

impl From<oxidgene_core::projection::PedigreeDelta> for GqlPedigreeDelta {
    fn from(d: oxidgene_core::projection::PedigreeDelta) -> Self {
        Self {
            new_nodes: d.new_nodes.into_iter().map(Into::into).collect(),
            new_edges: d.new_edges.into_iter().map(Into::into).collect(),
            ancestor_depth_loaded: d.ancestor_depth_loaded as i32,
            descendant_depth_loaded: d.descendant_depth_loaded as i32,
        }
    }
}
