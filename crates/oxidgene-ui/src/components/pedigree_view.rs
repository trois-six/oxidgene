//! Which drawing the tree view uses.
//!
//! The tree view can draw the same pedigree in several ways. This is the name
//! the viewer's preference keeps, like [`PedigreeThemeId`] for the card theme:
//! `localStorage` holds `"wheel"`, and a name no longer shipped falls back to
//! the default rather than failing.
//!
//! [`PedigreeThemeId`]: crate::components::pedigree_theme::PedigreeThemeId

use serde::{Deserialize, Serialize};

/// A kind of relative a chart's action picker offers to go to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relatives {
    Parents,
    Spouses,
    Children,
}

impl Relatives {
    /// Translation key for the heading of the list.
    #[must_use]
    pub const fn heading_key(self) -> &'static str {
        match self {
            Self::Parents => "pedigree.parents",
            Self::Spouses => "pedigree.spouses",
            Self::Children => "pedigree.children",
        }
    }
}

/// One way of drawing the tree view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PedigreeView {
    /// Ancestors above and descendants below the root, one row per
    /// generation: what the tree view has always drawn.
    #[default]
    Tree,
    /// The root at the centre of a full circle, each generation of ancestors
    /// one ring further out.
    Wheel,
    /// The wheel's principle on a half circle, the root at its base.
    Fan,
    /// The root at the centre of a full circle, each generation of
    /// descendants one ring further out, their unions between.
    DescendantWheel,
    /// The descendant wheel on a half circle opening downwards, the root at
    /// its top edge.
    DescendantFan,
    /// Gramps' *Pedigree* view: the root on the left, one column of
    /// ancestors per generation to its right, joined by elbow lines.
    Lineage,
    /// Gramps' *Descendant Tree*: the root on the left, one column of
    /// descendants per generation to its right, spouses under each person.
    DescendantLineage,
    /// webtrees' horizontal hourglass: the descendants on the left, the
    /// root in the middle, the ancestors on the right.
    Hourglass,
    /// The root in the middle, the father's ancestors on the left and the
    /// mother's on the right.
    Bowtie,
}

impl PedigreeView {
    /// Every view, in the order the selector offers them.
    pub const ALL: [Self; 9] = [
        Self::Tree,
        Self::Wheel,
        Self::Fan,
        Self::DescendantWheel,
        Self::DescendantFan,
        Self::Lineage,
        Self::DescendantLineage,
        Self::Hourglass,
        Self::Bowtie,
    ];

    /// Whether the view draws descendants, and so whether the descendant
    /// depth control means anything while it is shown.
    #[must_use]
    pub const fn shows_descendants(self) -> bool {
        matches!(
            self,
            Self::Tree
                | Self::DescendantWheel
                | Self::DescendantFan
                | Self::DescendantLineage
                | Self::Hourglass
        )
    }

    /// Whether the view draws its people's portraits: the card views do, the
    /// circular ones write names in their segments and draw none, so the page
    /// fetches no picture for them.
    #[must_use]
    pub const fn draws_portraits(self) -> bool {
        !matches!(
            self,
            Self::Wheel | Self::Fan | Self::DescendantWheel | Self::DescendantFan
        )
    }

    /// Whether the view draws ancestors, and so whether the ancestor depth
    /// control means anything while it is shown.
    #[must_use]
    pub const fn shows_ancestors(self) -> bool {
        !matches!(
            self,
            Self::DescendantWheel | Self::DescendantFan | Self::DescendantLineage
        )
    }

    /// The relatives the view does not draw around a person, which its
    /// action picker lists to go to: spouses and children in an ancestor
    /// chart, parents and spouses in a descendant one. The tree draws them
    /// all.
    #[must_use]
    pub const fn relatives_to_reach(self) -> &'static [Relatives] {
        match self {
            Self::Tree | Self::Hourglass => &[],
            Self::Wheel | Self::Fan | Self::Lineage | Self::Bowtie => {
                &[Relatives::Spouses, Relatives::Children]
            }
            Self::DescendantWheel | Self::DescendantFan | Self::DescendantLineage => {
                &[Relatives::Parents, Relatives::Spouses]
            }
        }
    }

    /// Translation key for the view's name.
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Tree => "app_settings.pedigree_view_tree",
            Self::Wheel => "app_settings.pedigree_view_wheel",
            Self::Fan => "app_settings.pedigree_view_fan",
            Self::DescendantWheel => "app_settings.pedigree_view_descendant_wheel",
            Self::DescendantFan => "app_settings.pedigree_view_descendant_fan",
            Self::Lineage => "app_settings.pedigree_view_lineage",
            Self::DescendantLineage => "app_settings.pedigree_view_descendant_lineage",
            Self::Hourglass => "app_settings.pedigree_view_hourglass",
            Self::Bowtie => "app_settings.pedigree_view_bowtie",
        }
    }

    /// Translation key for the one line describing it in the selector.
    #[must_use]
    pub const fn hint_key(self) -> &'static str {
        match self {
            Self::Tree => "app_settings.pedigree_view_tree_hint",
            Self::Wheel => "app_settings.pedigree_view_wheel_hint",
            Self::Fan => "app_settings.pedigree_view_fan_hint",
            Self::DescendantWheel => "app_settings.pedigree_view_descendant_wheel_hint",
            Self::DescendantFan => "app_settings.pedigree_view_descendant_fan_hint",
            Self::Lineage => "app_settings.pedigree_view_lineage_hint",
            Self::DescendantLineage => "app_settings.pedigree_view_descendant_lineage_hint",
            Self::Hourglass => "app_settings.pedigree_view_hourglass_hint",
            Self::Bowtie => "app_settings.pedigree_view_bowtie_hint",
        }
    }

    /// The view a stored preference names, or `None` for anything this build
    /// does not ship — the caller then keeps the default.
    #[must_use]
    pub fn from_stored(stored: &str) -> Option<Self> {
        serde_json::from_str(stored).ok()
    }

    /// What [`Self::from_stored`] reads back.
    #[must_use]
    pub fn to_stored(self) -> String {
        serde_json::to_string(&self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_the_tree_the_view_always_drew() {
        assert_eq!(PedigreeView::default(), PedigreeView::Tree);
    }

    #[test]
    fn a_stored_view_reads_back_as_itself() {
        for view in PedigreeView::ALL {
            assert_eq!(PedigreeView::from_stored(&view.to_stored()), Some(view));
        }
        // The name in storage is the view's own, not its position.
        assert_eq!(PedigreeView::Wheel.to_stored(), "\"wheel\"");
        assert_eq!(PedigreeView::Fan.to_stored(), "\"fan\"");
        assert_eq!(PedigreeView::Lineage.to_stored(), "\"lineage\"");
        assert_eq!(
            PedigreeView::DescendantWheel.to_stored(),
            "\"descendant-wheel\""
        );
        assert_eq!(
            PedigreeView::DescendantFan.to_stored(),
            "\"descendant-fan\""
        );
        assert_eq!(
            PedigreeView::DescendantLineage.to_stored(),
            "\"descendant-lineage\""
        );
        assert_eq!(PedigreeView::Hourglass.to_stored(), "\"hourglass\"");
        assert_eq!(PedigreeView::Bowtie.to_stored(), "\"bowtie\"");
    }

    #[test]
    fn an_unknown_or_corrupt_entry_keeps_the_default() {
        assert_eq!(PedigreeView::from_stored("\"spiral\""), None);
        assert_eq!(PedigreeView::from_stored("wheel"), None);
        assert_eq!(PedigreeView::from_stored(""), None);
    }

    #[test]
    fn each_chart_offers_the_relatives_it_does_not_draw() {
        use PedigreeView::*;
        assert!(Tree.relatives_to_reach().is_empty());
        assert!(Hourglass.relatives_to_reach().is_empty());
        for view in [Wheel, Fan, Lineage, Bowtie] {
            assert_eq!(
                view.relatives_to_reach(),
                &[Relatives::Spouses, Relatives::Children]
            );
        }
        for view in [DescendantWheel, DescendantFan, DescendantLineage] {
            assert_eq!(
                view.relatives_to_reach(),
                &[Relatives::Parents, Relatives::Spouses]
            );
        }
    }

    #[test]
    fn each_view_offers_the_depths_it_draws() {
        use PedigreeView::*;
        for (view, ancestors, descendants) in [
            (Tree, true, true),
            (Wheel, true, false),
            (Fan, true, false),
            (Lineage, true, false),
            (DescendantWheel, false, true),
            (DescendantFan, false, true),
            (DescendantLineage, false, true),
            (Hourglass, true, true),
            (Bowtie, true, false),
        ] {
            assert_eq!(view.shows_ancestors(), ancestors, "{view:?}");
            assert_eq!(view.shows_descendants(), descendants, "{view:?}");
        }
    }

    #[test]
    fn every_view_is_named_in_every_language() {
        use crate::i18n::{I18n, Language};

        for view in PedigreeView::ALL {
            for language in Language::ALL {
                let i18n = I18n(language);
                for key in [view.label_key(), view.hint_key()] {
                    let text = i18n.t(key);
                    assert_ne!(text, key, "{view:?}: {key} is untranslated in {language:?}");
                    assert!(!text.is_empty(), "{view:?}: {key} is empty in {language:?}");
                }
            }
        }
    }
}
