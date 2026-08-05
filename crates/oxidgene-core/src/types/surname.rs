//! Surname particle handling (GEDCOM `SPFX`).
//!
//! A surname particle is the nobiliary or toponymic word that precedes the
//! surname root — "de la" in "de la Cruz", "van der" in "van der Berg". GEDCOM
//! records it in its own `SPFX` sub-tag of `NAME`, separate from `SURN`, so
//! that "de la Cruz" can be filed under either D or C depending on the
//! reader's convention.
//!
//! OxidGene does not ask the user for the particle as a separate input. The
//! UI keeps a single "surname" field and calls [`split_surname_particle`] to
//! derive the split, showing the result so it can be corrected. That keeps
//! data entry to one field while still producing a structured `SPFX`.

/// Particle tokens that may *start* a particle run, lowercased.
///
/// These are the prepositional particles proper — "de la Cruz" files under C,
/// "von Berg" under B.
///
/// Deliberately excludes `mac` / `mc` / `o'`: in Gaelic surnames those are
/// bound to the root ("MacDonald", "O'Brien") rather than being separate
/// words, so treating them as particles would split names that should not be.
const HEAD_PARTICLES: &[&str] = &[
    // French
    "de", "du", "des", // Spanish / Portuguese
    "del", "dos", "das", "do", "da", // Italian
    "di", "dal", "dalla", "della", "dello", "dei", "degli", "delle", // Dutch / Flemish
    "van", "vander", "ver", "ten", "ter", "te", "op", "in", "'t", "aan", "uit",
    // German
    "von", "vom", "zu", "zur", "zum", "auf",
];

/// Particle tokens that only count *after* a [`HEAD_PARTICLES`] token.
///
/// These are bare articles, and a surname that opens with one keeps it: the
/// many Breton and Norman "Le …" / "La …" names, Italian "Lo …", Dutch
/// "Den …" all file under L / D as written, because the article is welded to
/// the name rather than preceding it. The same words *are* part of the
/// particle once a preposition has introduced them — "de **la** Cruz",
/// "van **der** Berg", "von **dem** Busche".
///
/// Spanish `y` / Portuguese `e` sit here for a different reason: they join two
/// surnames ("García **y** López") and are never a leading particle either.
const TAIL_PARTICLES: &[&str] = &[
    "le", "la", "les", "lo", // French / Italian articles
    "der", "den", "dem", // German / Dutch articles
    "y", "e", // Spanish / Portuguese conjunctions
];

/// Elided particles that may start a particle run (`d'Aubigné`, `Dell'Acqua`).
///
/// Matched against the part of a token *before* the apostrophe, lowercased.
const HEAD_ELIDED_PARTICLES: &[&str] = &["d", "dell", "all", "nell", "sull", "dall"];

/// Elided articles, subject to the same rule as [`TAIL_PARTICLES`]: "L'Étang"
/// keeps its article and files under L, but "de **l'**Étang" does not.
const TAIL_ELIDED_PARTICLES: &[&str] = &["l"];

/// Both apostrophe characters that show up in imported genealogy data.
const APOSTROPHES: [char; 2] = ['\'', '\u{2019}'];

/// Splits a raw surname into its particle and its root.
///
/// Returns `(particle, root)`. The root is never empty: a value made up
/// entirely of particle words (someone actually surnamed "Le") is returned
/// unsplit, since filing it under nothing would be worse than filing it under
/// its own first letter.
///
/// A leading article is not a particle — see [`TAIL_PARTICLES`] — so a
/// "Le …" surname stays whole while "de la Cruz" still yields "de la".
///
/// The original casing and apostrophe characters are preserved in both parts —
/// only the matching is case-insensitive.
///
/// ```
/// use oxidgene_core::types::split_surname_particle;
///
/// assert_eq!(split_surname_particle("de la Cruz"), (Some("de la".into()), "Cruz".into()));
/// assert_eq!(split_surname_particle("d'Aubigné"), (Some("d'".into()), "Aubigné".into()));
/// assert_eq!(split_surname_particle("Dupont"), (None, "Dupont".into()));
/// assert_eq!(split_surname_particle("MacDonald"), (None, "MacDonald".into()));
/// assert_eq!(split_surname_particle("Le Branch"), (None, "Le Branch".into()));
/// ```
#[must_use]
pub fn split_surname_particle(raw: &str) -> (Option<String>, String) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (None, String::new());
    }

    let tokens: Vec<&str> = trimmed.split_whitespace().collect();

    // Consume whole tokens that are particles, always leaving at least one
    // token behind to serve as the root. `taken > 0` is what makes an article
    // count: it may continue a run a preposition opened, never start one.
    let mut taken = 0;
    while taken + 1 < tokens.len() && is_particle(tokens[taken], taken > 0) {
        taken += 1;
    }

    // The first non-particle token may still carry an elided particle glued to
    // it by an apostrophe ("l'Étang"). Only split it when a root remains.
    let elided: Option<(&str, &str)> = tokens
        .get(taken)
        .and_then(|token| split_elided(token, taken > 0));

    if taken == 0 && elided.is_none() {
        return (None, trimmed.to_string());
    }

    let mut particle_parts: Vec<&str> = tokens[..taken].to_vec();
    let root = match elided {
        Some((head, rest)) => {
            particle_parts.push(head);
            let mut root = String::from(rest);
            for token in &tokens[taken + 1..] {
                root.push(' ');
                root.push_str(token);
            }
            root
        }
        None => tokens[taken..].join(" "),
    };

    if root.is_empty() {
        return (None, trimmed.to_string());
    }

    (Some(particle_parts.join(" ")), root)
}

/// Splits a surname using an explicitly supplied particle instead of guessing.
///
/// This is the override path for [`split_surname_particle`]: the UI uses it
/// when the user corrects a detected particle, and the GEDCOM importer uses it
/// when the file states its own `SPFX`. An empty `particle` means "this name
/// has no particle" — which is how someone actually surnamed "Le" or a
/// "Da Silva" that should file under D opts out of detection.
///
/// When `raw` already starts with `particle` the two are de-duplicated, keeping
/// the casing as written in `raw`; otherwise both are taken at face value.
///
/// ```
/// use oxidgene_core::types::split_surname_with;
///
/// // Correcting a detected particle.
/// assert_eq!(split_surname_with("VON BERG", "VON"), (Some("VON".into()), "BERG".into()));
/// // Opting out of the split entirely.
/// assert_eq!(split_surname_with("Da Silva", ""), (None, "Da Silva".into()));
/// ```
#[must_use]
pub fn split_surname_with(raw: &str, particle: &str) -> (Option<String>, String) {
    let raw = raw.trim();
    let particle = particle.trim();

    // `raw` usually still contains the particle (it is the full surname as
    // typed, or the value between GEDCOM's slashes), so cut rather than ending
    // up with "de la de la Cruz".
    if let Some(split) = split_surname_at_head(raw, particle) {
        return split;
    }

    // The particle is not in `raw` at all. A GEDCOM file may legitimately say
    // so — `2 SPFX de la` beside a bare `2 SURN Cruz` — so both are taken at
    // face value. Callers whose particle can only ever *cut* an existing
    // string should use [`split_surname_at_head`] and handle its `None`.
    if raw.is_empty() {
        return (Some(particle.to_string()), String::new());
    }
    (Some(particle.to_string()), raw.to_string())
}

/// Splits `raw` at a particle that must already sit at its head.
///
/// Returns `None` when `particle` is not the leading word (or words) of `raw`,
/// so it cannot be applied without inventing text. An empty `particle` always
/// succeeds and means "no particle": the whole value is the root.
///
/// This is what a single-field surname input needs. There the field *is* the
/// complete surname and the particle only chooses where to cut it, so accepting
/// a particle that is absent from the field would inject a word the user never
/// typed — and, worse, would not be undoable by clearing the particle again,
/// since by then the word has become part of the surname.
///
/// Matching is case-insensitive but respects word boundaries, so "d" does not
/// match the "D" of "DUPONT".
///
/// ```
/// use oxidgene_core::types::split_surname_at_head;
///
/// assert_eq!(
///     split_surname_at_head("de la Cruz", "de"),
///     Some((Some("de".into()), "la Cruz".into()))
/// );
/// assert_eq!(split_surname_at_head("Cruz", ""), Some((None, "Cruz".into())));
/// // "de" is not part of "DUPONT", so there is nothing to cut.
/// assert_eq!(split_surname_at_head("DUPONT", "de"), None);
/// ```
#[must_use]
pub fn split_surname_at_head(raw: &str, particle: &str) -> Option<(Option<String>, String)> {
    let raw = raw.trim();
    let particle = particle.trim();

    if particle.is_empty() {
        return Some((None, raw.to_string()));
    }

    // `get` rather than `split_at`: `particle.len()` may land inside a
    // multi-byte character, which would panic.
    let head = raw.get(..particle.len())?;
    if !head.eq_ignore_ascii_case(particle) {
        return None;
    }

    let rest = &raw[particle.len()..];
    // Without this, particle "d" would cut "DUPONT" into "D" + "UPONT".
    // Elided particles carry their own boundary ("d'" in "d'Aubigné").
    if !head.ends_with(APOSTROPHES) && !rest.starts_with(char::is_whitespace) {
        return None;
    }

    let root = rest.trim_start();
    if root.is_empty() {
        // The particle would swallow the whole surname.
        return None;
    }

    Some((Some(head.to_string()), root.to_string()))
}

/// Recombines a particle and a root back into a displayable surname.
///
/// Elided particles are joined without a space (`d'` + `Aubigné`), everything
/// else with one.
#[must_use]
pub fn join_surname_particle(particle: Option<&str>, root: &str) -> String {
    match particle.map(str::trim).filter(|p| !p.is_empty()) {
        None => root.to_string(),
        Some(p) if p.ends_with(APOSTROPHES) => format!("{p}{root}"),
        Some(p) => format!("{p} {root}"),
    }
}

/// Builds the key a surname should be filed under in dictionaries and lists.
///
/// With `include_particle` the whole surname sorts as written ("de la Cruz"
/// under D); without it, only the root counts ("de la Cruz" under C). The key
/// is lowercased so it can be compared directly.
#[must_use]
pub fn surname_sort_key(particle: Option<&str>, root: &str, include_particle: bool) -> String {
    if include_particle {
        join_surname_particle(particle, root).to_lowercase()
    } else {
        root.trim().to_lowercase()
    }
}

/// Is `token` a particle here? `after_head` says whether a
/// [`HEAD_PARTICLES`] token already opened the run, which is the only position
/// where a bare article counts.
fn is_particle(token: &str, after_head: bool) -> bool {
    let lowered = token.to_lowercase();
    HEAD_PARTICLES.contains(&lowered.as_str())
        || (after_head && TAIL_PARTICLES.contains(&lowered.as_str()))
}

/// Splits `l'Étang` into `("l'", "Étang")`, or returns `None` when the token
/// carries no elided particle or has nothing left after it.
///
/// `after_head` carries the same meaning as in [`is_particle`]: without a
/// preposition ahead of it, "L'Étang" keeps its article.
fn split_elided(token: &str, after_head: bool) -> Option<(&str, &str)> {
    let idx = token.find(APOSTROPHES)?;
    let (head, rest) = token.split_at(idx);
    // `rest` starts with the apostrophe, which belongs to the particle.
    let apostrophe_len = rest.chars().next()?.len_utf8();
    let (apostrophe, root) = rest.split_at(apostrophe_len);
    let lowered = head.to_lowercase();
    let is_elided_particle = HEAD_ELIDED_PARTICLES.contains(&lowered.as_str())
        || (after_head && TAIL_ELIDED_PARTICLES.contains(&lowered.as_str()));
    if root.is_empty() || !is_elided_particle {
        return None;
    }
    // Return the particle as one slice of the original token so casing and the
    // exact apostrophe character survive.
    Some((&token[..head.len() + apostrophe.len()], root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_multi_token_particles() {
        assert_eq!(
            split_surname_particle("de la Cruz"),
            (Some("de la".into()), "Cruz".into())
        );
        assert_eq!(
            split_surname_particle("van der Berg"),
            (Some("van der".into()), "Berg".into())
        );
        assert_eq!(
            split_surname_particle("von dem Busche"),
            (Some("von dem".into()), "Busche".into())
        );
    }

    #[test]
    fn splits_elided_particles() {
        assert_eq!(
            split_surname_particle("d'Aubigné"),
            (Some("d'".into()), "Aubigné".into())
        );
        assert_eq!(
            split_surname_particle("de l'Étang"),
            (Some("de l'".into()), "Étang".into())
        );
        // Typographic apostrophe, as produced by word processors and some
        // GEDCOM exporters.
        assert_eq!(
            split_surname_particle("d\u{2019}Artagnan"),
            (Some("d\u{2019}".into()), "Artagnan".into())
        );
    }

    #[test]
    fn a_leading_article_is_part_of_the_surname() {
        // Breton and Norman names are the bulk of these: the article is welded
        // to the name, so it files under L and must not lose a "Le" particle
        // on every person carrying it. Only the leading token drives the
        // split, so a placeholder root stands in for the real surnames.
        assert_eq!(
            split_surname_particle("Le Branch"),
            (None, "Le Branch".into())
        );
        assert_eq!(
            split_surname_particle("LE BRANCH"),
            (None, "LE BRANCH".into())
        );
        assert_eq!(
            split_surname_particle("La Branch"),
            (None, "La Branch".into())
        );
        assert_eq!(
            split_surname_particle("Les Branch"),
            (None, "Les Branch".into())
        );
        // Italian and Dutch articles behave the same way.
        assert_eq!(
            split_surname_particle("Lo Branch"),
            (None, "Lo Branch".into())
        );
        assert_eq!(
            split_surname_particle("Den Branch"),
            (None, "Den Branch".into())
        );
        // Elided too: "L'Étang" files under L, unlike "d'Aubigné" under A.
        assert_eq!(split_surname_particle("L'Étang"), (None, "L'Étang".into()));
    }

    #[test]
    fn an_article_still_counts_after_a_preposition() {
        // The same words that stay welded at the head do belong to the
        // particle once a preposition has introduced them.
        assert_eq!(
            split_surname_particle("de Le Branch"),
            (Some("de Le".into()), "Branch".into())
        );
        assert_eq!(
            split_surname_particle("van den Branch"),
            (Some("van den".into()), "Branch".into())
        );
        assert_eq!(
            split_surname_particle("de l'Étang"),
            (Some("de l'".into()), "Étang".into())
        );
    }

    #[test]
    fn a_joining_conjunction_is_never_a_leading_particle() {
        assert_eq!(
            split_surname_particle("Y Branch"),
            (None, "Y Branch".into())
        );
    }

    #[test]
    fn leaves_plain_surnames_alone() {
        assert_eq!(split_surname_particle("Dupont"), (None, "Dupont".into()));
        assert_eq!(
            split_surname_particle("MacDonald"),
            (None, "MacDonald".into())
        );
        assert_eq!(split_surname_particle("O'Brien"), (None, "O'Brien".into()));
        assert_eq!(
            split_surname_particle("Martin Dupont"),
            (None, "Martin Dupont".into())
        );
    }

    #[test]
    fn matching_is_case_insensitive_but_casing_is_preserved() {
        assert_eq!(
            split_surname_particle("DE LA CRUZ"),
            (Some("DE LA".into()), "CRUZ".into())
        );
    }

    #[test]
    fn never_consumes_the_whole_surname() {
        // Someone actually surnamed "Le" or "Da" keeps their name intact.
        assert_eq!(split_surname_particle("Le"), (None, "Le".into()));
        assert_eq!(
            split_surname_particle("de la"),
            (Some("de".into()), "la".into())
        );
    }

    #[test]
    fn handles_blank_input() {
        assert_eq!(split_surname_particle("   "), (None, String::new()));
    }

    #[test]
    fn join_is_the_inverse_of_split() {
        for raw in [
            "de la Cruz",
            "d'Aubigné",
            "Dupont",
            "van der Berg",
            "de l'Étang",
            "Le Branch",
            "L'Étang",
        ] {
            let (particle, root) = split_surname_particle(raw);
            assert_eq!(join_surname_particle(particle.as_deref(), &root), raw);
        }
    }

    #[test]
    fn explicit_particle_overrides_detection() {
        // The user disagrees with the guess and narrows it.
        assert_eq!(
            split_surname_with("de la Cruz", "de"),
            (Some("de".into()), "la Cruz".into())
        );
        // The user says there is no particle at all.
        assert_eq!(
            split_surname_with("Da Silva", ""),
            (None, "Da Silva".into())
        );
        assert_eq!(split_surname_with("Le", ""), (None, "Le".into()));
        // Casing follows the surname as typed, not the supplied particle.
        assert_eq!(
            split_surname_with("VON BERG", "von"),
            (Some("VON".into()), "BERG".into())
        );
    }

    #[test]
    fn explicit_particle_is_not_duplicated_or_dropped() {
        // Already stripped: the root does not start with the particle.
        assert_eq!(
            split_surname_with("Cruz", "de la"),
            (Some("de la".into()), "Cruz".into())
        );
        // A particle that would swallow the whole name leaves it intact.
        assert_eq!(
            split_surname_with("de la", "de la"),
            (Some("de la".into()), "de la".into())
        );
    }

    #[test]
    fn head_split_refuses_a_particle_that_is_not_there() {
        // The bug this guards: typing "de" against a plain "DUPONT" used to
        // return (Some("de"), "DUPONT"), injecting a word the field never
        // contained — and clearing the particle afterwards could not remove it,
        // because by then "de" had become part of the surname.
        assert_eq!(split_surname_at_head("DUPONT", "de"), None);
        assert_eq!(split_surname_at_head("Cruz", "de la"), None);
    }

    #[test]
    fn head_split_respects_word_boundaries() {
        // "d" must not cut "DUPONT" into "D" + "UPONT".
        assert_eq!(split_surname_at_head("DUPONT", "d"), None);
        // ...but an elided particle carries its own boundary.
        assert_eq!(
            split_surname_at_head("d'Aubigné", "d'"),
            Some((Some("d'".into()), "Aubigné".into()))
        );
    }

    #[test]
    fn head_split_is_reversible() {
        // Cutting then clearing must return the original string untouched.
        let raw = "de la Cruz";
        let (particle, root) = split_surname_at_head(raw, "de").unwrap();
        assert_eq!(join_surname_particle(particle.as_deref(), &root), raw);
        let (particle, root) = split_surname_at_head(raw, "").unwrap();
        assert_eq!(particle, None);
        assert_eq!(join_surname_particle(particle.as_deref(), &root), raw);
    }

    #[test]
    fn head_split_never_swallows_the_whole_surname() {
        assert_eq!(split_surname_at_head("de la", "de la"), None);
    }

    #[test]
    fn head_split_handles_multibyte_boundaries() {
        // `particle.len()` is a byte count and could land mid-character.
        assert_eq!(split_surname_at_head("Étang", "de"), None);
    }

    #[test]
    fn sort_key_honours_the_particle_setting() {
        let (particle, root) = split_surname_particle("de la Cruz");
        assert_eq!(
            surname_sort_key(particle.as_deref(), &root, true),
            "de la cruz"
        );
        assert_eq!(surname_sort_key(particle.as_deref(), &root, false), "cruz");
    }
}
