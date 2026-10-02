use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use oxidgene_ui::i18n::{
    CustomLanguage, CustomLanguageError, CustomLanguageSource, CustomLanguages,
    is_valid_custom_language_code, parse_custom_language,
};

pub struct DesktopLanguageSource {
    directory: PathBuf,
}

impl DesktopLanguageSource {
    pub fn install(directory: &Path) -> Arc<Self> {
        if std::fs::create_dir_all(directory).is_err() {
            tracing::warn!(
                error = "language_directory",
                "Could not create the custom languages directory"
            );
        }
        Arc::new(Self {
            directory: directory.to_owned(),
        })
    }
}

impl CustomLanguageSource for DesktopLanguageSource {
    fn location(&self) -> String {
        self.directory.display().to_string()
    }

    fn load(&self) -> CustomLanguages {
        let mut result = CustomLanguages::default();
        let entries = match std::fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return result,
            Err(error) => {
                result.errors.push(CustomLanguageError {
                    file: "languages".to_owned(),
                    message: error.to_string(),
                });
                return result;
            }
        };
        let mut files = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry)
                    if entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension == "json") =>
                {
                    files.push(entry.path())
                }
                Ok(_) => {}
                Err(error) => result.errors.push(CustomLanguageError {
                    file: "languages".to_owned(),
                    message: error.to_string(),
                }),
            }
        }
        files.sort();
        let mut seen = HashSet::new();
        for file in files {
            match read_language(&file) {
                Ok(language) if seen.insert(language.code().to_owned()) => {
                    result.languages.push(language)
                }
                Ok(_) => result.errors.push(CustomLanguageError {
                    file: filename(&file),
                    message: "duplicate language code".to_owned(),
                }),
                Err(error) => result.errors.push(error),
            }
        }
        result
    }

    fn load_code(&self, code: &str) -> Result<Option<CustomLanguage>, CustomLanguageError> {
        if !is_valid_custom_language_code(code) {
            return Err(CustomLanguageError {
                file: "languages".to_owned(),
                message: "invalid language code".to_owned(),
            });
        }
        let file = self.directory.join(format!("{code}.json"));
        match std::fs::read_to_string(&file) {
            Ok(source) => parse_file(&file, &source).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(CustomLanguageError {
                file: filename(&file),
                message: error.to_string(),
            }),
        }
    }
}

fn filename(file: &Path) -> String {
    file.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn parse_file(file: &Path, source: &str) -> Result<CustomLanguage, CustomLanguageError> {
    let locale = parse_custom_language(source).map_err(|message| CustomLanguageError {
        file: filename(file),
        message,
    })?;
    if file.file_stem().and_then(|name| name.to_str()) != Some(locale.code()) {
        return Err(CustomLanguageError {
            file: filename(file),
            message: "file name must match the locale code".to_owned(),
        });
    }
    Ok(locale)
}

fn read_language(file: &Path) -> Result<CustomLanguage, CustomLanguageError> {
    let source = std::fs::read_to_string(file).map_err(|error| CustomLanguageError {
        file: filename(file),
        message: error.to_string(),
    })?;
    parse_file(file, &source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(code: &str) -> String {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../../../assets/i18n/en.json")).unwrap();
        value["code"] = code.into();
        value["name"] = "Example locale".into();
        value.to_string()
    }

    #[test]
    fn discovers_json_in_order_and_reports_invalid_files() {
        let directory = tempfile::tempdir().unwrap();
        let source = DesktopLanguageSource::install(directory.path());
        std::fs::write(directory.path().join("zz.json"), document("zz")).unwrap();
        std::fs::write(directory.path().join("aa.json"), document("aa")).unwrap();
        std::fs::write(directory.path().join("broken.json"), "{").unwrap();
        std::fs::write(directory.path().join("notes.txt"), "ignored").unwrap();
        let loaded = source.load();
        assert_eq!(
            loaded
                .languages
                .iter()
                .map(CustomLanguage::code)
                .collect::<Vec<_>>(),
            ["aa", "zz"]
        );
        assert_eq!(loaded.errors.len(), 1);
        assert_eq!(loaded.errors[0].file, "broken.json");
    }

    #[test]
    fn restores_only_the_selected_file_and_handles_its_removal() {
        let directory = tempfile::tempdir().unwrap();
        let source = DesktopLanguageSource::install(directory.path());
        std::fs::write(directory.path().join("aa.json"), document("aa")).unwrap();
        std::fs::write(directory.path().join("broken.json"), "{").unwrap();
        assert_eq!(source.load_code("aa").unwrap().unwrap().code(), "aa");
        std::fs::remove_file(directory.path().join("aa.json")).unwrap();
        assert!(source.load_code("aa").unwrap().is_none());
        assert!(source.load_code("../aa").is_err());
    }

    #[test]
    fn rejects_mismatched_names_and_embedded_codes() {
        let directory = tempfile::tempdir().unwrap();
        let source = DesktopLanguageSource::install(directory.path());
        std::fs::write(directory.path().join("aa.json"), document("zz")).unwrap();
        std::fs::write(directory.path().join("en.json"), document("en")).unwrap();
        assert_eq!(source.load().errors.len(), 2);
    }
}
