//! Text normalization helpers for person search.
//!
//! Used both when writing rows to the `person_search_fts` table and when
//! normalizing incoming queries, so stored tokens and query tokens always
//! match regardless of the database backend.

/// Normalize a string for search: lowercase + accent folding.
///
/// This is a simple implementation that handles common Latin diacritics.
/// For more comprehensive accent folding, consider using the `deunicode` crate.
pub fn normalize_for_search(s: &str) -> String {
    s.to_lowercase().chars().map(fold_accent).collect()
}

/// Fold a single accented character to its ASCII equivalent.
fn fold_accent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'æ' => 'a', // simplified
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        'ð' => 'd',
        'ø' => 'o',
        'ß' => 's',
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_for_search() {
        assert_eq!(normalize_for_search("Éloïse"), "eloise");
        assert_eq!(normalize_for_search("François"), "francois");
        assert_eq!(normalize_for_search("Müller"), "muller");
        assert_eq!(normalize_for_search("Ñoño"), "nono");
        assert_eq!(normalize_for_search("DUPONT"), "dupont");
    }

    #[test]
    fn test_fold_accent() {
        assert_eq!(fold_accent('é'), 'e');
        assert_eq!(fold_accent('ç'), 'c');
        assert_eq!(fold_accent('ü'), 'u');
        assert_eq!(fold_accent('x'), 'x');
    }
}
