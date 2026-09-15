//! Reading the themes a user wrote, from a folder next to the database.
//!
//! `oxidgene-ui` compiles to WebAssembly and cannot touch a filesystem, so it
//! declares [`CustomThemeSource`] and this module supplies it — the same seam
//! the Geneanet login window uses. The browser build provides nothing and
//! offers only the built-in themes.
//!
//! The folder is read on demand rather than watched. A theme file changes
//! when someone edits it by hand, which is rare and already followed by a
//! trip to the settings page; a watcher would mean a dependency and a
//! background thread for an event that arrives a few times in a program's
//! life.

use std::path::PathBuf;
use std::sync::Arc;

use oxidgene_ui::theme::{CustomThemeError, CustomThemeSource, CustomThemes, parse_custom_theme};
use tracing::{debug, warn};

/// Folder name under the application data directory.
const THEMES_DIR: &str = "themes";

/// Reads `*.json` from `<data_dir>/themes/`.
pub struct DesktopThemeSource {
    dir: PathBuf,
}

impl DesktopThemeSource {
    /// Point a source at `<data_dir>/themes/`, creating the folder.
    ///
    /// The folder is created even when empty so that the path shown in
    /// settings is one the user can actually open: telling someone to drop a
    /// file into a directory that does not exist is a worse first step than
    /// finding it already there and empty.
    pub fn install(data_dir: &std::path::Path) -> Arc<Self> {
        let dir = data_dir.join(THEMES_DIR);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            // Not fatal: the built-in themes still work, and the settings
            // page will simply report an empty folder.
            warn!(error = %error, "Failed to create the custom themes directory");
        }
        Arc::new(Self { dir })
    }
}

impl CustomThemeSource for DesktopThemeSource {
    fn location(&self) -> String {
        self.dir.display().to_string()
    }

    fn load(&self) -> CustomThemes {
        let mut themes = Vec::new();
        let mut errors = Vec::new();

        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return CustomThemes::default();
            }
            Err(error) => {
                errors.push(CustomThemeError {
                    file: THEMES_DIR.to_owned(),
                    message: error.to_string(),
                });
                return CustomThemes { themes, errors };
            }
        };

        // Sorted, so the picker lists the same themes in the same order from
        // one launch to the next; directory order is not stable.
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        paths.sort();

        for path in paths {
            let file = path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );

            let outcome = std::fs::read_to_string(&path)
                .map_err(|error| error.to_string())
                .and_then(|source| parse_custom_theme(&source).map_err(|error| error.to_string()));

            match outcome {
                Ok(theme) => themes.push(theme),
                Err(message) => {
                    // Reported rather than logged and dropped: a theme that
                    // silently fails to appear gives the author nothing to
                    // work from.
                    warn!(file = %file, error = %message, "Ignoring an invalid theme file");
                    errors.push(CustomThemeError { file, message });
                }
            }
        }

        // Two files can carry the same id; the first one wins so that the
        // choice the user has stored keeps resolving to the same theme.
        let mut seen: Vec<String> = Vec::with_capacity(themes.len());
        themes.retain(|theme| {
            if seen.contains(&theme.id) {
                warn!(id = %theme.id, "Ignoring a duplicate custom theme id");
                false
            } else {
                seen.push(theme.id.clone());
                true
            }
        });

        debug!(
            loaded = themes.len(),
            failed = errors.len(),
            "Read the custom themes directory"
        );
        CustomThemes { themes, errors }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal theme file overriding one colour of the light palette.
    fn theme_json(id: &str, orange: &str) -> String {
        format!(
            r##"{{"id":"{id}","name":"{id}","base":"light","colors":{{"orange":"{orange}"}}}}"##
        )
    }

    fn source_over(dir: &std::path::Path) -> DesktopThemeSource {
        DesktopThemeSource {
            dir: dir.join(THEMES_DIR),
        }
    }

    #[test]
    fn reads_json_files_in_a_stable_order_and_ignores_everything_else() {
        let root = tempfile::tempdir().expect("tempdir");
        let dir = root.path().join(THEMES_DIR);
        std::fs::create_dir_all(&dir).expect("create");
        std::fs::write(dir.join("zinc.json"), theme_json("zinc", "#445566")).expect("write");
        std::fs::write(dir.join("amber.json"), theme_json("amber", "#cc8800")).expect("write");
        std::fs::write(dir.join("notes.txt"), "not a theme").expect("write");

        let loaded = source_over(root.path()).load();

        let ids: Vec<&str> = loaded
            .themes
            .iter()
            .map(|theme| theme.id.as_str())
            .collect();
        assert_eq!(ids, ["amber", "zinc"]);
        assert!(loaded.errors.is_empty());
    }

    /// One bad file must not cost the user the themes that are fine, and it
    /// must be named so they can go and fix it.
    #[test]
    fn a_broken_file_is_reported_without_hiding_the_good_ones() {
        let root = tempfile::tempdir().expect("tempdir");
        let dir = root.path().join(THEMES_DIR);
        std::fs::create_dir_all(&dir).expect("create");
        std::fs::write(dir.join("good.json"), theme_json("good", "#112233")).expect("write");
        std::fs::write(dir.join("broken.json"), "{ not json").expect("write");

        let loaded = source_over(root.path()).load();

        assert_eq!(loaded.themes.len(), 1);
        assert_eq!(loaded.themes[0].id, "good");
        assert_eq!(loaded.errors.len(), 1);
        assert_eq!(loaded.errors[0].file, "broken.json");
    }

    /// The stored choice is an id, so two files claiming the same one must
    /// resolve the same way on every launch.
    #[test]
    fn a_duplicate_id_keeps_the_first_file_read() {
        let root = tempfile::tempdir().expect("tempdir");
        let dir = root.path().join(THEMES_DIR);
        std::fs::create_dir_all(&dir).expect("create");
        std::fs::write(dir.join("a.json"), theme_json("twin", "#111111")).expect("write");
        std::fs::write(dir.join("b.json"), theme_json("twin", "#222222")).expect("write");

        let loaded = source_over(root.path()).load();

        assert_eq!(loaded.themes.len(), 1);
        assert_eq!(loaded.themes[0].colors.orange.as_str(), "#111111");
    }

    /// A theme taking a built-in id would make the built-in unreachable.
    #[test]
    fn a_file_cannot_claim_a_builtin_id() {
        let root = tempfile::tempdir().expect("tempdir");
        let dir = root.path().join(THEMES_DIR);
        std::fs::create_dir_all(&dir).expect("create");
        std::fs::write(dir.join("dark.json"), theme_json("dark", "#111111")).expect("write");

        let loaded = source_over(root.path()).load();

        assert!(loaded.themes.is_empty());
        assert_eq!(loaded.errors.len(), 1);
    }

    #[test]
    fn a_missing_folder_is_simply_empty() {
        let root = tempfile::tempdir().expect("tempdir");
        let loaded = source_over(root.path()).load();
        assert_eq!(loaded, CustomThemes::default());
    }

    #[test]
    fn install_creates_the_folder_so_the_path_shown_in_settings_exists() {
        let root = tempfile::tempdir().expect("tempdir");
        let source = DesktopThemeSource::install(root.path());
        assert!(root.path().join(THEMES_DIR).is_dir());
        assert!(source.location().ends_with(THEMES_DIR));
    }
}
