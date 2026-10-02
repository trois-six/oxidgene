//! The one map from an event type to its icon (`docs/ui-common.md` §4.6).

use dioxus::prelude::*;
use oxidgene_core::enums::EventType;

use crate::i18n::use_i18n;
use crate::utils::event_type_label_key;

/// The semantic family an event's icon is coloured by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Birth,
    Death,
    Marry,
    Other,
}

impl Tone {
    fn class(self) -> &'static str {
        match self {
            Tone::Birth => "ev-ic ev-ic-birth",
            Tone::Death => "ev-ic ev-ic-death",
            Tone::Marry => "ev-ic ev-ic-marry",
            Tone::Other => "ev-ic ev-ic-other",
        }
    }
}

/// An event type's glyph and tone.
fn icon(event_type: EventType) -> (&'static str, Tone) {
    match event_type {
        EventType::Birth => ("\u{2726}", Tone::Birth),
        EventType::Baptism => ("\u{271F}", Tone::Birth),
        EventType::Death | EventType::Cremation => ("\u{271D}", Tone::Death),
        EventType::Burial => ("\u{26B0}", Tone::Death),
        EventType::Marriage
        | EventType::Engagement
        | EventType::MarriageBann
        | EventType::MarriageContract
        | EventType::MarriageLicense
        | EventType::MarriageSettlement
        | EventType::CivilUnion => ("\u{1F48D}", Tone::Marry),
        EventType::MarriagesCount => ("\u{1F48D}", Tone::Other),
        EventType::Divorce
        | EventType::Annulment
        | EventType::Separation
        | EventType::DivorceFiled => ("\u{2696}", Tone::Other),
        EventType::Census | EventType::Will | EventType::Probate => ("\u{1F4DC}", Tone::Other),
        EventType::Occupation => ("\u{2692}", Tone::Other),
        EventType::Residence => ("\u{1F3E1}", Tone::Other),
        EventType::Adoption => ("\u{1FAC2}", Tone::Other),
        EventType::Education => ("\u{1F393}", Tone::Other),
        EventType::Religion => ("\u{271F}", Tone::Other),
        _ => ("\u{25C6}", Tone::Other),
    }
}

/// An event type's icon, named for assistive technology by the type's
/// localized name.
///
/// Framed in its tone's badge where a list sets it beside the event's name,
/// which it then only decorates; `bare`, the glyph alone, where it marks a
/// date by itself (`✦ 1842`) and is the event's only name on screen.
#[component]
pub fn EventIcon(event_type: EventType, #[props(default)] bare: bool) -> Element {
    let i18n = use_i18n();
    let (glyph, tone) = icon(event_type);
    let label = i18n.t(event_type_label_key(event_type));
    if bare {
        return rsx! {
            span { role: "img", "aria-label": "{label}", title: "{label}", "{glyph}" }
        };
    }
    rsx! {
        span { class: tone.class(), "aria-hidden": "true", "{glyph}" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every type has an icon, and the vital events keep the glyphs the
    /// summaries have always drawn.
    #[test]
    fn the_vital_events_keep_their_glyphs() {
        assert_eq!(icon(EventType::Birth), ("\u{2726}", Tone::Birth));
        assert_eq!(icon(EventType::Death), ("\u{271D}", Tone::Death));
        assert_eq!(icon(EventType::Burial).1, Tone::Death);
        assert_eq!(icon(EventType::MarriageBann).1, Tone::Marry);
        assert_eq!(icon(EventType::Other), ("\u{25C6}", Tone::Other));
    }
}
