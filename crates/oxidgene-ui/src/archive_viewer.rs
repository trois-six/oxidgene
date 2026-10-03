use std::sync::Arc;

use dioxus::prelude::try_use_context;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveViewerRequest {
    pub title: String,
    pub commune: String,
    pub act_code: char,
    pub year: u16,
    pub page_index: Option<u16>,
    pub page_count: Option<u16>,
}

impl ArchiveViewerRequest {
    pub fn from_source_title(title: &str) -> Option<Self> {
        let fields: Vec<_> = title.split(" - ").collect();
        if fields.first().copied()? != "AD44" {
            return None;
        }

        let (act_index, act_code, year) =
            fields
                .iter()
                .enumerate()
                .skip(2)
                .find_map(|(index, field)| {
                    let code = match *field {
                        "N" => 'N',
                        "B" => 'B',
                        _ => return None,
                    };
                    Some((index, code, fields.get(index + 1)?.parse::<u16>().ok()?))
                })?;
        let commune = fields.get(1..act_index.checked_sub(1)?)?.join(" - ");
        if commune.trim().is_empty() {
            return None;
        }

        let (page_index, page_count) = fields
            .last()
            .and_then(|field| parse_view_location(field))
            .map_or((None, None), |(index, count)| (Some(index), Some(count)));

        Some(Self {
            title: title.to_string(),
            commune,
            act_code,
            year,
            page_index,
            page_count,
        })
    }
}

fn parse_view_location(field: &str) -> Option<(u16, u16)> {
    let view = field.strip_prefix("vue ")?;
    let digit_count = view.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count == 0 {
        return None;
    }

    let index = view[..digit_count].parse::<u16>().ok()?;
    let (side, total) = view[digit_count..].split_once('/')?;
    let total = total.parse::<u16>().ok()?;
    if index == 0 || !matches!(side, "" | "d" | "g") || total < index {
        return None;
    }
    Some((index, total))
}

pub trait ArchiveViewerOpener: Send + Sync {
    fn open(&self, request: ArchiveViewerRequest);
}

#[derive(Clone)]
pub struct ArchiveViewerBridge(Arc<dyn ArchiveViewerOpener>);

impl ArchiveViewerBridge {
    pub fn new(opener: Arc<dyn ArchiveViewerOpener>) -> Self {
        Self(opener)
    }

    pub fn open(&self, request: ArchiveViewerRequest) {
        self.0.open(request);
    }
}

impl std::fmt::Debug for ArchiveViewerBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ArchiveViewerBridge")
    }
}

pub fn use_archive_viewer_bridge() -> Option<ArchiveViewerBridge> {
    try_use_context::<ArchiveViewerBridge>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_birth_source_with_a_right_hand_view() {
        let source = ArchiveViewerRequest::from_source_title(
            "AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2 - acte 26 - vue 5d/13",
        )
        .expect("the AD44 reference is supported");

        assert_eq!(source.commune, "Exampleville");
        assert_eq!(source.act_code, 'N');
        assert_eq!(source.year, 1877);
        assert_eq!(source.page_index, Some(5));
        assert_eq!(source.page_count, Some(13));
    }

    #[test]
    fn joins_commune_parts_before_the_act_code() {
        let source = ArchiveViewerRequest::from_source_title(
            "AD44 - Example - Part - (aucun) - B - 1791 - 3E1/2 - acte 4 - vue 3g/12",
        )
        .expect("the AD44 reference is supported");

        assert_eq!(source.commune, "Example - Part");
        assert_eq!(source.act_code, 'B');
        assert_eq!(source.page_index, Some(3));
        assert_eq!(source.page_count, Some(12));
    }

    #[test]
    fn rejects_other_archives_and_malformed_views() {
        assert!(
            ArchiveViewerRequest::from_source_title(
                "AD67 - Exampleville - (aucun) - N - 1877 - 3E1/2 - acte 26 - vue 5d/13"
            )
            .is_none()
        );
        assert!(
            ArchiveViewerRequest::from_source_title(
                "AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2 - acte 26 - vue 14d/13"
            )
            .is_some_and(|source| source.page_index.is_none() && source.page_count.is_none())
        );
    }
}
