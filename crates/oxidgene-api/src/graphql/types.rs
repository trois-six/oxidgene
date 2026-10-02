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

use super::scope::uuid;

use oxidgene_db::repo::{
    CitationRepo, EventRepo, EventWitnessRepo, FamilyChildRepo, FamilySpouseRepo, MediaLinkRepo,
    MediaLinkTarget, NoteRepo, PersonNameRepo, PersonRepo, PlaceRepo, PortraitRow, RepositoryRepo,
    SourceRepo, SourceRepositoryRepo,
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

/// How much of a date a tree writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlDateDisplayFormat {
    DayMonthYear,
    Numeric,
    MonthYear,
    Year,
}

impl From<oxidgene_core::enums::DateDisplayFormat> for GqlDateDisplayFormat {
    fn from(v: oxidgene_core::enums::DateDisplayFormat) -> Self {
        match v {
            oxidgene_core::enums::DateDisplayFormat::DayMonthYear => Self::DayMonthYear,
            oxidgene_core::enums::DateDisplayFormat::Numeric => Self::Numeric,
            oxidgene_core::enums::DateDisplayFormat::MonthYear => Self::MonthYear,
            oxidgene_core::enums::DateDisplayFormat::Year => Self::Year,
        }
    }
}

impl From<GqlDateDisplayFormat> for oxidgene_core::enums::DateDisplayFormat {
    fn from(v: GqlDateDisplayFormat) -> Self {
        match v {
            GqlDateDisplayFormat::DayMonthYear => Self::DayMonthYear,
            GqlDateDisplayFormat::Numeric => Self::Numeric,
            GqlDateDisplayFormat::MonthYear => Self::MonthYear,
            GqlDateDisplayFormat::Year => Self::Year,
        }
    }
}

/// The order and form of a tree's date fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlDateInputFormat {
    Slashes,
    Dashes,
    Iso,
    MonthName,
}

impl From<oxidgene_core::enums::DateInputFormat> for GqlDateInputFormat {
    fn from(v: oxidgene_core::enums::DateInputFormat) -> Self {
        match v {
            oxidgene_core::enums::DateInputFormat::Slashes => Self::Slashes,
            oxidgene_core::enums::DateInputFormat::Dashes => Self::Dashes,
            oxidgene_core::enums::DateInputFormat::Iso => Self::Iso,
            oxidgene_core::enums::DateInputFormat::MonthName => Self::MonthName,
        }
    }
}

impl From<GqlDateInputFormat> for oxidgene_core::enums::DateInputFormat {
    fn from(v: GqlDateInputFormat) -> Self {
        match v {
            GqlDateInputFormat::Slashes => Self::Slashes,
            GqlDateInputFormat::Dashes => Self::Dashes,
            GqlDateInputFormat::Iso => Self::Iso,
            GqlDateInputFormat::MonthName => Self::MonthName,
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

/// The format of a media's file, read from its MIME type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlMediaFileKind {
    Image,
    Pdf,
    Video,
    Audio,
    Other,
}

impl From<oxidgene_core::enums::MediaFileKind> for GqlMediaFileKind {
    fn from(kind: oxidgene_core::enums::MediaFileKind) -> Self {
        use oxidgene_core::enums::MediaFileKind as K;
        match kind {
            K::Image => Self::Image,
            K::Pdf => Self::Pdf,
            K::Video => Self::Video,
            K::Audio => Self::Audio,
            K::Other => Self::Other,
        }
    }
}

impl From<GqlMediaFileKind> for oxidgene_core::enums::MediaFileKind {
    fn from(kind: GqlMediaFileKind) -> Self {
        match kind {
            GqlMediaFileKind::Image => Self::Image,
            GqlMediaFileKind::Pdf => Self::Pdf,
            GqlMediaFileKind::Video => Self::Video,
            GqlMediaFileKind::Audio => Self::Audio,
            GqlMediaFileKind::Other => Self::Other,
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

/// The writer: what a mutation writes through.
pub(crate) fn db_from_ctx<'a>(ctx: &'a Context<'_>) -> &'a DatabaseConnection {
    ctx.data_unchecked::<DatabaseConnection>()
}

/// The connections queries read through: the read pool of a file-backed
/// SQLite database, the writer elsewhere (see `oxidgene_db::repo::Connections`).
pub(crate) struct Reader(pub(crate) DatabaseConnection);

/// What a query, or a field read from a returned object, reads through.
pub(crate) fn reader_from_ctx<'a>(ctx: &'a Context<'_>) -> &'a DatabaseConnection {
    &ctx.data_unchecked::<Reader>().0
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

pub(crate) fn work_dir_from_ctx<'a>(ctx: &'a Context<'_>) -> &'a crate::workdir::WorkDir {
    ctx.data_unchecked::<crate::workdir::WorkDir>()
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
    /// Whether entry fields suggest values as the user types.
    pub entry_suggestions: bool,
    /// How much of a date the tree's pages write.
    pub date_format: GqlDateDisplayFormat,
    /// Whether lifespans write the birth and death symbols.
    pub date_symbols: bool,
    /// Whether an approximate date reads « c. ».
    pub date_circa: bool,
    /// The calendar a date recorded in another one is also given in.
    pub date_calendar: GqlCalendar,
    /// Whether surname fields write in capitals.
    pub surname_uppercase: bool,
    /// Whether adding a relative offers the persons already in the tree.
    pub suggest_persons: bool,
    /// The order and form of a date field's parts.
    pub date_input_format: GqlDateInputFormat,
    /// The calendar an empty date field starts in.
    pub date_input_calendar: GqlCalendar,
    /// Who the tree's GEDCOM exports say they are from.
    pub submitter_name: Option<String>,
    pub submitter_email: Option<String>,
    pub submitter_address: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// The import running into the tree, when the tree list already knows it:
    /// the list reads every tree's at once rather than one query per tree.
    #[graphql(skip)]
    pub import_job: Option<Option<Uuid>>,
}

impl GqlTree {
    /// The import queued or running into the tree, if any.
    async fn active_import(&self, ctx: &Context<'_>) -> Result<Option<Uuid>> {
        match self.import_job {
            Some(known) => Ok(known),
            None => Ok(
                crate::service::tree::active_import(reader_from_ctx(ctx), uuid(&self.id)?).await?,
            ),
        }
    }
}

#[ComplexObject]
impl GqlTree {
    /// Whether an import is queued or running into the tree. Read from the
    /// job queue, not stored on the tree: it turns false once the job ends.
    async fn import_in_progress(&self, ctx: &Context<'_>) -> Result<bool> {
        Ok(self.active_import(ctx).await?.is_some())
    }

    /// The import job running into the tree, while one is.
    async fn import_job_id(&self, ctx: &Context<'_>) -> Result<Option<ID>> {
        Ok(self.active_import(ctx).await?.map(|id| ID(id.to_string())))
    }

    /// Count of persons in this tree.
    async fn person_count(&self, ctx: &Context<'_>) -> Result<i64> {
        Ok(PersonRepo::count(reader_from_ctx(ctx), uuid(&self.id)?).await?)
    }

    /// Count of families in this tree.
    async fn family_count(&self, ctx: &Context<'_>) -> Result<i64> {
        Ok(oxidgene_db::repo::FamilyRepo::count(reader_from_ctx(ctx), uuid(&self.id)?).await?)
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
            entry_suggestions: t.entry_suggestions,
            date_format: t.date_format.into(),
            date_symbols: t.date_symbols,
            date_circa: t.date_circa,
            date_calendar: t.date_calendar.into(),
            surname_uppercase: t.surname_uppercase,
            suggest_persons: t.suggest_persons,
            date_input_format: t.date_input_format.into(),
            date_input_calendar: t.date_input_calendar.into(),
            submitter_name: t.submitter_name,
            submitter_email: t.submitter_email,
            submitter_address: t.submitter_address,
            created_at: t.created_at,
            updated_at: t.updated_at,
            import_job: None,
        }
    }
}

impl From<crate::service::tree::TreeListItem> for GqlTree {
    fn from(item: crate::service::tree::TreeListItem) -> Self {
        Self {
            import_job: Some(item.import_job_id),
            ..item.tree.into()
        }
    }
}

// ── Tree Connection ──────────────────────────────────────────────────

connection!(
    GqlTreeEdge,
    GqlTreeConnection,
    GqlTree,
    crate::service::tree::TreeListItem
);

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
        let db = reader_from_ctx(ctx);
        let id = uuid(&self.id)?;
        let names = PersonNameRepo::list_by_person(db, id).await?;
        Ok(names.into_iter().map(GqlPersonName::from).collect())
    }

    /// Primary name of this person.
    async fn primary_name(&self, ctx: &Context<'_>) -> Result<Option<GqlPersonName>> {
        let db = reader_from_ctx(ctx);
        let id = uuid(&self.id)?;
        let names = PersonNameRepo::list_by_person(db, id).await?;
        Ok(names
            .into_iter()
            .find(|n| n.is_primary)
            .map(GqlPersonName::from))
    }

    /// Events associated with this person.
    async fn events(&self, ctx: &Context<'_>) -> Result<Vec<GqlEvent>> {
        let mut events =
            EventRepo::list_by_persons(reader_from_ctx(ctx), &[uuid(&self.id)?]).await?;
        events.sort_by_key(|event| event.id);
        Ok(events.into_iter().map(GqlEvent::from).collect())
    }

    /// Families this person belongs to (as spouse).
    async fn families(&self, ctx: &Context<'_>) -> Result<Vec<GqlFamily>> {
        let db = reader_from_ctx(ctx);
        let family_ids: Vec<Uuid> = FamilySpouseRepo::list_by_person(db, uuid(&self.id)?)
            .await?
            .into_iter()
            .map(|spouse| spouse.family_id)
            .collect();
        let mut families = oxidgene_db::repo::FamilyRepo::get_many(db, &family_ids).await?;
        families.sort_by_key(|family| family.id);
        Ok(families.into_iter().map(GqlFamily::from).collect())
    }

    /// Citations referencing this person directly.
    async fn citations(&self, ctx: &Context<'_>) -> Result<Vec<GqlCitation>> {
        let citations = CitationRepo::list_for_person_events(
            reader_from_ctx(ctx),
            uuid(&self.tree_id)?,
            uuid(&self.id)?,
            &[],
        )
        .await?;
        Ok(citations.into_iter().map(GqlCitation::from).collect())
    }

    /// Media linked to this person, in gallery order.
    async fn media(&self, ctx: &Context<'_>) -> Result<Vec<GqlMedia>> {
        linked_media(ctx, MediaLinkTarget::Person, &self.id).await
    }

    /// Notes attached to this person.
    async fn notes(&self, ctx: &Context<'_>) -> Result<Vec<GqlNote>> {
        let db = reader_from_ctx(ctx);
        let tree_id = uuid(&self.tree_id)?;
        let person_id = uuid(&self.id)?;
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

connection!(
    GqlPersonEdge,
    GqlPersonConnection,
    GqlPerson,
    oxidgene_core::types::Person
);

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
        let db = reader_from_ctx(ctx);
        let spouses = FamilySpouseRepo::list_by_family(db, uuid(&self.id)?).await?;
        let mut persons = persons_by_id(db, spouses.iter().map(|s| s.person_id)).await?;
        Ok(spouses
            .into_iter()
            .filter_map(|s| {
                Some(GqlFamilySpouseDetail {
                    id: ID(s.id.to_string()),
                    person: persons.remove(&s.person_id)?.into(),
                    role: s.role.into(),
                    sort_order: s.sort_order,
                })
            })
            .collect())
    }

    /// Children in this family.
    async fn children(&self, ctx: &Context<'_>) -> Result<Vec<GqlFamilyChildDetail>> {
        let db = reader_from_ctx(ctx);
        let children = FamilyChildRepo::list_by_family(db, uuid(&self.id)?).await?;
        let mut persons = persons_by_id(db, children.iter().map(|c| c.person_id)).await?;
        Ok(children
            .into_iter()
            .filter_map(|c| {
                Some(GqlFamilyChildDetail {
                    id: ID(c.id.to_string()),
                    person: persons.remove(&c.person_id)?.into(),
                    child_type: c.child_type.into(),
                    sort_order: c.sort_order,
                })
            })
            .collect())
    }

    /// Events associated with this family.
    async fn events(&self, ctx: &Context<'_>) -> Result<Vec<GqlEvent>> {
        let mut events =
            EventRepo::list_by_families(reader_from_ctx(ctx), &[uuid(&self.id)?]).await?;
        events.sort_by_key(|event| event.id);
        Ok(events.into_iter().map(GqlEvent::from).collect())
    }
}

/// The live persons among `ids`, by id, read in one query.
async fn persons_by_id(
    db: &DatabaseConnection,
    ids: impl Iterator<Item = Uuid>,
) -> Result<std::collections::HashMap<Uuid, oxidgene_core::types::Person>> {
    let ids: Vec<Uuid> = ids.collect();
    Ok(PersonRepo::get_many(db, &ids)
        .await?
        .into_iter()
        .map(|person| (person.id, person))
        .collect())
}

/// The media linked to entity `id`, in gallery order, each once.
async fn linked_media(
    ctx: &Context<'_>,
    target: MediaLinkTarget,
    id: &ID,
) -> Result<Vec<GqlMedia>> {
    let rows = MediaLinkRepo::list_with_media(reader_from_ctx(ctx), target, uuid(id)?).await?;
    let mut seen = std::collections::HashSet::new();
    Ok(rows
        .into_iter()
        .filter(|(_, media)| seen.insert(media.id))
        .map(|(_, media)| GqlMedia::from(media))
        .collect())
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

connection!(
    GqlFamilyEdge,
    GqlFamilyConnection,
    GqlFamily,
    oxidgene_core::types::Family
);

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

/// The age a family event's record gives for one spouse.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlSpouseAge {
    pub person_id: ID,
    /// Canonical GEDCOM age.
    pub age: String,
}

impl From<oxidgene_core::types::SpouseAge> for GqlSpouseAge {
    fn from(s: oxidgene_core::types::SpouseAge) -> Self {
        Self {
            person_id: ID(s.person_id.to_string()),
            age: s.age,
        }
    }
}

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
    /// The age the record gives, in canonical GEDCOM form.
    pub age: Option<String>,
    /// The authority responsible for the event's record.
    pub agency: Option<String>,
    /// For a family event, the ages its record gives for the spouses.
    pub spouse_ages: Vec<GqlSpouseAge>,
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
        let db = reader_from_ctx(ctx);
        let id = uuid(pid)?;
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
        let db = reader_from_ctx(ctx);
        let id = uuid(pid)?;
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
        let db = reader_from_ctx(ctx);
        let id = uuid(fid)?;
        match oxidgene_db::repo::FamilyRepo::get(db, id).await {
            Ok(f) => Ok(Some(GqlFamily::from(f))),
            Err(_) => Ok(None),
        }
    }

    /// Citations for this event.
    async fn citations(&self, ctx: &Context<'_>) -> Result<Vec<GqlCitation>> {
        let citations = CitationRepo::list_for_event(
            reader_from_ctx(ctx),
            uuid(&self.tree_id)?,
            uuid(&self.id)?,
        )
        .await?;
        Ok(citations.into_iter().map(GqlCitation::from).collect())
    }

    /// Media linked to this event, in gallery order.
    async fn media(&self, ctx: &Context<'_>) -> Result<Vec<GqlMedia>> {
        linked_media(ctx, MediaLinkTarget::Event, &self.id).await
    }

    /// Notes for this event.
    async fn notes(&self, ctx: &Context<'_>) -> Result<Vec<GqlNote>> {
        let db = reader_from_ctx(ctx);
        let tree_id = uuid(&self.tree_id)?;
        let event_id = uuid(&self.id)?;
        let notes =
            NoteRepo::list_by_entity(db, tree_id, None, Some(event_id), None, None, None).await?;
        Ok(notes.into_iter().map(GqlNote::from).collect())
    }

    /// Witnesses (godparents, etc.) linked to this event.
    async fn witnesses(&self, ctx: &Context<'_>) -> Result<Vec<GqlEventWitness>> {
        let db = reader_from_ctx(ctx);
        let event_id = uuid(&self.id)?;
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
            age: e.age,
            agency: e.agency,
            spouse_ages: e.spouse_ages.into_iter().map(GqlSpouseAge::from).collect(),
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
        let db = reader_from_ctx(ctx);
        let id = uuid(&self.person_id)?;
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

connection!(
    GqlEventEdge,
    GqlEventConnection,
    GqlEvent,
    oxidgene_core::types::Event
);

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

connection!(
    GqlPlaceEdge,
    GqlPlaceConnection,
    GqlPlace,
    oxidgene_core::types::Place
);

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
    /// The organisation responsible for the source's data.
    pub agency: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[ComplexObject]
impl GqlSource {
    /// Citations from this source.
    async fn citations(&self, ctx: &Context<'_>) -> Result<Vec<GqlCitation>> {
        let db = reader_from_ctx(ctx);
        let id = uuid(&self.id)?;
        let cits = CitationRepo::list_by_source(db, id).await?;
        Ok(cits.into_iter().map(GqlCitation::from).collect())
    }

    /// The repositories holding this source, each under one call number.
    async fn repositories(&self, ctx: &Context<'_>) -> Result<Vec<GqlSourceRepository>> {
        let links =
            SourceRepositoryRepo::list_by_source(reader_from_ctx(ctx), uuid(&self.id)?).await?;
        Ok(links.into_iter().map(GqlSourceRepository::from).collect())
    }
}

// ── Repository ───────────────────────────────────────────────────────

/// A place holding sources: an archive, a library, a registry office.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct GqlRepository {
    pub id: ID,
    pub tree_id: ID,
    pub name: String,
    /// The postal address, over several lines.
    pub address: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub website: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[ComplexObject]
impl GqlRepository {
    /// The live sources this repository holds, one link per call number.
    async fn sources(&self, ctx: &Context<'_>) -> Result<Vec<GqlSourceRepository>> {
        let links =
            SourceRepositoryRepo::list_by_repository(reader_from_ctx(ctx), uuid(&self.id)?).await?;
        Ok(links.into_iter().map(GqlSourceRepository::from).collect())
    }
}

impl From<oxidgene_core::types::Repository> for GqlRepository {
    fn from(r: oxidgene_core::types::Repository) -> Self {
        Self {
            id: ID(r.id.to_string()),
            tree_id: ID(r.tree_id.to_string()),
            name: r.name,
            address: r.address,
            phone: r.phone,
            email: r.email,
            website: r.website,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

connection!(
    GqlRepositoryEdge,
    GqlRepositoryConnection,
    GqlRepository,
    oxidgene_core::types::Repository
);

/// That a source is held at a repository, under one call number.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct GqlSourceRepository {
    pub id: ID,
    pub source_id: ID,
    pub repository_id: ID,
    pub call_number: Option<String>,
    /// The medium the source is kept on there.
    pub media_type: Option<GqlSourceMediaType>,
    pub sort_order: i32,
}

#[ComplexObject]
impl GqlSourceRepository {
    /// The repository, unless it was deleted.
    async fn repository(&self, ctx: &Context<'_>) -> Result<Option<GqlRepository>> {
        let id = uuid(&self.repository_id)?;
        Ok(RepositoryRepo::get_many(reader_from_ctx(ctx), &[id])
            .await?
            .into_iter()
            .next()
            .map(GqlRepository::from))
    }

    /// The source, unless it was deleted.
    async fn source(&self, ctx: &Context<'_>) -> Result<Option<GqlSource>> {
        match SourceRepo::get(reader_from_ctx(ctx), uuid(&self.source_id)?).await {
            Ok(source) => Ok(Some(source.into())),
            Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

impl From<oxidgene_core::types::SourceRepository> for GqlSourceRepository {
    fn from(l: oxidgene_core::types::SourceRepository) -> Self {
        Self {
            id: ID(l.id.to_string()),
            source_id: ID(l.source_id.to_string()),
            repository_id: ID(l.repository_id.to_string()),
            call_number: l.call_number,
            media_type: l.media_type.map(Into::into),
            sort_order: l.sort_order,
        }
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
            agency: s.agency,
            created_at: s.created_at,
            updated_at: s.updated_at,
        }
    }
}

// ── Source Connection ────────────────────────────────────────────────

connection!(
    GqlSourceEdge,
    GqlSourceConnection,
    GqlSource,
    oxidgene_core::types::Source
);

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
    /// Null when the evidence is not assessed.
    pub confidence: Option<GqlConfidence>,
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
            confidence: c.confidence.map(Into::into),
            text: c.text,
            created_at: c.created_at,
            updated_at: c.updated_at,
        }
    }
}

connection!(
    GqlCitationEdge,
    GqlCitationConnection,
    GqlCitation,
    oxidgene_core::types::Citation
);

// ── Media ────────────────────────────────────────────────────────────

/// A checked media attachment URL; binary content is read over HTTP.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMediaDownload {
    /// Same-origin attachment URL. Availability is checked when this is resolved;
    /// the HTTP request revalidates it and can still fail if storage changes.
    pub url: String,
}

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
    /// The document this is a page of, or null when this row *is* a document.
    ///
    /// The only thing separating the two kinds of row, on both APIs: a null
    /// parent means a document — the container a gallery lists, carrying the
    /// title, date, place, category, medium, privacy, description, tags and
    /// note, and no bytes. Anything else is a page, which carries the bytes
    /// or the remote URL and nothing else of consequence.
    pub parent_media_id: Option<ID>,
    /// Zero-based position within that document.
    pub page_index: i32,
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
    /// How many records — persons, families, events and sources — the
    /// document is attached to, through itself or one of its pages.
    pub usage_count: i64,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMediaConnection {
    pub edges: Vec<GqlMediaEdge>,
    pub page_info: GqlPageInfo,
    pub total_count: i64,
}

type MediaListConnection =
    oxidgene_core::types::Connection<crate::service::media_library::MediaListItem>;

impl From<MediaListConnection> for GqlMediaConnection {
    fn from(c: MediaListConnection) -> Self {
        Self {
            edges: c
                .edges
                .into_iter()
                .map(|e| GqlMediaEdge {
                    cursor: e.cursor,
                    usage_count: e.node.usage_count,
                    node: e.node.media.into(),
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

// ── Media library facets ─────────────────────────────────────────────

/// A tag and how many documents carry it.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMediaTagFacet {
    /// The spelling most of those documents carry.
    pub tag: String,
    pub count: i64,
}

/// A file kind and how many documents hold a page of it.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMediaKindFacet {
    pub kind: GqlMediaFileKind,
    pub count: i64,
}

/// A document category and how many documents are filed under it.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMediaCategoryFacet {
    pub category: GqlDocumentCategory,
    pub count: i64,
}

/// The values the media list's filters can take in a tree.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMediaFacets {
    pub tags: Vec<GqlMediaTagFacet>,
    pub kinds: Vec<GqlMediaKindFacet>,
    pub categories: Vec<GqlMediaCategoryFacet>,
}

impl From<crate::service::media_library::MediaFacets> for GqlMediaFacets {
    fn from(facets: crate::service::media_library::MediaFacets) -> Self {
        Self {
            tags: facets
                .tags
                .into_iter()
                .map(|tag| GqlMediaTagFacet {
                    tag: tag.tag,
                    count: tag.count,
                })
                .collect(),
            kinds: facets
                .kinds
                .into_iter()
                .map(|kind| GqlMediaKindFacet {
                    kind: kind.kind.into(),
                    count: kind.count,
                })
                .collect(),
            categories: facets
                .categories
                .into_iter()
                .map(|category| GqlMediaCategoryFacet {
                    category: category.category.into(),
                    count: category.count,
                })
                .collect(),
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

connection!(
    GqlNoteEdge,
    GqlNoteConnection,
    GqlNote,
    oxidgene_core::types::Note
);

// ── Import/Export Results ─────────────────────────────────────────────

/// Result of an import operation, whatever the source format.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlImportResult {
    pub persons_count: i32,
    pub families_count: i32,
    pub events_count: i32,
    pub sources_count: i32,
    /// Media records of a single page: photographs, single scans.
    pub images_count: i32,
    /// Media records of any other number of pages.
    pub documents_count: i32,
    /// The pages of those documents.
    pub document_pages_count: i32,
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
    /// When the archive stops being downloadable, set exactly when
    /// `download_url` is.
    pub expires_at: Option<DateTime<Utc>>,
    /// The archive's size in bytes, once the export has completed.
    pub size_bytes: Option<i64>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

/// A completed export of a tree whose archive can still be downloaded.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlDownloadableExport {
    pub job_id: ID,
    /// The archive's format: `gedzip`.
    pub format: String,
    pub download_url: String,
    /// When the archive stops being downloadable.
    pub expires_at: DateTime<Utc>,
    /// The archive's size in bytes.
    pub size_bytes: Option<i64>,
    pub include_notes_and_sources: bool,
    pub include_media: bool,
}

impl From<crate::service::background_job::DownloadableExport> for GqlDownloadableExport {
    fn from(export: crate::service::background_job::DownloadableExport) -> Self {
        Self {
            job_id: ID(export.job_id.to_string()),
            format: export.format,
            download_url: export.download_url,
            expires_at: export.expires_at,
            size_bytes: export.size_bytes,
            include_notes_and_sources: export.include_notes_and_sources,
            include_media: export.include_media,
        }
    }
}

impl From<crate::service::gedcom::ImportSummary> for GqlImportResult {
    fn from(summary: crate::service::gedcom::ImportSummary) -> Self {
        Self {
            persons_count: summary.persons_count as i32,
            families_count: summary.families_count as i32,
            events_count: summary.events_count as i32,
            sources_count: summary.sources_count as i32,
            images_count: summary.media.images_count as i32,
            documents_count: summary.media.documents_count as i32,
            document_pages_count: summary.media.document_pages_count as i32,
            places_count: summary.places_count as i32,
            notes_count: summary.notes_count as i32,
            warnings: summary.warnings,
        }
    }
}

impl From<crate::service::background_job::ExportJobStatus> for GqlExportJobStatus {
    fn from(status: crate::service::background_job::ExportJobStatus) -> Self {
        Self {
            phase: status.phase,
            done: status.done,
            total: status.total,
            download_url: status.download_url,
            expires_at: status.expires_at,
            size_bytes: status.size_bytes,
            warnings: status.warnings,
            error: status.error,
        }
    }
}

impl From<crate::service::background_job::ImportJobStatus> for GqlImportJobStatus {
    fn from(status: crate::service::background_job::ImportJobStatus) -> Self {
        Self {
            phase: status.phase,
            done: status.done,
            total: status.total,
            result: status.result.map(Into::into),
            geneanet_result: status.geneanet_result.map(Into::into),
            error: status.error,
        }
    }
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

/// Which bytes a Geneanet import keeps for each medium.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Enum)]
pub enum GqlGeneanetMediaFidelity {
    /// Geneanet's own `normal` rendition of every page: recompressed and
    /// resized, and needing nothing from the user but their login.
    #[default]
    Renditions,
    /// The uploaded files, taken from the user's data archives where they can
    /// be identified there and downloaded otherwise.
    Originals,
}

impl From<GqlGeneanetMediaFidelity> for crate::service::geneanet::MediaFidelity {
    fn from(fidelity: GqlGeneanetMediaFidelity) -> Self {
        match fidelity {
            GqlGeneanetMediaFidelity::Renditions => Self::Renditions,
            GqlGeneanetMediaFidelity::Originals => Self::Originals,
        }
    }
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
    /// As `ImportResult.imagesCount`.
    pub images_count: i64,
    /// As `ImportResult.documentsCount`.
    pub documents_count: i64,
    /// As `ImportResult.documentPagesCount`.
    pub document_pages_count: i64,
    pub links_count: i64,
    pub portraits_count: i64,
    pub isolated_count: i64,
    /// The people counted by `isolatedCount`, in creation order.
    pub isolated_people: Vec<GqlGeneanetIsolatedPerson>,
    pub vignettes_count: i64,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

impl From<crate::service::geneanet::GeneanetImportSummary> for GqlGeneanetImportResult {
    fn from(summary: crate::service::geneanet::GeneanetImportSummary) -> Self {
        let receipt = summary.receipt;
        Self {
            persons_count: receipt.persons_count as i64,
            families_count: receipt.families_count as i64,
            events_count: receipt.events_count as i64,
            sources_count: receipt.sources_count as i64,
            places_count: receipt.places_count as i64,
            notes_count: receipt.notes_count as i64,
            images_count: receipt.media.images_count as i64,
            documents_count: receipt.media.documents_count as i64,
            document_pages_count: receipt.media.document_pages_count as i64,
            links_count: summary.links_count as i64,
            portraits_count: summary.portraits_count as i64,
            isolated_count: summary.isolated_count as i64,
            isolated_people: summary
                .isolated_people
                .into_iter()
                .map(Into::into)
                .collect(),
            vignettes_count: summary.vignettes_count as i64,
            skipped: summary.skipped,
            warnings: receipt.warnings,
        }
    }
}

/// A person the Geneanet import created for an identification outside the
/// tree.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGeneanetIsolatedPerson {
    pub person_id: ID,
    pub surname: String,
    pub given_names: String,
}

impl From<crate::service::geneanet::IsolatedPerson> for GqlGeneanetIsolatedPerson {
    fn from(person: crate::service::geneanet::IsolatedPerson) -> Self {
        Self {
            person_id: ID(person.person_id.to_string()),
            surname: person.surname,
            given_names: person.given_names,
        }
    }
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

connection!(
    GqlPersonProfileEdge,
    GqlPersonProfileConnection,
    GqlPersonProfile,
    oxidgene_core::projection::PersonProfile
);

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
    /// The age the record gives for the profile's person at this event.
    pub age: Option<String>,
}

/// A family link (spouse relationship).
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlProfileFamilyLink {
    pub family_id: ID,
    pub role: GqlSpouseRole,
    pub spouse_id: Option<ID>,
    pub spouse_display_name: Option<String>,
    pub spouse_surname: Option<String>,
    pub spouse_given_names: Option<String>,
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
    pub father_surname: Option<String>,
    pub father_given_names: Option<String>,
    pub mother_id: Option<ID>,
    pub mother_display_name: Option<String>,
    pub mother_surname: Option<String>,
    pub mother_given_names: Option<String>,
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
    pub birth_qualifier: GqlDateQualifier,
    pub birth_place: Option<String>,
    pub death_year: Option<String>,
    pub death_qualifier: GqlDateQualifier,
    /// Display names of every spouse, in family order.
    pub spouse_names: Vec<String>,
    pub father_name: Option<String>,
    pub mother_name: Option<String>,
    pub children_count: i32,
    /// Where the row's portrait is drawn from, when the person has one.
    pub portrait: Option<GqlPortraitRef>,
}

/// One tree's most recently modified persons, from a batch.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlTreeRecentPersons {
    pub tree_id: ID,
    pub persons: Vec<GqlSearchEntry>,
}

impl From<crate::service::history::TreeRecentPersons> for GqlTreeRecentPersons {
    fn from(tree: crate::service::history::TreeRecentPersons) -> Self {
        Self {
            tree_id: ID(tree.tree_id.to_string()),
            persons: tree.persons.into_iter().map(Into::into).collect(),
        }
    }
}

/// Two records of a tree that may be one person.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlDuplicatePair {
    /// From 0 to 100, how alike the two records are.
    pub score: i64,
    /// What they share, strongest first.
    pub reasons: Vec<String>,
    pub first: GqlSearchEntry,
    pub second: GqlSearchEntry,
    /// Both records' full birth and death dates.
    pub first_dates: crate::service::duplicates::LifeDates,
    pub second_dates: crate::service::duplicates::LifeDates,
}

/// A tree's potential duplicates: every pair found, and the best listed.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPotentialDuplicates {
    pub count: i64,
    pub pairs: Vec<GqlDuplicatePair>,
}

impl From<crate::service::duplicates::PotentialDuplicates> for GqlPotentialDuplicates {
    fn from(found: crate::service::duplicates::PotentialDuplicates) -> Self {
        Self {
            count: found.count,
            pairs: found
                .pairs
                .into_iter()
                .map(|pair| GqlDuplicatePair {
                    score: pair.score,
                    reasons: pair.reasons,
                    first: pair.first.into(),
                    second: pair.second.into(),
                    first_dates: pair.first_dates,
                    second_dates: pair.second_dates,
                })
                .collect(),
        }
    }
}

/// Paginated search results.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlSearchResult {
    pub entries: Vec<GqlSearchEntry>,
    pub total_count: i32,
}

/// Every way found to go from one person of a tree to another.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlKinship {
    pub from_person_id: ID,
    pub to_person_id: ID,
    /// Closest relationship first.
    pub paths: Vec<GqlKinshipPath>,
    /// More paths exist than were enumerated.
    pub truncated: bool,
    /// A search row for every person the paths name.
    pub persons: Vec<GqlSearchEntry>,
}

/// One way to go from the first person to the second: one segment for a
/// blood relationship, one more per union crossed.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlKinshipPath {
    pub segments: Vec<GqlKinshipSegment>,
}

/// A stretch of a path that climbs to its highest generation and comes back
/// down without passing through a union.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlKinshipSegment {
    /// One person, or both spouses of the family the two lines descend from.
    pub ancestor_ids: Vec<ID>,
    pub family_id: Option<ID>,
    /// From below the ancestors down to the segment's first person.
    pub from_line: Vec<ID>,
    /// From below the ancestors down to the segment's last person.
    pub to_line: Vec<ID>,
    pub half: bool,
    /// The union joining the previous segment's last person to this one's
    /// first.
    pub union_family_id: Option<ID>,
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
    /// Family names only: how many of `count` carry the value as their
    /// primary name, i.e. how many a rename would reach.
    pub primary_count: Option<i64>,
}

impl From<oxidgene_db::repo::DictionaryValueEntry> for GqlDictionaryEntry {
    fn from(entry: oxidgene_db::repo::DictionaryValueEntry) -> Self {
        Self {
            value: entry.value,
            sort_key: entry.sort_key,
            count: entry.count,
            primary_count: entry.primary_count,
        }
    }
}

/// The entry-form field a value suggestion is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlSuggestionField {
    FamilyNames,
    /// One given name: the form completes the word being typed.
    GivenNames,
    Occupations,
    /// Source titles.
    Sources,
    /// The tree's place names.
    Places,
}

impl From<GqlSuggestionField> for crate::service::suggestions::SuggestionField {
    fn from(field: GqlSuggestionField) -> Self {
        match field {
            GqlSuggestionField::FamilyNames => Self::FamilyNames,
            GqlSuggestionField::GivenNames => Self::GivenNames,
            GqlSuggestionField::Occupations => Self::Occupations,
            GqlSuggestionField::Sources => Self::Sources,
            GqlSuggestionField::Places => Self::Places,
        }
    }
}

/// A value an entry-form field suggests.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlValueSuggestion {
    pub value: String,
    /// Persons carrying the value, or citations of the source; 0 for a term
    /// only a reference sheet knows.
    pub count: i64,
    /// Whether a reference sheet answers to the value itself.
    pub reference: bool,
}

impl From<crate::service::suggestions::ValueSuggestion> for GqlValueSuggestion {
    fn from(suggestion: crate::service::suggestions::ValueSuggestion) -> Self {
        Self {
            value: suggestion.value,
            count: suggestion.count,
            reference: suggestion.reference,
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
    /// The names of the repositories holding the source, each once.
    pub repositories: Vec<String>,
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
    /// The sources under `prefix`, when there is no group left to choose.
    pub sources: Option<Vec<GqlSourceDictionaryEntry>>,
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

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlOccupationReferenceMatch {
    pub term: String,
    pub reference: GqlOccupationReference,
}

impl From<crate::reference::OccupationMatch> for GqlOccupationReferenceMatch {
    fn from(result: crate::reference::OccupationMatch) -> Self {
        Self {
            term: result.term,
            reference: result.entry.into(),
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

/// What a place dictionary entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlPlaceKind {
    Commune,
    MunicipalArrondissement,
    Settlement,
    Parish,
    FormerName,
    FormerCommune,
}

impl From<crate::reference::PlaceKind> for GqlPlaceKind {
    fn from(kind: crate::reference::PlaceKind) -> Self {
        use crate::reference::PlaceKind;
        match kind {
            PlaceKind::Commune => Self::Commune,
            PlaceKind::MunicipalArrondissement => Self::MunicipalArrondissement,
            PlaceKind::Settlement => Self::Settlement,
            PlaceKind::Parish => Self::Parish,
            PlaceKind::FormerName => Self::FormerName,
            PlaceKind::FormerCommune => Self::FormerCommune,
        }
    }
}

/// A place suggested from the place dictionary.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPlaceSuggestion {
    /// The place as a genealogy records it, most specific part first.
    pub label: String,
    pub name: String,
    /// The INSEE commune code in France.
    pub code: Option<String>,
    pub subdivision: String,
    pub region: String,
    pub country: String,
    pub kind: GqlPlaceKind,
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
    /// INSEE code of the commune holding a former commune's territory today.
    pub successor: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// Filed under today's subdivision and region rather than a former one.
    pub current: bool,
}

impl From<crate::reference::PlaceSuggestion> for GqlPlaceSuggestion {
    fn from(place: crate::reference::PlaceSuggestion) -> Self {
        Self {
            label: place.label,
            name: place.name,
            code: place.code,
            subdivision: place.subdivision,
            region: place.region,
            country: place.country,
            kind: place.kind.into(),
            valid_from: place.valid_from,
            valid_until: place.valid_until,
            successor: place.successor,
            latitude: place.latitude,
            longitude: place.longitude,
            current: place.current,
        }
    }
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

/// A region of a picture the client fetches for itself, and therefore has to
/// cut for itself.
#[derive(Debug, Clone, Copy, SimpleObject)]
pub struct GqlImageCrop {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// The full picture's pixel size, which is what `x`/`y` are measured in.
    pub source_width: i32,
    pub source_height: i32,
}

impl From<oxidgene_core::types::ImageCrop> for GqlImageCrop {
    fn from(crop: oxidgene_core::types::ImageCrop) -> Self {
        Self {
            x: crop.x,
            y: crop.y,
            width: crop.width,
            height: crop.height,
            source_width: crop.source_width,
            source_height: crop.source_height,
        }
    }
}

/// Where a person's portrait is drawn from, as pedigree nodes and search rows
/// carry it.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPortraitRef {
    pub source: GqlImageSource,
    /// Set when `source` is a whole picture the client must crop itself.
    pub crop: Option<GqlImageCrop>,
}

impl From<oxidgene_core::types::PortraitRef> for GqlPortraitRef {
    fn from(portrait: oxidgene_core::types::PortraitRef) -> Self {
        Self {
            source: portrait.source.into(),
            crop: portrait.crop.map(Into::into),
        }
    }
}

/// One display-ready portrait returned by the batched image query.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPortraitImage {
    pub person_id: ID,
    pub source: GqlImageSource,
    /// Set when `source` is a whole picture the client must crop itself.
    pub crop: Option<GqlImageCrop>,
}

impl From<crate::service::portrait::PortraitImage> for GqlPortraitImage {
    fn from(image: crate::service::portrait::PortraitImage) -> Self {
        Self {
            person_id: ID(image.person_id.to_string()),
            source: image.source.into(),
            crop: image.crop.map(Into::into),
        }
    }
}

/// Which resource a picture comes from. Never the picture itself — the bytes
/// travel over their own request, so a payload listing a hundred images stays
/// small and each one is cached, decoded and lazily loaded by the engine that
/// draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, async_graphql::Enum)]
pub enum GqlImageSourceKind {
    /// An address outside our control, which the client fetches directly.
    Remote,
    /// The thumbnail this backend generated for a media it holds.
    Thumbnail,
    /// The region this backend cuts out of a media it holds.
    Crop,
}

/// Where a picture lives.
///
/// Exactly one of the three payload fields is set, matching `kind`. A held
/// picture names its resource rather than a URL: turning it into something
/// drawable is the client's business, because until authentication ships no
/// backend address may appear in the markup.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlImageSource {
    pub kind: GqlImageSourceKind,
    /// Set when `kind` is `REMOTE`.
    pub url: Option<String>,
    /// Set when `kind` is `THUMBNAIL`.
    pub media_id: Option<ID>,
    /// Set when `kind` is `CROP`.
    pub vignette_id: Option<ID>,
}

impl From<oxidgene_core::types::ImageSource> for GqlImageSource {
    fn from(source: oxidgene_core::types::ImageSource) -> Self {
        use oxidgene_core::types::ImageSource;
        match source {
            ImageSource::Remote { url } => Self {
                kind: GqlImageSourceKind::Remote,
                url: Some(url),
                media_id: None,
                vignette_id: None,
            },
            ImageSource::Thumbnail { media_id } => Self {
                kind: GqlImageSourceKind::Thumbnail,
                url: None,
                media_id: Some(ID(media_id.to_string())),
                vignette_id: None,
            },
            ImageSource::Crop { vignette_id } => Self {
                kind: GqlImageSourceKind::Crop,
                url: None,
                media_id: None,
                vignette_id: Some(ID(vignette_id.to_string())),
            },
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
    pub source: Option<GqlImageSource>,
    pub event_ids: Vec<ID>,
    pub document_previews: Vec<GqlImageSource>,
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlGalleryVignette {
    pub vignette_id: ID,
    pub source: GqlImageSource,
    /// Set when `source` is a whole picture the client must crop itself.
    pub crop: Option<GqlImageCrop>,
}

impl From<crate::service::gallery::GalleryBundle> for GqlGalleryBundle {
    fn from(bundle: crate::service::gallery::GalleryBundle) -> Self {
        Self {
            media: bundle
                .media
                .into_iter()
                .map(|item| GqlGalleryMedia {
                    media_id: ID(item.media_id.to_string()),
                    source: item.source.map(Into::into),
                    event_ids: item
                        .event_ids
                        .into_iter()
                        .map(|id| ID(id.to_string()))
                        .collect(),
                    document_previews: item.document_previews.into_iter().map(Into::into).collect(),
                })
                .collect(),
            vignettes: bundle
                .vignettes
                .into_iter()
                .map(|item| GqlGalleryVignette {
                    vignette_id: ID(item.vignette_id.to_string()),
                    source: item.source.into(),
                    crop: item.crop.map(Into::into),
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
    /// Where the person's own portrait is drawn from.
    pub portrait: Option<GqlPortraitRef>,
    /// Those of `persons` who are the SOSA root or one of its ancestors.
    pub sosa_ancestor_ids: Vec<ID>,
}

/// Everything the couple page draws. Mirrors REST's
/// `GET /families/{familyId}/detail-bundle`.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlCoupleDetailBundle {
    pub family: GqlFamily,
    pub spouses: Vec<GqlFamilySpouse>,
    /// One per spouse, in the order of `spouses`.
    pub persons: Vec<GqlPersonDetailBundle>,
    /// The family's notes, then each spouse's.
    pub notes: Vec<GqlNote>,
    /// The media attached to the family itself.
    pub media: Vec<GqlProfileMediaTile>,
    pub gallery: GqlGalleryBundle,
}

impl From<crate::service::couple_detail::CoupleDetailBundle> for GqlCoupleDetailBundle {
    fn from(bundle: crate::service::couple_detail::CoupleDetailBundle) -> Self {
        Self {
            family: bundle.family.into(),
            spouses: bundle.spouses.into_iter().map(Into::into).collect(),
            persons: bundle.persons.into_iter().map(Into::into).collect(),
            notes: bundle.notes.into_iter().map(Into::into).collect(),
            media: bundle.media.into_iter().map(Into::into).collect(),
            gallery: bundle.gallery.into(),
        }
    }
}

impl From<crate::service::person_detail::ProfileMediaTile> for GqlProfileMediaTile {
    fn from(item: crate::service::person_detail::ProfileMediaTile) -> Self {
        Self {
            link_id: ID(item.link_id.to_string()),
            sort_order: item.sort_order,
            family_id: item.family_id.map(|id| ID(id.to_string())),
            media: item.media.into(),
        }
    }
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
    /// The conjugal family this media reaches the profile through; null when
    /// it is attached to the person directly.
    pub family_id: Option<ID>,
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
            profile_media: bundle.profile_media.into_iter().map(Into::into).collect(),
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
            portrait: bundle.portrait.map(Into::into),
            sosa_ancestor_ids: bundle
                .sosa_ancestor_ids
                .into_iter()
                .map(|id| ID(id.to_string()))
                .collect(),
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

/// Result of the dictionary's family-name rename.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlFamilyNameRename {
    pub value: String,
    pub new_value: String,
    pub surname_prefix: Option<String>,
    pub surname: String,
    pub names_updated: i32,
    pub persons_updated: i32,
    /// `newValue` was already listed: the renamed names joined it.
    pub merged: bool,
}

impl From<oxidgene_db::repo::FamilyNameRename> for GqlFamilyNameRename {
    fn from(r: oxidgene_db::repo::FamilyNameRename) -> Self {
        Self {
            value: r.value,
            new_value: r.new_value,
            surname_prefix: r.surname_prefix,
            surname: r.surname,
            names_updated: r.names_updated as i32,
            persons_updated: r.persons_updated as i32,
            merged: r.merged,
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
            age: e.age,
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
            spouse_surname: f.spouse_surname,
            spouse_given_names: f.spouse_given_names,
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
            father_surname: c.father_surname,
            father_given_names: c.father_given_names,
            mother_id: c.mother_id.map(|id| ID(id.to_string())),
            mother_display_name: c.mother_display_name,
            mother_surname: c.mother_surname,
            mother_given_names: c.mother_given_names,
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
            birth_qualifier: e.birth_qualifier.into(),
            birth_place: e.birth_place,
            death_year: e.death_year,
            death_qualifier: e.death_qualifier.into(),
            spouse_names: e.spouse_names,
            father_name: e.father_name,
            mother_name: e.mother_name,
            children_count: e.children_count as i32,
            portrait: e.portrait.map(Into::into),
        }
    }
}

impl From<oxidgene_core::types::Kinship> for GqlKinship {
    fn from(k: oxidgene_core::types::Kinship) -> Self {
        Self {
            from_person_id: ID(k.from_person_id.to_string()),
            to_person_id: ID(k.to_person_id.to_string()),
            paths: k
                .paths
                .into_iter()
                .map(|path| GqlKinshipPath {
                    segments: path.segments.into_iter().map(Into::into).collect(),
                })
                .collect(),
            truncated: k.truncated,
            persons: k.persons.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<oxidgene_core::types::KinshipSegment> for GqlKinshipSegment {
    fn from(s: oxidgene_core::types::KinshipSegment) -> Self {
        let ids = |ids: Vec<Uuid>| ids.into_iter().map(|id| ID(id.to_string())).collect();
        Self {
            ancestor_ids: ids(s.ancestor_ids),
            family_id: s.family_id.map(|id| ID(id.to_string())),
            from_line: ids(s.from_line),
            to_line: ids(s.to_line),
            half: s.half,
            union_family_id: s.union_family_id.map(|id| ID(id.to_string())),
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
    /// Where the card's portrait is drawn from, when the person has one.
    pub portrait: Option<GqlPortraitRef>,
    /// Whether the person is the tree's SOSA root or one of its ancestors.
    pub sosa_ancestor: bool,
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

/// One pedigree from a batched request, paired with the root it was asked for.
#[derive(SimpleObject, Debug, Clone)]
pub struct GqlPedigreeEntry {
    pub root_person_id: ID,
    pub pedigree: GqlPedigree,
}

impl From<crate::service::pedigrees::PedigreeEntry> for GqlPedigreeEntry {
    fn from(entry: crate::service::pedigrees::PedigreeEntry) -> Self {
        Self {
            root_person_id: ID(entry.root_person_id.to_string()),
            pedigree: entry.pedigree.into(),
        }
    }
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
            portrait: n.portrait.map(Into::into),
            sosa_ancestor: n.sosa_ancestor,
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
