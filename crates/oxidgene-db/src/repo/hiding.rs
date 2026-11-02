//! What a restricted viewer may not see, as SQL selects it
//! (docs/privacy.md §3.2, §7): the persons a viewer's [`HidingRules`] hide,
//! the families whose content they withhold, and the events, citations,
//! media, sources, repositories, places, notes, media links and vignettes
//! hidden with them.
//!
//! Every set is a subquery over `excluded_person`, `access_scope_person`
//! and `person_visibility` and the records' own links, never a list of
//! identifiers: a query leaving hidden records out binds the same few
//! values however many there are, under SQLite's 32,766 bound parameters
//! and PostgreSQL's 65,535 alike. Each subquery is uncorrelated, so both
//! backends evaluate it once per statement.
//!
//! The person rules mirror `oxidgene_core::redaction::Redaction::build`,
//! which decides for the projections in memory; the records' rules are
//! decided here alone. A set is selected in one of two ways: inline, a
//! subquery in the condition of a list ([`Unseen`]), or as a reference to a
//! common table expression of the one statement that reads every hidden
//! record at once ([`Unseen::hidden_records_query`]).

use oxidgene_core::redaction::HidingRules;
use oxidgene_core::types::RecordKind;
use sea_orm::sea_query::{
    Alias, CommonTableExpression, Cond, Condition, Expr, ExprTrait, Func, JoinType, Query,
    SelectStatement, UnionType, WithClause, WithQuery,
};
use uuid::Uuid;

/// The persons, records and families a viewer may not see in one tree, as
/// SQL selects them. There is no default: what hides nothing, the owner's,
/// is spelled [`Unseen::NONE`], so that no `..Default::default()` drops a
/// restricted viewer's rules unnoticed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unseen {
    pub tree_id: Uuid,
    pub rules: HidingRules,
}

/// How a set is selected: inline, or by the name of a common table
/// expression of the statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Inline,
    Named,
}

/// The sets of hidden records, in the order a common table expression may
/// name the ones before it.
const SETS: [&str; 13] = [
    "hidden_person",
    "strict_person",
    "withheld_family",
    "hidden_event",
    "tainted_event",
    "hidden_citation",
    "hidden_media",
    "hidden_source",
    "hidden_repository",
    "hidden_place",
    "hidden_note",
    "hidden_media_link",
    "hidden_vignette",
];

fn t(name: &str) -> Alias {
    Alias::new(name)
}

/// Column `column` of table or alias `table`.
fn col(table: &str, column: &str) -> Expr {
    Expr::col((t(table), t(column)))
}

/// `SELECT id FROM name`: a named set.
fn named(name: &str) -> SelectStatement {
    Query::select().column(t("id")).from(t(name)).to_owned()
}

impl Unseen {
    /// Nothing hidden: the owner's.
    pub const NONE: Unseen = Unseen {
        tree_id: Uuid::nil(),
        rules: HidingRules {
            exclusions: false,
            scope: None,
            privacy: None,
        },
    };

    /// What `rules` hide in tree `tree_id`.
    #[must_use]
    pub fn new(tree_id: Uuid, rules: HidingRules) -> Self {
        Self { tree_id, rules }
    }

    /// Whether nothing is hidden: no query needs a condition.
    #[must_use]
    pub fn hides_nothing(&self) -> bool {
        self.rules.hide_nothing()
    }

    /// The condition that person `column` names someone the viewer sees:
    /// not excluded, inside the scope, and not hidden by the privacy rules.
    pub fn condition<C: sea_orm::ColumnTrait>(&self, column: C) -> Condition {
        self.person_seen(Expr::col(column.as_column_ref()))
    }

    /// [`Self::condition`] on the person expression `person`.
    pub(crate) fn condition_on(&self, person: Expr) -> Condition {
        self.person_seen(person)
    }

    /// The condition that `column`, the identifier of a record of `kind`,
    /// names a record the viewer sees: a person shown in full, a family
    /// whose content is not withheld, a record of another kind that is not
    /// hidden.
    pub fn keeping<C: sea_orm::ColumnTrait>(&self, kind: RecordKind, column: C) -> Condition {
        if self.hides_nothing() {
            return Condition::all();
        }
        if kind == RecordKind::Person {
            return self.condition(column);
        }
        Cond::all()
            .add(Expr::col(column.as_column_ref()).not_in_subquery(self.set_of(kind, Mode::Inline)))
    }

    /// The condition that `column`, the identifier of a media link, names
    /// one the viewer sees.
    pub fn keeping_media_link<C: sea_orm::ColumnTrait>(&self, column: C) -> Condition {
        if self.hides_nothing() {
            return Condition::all();
        }
        Cond::all()
            .add(Expr::col(column.as_column_ref()).not_in_subquery(self.media_links(Mode::Inline)))
    }

    /// The one statement reading every hidden record of the tree, as rows
    /// of a kind — `event`, `citation`, `media`, `source`, `repository`,
    /// `place`, `note`, `media_link`, `vignette` — and an identifier: each
    /// set a common table expression, built once and named by those after
    /// it.
    #[must_use]
    pub fn hidden_records_query(&self) -> WithQuery {
        let mut with = WithClause::new();
        for name in SETS {
            let query = self.build(name, Mode::Named);
            with.cte(
                CommonTableExpression::new()
                    .query(query)
                    .column(t("id"))
                    .table_name(t(name))
                    .materialized(true)
                    .to_owned(),
            );
        }
        // The persons and families a request knows from its redaction.
        let rows = [
            ("event", "hidden_event"),
            ("citation", "hidden_citation"),
            ("media", "hidden_media"),
            ("source", "hidden_source"),
            ("repository", "hidden_repository"),
            ("place", "hidden_place"),
            ("note", "hidden_note"),
            ("media_link", "hidden_media_link"),
            ("vignette", "hidden_vignette"),
        ]
        .map(|(kind, set)| {
            Query::select()
                .expr_as(Expr::val(kind), t("kind"))
                .column(t("id"))
                .from(t(set))
                .to_owned()
        });
        let [first, rest @ ..] = rows;
        let mut select = first;
        select.unions(rest.into_iter().map(|query| (UnionType::All, query)));
        select.with(with)
    }

    // ── The sets ─────────────────────────────────────────────────────────

    /// The set of hidden records of `kind`, `mode` naming the sets it
    /// draws on.
    fn set_of(&self, kind: RecordKind, mode: Mode) -> SelectStatement {
        match kind {
            RecordKind::Person => self.persons(mode),
            RecordKind::Family => self.families(mode),
            RecordKind::Event => self.events(mode),
            RecordKind::Citation => self.citations(mode),
            RecordKind::Media => self.media(mode),
            RecordKind::Source => self.sources(mode),
            RecordKind::Repository => self.repositories(mode),
            RecordKind::Place => self.places(mode),
            RecordKind::Note => self.notes(mode),
        }
    }

    /// Set `name`, built as `mode` says the sets it draws on are.
    fn build(&self, name: &str, mode: Mode) -> SelectStatement {
        match name {
            "hidden_person" => self.person_parts(false),
            "strict_person" => self.person_parts(true),
            "withheld_family" => self.family_parts(mode),
            "hidden_event" => self.event_set(mode, false),
            "tainted_event" => self.event_set(mode, true),
            "hidden_citation" => self.citation_set(mode),
            "hidden_media" => self.media_set(mode),
            "hidden_source" => self.source_set(mode),
            "hidden_repository" => self.repository_set(mode),
            "hidden_place" => self.place_set(mode),
            "hidden_note" => self.note_set(mode),
            "hidden_media_link" => self.media_link_set(mode),
            "hidden_vignette" => self.vignette_set(mode),
            _ => unreachable!("an unknown set: {name}"),
        }
    }

    /// Set `name`: by its name in [`Mode::Named`], built inline otherwise.
    fn set(&self, name: &str, mode: Mode) -> SelectStatement {
        match mode {
            Mode::Named => named(name),
            Mode::Inline => self.build(name, mode),
        }
    }

    fn persons(&self, mode: Mode) -> SelectStatement {
        self.set("hidden_person", mode)
    }

    /// The hidden persons whose photographs are withheld: all but those
    /// hidden only for being contemporary, where the tree shows
    /// contemporaries' photographs.
    fn strict_persons(&self, mode: Mode) -> SelectStatement {
        self.set("strict_person", mode)
    }

    fn families(&self, mode: Mode) -> SelectStatement {
        self.set("withheld_family", mode)
    }

    fn events(&self, mode: Mode) -> SelectStatement {
        self.set("hidden_event", mode)
    }

    fn citations(&self, mode: Mode) -> SelectStatement {
        self.set("hidden_citation", mode)
    }

    fn media(&self, mode: Mode) -> SelectStatement {
        self.set("hidden_media", mode)
    }

    fn sources(&self, mode: Mode) -> SelectStatement {
        self.set("hidden_source", mode)
    }

    fn repositories(&self, mode: Mode) -> SelectStatement {
        self.set("hidden_repository", mode)
    }

    fn places(&self, mode: Mode) -> SelectStatement {
        self.set("hidden_place", mode)
    }

    fn notes(&self, mode: Mode) -> SelectStatement {
        self.set("hidden_note", mode)
    }

    fn media_links(&self, mode: Mode) -> SelectStatement {
        self.set("hidden_media_link", mode)
    }

    // ── Persons ──────────────────────────────────────────────────────────

    /// The condition that person expression `person` names someone seen.
    fn person_seen(&self, person: Expr) -> Condition {
        let mut seen = Cond::all();
        if self.rules.exclusions {
            seen = seen.add(person.clone().not_in_subquery(self.excluded()));
        }
        if let Some(scope) = self.rules.scope {
            seen = seen.add(person.clone().in_subquery(scope_persons(scope)));
        }
        if self.rules.privacy.is_some() {
            seen = seen.add(person.not_in_subquery(self.private_persons(false)));
        }
        seen
    }

    /// `excluded_person`'s rows of the tree.
    fn excluded(&self) -> SelectStatement {
        Query::select()
            .expr_as(col("ep", "person_id"), t("id"))
            .from_as(t("excluded_person"), t("ep"))
            .and_where(col("ep", "tree_id").eq(self.tree_id))
            .to_owned()
    }

    /// The tree's soft-deleted persons.
    fn deleted_persons(&self) -> SelectStatement {
        Query::select()
            .expr_as(col("dp", "id"), t("id"))
            .from_as(t("person"), t("dp"))
            .and_where(col("dp", "tree_id").eq(self.tree_id))
            .and_where(col("dp", "deleted_at").is_not_null())
            .to_owned()
    }

    /// The tree's live persons outside the scope.
    fn outside_scope(&self, scope: Uuid) -> SelectStatement {
        Query::select()
            .expr_as(col("os", "id"), t("id"))
            .from_as(t("person"), t("os"))
            .and_where(col("os", "tree_id").eq(self.tree_id))
            .and_where(col("os", "deleted_at").is_null())
            .and_where(col("os", "id").not_in_subquery(scope_persons(scope)))
            .to_owned()
    }

    /// The persons the privacy rules hide (§3.1, rules 2 and 3); with
    /// `strict`, those a contemporary's photograph does not show when the
    /// tree shows them.
    fn private_persons(&self, strict: bool) -> SelectStatement {
        let mut hidden = Cond::any();
        if let Some(privacy) = self.rules.privacy {
            hidden = hidden.add(private(col("pv", "privacy"), privacy.default_private));
            if let Some(after) = privacy.contemporary_after
                && !(strict && privacy.contemporary_photos)
            {
                hidden = hidden.add(
                    Cond::all().add(col("pv", "deceased").eq(false)).add(
                        Cond::any()
                            .add(col("pv", "latest_birth_year").is_null())
                            .add(col("pv", "latest_birth_year").gt(after)),
                    ),
                );
            }
        }
        Query::select()
            .expr_as(col("pv", "person_id"), t("id"))
            .from_as(t("person_visibility"), t("pv"))
            .and_where(col("pv", "tree_id").eq(self.tree_id))
            .cond_where(hidden)
            .to_owned()
    }

    /// Every hidden person, or with `strict` those of [`Self::strict_persons`]:
    /// excluded, outside the scope, or hidden by the privacy rules; and the
    /// tree's deleted persons, whose records outlive them — a deletion
    /// cascades to nothing — and whom no rule reads any more.
    fn person_parts(&self, strict: bool) -> SelectStatement {
        let mut parts = vec![self.deleted_persons()];
        if self.rules.exclusions {
            parts.push(self.excluded());
        }
        if let Some(scope) = self.rules.scope {
            parts.push(self.outside_scope(scope));
        }
        if self.rules.privacy.is_some() {
            parts.push(self.private_persons(strict));
        }
        union(parts, Query::select)
    }

    // ── Records ──────────────────────────────────────────────────────────

    /// The families whose content is withheld: a live spouse is hidden, or
    /// the family is private to a viewer without `read_private`.
    fn family_parts(&self, mode: Mode) -> SelectStatement {
        let mut parts = vec![
            Query::select()
                .expr_as(col("wfs", "family_id"), t("id"))
                .from_as(t("family_spouse"), t("wfs"))
                .join_as(
                    JoinType::InnerJoin,
                    t("person"),
                    t("wp"),
                    col("wp", "id").equals((t("wfs"), t("person_id"))),
                )
                .and_where(col("wp", "deleted_at").is_null())
                .and_where(col("wfs", "person_id").in_subquery(self.persons(mode)))
                .to_owned(),
        ];
        if let Some(privacy) = self.rules.privacy {
            parts.push(
                Query::select()
                    .expr_as(col("wf", "id"), t("id"))
                    .from_as(t("family"), t("wf"))
                    .and_where(col("wf", "tree_id").eq(self.tree_id))
                    .and_where(private(col("wf", "privacy"), privacy.default_private))
                    .to_owned(),
            );
        }
        union(parts, Query::select)
    }

    /// The events of a hidden person or of a withheld family; with
    /// `tainted`, of a person whose photographs are withheld. A deleted
    /// event counts: its notes, citations and media links outlive it.
    fn event_set(&self, mode: Mode, tainted: bool) -> SelectStatement {
        let persons = if tainted {
            self.strict_persons(mode)
        } else {
            self.persons(mode)
        };
        Query::select()
            .expr_as(col("he", "id"), t("id"))
            .from_as(t("event"), t("he"))
            .and_where(col("he", "tree_id").eq(self.tree_id))
            .cond_where(
                Cond::any()
                    .add(col("he", "person_id").in_subquery(persons))
                    .add(col("he", "family_id").in_subquery(self.families(mode))),
            )
            .to_owned()
    }

    /// The citations on a hidden person, a withheld family or a hidden
    /// event.
    fn citation_set(&self, mode: Mode) -> SelectStatement {
        Query::select()
            .expr_as(col("hc", "id"), t("id"))
            .from_as(t("citation"), t("hc"))
            .and_where(col("hc", "source_id").in_subquery(self.tree_rows("source")))
            .cond_where(
                Cond::any()
                    .add(col("hc", "person_id").in_subquery(self.persons(mode)))
                    .add(col("hc", "family_id").in_subquery(self.families(mode)))
                    .add(col("hc", "event_id").in_subquery(self.events(mode))),
            )
            .to_owned()
    }

    /// The live media withheld (§3.2): a document, with its pages, when
    /// it or one of its pages shows — through a link, a crop, or an event
    /// either names — a person whose photographs are withheld or a withheld
    /// family, or when it is private to a viewer without `read_private`.
    fn media_set(&self, mode: Mode) -> SelectStatement {
        let tainted_events = self.set("tainted_event", mode);
        let showing = union(
            vec![
                Query::select()
                    .expr_as(col("tl", "media_id"), t("id"))
                    .from_as(t("media_link"), t("tl"))
                    .cond_where(
                        Cond::any()
                            .add(col("tl", "person_id").in_subquery(self.strict_persons(mode)))
                            .add(col("tl", "family_id").in_subquery(self.families(mode)))
                            .add(col("tl", "event_id").in_subquery(tainted_events.clone())),
                    )
                    .to_owned(),
                Query::select()
                    .expr_as(col("tv", "media_id"), t("id"))
                    .from_as(t("vignette"), t("tv"))
                    .cond_where(
                        Cond::any()
                            .add(col("tv", "person_id").in_subquery(self.strict_persons(mode)))
                            .add(col("tv", "event_id").in_subquery(tainted_events)),
                    )
                    .to_owned(),
            ],
            Query::select,
        );
        // The documents deciding: a page decides for its document.
        let documents = Query::select()
            .expr_as(document_of("tm"), t("id"))
            .from_as(t("media"), t("tm"))
            .and_where(col("tm", "tree_id").eq(self.tree_id))
            .and_where(col("tm", "deleted_at").is_null())
            .and_where(col("tm", "id").in_subquery(showing))
            .to_owned();
        let mut withheld = Cond::any().add(document_of("hm").in_subquery(documents));
        if let Some(privacy) = self.rules.privacy {
            let private_documents = Query::select()
                .expr_as(col("pd", "id"), t("id"))
                .from_as(t("media"), t("pd"))
                .and_where(col("pd", "tree_id").eq(self.tree_id))
                .and_where(private(col("pd", "privacy"), privacy.default_private))
                .to_owned();
            withheld = withheld
                .add(
                    Cond::all()
                        .add(col("hm", "parent_media_id").is_null())
                        .add(private(col("hm", "privacy"), privacy.default_private)),
                )
                .add(col("hm", "parent_media_id").in_subquery(private_documents));
        }
        Query::select()
            .expr_as(col("hm", "id"), t("id"))
            .from_as(t("media"), t("hm"))
            .and_where(col("hm", "tree_id").eq(self.tree_id))
            .and_where(col("hm", "deleted_at").is_null())
            .cond_where(withheld)
            .to_owned()
    }

    /// The sources cited only by hidden citations.
    fn source_set(&self, mode: Mode) -> SelectStatement {
        let cited_hidden = Query::select()
            .expr_as(col("c1", "source_id"), t("id"))
            .from_as(t("citation"), t("c1"))
            .and_where(col("c1", "id").in_subquery(self.citations(mode)))
            .to_owned();
        let cited_shown = Query::select()
            .expr(Expr::val(1))
            .from_as(t("citation"), t("c2"))
            .and_where(col("c2", "source_id").equals((t("hs"), t("id"))))
            .and_where(col("c2", "id").not_in_subquery(self.citations(mode)))
            .to_owned();
        Query::select()
            .expr_as(col("hs", "id"), t("id"))
            .from_as(t("source"), t("hs"))
            .and_where(col("hs", "tree_id").eq(self.tree_id))
            .and_where(col("hs", "id").in_subquery(cited_hidden))
            .and_where(Expr::exists(cited_shown).not())
            .to_owned()
    }

    /// The repositories holding only hidden sources.
    fn repository_set(&self, mode: Mode) -> SelectStatement {
        let holding_hidden = Query::select()
            .expr_as(col("r1", "repository_id"), t("id"))
            .from_as(t("source_repository"), t("r1"))
            .and_where(col("r1", "source_id").in_subquery(self.sources(mode)))
            .to_owned();
        let holding_shown = Query::select()
            .expr(Expr::val(1))
            .from_as(t("source_repository"), t("r2"))
            .and_where(col("r2", "repository_id").equals((t("hr"), t("id"))))
            .and_where(col("r2", "source_id").not_in_subquery(self.sources(mode)))
            .to_owned();
        Query::select()
            .expr_as(col("hr", "id"), t("id"))
            .from_as(t("repository"), t("hr"))
            .and_where(col("hr", "tree_id").eq(self.tree_id))
            .and_where(col("hr", "id").in_subquery(holding_hidden))
            .and_where(Expr::exists(holding_shown).not())
            .to_owned()
    }

    /// The places only hidden events and media use.
    fn place_set(&self, mode: Mode) -> SelectStatement {
        let at_hidden = union(
            vec![
                Query::select()
                    .expr_as(col("p1", "place_id"), t("id"))
                    .from_as(t("event"), t("p1"))
                    .and_where(col("p1", "place_id").is_not_null())
                    .and_where(col("p1", "id").in_subquery(self.events(mode)))
                    .to_owned(),
                Query::select()
                    .expr_as(col("p2", "place_id"), t("id"))
                    .from_as(t("media"), t("p2"))
                    .and_where(col("p2", "place_id").is_not_null())
                    .and_where(col("p2", "id").in_subquery(self.media(mode)))
                    .to_owned(),
            ],
            Query::select,
        );
        let shown_event = Query::select()
            .expr(Expr::val(1))
            .from_as(t("event"), t("p3"))
            .and_where(col("p3", "place_id").equals((t("hp"), t("id"))))
            .and_where(col("p3", "deleted_at").is_null())
            .and_where(col("p3", "id").not_in_subquery(self.events(mode)))
            .to_owned();
        let shown_medium = Query::select()
            .expr(Expr::val(1))
            .from_as(t("media"), t("p4"))
            .and_where(col("p4", "place_id").equals((t("hp"), t("id"))))
            .and_where(col("p4", "deleted_at").is_null())
            .and_where(col("p4", "id").not_in_subquery(self.media(mode)))
            .to_owned();
        Query::select()
            .expr_as(col("hp", "id"), t("id"))
            .from_as(t("place"), t("hp"))
            .and_where(col("hp", "tree_id").eq(self.tree_id))
            .and_where(col("hp", "id").in_subquery(at_hidden))
            .and_where(Expr::exists(shown_event).not())
            .and_where(Expr::exists(shown_medium).not())
            .to_owned()
    }

    /// The live notes about a hidden record. Confidential notes are left
    /// out by the lists themselves.
    fn note_set(&self, mode: Mode) -> SelectStatement {
        Query::select()
            .expr_as(col("hn", "id"), t("id"))
            .from_as(t("note"), t("hn"))
            .and_where(col("hn", "tree_id").eq(self.tree_id))
            .and_where(col("hn", "deleted_at").is_null())
            .cond_where(
                Cond::any()
                    .add(col("hn", "person_id").in_subquery(self.persons(mode)))
                    .add(col("hn", "family_id").in_subquery(self.families(mode)))
                    .add(col("hn", "event_id").in_subquery(self.events(mode)))
                    .add(col("hn", "media_id").in_subquery(self.media(mode)))
                    .add(col("hn", "source_id").in_subquery(self.sources(mode)))
                    .add(col("hn", "repository_id").in_subquery(self.repositories(mode))),
            )
            .to_owned()
    }

    /// The media links of a hidden medium, or attaching one to a hidden
    /// person, family, event or source.
    fn media_link_set(&self, mode: Mode) -> SelectStatement {
        Query::select()
            .expr_as(col("hl", "id"), t("id"))
            .from_as(t("media_link"), t("hl"))
            .and_where(col("hl", "media_id").in_subquery(self.tree_rows("media")))
            .cond_where(
                Cond::any()
                    .add(col("hl", "media_id").in_subquery(self.media(mode)))
                    .add(col("hl", "person_id").in_subquery(self.persons(mode)))
                    .add(col("hl", "family_id").in_subquery(self.families(mode)))
                    .add(col("hl", "event_id").in_subquery(self.events(mode)))
                    .add(col("hl", "source_id").in_subquery(self.sources(mode))),
            )
            .to_owned()
    }

    /// The crops of a hidden medium, or showing a hidden person or cut for
    /// a hidden event.
    fn vignette_set(&self, mode: Mode) -> SelectStatement {
        Query::select()
            .expr_as(col("hv", "id"), t("id"))
            .from_as(t("vignette"), t("hv"))
            .and_where(col("hv", "media_id").in_subquery(self.tree_rows("media")))
            .cond_where(
                Cond::any()
                    .add(col("hv", "media_id").in_subquery(self.media(mode)))
                    .add(col("hv", "person_id").in_subquery(self.persons(mode)))
                    .add(col("hv", "event_id").in_subquery(self.events(mode))),
            )
            .to_owned()
    }

    /// The identifiers of the tree's rows of `table`.
    fn tree_rows(&self, table: &str) -> SelectStatement {
        Query::select()
            .expr_as(col("tr", "id"), t("id"))
            .from_as(t(table), t("tr"))
            .and_where(col("tr", "tree_id").eq(self.tree_id))
            .to_owned()
    }
}

/// The persons of access scope `scope`.
fn scope_persons(scope: Uuid) -> SelectStatement {
    Query::select()
        .expr_as(col("asp", "person_id"), t("id"))
        .from_as(t("access_scope_person"), t("asp"))
        .and_where(col("asp", "scope_id").eq(scope))
        .to_owned()
}

/// The condition that a record of privacy `privacy` is private (§3.1, rule
/// 2): `private`, or `default` on a tree whose default is private.
fn private(privacy: Expr, default_private: bool) -> Expr {
    let mut values = vec!["private"];
    if default_private {
        values.push("default");
    }
    privacy.is_in(values)
}

/// The document a medium of alias `alias` belongs to: its own identifier
/// for a document.
fn document_of(alias: &str) -> Expr {
    Func::coalesce([col(alias, "parent_media_id"), col(alias, "id")]).into()
}

/// `parts` as one set; `empty` when there are none.
fn union(parts: Vec<SelectStatement>, empty: impl FnOnce() -> SelectStatement) -> SelectStatement {
    let mut parts = parts.into_iter();
    let Some(mut first) = parts.next() else {
        return empty();
    };
    first.unions(parts.map(|part| (UnionType::Distinct, part)));
    first
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidgene_core::redaction::PrivacyRules;
    use sea_orm::sea_query::{PostgresQueryBuilder, SqliteQueryBuilder};

    fn visitor() -> Unseen {
        Unseen::new(
            Uuid::nil(),
            HidingRules {
                exclusions: true,
                scope: Some(Uuid::nil()),
                privacy: Some(PrivacyRules {
                    default_private: true,
                    contemporary_after: Some(1926),
                    contemporary_photos: true,
                }),
            },
        )
    }

    #[test]
    fn the_hidden_records_bind_the_same_values_however_many_are_hidden() {
        let query = visitor().hidden_records_query();
        for (sql, values) in [
            query.build(SqliteQueryBuilder),
            query.build(PostgresQueryBuilder),
        ] {
            assert!(sql.starts_with("WITH "), "{sql}");
            assert!(values.0.len() < 100, "{} values", values.0.len());
        }
    }

    #[test]
    fn nothing_hidden_adds_no_condition() {
        let owner = Unseen::NONE;
        assert!(owner.hides_nothing());
        let condition = owner.keeping(RecordKind::Event, crate::entities::event::Column::Id);
        assert!(condition.is_empty());
    }
}
