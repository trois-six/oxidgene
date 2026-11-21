//! Delivery status: what the specifications say is planned, built or
//! complete agrees from one document to the next, and the crate map names
//! the crates that exist.
//!
//! Drift it prevents: a specification still described as planned in its
//! frontmatter — and so in the index, which copies it — once its body says
//! it is built; the roadmap's status table calling planned an EPIC whose
//! section has delivered items, or complete one whose specification is
//! still planned; a roadmap section titled *Planned* that lists delivered
//! work; and a workspace layout or dependency graph that kept a removed
//! crate or forgot a new one.
//!
//! Fixing a failure: status is written in three places that must move
//! together — the `**Status:` line of the specification and the first word
//! of its frontmatter `description` (with the same description in
//! docs/index.md), the roadmap's §2 table, and the title of the roadmap's
//! section. Change all of them in the commit that delivers the work. A
//! crate added or removed is added to or removed from both blocks of
//! docs/architecture.md §9.

use std::collections::BTreeSet;

use oxidgene_guards::{read, workspace_members};

use super::specs::{docs, frontmatter};

/// The description of each specification, by file name.
fn descriptions() -> Vec<(String, String)> {
    docs()
        .iter()
        .filter_map(|doc| {
            let description = frontmatter(&doc.text)?.get("description")?.clone();
            Some((doc.name.clone(), description.trim_matches('"').to_string()))
        })
        .collect()
}

fn is_planned(description: &str) -> bool {
    description.starts_with("Planned")
}

#[test]
fn a_specification_is_described_as_planned_exactly_while_its_status_says_so() {
    let mut problems = Vec::new();
    for doc in docs() {
        let Some(description) = frontmatter(&doc.text).and_then(|f| f.get("description").cloned())
        else {
            continue;
        };
        let status = doc
            .text
            .lines()
            .find_map(|line| line.strip_prefix("**Status:"))
            .map(|rest| rest.trim().to_lowercase());
        let described = is_planned(description.trim_matches('"'));
        let stated = status.as_deref().is_some_and(|s| s.starts_with("planned"));
        match (described, stated) {
            (true, false) => problems.push(format!(
                "{}: its description starts with \"Planned\" but its body has no `**Status: planned` line",
                doc.name
            )),
            (false, true) => problems.push(format!(
                "{}: its `**Status:` line says planned but its description does not start with \"Planned\"",
                doc.name
            )),
            _ => {}
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The roadmap's sections: each `## ` title with its text.
fn roadmap_sections(roadmap: &str) -> Vec<(&str, &str)> {
    let mut sections = Vec::new();
    let mut rest = roadmap;
    while let Some(at) = rest.find("\n## ") {
        let section = &rest[at + 1..];
        let end = section[1..].find("\n## ").map_or(section.len(), |e| e + 1);
        let title = section.lines().next().unwrap_or_default();
        sections.push((title.trim_start_matches("## "), &section[..end]));
        rest = &section[end..];
    }
    sections
}

/// The section a status cell points at, as in "Planned (§8)".
fn cited_section(status: &str) -> Option<&str> {
    let after = status.split("(§").nth(1)?;
    after.split(')').next()
}

/// The EPIC rows of the roadmap's status table: letter, status, and the
/// specifications it links.
fn epics(roadmap: &str) -> Vec<(String, String, Vec<String>)> {
    roadmap
        .lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line.strip_prefix("| ")?.split(" | ").collect();
            let letter = cells.first()?.trim();
            (cells.len() >= 4
                && letter.len() == 1
                && letter.chars().all(|c| c.is_ascii_uppercase()))
            .then(|| {
                let specifications = cells[3]
                    .split("](")
                    .skip(1)
                    .filter_map(|link| link.split([')', '#']).next())
                    .map(str::to_string)
                    .collect();
                (
                    letter.to_string(),
                    cells[2].trim().to_string(),
                    specifications,
                )
            })
        })
        .collect()
}

#[test]
fn the_roadmap_calls_planned_only_what_has_delivered_nothing() {
    let roadmap = read("docs/roadmap.md");
    let sections = roadmap_sections(&roadmap);
    let delivered = |number: &str| {
        sections
            .iter()
            .find(|(title, _)| title.starts_with(&format!("{number}. ")))
            .is_some_and(|(_, text)| text.contains("- [x]"))
    };
    let descriptions = descriptions();
    let epics = epics(&roadmap);
    assert!(epics.len() >= 8, "the roadmap's status table was not read");
    let mut problems = Vec::new();
    for (title, text) in &sections {
        if title.contains("Planned") && text.contains("- [x]") {
            problems.push(format!(
                "roadmap.md: \"{title}\" lists delivered items under a Planned title"
            ));
        }
    }
    for (letter, status, specifications) in &epics {
        if status.starts_with("Planned")
            && let Some(number) = cited_section(status)
            && delivered(number)
        {
            problems.push(format!(
                "roadmap.md: EPIC {letter} is \"{status}\" while §{number} lists delivered items"
            ));
        }
        if status.starts_with("Complete") {
            for file in specifications {
                if descriptions
                    .iter()
                    .any(|(name, description)| name == file && is_planned(description))
                {
                    problems.push(format!(
                        "roadmap.md: EPIC {letter} is complete, yet {file} is described as planned"
                    ));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The `oxidgene-…` crate names a text mentions.
fn crates_named(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !(c.is_ascii_lowercase() || c == '-'))
        .filter(|word| word.starts_with("oxidgene-") && word.len() > "oxidgene-".len())
        .map(|word| word.trim_end_matches('-').to_string())
        .collect()
}

/// The fenced block that follows `heading` in `text`.
fn block_after<'a>(text: &'a str, heading: &str) -> &'a str {
    let section = text
        .split_once(heading)
        .unwrap_or_else(|| panic!("architecture.md lost \"{heading}\""))
        .1;
    section.split("```").nth(1).unwrap_or_default()
}

#[test]
fn the_crate_map_names_exactly_the_workspace_members() {
    let members: BTreeSet<String> = workspace_members()
        .iter()
        .filter_map(|member| member.rsplit('/').next())
        .map(str::to_string)
        .collect();
    let architecture = read("docs/architecture.md");
    let mut problems = Vec::new();
    for heading in [
        "### 9.1 Cargo Workspace Layout",
        "### 9.2 Crate Dependency Graph",
    ] {
        let named = crates_named(block_after(&architecture, heading));
        for missing in members.difference(&named) {
            problems.push(format!("architecture.md, {heading}: {missing} is missing"));
        }
        for unknown in named.difference(&members) {
            problems.push(format!(
                "architecture.md, {heading}: {unknown} is no workspace member"
            ));
        }
    }
    // The root manifest's comments name crates too.
    for unknown in crates_named(&read("Cargo.toml")).difference(&members) {
        problems.push(format!(
            "Cargo.toml names {unknown}, which is no workspace member"
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_guard_reads_the_roadmap() {
    let roadmap = "\
# Roadmap

## 2. Current status

| EPIC | Scope | Status | Canonical specifications |
|---|---|---|---|
| A | Foundation | Complete | [Architecture](architecture.md), [Data](data-model.md#5-history) |
| G | Security | Planned (§8) | [Privacy](privacy.md) |

## 8. Planned: security

- [x] Sign-in.
- [ ] Messaging.
";
    let sections = roadmap_sections(roadmap);
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[1].0, "8. Planned: security");
    assert!(sections[1].1.contains("- [x] Sign-in."));
    let epics = epics(roadmap);
    assert_eq!(epics.len(), 2);
    assert_eq!(epics[0].2, ["architecture.md", "data-model.md"]);
    assert_eq!(cited_section(&epics[1].1), Some("8"));
    assert_eq!(
        crates_named("core <- oxidgene-db, `oxidgene-api`; oxidgene-web/"),
        BTreeSet::from([
            "oxidgene-api".to_string(),
            "oxidgene-db".to_string(),
            "oxidgene-web".to_string()
        ])
    );
}
