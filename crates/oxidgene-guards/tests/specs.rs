//! Specification conformance: `docs/` stays a valid OKF v0.2 bundle whose
//! links and section references resolve (docs/cross-cutting.md §9).
//!
//! Drift it prevents: a specification without its frontmatter, an index
//! that forgot a document or kept a removed one, an index description that
//! no longer matches the document's, a link to a renamed file or heading,
//! a `§N` pointing at a section that moved, and — on a pull request — a
//! body edited without its `generated.at` stamp.
//!
//! Fixing a failure: the message names the file and what is wrong. Anchors
//! follow GitHub's heading slugs (lowercase, punctuation dropped, spaces to
//! hyphens). A `§N` resolves in the document of the link it is written in
//! or right after (`[Data Model §4](data-model.md)`,
//! `[Data Model](data-model.md) §4`), otherwise in its own document. The
//! stamp check runs only when `OXIDGENE_SPEC_BASE` names the git revision
//! to compare with, as the CI Specs job does on a pull request.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use oxidgene_guards::{files, root};

struct Doc {
    name: String,
    text: String,
}

fn docs() -> Vec<Doc> {
    files("docs", "md")
        .into_iter()
        .filter(|path| path.parent() == Some(&root().join("docs")))
        .map(|path| Doc {
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            text: std::fs::read_to_string(&path).unwrap(),
        })
        .collect()
}

/// The frontmatter's `key: value` lines.
fn frontmatter(text: &str) -> Option<BTreeMap<String, String>> {
    let body = text.strip_prefix("---\n")?;
    let end = body.find("\n---\n")?;
    Some(
        body[..end]
            .lines()
            .filter_map(|line| line.split_once(':'))
            .filter(|(key, _)| !key.starts_with(' '))
            .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
            .collect(),
    )
}

fn unquote(value: &str) -> &str {
    value.trim_matches('"')
}

/// The text outside fenced code blocks, line by line, with 1-based line
/// numbers.
fn numbered_prose(text: &str) -> Vec<(usize, &str)> {
    let mut fenced = false;
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                return false;
            }
            !fenced
        })
        .map(|(index, line)| (index + 1, line))
        .collect()
}

fn prose(text: &str) -> Vec<&str> {
    numbered_prose(text)
        .into_iter()
        .map(|(_, line)| line)
        .collect()
}

fn headings(text: &str) -> Vec<&str> {
    prose(text)
        .into_iter()
        .filter_map(|line| {
            let level = line.chars().take_while(|c| *c == '#').count();
            (level > 0 && line[level..].starts_with(' ')).then(|| line[level..].trim())
        })
        .collect()
}

/// GitHub's anchor of a heading.
fn slug(heading: &str) -> String {
    heading
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

fn anchors(text: &str) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    headings(text)
        .into_iter()
        .map(|heading| {
            let base = slug(heading);
            let count = seen.entry(base.clone()).or_default();
            let anchor = if *count == 0 {
                base.clone()
            } else {
                format!("{base}-{count}")
            };
            *count += 1;
            anchor
        })
        .collect()
}

/// The section numbers of a document's headings: `2`, `2.1`, …
fn sections(text: &str) -> Vec<String> {
    headings(text)
        .into_iter()
        .filter_map(|heading| {
            let first = heading.split_whitespace().next()?;
            let number = first.trim_end_matches('.');
            number
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.')
                .then(|| number.to_string())
        })
        .collect()
}

/// Every markdown link target of a line, with the byte range of the link.
fn links(line: &str) -> Vec<(usize, usize, &str)> {
    let mut found = Vec::new();
    let mut search = 0;
    while let Some(at) = line[search..].find("](") {
        let start = search + at + 2;
        let Some(len) = line[start..].find(')') else {
            break;
        };
        let open = line[..search + at].rfind('[').unwrap_or(0);
        found.push((open, start + len + 1, &line[start..start + len]));
        search = start + len;
    }
    found
}

#[test]
fn every_specification_carries_its_frontmatter() {
    let mut problems = Vec::new();
    for doc in docs().iter().filter(|doc| doc.name != "index.md") {
        let Some(front) = frontmatter(&doc.text) else {
            problems.push(format!("{}: no frontmatter", doc.name));
            continue;
        };
        for key in ["type", "title", "description", "tags", "generated"] {
            if !front.contains_key(key) {
                problems.push(format!("{}: frontmatter lacks `{key}`", doc.name));
            }
        }
        if front.contains_key("timestamp") {
            problems.push(format!("{}: the retired `timestamp` key", doc.name));
        }
        if let Some(tags) = front.get("tags")
            && !tags
                .trim_start_matches('[')
                .trim_start()
                .starts_with("oxidgene")
        {
            problems.push(format!("{}: tags must start with oxidgene", doc.name));
        }
        if let Some(generated) = front.get("generated")
            && !is_stamp(generated)
        {
            problems.push(format!(
                "{}: generated must be {{ by: …, at: <UTC> }}",
                doc.name
            ));
        }
        let h1 = headings(&doc.text)
            .into_iter()
            .find(|_| true)
            .map(str::to_string);
        let title = front.get("title").map(|t| unquote(t).to_string());
        if h1.is_none() || h1 != title {
            problems.push(format!(
                "{}: title {title:?} is not the H1 {h1:?}",
                doc.name
            ));
        }
        if doc.text.contains("\n# Citations") {
            problems.push(format!("{}: the retired `# Citations` list", doc.name));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// `{ by: actor, at: 2026-01-02T03:04:05Z }`.
fn is_stamp(value: &str) -> bool {
    let Some(at) = value.split("at:").nth(1) else {
        return false;
    };
    let at = at.trim().trim_end_matches('}').trim();
    value.contains("by:")
        && at.len() == 20
        && at.ends_with('Z')
        && at.as_bytes()[10] == b'T'
        && at[..4].chars().all(|c| c.is_ascii_digit())
}

#[test]
fn the_index_lists_every_specification_once_as_it_describes_itself() {
    let docs = docs();
    let index = docs
        .iter()
        .find(|doc| doc.name == "index.md")
        .expect("docs/index.md");
    assert!(
        index.text.starts_with("---\nokf_version: \"0.2\"\n---\n"),
        "index.md carries only okf_version: \"0.2\" as frontmatter"
    );
    let descriptions: HashMap<&str, String> = docs
        .iter()
        .filter_map(|doc| {
            let front = frontmatter(&doc.text)?;
            Some((
                doc.name.as_str(),
                unquote(front.get("description")?).to_string(),
            ))
        })
        .collect();
    let mut listed: HashMap<String, usize> = HashMap::new();
    let mut problems = Vec::new();
    for line in prose(&index.text).into_iter().skip(3) {
        if line.is_empty() || line.starts_with("# ") {
            continue;
        }
        let Some((target, description)) = line
            .strip_prefix("* [")
            .and_then(|rest| rest.split_once("]("))
            .and_then(|(_, rest)| rest.split_once(") - "))
        else {
            problems.push(format!("index.md: not an entry: {line}"));
            continue;
        };
        *listed.entry(target.to_string()).or_default() += 1;
        match descriptions.get(target) {
            None => problems.push(format!("index.md lists {target}, which does not exist")),
            Some(own) if own != description => problems.push(format!(
                "index.md describes {target} differently from its frontmatter"
            )),
            Some(_) => {}
        }
    }
    for doc in docs.iter().filter(|doc| doc.name != "index.md") {
        match listed.get(&doc.name) {
            None => problems.push(format!("index.md does not list {}", doc.name)),
            Some(1) => {}
            Some(n) => problems.push(format!("index.md lists {} {n} times", doc.name)),
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_link_anchor_and_section_reference_resolves() {
    let docs = docs();
    let by_name: HashMap<&str, &str> = docs
        .iter()
        .map(|doc| (doc.name.as_str(), doc.text.as_str()))
        .collect();
    let titles = index_titles(&docs);
    let mut problems = Vec::new();
    for doc in &docs {
        for (number, raw) in numbered_prose(&doc.text) {
            let place = format!("{}:{number}", doc.name);
            let line = &without_code_spans(raw);
            let line_links = links(line);
            for (_, _, target) in &line_links {
                if let Some(problem) = check_link(&doc.name, target, &by_name) {
                    problems.push(format!("{place}: {problem}"));
                }
            }
            for (at, _) in line.match_indices('§') {
                let number: String = line[at + '§'.len_utf8()..]
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                let number = number.trim_end_matches('.');
                if number.is_empty() {
                    continue;
                }
                let target = section_target(line, at, &line_links)
                    .or_else(|| titled_target(&line[..at], &titles))
                    .unwrap_or(&doc.name);
                let Some(text) = by_name.get(target) else {
                    continue;
                };
                if !sections(text).iter().any(|s| s == number) {
                    problems.push(format!("{place}: §{number} is not a section of {target}"));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// `line` with its inline code spans blanked out, offsets kept: a link or a
/// `§` quoted as code is an example, not a reference.
fn without_code_spans(line: &str) -> String {
    let mut inside = false;
    line.chars()
        .map(|c| {
            if c == '`' {
                inside = !inside;
                ' '
            } else if inside {
                // Same byte length, so offsets still line up.
                if c.is_ascii() { ' ' } else { c }
            } else {
                c
            }
        })
        .collect()
}

/// Each document's title in the index (`Common UI` → `ui-common.md`),
/// longest first.
fn index_titles(docs: &[Doc]) -> Vec<(String, String)> {
    let index = docs
        .iter()
        .find(|doc| doc.name == "index.md")
        .expect("docs/index.md");
    let mut titles: Vec<(String, String)> = index
        .text
        .lines()
        .filter_map(|line| {
            let (title, rest) = line.strip_prefix("* [")?.split_once("](")?;
            Some((title.to_string(), rest.split_once(')')?.0.to_string()))
        })
        .collect();
    titles.sort_by_key(|(title, _)| std::cmp::Reverse(title.len()));
    titles
}

/// The document a `§` refers to by name, as in "(Common UI §7.3)".
fn titled_target<'a>(before: &str, titles: &'a [(String, String)]) -> Option<&'a str> {
    let before = before.trim_end();
    titles
        .iter()
        .find(|(title, _)| before.ends_with(title.as_str()))
        .map(|(_, file)| file.as_str())
}

/// The document a `§` at `at` refers to: the link it sits in, or the one it
/// directly follows.
fn section_target<'a>(line: &str, at: usize, links: &[(usize, usize, &'a str)]) -> Option<&'a str> {
    let document = |target: &'a str| {
        let file = target.split('#').next().unwrap_or(target);
        (file.ends_with(".md") && !file.contains('/')).then_some(file)
    };
    for (start, end, target) in links {
        if (*start..*end).contains(&at) {
            return document(target);
        }
        if *end <= at && line[*end..at].trim().is_empty() {
            return document(target);
        }
    }
    None
}

fn check_link(from: &str, target: &str, docs: &HashMap<&str, &str>) -> Option<String> {
    if target.contains("://") || target.starts_with("mailto:") {
        return None;
    }
    let (file, anchor) = target.split_once('#').unwrap_or((target, ""));
    let text = if file.is_empty() {
        docs.get(from).copied()
    } else if let Some(text) = docs.get(file) {
        Some(*text)
    } else {
        let path = root().join("docs").join(file);
        return (!Path::new(&path).exists()).then(|| format!("link to missing {file}"));
    };
    let text = text?;
    (!anchor.is_empty() && !anchors(text).iter().any(|a| a == anchor)).then(|| {
        format!(
            "no heading for #{anchor} in {}",
            if file.is_empty() { from } else { file }
        )
    })
}

#[test]
fn an_edited_specification_carries_a_new_stamp() {
    let Ok(base) = std::env::var("OXIDGENE_SPEC_BASE") else {
        return;
    };
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root())
            .output()
            .expect("git runs");
        assert!(output.status.success(), "git {args:?} failed");
        String::from_utf8(output.stdout).unwrap()
    };
    let changed = git(&["diff", "--name-only", &base, "--", "docs/*.md"]);
    let mut stale = Vec::new();
    for file in changed.lines().filter(|f| !f.ends_with("index.md")) {
        if !root().join(file).exists() {
            continue;
        }
        let diff = git(&["diff", "-U0", &base, "--", file]);
        if !diff.lines().any(|line| line.starts_with("+generated:")) {
            stale.push(file.to_string());
        }
    }
    assert!(
        stale.is_empty(),
        "edited without a new `generated.at`: {stale:?}"
    );
}

#[test]
fn slugs_follow_github() {
    assert_eq!(slug("2.1 Build and Quality"), "21-build-and-quality");
    assert_eq!(slug("Tiers — CI & release"), "tiers--ci--release");
    assert_eq!(sections("## 2. Commands\n### 2.1 Build\n"), ["2", "2.1"]);
}
