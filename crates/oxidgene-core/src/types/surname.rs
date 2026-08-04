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

/// Particle tokens recognised at the head of a surname, lowercased.
///
/// Deliberately excludes `mac` / `mc` / `o'`: in Gaelic surnames those are
/// bound to the root ("MacDonald", "O'Brien") rather than being separate
/// words, so treating them as particles would split names that should not be.
const PARTICLES: &[&str] = &[
    // French
    "de", "du", "des", "le", "la", "les", // Spanish / Portuguese
    "del", "dos", "das", "do", "da", "y", "e", // Italian
    "di", "dal", "dalla", "della", "dello", "dei", "degli", "delle", "lo",
    // Dutch / Flemish
    "van", "vander", "ver", "ten", "ter", "te", "op", "in", "'t", "aan", "uit",
    // German
    "von", "vom", "zu", "zur", "zum", "auf", "der", "den", "dem",
];

/// Particles that elide onto the next word with an apostrophe (`d'Aubigné`).
///
/// Matched against the part of a token *before* the apostrophe, lowercased.
const ELIDED_PARTICLES: &[&str] = &["d", "l", "dell", "all", "nell", "sull", "dall"];

/// Both apostrophe characters that show up in imported genealogy data.
const APOSTROPHES: [char; 2] = ['\'', '\u{2019}'];

/// Splits a raw surname into its particle and its root.
///
/// Returns `(particle, root)`. The root is never empty: a value made up
/// entirely of particle words (someone actually surnamed "Le") is returned
/// unsplit, since filing it under nothing would be worse than filing it under
/// its own first letter.
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
/// ```
#[must_use]
pub fn split_surname_particle(raw: &str) -> (Option<String>, String) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (None, String::new());
    }

    let tokens: Vec<&str> = trimmed.split_whitespace().collect();

    // Consume whole tokens that are particles, always leaving at least one
    // token behind to serve as the root.
    let mut taken = 0;
    while taken + 1 < tokens.len() && is_particle(tokens[taken]) {
        taken += 1;
    }

    // The first non-particle token may still carry an elided particle glued to
    // it by an apostrophe ("l'Étang"). Only split it when a root remains.
    let elided: Option<(&str, &str)> = tokens.get(taken).and_then(|token| split_elided(token));

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

fn is_particle(token: &str) -> bool {
    let lowered = token.to_lowercase();
    PARTICLES.contains(&lowered.as_str())
}

/// Splits `l'Étang` into `("l'", "Étang")`, or returns `None` when the token
/// carries no elided particle or has nothing left after it.
fn split_elided(token: &str) -> Option<(&str, &str)> {
    let idx = token.find(APOSTROPHES)?;
    let (head, rest) = token.split_at(idx);
    // `rest` starts with the apostrophe, which belongs to the particle.
    let apostrophe_len = rest.chars().next()?.len_utf8();
    let (apostrophe, root) = rest.split_at(apostrophe_len);
    if root.is_empty() || !ELIDED_PARTICLES.contains(&head.to_lowercase().as_str()) {
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
        ] {
            let (particle, root) = split_surname_particle(raw);
            assert_eq!(join_surname_particle(particle.as_deref(), &root), raw);
        }
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
