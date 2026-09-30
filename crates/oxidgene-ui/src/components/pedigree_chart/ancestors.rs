//! The root's ancestors numbered SOSA-style: what the ancestor-only views
//! draw.
//!
//! The root is 1, the father of `n` is `2n` and the mother `2n + 1`, so a
//! number alone says both the generation (`⌊log₂ n⌋`) and the position within
//! it (`n − 2^generation`), father's side first. The wheel and the fan turn
//! that position into an angle; the lineage view into a row.

use super::*;

/// One position of an ancestor chart: a person, or the empty slot of a
/// parent the tree does not record.
#[derive(Clone, Debug)]
pub(super) struct AncestorEntry {
    pub(super) sosa: u64,
    /// Drawn with the pedigree's own card data, so every view speaks of a
    /// person — name, lifespan and marks — exactly as the tree view does.
    /// Its position is left at the origin for each view to set.
    pub(super) node: LayoutNode,
}

/// The generation of a SOSA number: 0 for the root, 1 for its parents.
pub(super) fn generation_of(sosa: u64) -> u32 {
    sosa.max(1).ilog2()
}

/// The position of a SOSA number within its generation, from 0 (the
/// father's father's … line) to `2^generation − 1`.
pub(super) fn index_in_generation(sosa: u64) -> u64 {
    sosa.max(1) - (1u64 << generation_of(sosa))
}

/// The root and its ancestors up to `generations` generations back, sorted
/// by SOSA number.
///
/// A person short of the last generation whose father or mother the tree
/// does not record gets an empty slot in that parent's place — one slot per
/// missing parent, as on the tree view, so the chart offers to add them.
/// Nothing is drawn above an empty slot.
pub(super) fn collect_ancestors(
    root_id: Uuid,
    data: &PedigreeData,
    generations: usize,
    sosa_root_id: Option<Uuid>,
    sosa_ancestors: &HashSet<Uuid>,
) -> Vec<AncestorEntry> {
    let mut entries = Vec::new();
    let mut pending = vec![(1u64, root_id)];
    while let Some((sosa, pid)) = pending.pop() {
        let node =
            PersonNode::from_data(pid, data, sosa_root_id, sosa_ancestors).card_at(pid, 0.0, 0.0);
        entries.push(AncestorEntry { sosa, node });
        if (generation_of(sosa) as usize) < generations {
            let (father, mother) = data.parents_of(pid);
            for (parent, parent_sosa, is_father) in
                [(father, 2 * sosa, true), (mother, 2 * sosa + 1, false)]
            {
                match parent {
                    Some(parent) => pending.push((parent_sosa, parent)),
                    None => entries.push(empty_slot(parent_sosa, pid, is_father)),
                }
            }
        }
    }
    entries.sort_by_key(|entry| entry.sosa);
    entries
}

/// The empty slot standing for the missing father or mother of `child`.
fn empty_slot(sosa: u64, child: Uuid, is_father: bool) -> AncestorEntry {
    AncestorEntry {
        sosa,
        node: LayoutNode {
            id: None,
            x: 0.0,
            y: 0.0,
            sex: if is_father { Sex::Male } else { Sex::Female },
            label_surname: String::new(),
            label_given: String::new(),
            birth_year: None,
            death_year: None,
            photo_url: None,
            sosa_badge: SosaBadge::None,
            is_self: false,
            is_compact: false,
            child_of: Some(child),
            is_father,
            is_sibling: false,
            has_more_relations: false,
        },
    }
}

/// What a hover over an ancestor says: the name, the lifespan with its
/// qualifiers spelled out, and the SOSA number when the chart's root is the
/// tree's SOSA 1 — the only case where a position in the chart *is* the
/// person's SOSA number.
pub(super) fn ancestor_tooltip(
    entry: &AncestorEntry,
    root_is_sosa_root: bool,
    i18n: &I18n,
) -> String {
    let card = card_tooltip(&entry.node, i18n);
    let mut lines = vec![card.name];
    if !card.lifespan.is_empty() {
        lines.push(card.lifespan);
    }
    if root_is_sosa_root {
        lines.push(i18n.t_args("search.sosa_badge", &[("number", &entry.sosa.to_string())]));
    }
    lines.join("\n")
}

/// The colour that marks an ancestor where a card would carry its badge: the
/// user's own mark before the SOSA one, as on a card.
pub(super) fn mark_colour(node: &LayoutNode) -> Option<&'static str> {
    if node.is_self {
        Some("var(--pn-self)")
    } else {
        match node.sosa_badge {
            SosaBadge::Root => Some("var(--pn-sosa-root)"),
            SosaBadge::Direct => Some("var(--pn-sosa)"),
            SosaBadge::None => None,
        }
    }
}

/// The markup of a native SVG tooltip, for `dangerous_inner_html` — rsx
/// cannot make an SVG `<title>` (see the card's lifespan).
pub(super) fn svg_title(text: &str) -> String {
    if text.is_empty() {
        String::new()
    } else {
        format!("<title>{}</title>", escape_xml(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sosa_number_names_its_generation_and_position() {
        assert_eq!((generation_of(1), index_in_generation(1)), (0, 0));
        assert_eq!((generation_of(2), index_in_generation(2)), (1, 0));
        assert_eq!((generation_of(3), index_in_generation(3)), (1, 1));
        assert_eq!((generation_of(4), index_in_generation(4)), (2, 0));
        assert_eq!((generation_of(7), index_in_generation(7)), (2, 3));
        assert_eq!((generation_of(1023), index_in_generation(1023)), (9, 511));
        assert_eq!((generation_of(1024), index_in_generation(1024)), (10, 0));
    }
}
