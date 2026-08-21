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
///
/// `Alias`, `Byname`, `Sobriquet` and `GivenName` are OxidGene-specific
/// refinements of "also known as": GEDCOM's `NAME.TYPE` enumeration has no
/// equivalent, so all four export as `aka` (see `oxidgene-gedcom`). They exist
/// because the UI lets the user pick between them, and collapsing them onto
/// [`Self::AlsoKnownAs`] on save made the choice unrecoverable on reload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NameType {
    Birth,
    Married,
    AlsoKnownAs,
    Maiden,
    Religious,
    /// An additional given name, recorded on its own rather than as part of
    /// the birth name's `given_names` piece.
    GivenName,
    /// A formally adopted alternative name (stage name, pen name).
    Alias,
    /// A byname the person was commonly known by.
    Byname,
    /// A familiar or ironic nickname, distinct from a plain byname.
    Sobriquet,
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
            Self::GivenName => write!(f, "Given name"),
            Self::Alias => write!(f, "Alias"),
            Self::Byname => write!(f, "Byname"),
            Self::Sobriquet => write!(f, "Sobriquet"),
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
    /// Worked out from other facts (GEDCOM `CAL`).
    Calculated,
    /// Guessed from indirect evidence (GEDCOM `EST`).
    Estimated,
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
            Self::Calculated => write!(f, "calculated"),
            Self::Estimated => write!(f, "estimated"),
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

    /// The mark that goes in front of a bare year on a pedigree card, so that
    /// `ca 1849–< 1917` reads as "born about 1849, died before 1917" at a
    /// glance rather than claiming two dates we do not have.
    ///
    /// These are GeneWeb's own symbols (`prec_text`, `lib/dateDisplay.ml`),
    /// which is what Geneanet draws and therefore what anyone arriving from a
    /// Geneanet tree already reads fluently. The trailing space is part of the
    /// mark: `ca 1849`, `< 1917`.
    ///
    /// `Calculated`, `Estimated` and `FromAge` have no GeneWeb counterpart and
    /// all fold into `ca`. Each is an approximation arrived at by a different
    /// route, and once the arithmetic is done the reader of a *card* wants the
    /// same warning from all three. The distinction is not lost — it stays on
    /// the event, and the edit modal and the events panel still name it in
    /// full — it is simply not what four characters of card should spend
    /// themselves on.
    pub fn short_prefix(&self) -> &'static str {
        match self {
            Self::Exact => "",
            Self::About | Self::Calculated | Self::Estimated | Self::FromAge => "ca ",
            Self::Perhaps => "? ",
            Self::Before => "< ",
            Self::After => "> ",
            Self::Or => "| ",
            Self::Between => ".. ",
        }
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

/// What `Privacy::Default` resolves to, for one tree.
///
/// Deliberately *not* [`Privacy`]: that enum's own `Default` variant means
/// "follow the tree", and a tree whose default followed the tree would be
/// circular. Two variants, so the nonsensical state cannot be written down.
///
/// `Private` is the default. A genealogy holds living people, and a tree
/// nobody has classified has not been cleared for publication — the value that
/// applies before anyone has thought about it should be the one that
/// withholds. Publishing is the deliberate act.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeDefaultPrivacy {
    Public,
    #[default]
    Private,
}

impl std::fmt::Display for TreeDefaultPrivacy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TreeDefaultPrivacy {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "public" => Some(Self::Public),
            "private" => Some(Self::Private),
            _ => None,
        }
    }

    /// Resolve one record's setting against this tree's default.
    ///
    /// The whole point of the pair: a record saying `Default` has not made a
    /// choice, so the tree makes it. `Public` and `Private` on the record are
    /// choices, and override.
    #[must_use]
    pub fn resolve(self, privacy: Privacy) -> Self {
        match privacy {
            Privacy::Public => Self::Public,
            Privacy::Private => Self::Private,
            Privacy::Default => self,
        }
    }
}

/// What kind of thing a medium physically *is* — GEDCOM's
/// `SOURCE_MEDIA_TYPE`.
///
/// This is the vocabulary GEDCOM defines, exactly: `OBJE.FILE.FORM.TYPE` in
/// 5.5.1, `FORM.MEDI` in 7.0. Keeping it enumerated rather than free text is
/// what makes an export readable by other genealogy software, so the variants
/// are GEDCOM's and are not ours to extend — a distinction GEDCOM does not
/// draw belongs in [`DocumentCategory`] instead.
///
/// Note what it describes: the *carrier*, not the content. A census return
/// and a notarial deed are both `Manuscript` here; that they are different
/// kinds of record is a genealogical fact GEDCOM has no field for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceMediaType {
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
    #[default]
    Other,
}

impl std::fmt::Display for SourceMediaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SourceMediaType {
    /// The snake_case spelling used on the wire and in the database.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Book => "book",
            Self::Card => "card",
            Self::Electronic => "electronic",
            Self::Fiche => "fiche",
            Self::Film => "film",
            Self::Magazine => "magazine",
            Self::Manuscript => "manuscript",
            Self::Map => "map",
            Self::Newspaper => "newspaper",
            Self::Photo => "photo",
            Self::Tombstone => "tombstone",
            Self::Video => "video",
            Self::Other => "other",
        }
    }

    /// The GEDCOM tag value, which is the same word in upper case.
    #[must_use]
    pub fn gedcom_value(self) -> &'static str {
        match self {
            Self::Audio => "AUDIO",
            Self::Book => "BOOK",
            Self::Card => "CARD",
            Self::Electronic => "ELECTRONIC",
            Self::Fiche => "FICHE",
            Self::Film => "FILM",
            Self::Magazine => "MAGAZINE",
            Self::Manuscript => "MANUSCRIPT",
            Self::Map => "MAP",
            Self::Newspaper => "NEWSPAPER",
            Self::Photo => "PHOTO",
            Self::Tombstone => "TOMBSTONE",
            Self::Video => "VIDEO",
            Self::Other => "OTHER",
        }
    }

    /// Read a GEDCOM value or a stored spelling, case-insensitively.
    ///
    /// `None` for anything outside the enumeration — a producer that wrote
    /// something of its own. The caller keeps `Other` rather than inventing a
    /// meaning.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "audio" => Self::Audio,
            "book" => Self::Book,
            "card" => Self::Card,
            "electronic" => Self::Electronic,
            "fiche" => Self::Fiche,
            "film" => Self::Film,
            "magazine" => Self::Magazine,
            "manuscript" => Self::Manuscript,
            "map" => Self::Map,
            "newspaper" => Self::Newspaper,
            "photo" => Self::Photo,
            "tombstone" => Self::Tombstone,
            "video" => Self::Video,
            "other" => Self::Other,
            _ => return None,
        })
    }

    /// Every variant, in the order a picker should list them.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::Photo,
            Self::Manuscript,
            Self::Book,
            Self::Card,
            Self::Newspaper,
            Self::Magazine,
            Self::Map,
            Self::Tombstone,
            Self::Fiche,
            Self::Film,
            Self::Audio,
            Self::Video,
            Self::Electronic,
            Self::Other,
        ]
    }
}

/// What kind of *record* a medium is — the distinction GEDCOM does not draw.
///
/// [`SourceMediaType`] says a document is a `Manuscript`; it cannot say
/// whether that manuscript is a census return, a notarial deed or a military
/// register, and to a genealogist that is most of what matters. Geneanet's
/// own media types make exactly this distinction, which is why importing from
/// there and exporting to GEDCOM need two fields rather than one.
///
/// Optional: a photograph somebody uploaded is a photograph, and forcing a
/// category onto it would be inventing information. Each variant knows the
/// physical medium it implies, so a category answers both questions and a
/// bare `SourceMediaType` still answers the one GEDCOM asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentCategory {
    /// A portrait of one person.
    Portrait,
    /// A photograph of several people together.
    GroupPhoto,
    /// Family papers — letters, faire-parts, remembrance cards.
    FamilyDocument,
    /// Civil registration: birth, marriage and death records.
    CivilRecord,
    /// Parish registers, which predate civil registration.
    ParishRecord,
    /// Deeds, wills, marriage contracts and inventories.
    NotarialArchive,
    /// Conscription registers and service records.
    MilitaryArchive,
    /// Census returns.
    Census,
    /// A coat of arms.
    CoatOfArms,
    /// A grave or headstone.
    Grave,
    Other,
}

impl std::fmt::Display for DocumentCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl DocumentCategory {
    /// The snake_case spelling used on the wire and in the database.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::GroupPhoto => "group_photo",
            Self::FamilyDocument => "family_document",
            Self::CivilRecord => "civil_record",
            Self::ParishRecord => "parish_record",
            Self::NotarialArchive => "notarial_archive",
            Self::MilitaryArchive => "military_archive",
            Self::Census => "census",
            Self::CoatOfArms => "coat_of_arms",
            Self::Grave => "grave",
            Self::Other => "other",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "portrait" => Self::Portrait,
            "group_photo" => Self::GroupPhoto,
            "family_document" => Self::FamilyDocument,
            "civil_record" => Self::CivilRecord,
            "parish_record" => Self::ParishRecord,
            "notarial_archive" => Self::NotarialArchive,
            "military_archive" => Self::MilitaryArchive,
            "census" => Self::Census,
            "coat_of_arms" => Self::CoatOfArms,
            "grave" => Self::Grave,
            "other" => Self::Other,
            _ => return None,
        })
    }

    /// The physical medium this kind of record is normally carried on.
    ///
    /// What a GEDCOM export writes when the user chose a category and never
    /// touched the medium — which is the common case, since the category is
    /// the question they can actually answer about a scan.
    #[must_use]
    pub fn implied_medium(self) -> SourceMediaType {
        match self {
            Self::Portrait | Self::GroupPhoto => SourceMediaType::Photo,
            Self::FamilyDocument
            | Self::CivilRecord
            | Self::ParishRecord
            | Self::NotarialArchive
            | Self::MilitaryArchive
            | Self::Census => SourceMediaType::Manuscript,
            Self::Grave => SourceMediaType::Tombstone,
            Self::CoatOfArms | Self::Other => SourceMediaType::Other,
        }
    }

    /// Every variant, in the order a picker should list them.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::Portrait,
            Self::GroupPhoto,
            Self::FamilyDocument,
            Self::CivilRecord,
            Self::ParishRecord,
            Self::NotarialArchive,
            Self::MilitaryArchive,
            Self::Census,
            Self::CoatOfArms,
            Self::Grave,
            Self::Other,
        ]
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
    // GeneWeb's own vocabulary — see `oxidgene_gedcom::import`. GEDCOM has
    // no tag for most of these, so they travel as a generic `EVEN` whose
    // `TYPE` names them.
    /// GEDCOM `Blessing`.
    Blessing,
    /// GEDCOM `Ordination`.
    Ordination,
    /// GEDCOM `Christening`.
    Christening,
    /// GEDCOM `AdultChristening`.
    AdultChristening,
    /// GeneWeb generic `EVEN` labelled `Accomplishment`.
    Accomplishment,
    /// GeneWeb generic `EVEN` labelled `Acquisition`.
    Acquisition,
    /// GeneWeb generic `EVEN` labelled `Membership`.
    Membership,
    /// GeneWeb generic `EVEN` labelled `Change name`.
    ChangeName,
    /// GeneWeb generic `EVEN` labelled `Circumcision`.
    Circumcision,
    /// GeneWeb generic `EVEN` labelled `Award`.
    Award,
    /// GeneWeb generic `EVEN` labelled `Military discharge`.
    MilitaryDischarge,
    /// GeneWeb generic `EVEN` labelled `Degree`.
    Degree,
    /// GeneWeb generic `EVEN` labelled `Distinction`.
    Distinction,
    /// GeneWeb generic `EVEN` labelled `Election`.
    Election,
    /// GeneWeb generic `EVEN` labelled `Excommunication`.
    Excommunication,
    /// GeneWeb generic `EVEN` labelled `Funeral`.
    Funeral,
    /// GeneWeb generic `EVEN` labelled `Hospitalization`.
    Hospitalization,
    /// GeneWeb generic `EVEN` labelled `Illness`.
    Illness,
    /// GeneWeb generic `EVEN` labelled `Passenger list`.
    PassengerList,
    /// GeneWeb generic `EVEN` labelled `Military distinction`.
    MilitaryDistinction,
    /// GeneWeb generic `EVEN` labelled `Military promotion`.
    MilitaryPromotion,
    /// GeneWeb generic `EVEN` labelled `Military mobilization`.
    MilitaryMobilization,
    /// GeneWeb generic `EVEN` labelled `Property sale`.
    PropertySale,
    /// GeneWeb generic `EVEN` labelled `ENDL`.
    Endowment,
    /// GeneWeb generic `EVEN` labelled `DotationLDS`.
    LdsDotation,
    /// GeneWeb generic `EVEN` labelled `SLGC`.
    SealingChild,
    /// GeneWeb generic `EVEN` labelled `SLGS`.
    SealingSpouse,
    /// GeneWeb generic `EVEN` labelled `Scellent parent LDS`.
    SealingParent,
    /// GeneWeb generic `EVEN` labelled `Family link LDS`.
    FamilyLinkLds,
    /// GeneWeb generic `EVEN` labelled `unmarried`.
    NoMarriage,
    /// GeneWeb generic `EVEN` labelled `nomen`.
    NoMention,
    /// GeneWeb generic `EVEN` labelled `BAPL`.
    LdsBaptism,
    /// GeneWeb generic `EVEN` labelled `CONL`.
    LdsConfirmation,
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
            Self::Blessing => write!(f, "blessing"),
            Self::Ordination => write!(f, "ordination"),
            Self::Christening => write!(f, "christening"),
            Self::AdultChristening => write!(f, "adult_christening"),
            Self::Accomplishment => write!(f, "accomplishment"),
            Self::Acquisition => write!(f, "acquisition"),
            Self::Membership => write!(f, "membership"),
            Self::ChangeName => write!(f, "change_name"),
            Self::Circumcision => write!(f, "circumcision"),
            Self::Award => write!(f, "award"),
            Self::MilitaryDischarge => write!(f, "military_discharge"),
            Self::Degree => write!(f, "degree"),
            Self::Distinction => write!(f, "distinction"),
            Self::Election => write!(f, "election"),
            Self::Excommunication => write!(f, "excommunication"),
            Self::Funeral => write!(f, "funeral"),
            Self::Hospitalization => write!(f, "hospitalization"),
            Self::Illness => write!(f, "illness"),
            Self::PassengerList => write!(f, "passenger_list"),
            Self::MilitaryDistinction => write!(f, "military_distinction"),
            Self::MilitaryPromotion => write!(f, "military_promotion"),
            Self::MilitaryMobilization => write!(f, "military_mobilization"),
            Self::PropertySale => write!(f, "property_sale"),
            Self::Endowment => write!(f, "endowment"),
            Self::LdsDotation => write!(f, "lds_dotation"),
            Self::SealingChild => write!(f, "sealing_child"),
            Self::SealingSpouse => write!(f, "sealing_spouse"),
            Self::SealingParent => write!(f, "sealing_parent"),
            Self::FamilyLinkLds => write!(f, "family_link_lds"),
            Self::NoMarriage => write!(f, "no_marriage"),
            Self::NoMention => write!(f, "no_mention"),
            Self::LdsBaptism => write!(f, "lds_baptism"),
            Self::LdsConfirmation => write!(f, "lds_confirmation"),
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

    /// The marks Geneanet draws on the tree this was modelled from, read off
    /// the live page: `ca 1849-< 1917`, `< 1907-`, `> 1912-`.
    #[test]
    fn short_prefix_matches_geneweb_symbols() {
        assert_eq!(DateQualifier::Exact.short_prefix(), "");
        assert_eq!(DateQualifier::About.short_prefix(), "ca ");
        assert_eq!(DateQualifier::Perhaps.short_prefix(), "? ");
        assert_eq!(DateQualifier::Before.short_prefix(), "< ");
        assert_eq!(DateQualifier::After.short_prefix(), "> ");
        assert_eq!(DateQualifier::Or.short_prefix(), "| ");
        assert_eq!(DateQualifier::Between.short_prefix(), ".. ");
    }

    /// GEDCOM's `CAL`/`EST` and our own `FromAge` have no GeneWeb counterpart.
    /// They read as `ca` on a card rather than inventing a symbol nobody knows;
    /// the exact qualifier survives on the event for the modal and the panel.
    #[test]
    fn the_approximations_geneweb_lacks_read_as_about() {
        for q in [
            DateQualifier::Calculated,
            DateQualifier::Estimated,
            DateQualifier::FromAge,
        ] {
            assert_eq!(q.short_prefix(), "ca ", "{q} should read as about");
        }
    }

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

#[cfg(test)]
mod media_type_tests {
    use super::{DocumentCategory, SourceMediaType};

    #[test]
    fn gedcom_values_round_trip_through_the_parser() {
        for medium in SourceMediaType::all() {
            assert_eq!(
                SourceMediaType::parse(medium.gedcom_value()),
                Some(*medium),
                "{medium} must survive an export and a re-import"
            );
            assert_eq!(SourceMediaType::parse(medium.as_str()), Some(*medium));
        }
    }

    #[test]
    fn a_vocabulary_of_someone_elses_is_not_guessed_at() {
        // A producer writing its own word. Reading it as anything in the
        // enumeration would be inventing a fact about the document.
        assert_eq!(SourceMediaType::parse("Acte notarié"), None);
        assert_eq!(SourceMediaType::parse(""), None);
    }

    #[test]
    fn categories_round_trip_through_the_parser() {
        for category in DocumentCategory::all() {
            assert_eq!(DocumentCategory::parse(category.as_str()), Some(*category));
        }
    }

    #[test]
    fn every_category_implies_a_medium_gedcom_can_express() {
        // The point of keeping two fields: whatever the user classified a
        // scan as, the export still has something truthful to write.
        assert_eq!(
            DocumentCategory::Portrait.implied_medium(),
            SourceMediaType::Photo
        );
        assert_eq!(
            DocumentCategory::Grave.implied_medium(),
            SourceMediaType::Tombstone
        );
        // The lossy direction, and the reason `document_category` exists:
        // three different kinds of record, one GEDCOM word.
        for category in [
            DocumentCategory::Census,
            DocumentCategory::NotarialArchive,
            DocumentCategory::MilitaryArchive,
        ] {
            assert_eq!(category.implied_medium(), SourceMediaType::Manuscript);
        }
    }

    #[test]
    fn an_unclassified_medium_defaults_to_admitting_it_does_not_know() {
        // Not `Photo`: the table holds scans and PDFs as readily as
        // photographs, and a default that guessed would mislabel them.
        assert_eq!(SourceMediaType::default(), SourceMediaType::Other);
    }
}

#[cfg(test)]
mod tree_privacy_tests {
    use super::{Privacy, TreeDefaultPrivacy};

    #[test]
    fn a_record_that_has_not_chosen_takes_the_trees_answer() {
        assert_eq!(
            TreeDefaultPrivacy::Public.resolve(Privacy::Default),
            TreeDefaultPrivacy::Public
        );
        assert_eq!(
            TreeDefaultPrivacy::Private.resolve(Privacy::Default),
            TreeDefaultPrivacy::Private
        );
    }

    #[test]
    fn a_record_that_has_chosen_overrides_the_tree() {
        // Marking one person public in a private tree is the whole reason the
        // per-record field exists.
        assert_eq!(
            TreeDefaultPrivacy::Private.resolve(Privacy::Public),
            TreeDefaultPrivacy::Public
        );
        assert_eq!(
            TreeDefaultPrivacy::Public.resolve(Privacy::Private),
            TreeDefaultPrivacy::Private
        );
    }

    #[test]
    fn an_unclassified_tree_withholds() {
        // A genealogy holds living people; publishing is the deliberate act.
        assert_eq!(TreeDefaultPrivacy::default(), TreeDefaultPrivacy::Private);
    }

    #[test]
    fn the_stored_spellings_round_trip() {
        for value in [TreeDefaultPrivacy::Public, TreeDefaultPrivacy::Private] {
            assert_eq!(TreeDefaultPrivacy::parse(value.as_str()), Some(value));
        }
        assert_eq!(TreeDefaultPrivacy::parse("default"), None);
    }
}
