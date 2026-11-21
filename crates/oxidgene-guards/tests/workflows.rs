//! Workflows and the justfile: every check a GitHub workflow runs is a
//! recipe, called as `just <recipe>` (docs/development.md §2.9).
//!
//! Drift it prevents: a workflow step that retypes a cargo, npm, uv or
//! script command beside the recipe a developer runs — the two then differ
//! by a flag nobody notices, and a change passes locally to fail in CI, or
//! the reverse; a step calling a recipe that was renamed; an action that
//! checks something in place of a recipe; a job added to the CI gate that
//! `just check` does not run, or the reverse, so that a commit checked
//! locally fails once pushed; and the two pins of `just` (the CI image's
//! and the one installed elsewhere) drifting apart.
//!
//! Fixing a failure: move the command into a recipe of the justfile and
//! call `just <recipe>` from the step. What CI must do differently is a
//! recipe parameter or an environment variable the recipe reads, never
//! another command. A check added to the gate joins both `check` in the
//! justfile and a job of ci.yml; one the gate runs for Renovate's pull
//! requests alone is declared in `CI_ONLY` with its reason. Installing a tool (`rustup toolchain install`, an
//! install action), restoring a cache, and packaging or publishing what was
//! built are not checks and stay in the workflow. An action is added to
//! `ACTIONS` with what it does — only when it installs, caches, builds an
//! image or publishes.

use std::collections::BTreeSet;

use oxidgene_guards::{Recipe, files, just_recipe_graph, just_recipes, read, relative};

/// The actions a workflow may use, each with what it does: none of them
/// checks the code, which only a recipe does.
const ACTIONS: &[(&str, &str)] = &[
    ("actions/checkout", "fetches the sources"),
    ("actions/cache", "restores and saves a build"),
    (
        "Swatinem/rust-cache",
        "restores and saves the dependencies' build",
    ),
    ("dorny/paths-filter", "tells which jobs a change concerns"),
    ("taiki-e/install-action", "installs a prebuilt tool"),
    ("astral-sh/setup-uv", "installs uv"),
    ("actions/setup-node", "installs Node.js"),
    ("azure/setup-helm", "installs Helm"),
    ("jdx/mise-action", "installs a tool of mise.toml"),
    ("docker/setup-buildx-action", "sets the image builder up"),
    (
        "docker/setup-qemu-action",
        "emulates the other architecture",
    ),
    ("docker/login-action", "signs in to the registry"),
    (
        "docker/build-push-action",
        "builds an image with the runner's layer cache, and publishes it",
    ),
    ("actions/upload-artifact", "keeps a file of the run"),
    ("actions/download-artifact", "fetches a file of the run"),
    (
        "actions/delete-package-versions",
        "prunes the registry's untagged images",
    ),
    ("actions/upload-pages-artifact", "hands the site to Pages"),
    ("actions/deploy-pages", "publishes the site"),
    ("actions/attest", "signs the downloads' provenance"),
    (
        "signpath/github-action-submit-signing-request",
        "has the Windows executable signed",
    ),
];

/// The recipes ci.yml runs that `just check` does not, each with why.
const CI_ONLY: &[(&str, &str)] = &[
    (
        "guards",
        "part of `test`; a job of its own so that documentation-only changes run it",
    ),
    (
        "compose-check",
        "Renovate's pull requests that touch docker/, which need Docker",
    ),
    (
        "chart-test",
        "Renovate's pull requests that touch the chart, which need Helm",
    ),
];

/// A workflow or a composite action: its path and its text.
struct Source {
    path: String,
    text: String,
}

fn sources() -> Vec<Source> {
    let sources: Vec<Source> = files(".github", "yml")
        .into_iter()
        .map(|path| Source {
            text: std::fs::read_to_string(&path).expect("a readable workflow"),
            path: relative(&path),
        })
        .collect();
    assert!(
        sources.iter().any(|s| s.path.ends_with("workflows/ci.yml")),
        "the guard found no workflow to read"
    );
    sources
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The value of `key:` on a line, the list dash before the key dropped,
/// with the column the key starts at.
fn value_of<'a>(line: &'a str, key: &str) -> Option<(usize, &'a str)> {
    let trimmed = line.trim_start();
    let entry = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim_start();
    let value = entry.strip_prefix(key)?.strip_prefix(':')?;
    Some((line.len() - entry.len(), value.trim()))
}

/// Every `run:` script of a workflow, with the 1-based line it starts at: a
/// value on the key's line, or the block scalar below it.
fn scripts(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let Some((column, value)) = value_of(lines[index], "run") else {
            index += 1;
            continue;
        };
        let start = index + 1;
        index += 1;
        if !value.starts_with(['|', '>']) {
            found.push((start, value.to_string()));
            continue;
        }
        let mut body = String::new();
        while index < lines.len()
            && (lines[index].trim().is_empty() || indent_of(lines[index]) > column)
        {
            body.push_str(lines[index]);
            body.push('\n');
            index += 1;
        }
        found.push((start, body));
    }
    found
}

/// A reader of shell text, splitting it into simple commands and those
/// into words: quoted text stays one word, and a command substitution
/// starts a command of its own, inside double quotes too.
#[derive(Default)]
struct Shell {
    commands: Vec<Vec<String>>,
    words: Vec<String>,
    word: String,
    quote: Option<char>,
    /// The quote each open `$(` was written in.
    substitutions: Vec<Option<char>>,
}

impl Shell {
    fn end_word(&mut self) {
        if !self.word.is_empty() {
            self.words.push(std::mem::take(&mut self.word));
        }
    }

    /// Ends the command being read: its words, without the variable
    /// assignments, `sudo` or `exec` before the command's name.
    fn end_command(&mut self) {
        self.end_word();
        let words: Vec<String> = std::mem::take(&mut self.words)
            .into_iter()
            .skip_while(|word| is_assignment(word) || word == "sudo" || word == "exec")
            .collect();
        if !words.is_empty() {
            self.commands.push(words);
        }
    }

    /// Reads the start of `rest` and returns how many bytes it took.
    fn step(&mut self, rest: &str) -> usize {
        let c = rest.chars().next().unwrap_or(' ');
        if self.quote != Some('\'') && rest.starts_with("$(") {
            self.end_command();
            self.substitutions.push(self.quote.take());
            return 2;
        }
        if self.quote != Some('\'') && c == '\\' {
            // An escaped character is itself, whatever it is.
            let escaped = rest[1..].chars().next();
            self.word.extend(escaped);
            return 1 + escaped.map_or(0, char::len_utf8);
        }
        match (self.quote, c) {
            (Some(open), _) if open == c => self.quote = None,
            (Some('"'), '`') | (None, '`' | ';' | '|' | '&' | '(') => self.end_command(),
            (Some(_), _) => self.word.push(c),
            (None, '\'' | '"') => self.quote = Some(c),
            (None, ')') => {
                self.end_command();
                self.quote = self.substitutions.pop().flatten();
            }
            (None, _) if c.is_whitespace() => self.end_word(),
            (None, _) => self.word.push(c),
        }
        c.len_utf8()
    }
}

/// The simple commands of a shell script, each as its words: comments
/// dropped and continued lines joined.
fn commands(script: &str) -> Vec<Vec<String>> {
    let joined = script.replace("\\\n", " ");
    let mut shell = Shell::default();
    for line in joined.lines().filter(|l| !l.trim_start().starts_with('#')) {
        let mut at = 0;
        while at < line.len() {
            at += shell.step(&line[at..]);
        }
        // A quoted text may run over several lines; a command does not.
        if shell.quote.is_none() {
            shell.end_command();
        } else {
            shell.word.push('\n');
        }
    }
    shell.end_command();
    shell.commands
}

/// `NAME=value`, which may precede a command.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn names_a_script(word: &str) -> bool {
    word.starts_with("scripts/") || word.starts_with("./scripts/")
}

/// The tool a command runs that the recipes wrap, if any.
fn wrapped_tool(words: &[String]) -> Option<&'static str> {
    let command = words[0].as_str();
    let argument = |at: usize| words.get(at).map_or("", String::as_str);
    match command {
        "cargo" => Some("cargo"),
        "npm" | "npx" | "node" | "playwright" => Some("Node.js tooling"),
        "uv" | "uvx" => Some("uv"),
        "dx" => Some("the Dioxus CLI"),
        "scc" | "actionlint" => Some("a checking tool"),
        "python" | "python3" | "bash" | "sh" if words.iter().any(|w| names_a_script(w)) => {
            Some("a script of scripts/")
        }
        "helm" if matches!(argument(1), "lint" | "template" | "dependency") => Some("Helm"),
        "docker"
            if matches!(argument(1), "compose" | "build")
                || (argument(1) == "buildx" && argument(2) == "build") =>
        {
            Some("Docker")
        }
        _ if command.starts_with("cargo-") => Some("cargo"),
        _ if names_a_script(command) => Some("a script of scripts/"),
        _ => None,
    }
}

#[test]
fn no_workflow_step_runs_a_wrapped_tool_directly() {
    let mut violations = Vec::new();
    for source in sources() {
        for (line, script) in scripts(&source.text) {
            for words in commands(&script) {
                if let Some(tool) = wrapped_tool(&words) {
                    violations.push(format!(
                        "{}:{line}: `{}` runs {tool} directly",
                        source.path,
                        words.join(" ")
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "a workflow runs its checks through `just <recipe>` (docs/development.md §2.9):\n{}",
        violations.join("\n")
    );
}

/// The recipe a `just` command names: its first word that is not an option.
fn recipe_of(words: &[String]) -> Option<&str> {
    (words[0] == "just").then(|| {
        words[1..]
            .iter()
            .map(String::as_str)
            .find(|word| !word.starts_with('-'))
            .unwrap_or("")
    })
}

#[test]
fn every_recipe_a_workflow_calls_exists() {
    let recipes = just_recipes();
    let known = |name: &str| recipes.iter().any(|recipe| recipe == name);
    let mut violations = Vec::new();
    let mut called = 0;
    for source in sources() {
        // A matrix leg names its recipe under the `recipe` key.
        for (number, line) in source.text.lines().enumerate() {
            if let Some((_, recipe)) = value_of(line, "recipe")
                && !known(recipe)
            {
                violations.push(format!(
                    "{}:{}: the matrix names `just {recipe}`, which the justfile lacks",
                    source.path,
                    number + 1
                ));
            }
        }
        for (line, script) in scripts(&source.text) {
            for words in commands(&script) {
                let Some(recipe) = recipe_of(&words) else {
                    continue;
                };
                called += 1;
                let problem = if recipe == "${{" {
                    if words.get(2).is_some_and(|w| w == "matrix.recipe") {
                        continue;
                    }
                    "takes its recipe from an expression other than `matrix.recipe`"
                } else if recipe.starts_with(['$', '"', '\'']) || recipe.is_empty() {
                    "names no recipe the guard can read"
                } else if known(recipe) {
                    continue;
                } else {
                    "names a recipe the justfile lacks"
                };
                violations.push(format!(
                    "{}:{line}: `{}` {problem}",
                    source.path,
                    words.join(" ")
                ));
            }
        }
    }
    assert!(called > 20, "the guard read only {called} `just` calls");
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn every_action_is_one_that_installs_caches_or_publishes() {
    let mut violations = Vec::new();
    for source in sources() {
        for (number, line) in source.text.lines().enumerate() {
            let Some((_, action)) = value_of(line, "uses") else {
                continue;
            };
            let name = action.split(['@', ' ']).next().unwrap_or(action);
            let allowed = name.starts_with("./")
                || ACTIONS
                    .iter()
                    .any(|(known, _)| name == *known || name.starts_with(&format!("{known}/")));
            if !allowed {
                violations.push(format!("{}:{}: {name}", source.path, number + 1));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "actions the guard does not know: a check is a recipe of the justfile, never an \
         action; one that installs, caches, builds an image or publishes joins `ACTIONS` \
         with what it does:\n{}",
        violations.join("\n")
    );
}

/// The recipes ci.yml calls, its matrix legs included.
fn ci_recipes() -> BTreeSet<String> {
    let text = read(".github/workflows/ci.yml");
    let mut called: BTreeSet<String> = text
        .lines()
        .filter_map(|line| value_of(line, "recipe"))
        .map(|(_, recipe)| recipe.to_string())
        .collect();
    for (_, script) in scripts(&text) {
        for words in commands(&script) {
            if let Some(recipe) = recipe_of(&words)
                && recipe != "${{"
            {
                called.insert(recipe.to_string());
            }
        }
    }
    called
}

/// `name` and every recipe it runs first, directly or not.
fn closure(graph: &[Recipe], name: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut pending = vec![name.to_string()];
    while let Some(recipe) = pending.pop() {
        if let Some(found) = graph.iter().find(|r| r.name == recipe)
            && seen.insert(recipe)
        {
            pending.extend(found.dependencies.iter().cloned());
        }
    }
    seen
}

/// Whether CI runs `name`: itself, or — for a recipe that only gathers
/// others — every recipe it gathers.
fn covered(graph: &[Recipe], called: &BTreeSet<String>, name: &str) -> bool {
    if called.contains(name) {
        return true;
    }
    graph.iter().find(|r| r.name == name).is_some_and(|recipe| {
        !recipe.has_body
            && !recipe.dependencies.is_empty()
            && recipe
                .dependencies
                .iter()
                .all(|dependency| covered(graph, called, dependency))
    })
}

#[test]
fn the_ci_gate_runs_what_just_check_runs() {
    let graph = just_recipe_graph();
    let called = ci_recipes();
    let gate = closure(&graph, "check");
    assert!(gate.len() > 10, "`just check` gathers only {gate:?}");
    let mut problems = Vec::new();
    for recipe in &called {
        if !gate.contains(recipe) && !CI_ONLY.iter().any(|(name, _)| name == recipe) {
            problems.push(format!(
                "ci.yml runs `just {recipe}`, which `just check` does not: add it to `check`"
            ));
        }
    }
    for recipe in &gate {
        if !covered(&graph, &called, recipe) {
            problems.push(format!(
                "`just check` runs `{recipe}`, which no job of ci.yml does: give it a job"
            ));
        }
    }
    for (recipe, _) in CI_ONLY {
        if !called.contains(*recipe) {
            problems.push(format!(
                "`CI_ONLY` declares `{recipe}`, which ci.yml no longer runs"
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "the local gate and the CI gate differ (docs/development.md §2.9):\n{}",
        problems.join("\n")
    );
}

/// The release of `just` named after `marker` in a file.
fn just_version(file: &str, marker: &str) -> String {
    let text = read(file);
    let after = text
        .split_once(marker)
        .unwrap_or_else(|| panic!("{file} no longer pins just with `{marker}`"))
        .1;
    after
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect()
}

#[test]
fn the_ci_image_and_the_runners_install_the_same_just() {
    let image = just_version("docker/Dockerfile.ci", "ARG JUST_VERSION=");
    let runners = just_version(".github/actions/setup-just/action.yml", "tool: just@");
    assert!(
        !image.is_empty(),
        "docker/Dockerfile.ci pins no just release"
    );
    assert_eq!(
        image, runners,
        "docker/Dockerfile.ci and .github/actions/setup-just pin different releases of just"
    );
}

#[test]
fn the_guard_reads_scripts_and_their_commands() {
    let workflow = r#"jobs:
  build:
    outputs:
      run: ${{ steps.since.outputs.run }}
    steps:
      - run: just fmt-check
      - name: Several commands
        env:
          FLAG: "1"
        run: |
          # cargo build in a comment
          rustup toolchain install nightly
          RUSTFLAGS=-Dwarnings cargo build --locked \
            --release
          (cd e2e && npx playwright test) | tee log.txt
          git commit -m "vendor the release (scripts/pdfjs.sh); cargo is not run"
          version="$(uv run scripts/version.py "$TAG")"
      - name: Next
        run: >-
          python3 scripts/budgets.py binaries
"#;
    let found = scripts(workflow);
    assert_eq!(found.len(), 4);
    assert_eq!(found[1], (6, "just fmt-check".to_string()));
    let tools: Vec<&str> = found
        .iter()
        .flat_map(|(_, script)| commands(script))
        .filter_map(|words| wrapped_tool(&words))
        .collect();
    assert_eq!(
        tools,
        ["cargo", "Node.js tooling", "uv", "a script of scripts/"],
        "the comment, rustup, cd, tee, just and the quoted message are not wrapped tools"
    );
}

#[test]
fn the_guard_tells_publication_from_checks() {
    let words =
        |command: &str| -> Vec<String> { command.split_whitespace().map(str::to_string).collect() };
    for allowed in [
        "helm package charts/oxidgene --destination dist",
        "docker buildx imagetools create --tag a b",
        "python3 -I -c 'import json'",
        "rustup target add wasm32-unknown-unknown",
        "just e2e --project=chromium",
    ] {
        assert_eq!(wrapped_tool(&words(allowed)), None, "{allowed}");
    }
    for refused in [
        "helm lint charts/oxidgene",
        "docker compose -f docker/docker-compose.yml config",
        "scripts/archives-live.sh fr-ad00",
        "cargo-nextest nextest run",
        "bash scripts/dev-web.sh",
    ] {
        assert!(wrapped_tool(&words(refused)).is_some(), "{refused}");
    }
    assert_eq!(
        recipe_of(&words("just --quiet e2e --headed")),
        Some("e2e"),
        "options before the recipe are skipped"
    );
}
