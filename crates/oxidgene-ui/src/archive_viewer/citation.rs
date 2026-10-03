//! Normalized archive citations read from a source title.
//!
//! A citation names its archive first, then the locality, the act and its
//! year, and may end with the cited view:
//! `AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2 - acte 26 - vue 5d/13`.
//! The archive code selects a catalogue entry; nothing here is specific to
//! one archive or one portal.

use serde::{Deserialize, Serialize};

/// The kind of act a register holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActKind {
    Birth,
    Baptism,
    Marriage,
    Death,
    Burial,
}

impl ActKind {
    /// The act letter of a normalized citation.
    fn from_letter(letter: &str) -> Option<Self> {
        Some(match letter {
            "N" => Self::Birth,
            "B" => Self::Baptism,
            "M" => Self::Marriage,
            "D" => Self::Death,
            "S" => Self::Burial,
            _ => return None,
        })
    }
}

/// A cited image of a register: its position and the register's image count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ViewLocation {
    pub index: u16,
    pub count: u16,
}

/// What a normalized citation identifies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArchiveCitation {
    /// The archive code that opens the title, such as `AD44`.
    pub archive: String,
    /// The locality, which may itself contain ` - `.
    pub locality: String,
    pub act: ActKind,
    pub year: u16,
    /// The cited view, when the title gives a consistent one.
    pub view: Option<ViewLocation>,
}

impl ArchiveCitation {
    /// Read a normalized source title, or `None` for any other title.
    pub fn parse(title: &str) -> Option<Self> {
        let fields: Vec<_> = title.split(" - ").collect();
        let archive = fields.first().copied()?;
        if archive.is_empty()
            || !archive
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            return None;
        }

        // The act letter is followed by the year; one complement field
        // separates it from the locality.
        let (act_index, act, year) =
            fields
                .iter()
                .enumerate()
                .skip(2)
                .find_map(|(index, field)| {
                    let act = ActKind::from_letter(field)?;
                    Some((index, act, fields.get(index + 1)?.parse::<u16>().ok()?))
                })?;
        let locality = fields.get(1..act_index.checked_sub(1)?)?.join(" - ");
        if locality.trim().is_empty() {
            return None;
        }

        Some(Self {
            archive: archive.to_owned(),
            locality,
            act,
            year,
            view: fields.last().and_then(|field| parse_view(field)),
        })
    }
}

/// `vue 5/13`, `vue 5d/13` or `vue 5g/13`: the right- or left-hand page of
/// a double image is the same view.
fn parse_view(field: &str) -> Option<ViewLocation> {
    let view = field.strip_prefix("vue ")?;
    let digit_count = view.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count == 0 {
        return None;
    }

    let index = view[..digit_count].parse::<u16>().ok()?;
    let (side, total) = view[digit_count..].split_once('/')?;
    let count = total.parse::<u16>().ok()?;
    if index == 0 || !matches!(side, "" | "d" | "g") || count < index {
        return None;
    }
    Some(ViewLocation { index, count })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_birth_with_a_right_hand_view() {
        let citation = ArchiveCitation::parse(
            "AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2 - acte 26 - vue 5d/13",
        )
        .expect("a normalized citation");

        assert_eq!(citation.archive, "AD44");
        assert_eq!(citation.locality, "Exampleville");
        assert_eq!(citation.act, ActKind::Birth);
        assert_eq!(citation.year, 1877);
        assert_eq!(
            citation.view,
            Some(ViewLocation {
                index: 5,
                count: 13
            })
        );
    }

    #[test]
    fn keeps_a_locality_that_contains_the_separator() {
        let citation = ArchiveCitation::parse(
            "AD44 - Example - Part - (aucun) - B - 1791 - 3E1/2 - acte 4 - vue 3g/12",
        )
        .expect("a normalized citation");

        assert_eq!(citation.locality, "Example - Part");
        assert_eq!(citation.act, ActKind::Baptism);
        assert_eq!(
            citation.view,
            Some(ViewLocation {
                index: 3,
                count: 12
            })
        );
    }

    #[test]
    fn reads_every_act_letter_and_any_archive_code() {
        for (letter, act) in [
            ("N", ActKind::Birth),
            ("B", ActKind::Baptism),
            ("M", ActKind::Marriage),
            ("D", ActKind::Death),
            ("S", ActKind::Burial),
        ] {
            let citation =
                ArchiveCitation::parse(&format!("AD85 - Exampleville - (aucun) - {letter} - 1802"))
                    .expect("a normalized citation");
            assert_eq!(citation.archive, "AD85");
            assert_eq!(citation.act, act);
            assert_eq!(citation.view, None);
        }
    }

    #[test]
    fn drops_an_inconsistent_view_but_keeps_the_register() {
        let citation = ArchiveCitation::parse(
            "AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2 - acte 26 - vue 14d/13",
        )
        .expect("a normalized citation");
        assert_eq!(citation.view, None);
    }

    #[test]
    fn rejects_titles_that_are_not_normalized() {
        for title in [
            "Parish register of Exampleville",
            "ad44 - Exampleville - (aucun) - N - 1877",
            "AD44 - (aucun) - N - 1877",
            "AD44 - Exampleville - (aucun) - X - 1877",
            "AD44 - Exampleville - (aucun) - N - circa",
        ] {
            assert_eq!(ArchiveCitation::parse(title), None, "{title}");
        }
    }
}
