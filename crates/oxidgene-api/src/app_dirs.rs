//! Where the application keeps its files on this machine, by kind, following
//! the XDG Base Directory convention.
//!
//! Every directory a process derives from the user's home goes through
//! [`AppDirs`]; no other module asks the platform where to write.
//!
//! | Kind | Linux default | Holds |
//! |---|---|---|
//! | data | `$XDG_DATA_HOME/oxidgene` (`~/.local/share/oxidgene`) | the database and the media: the user's genealogy |
//! | config | `$XDG_CONFIG_HOME/oxidgene` (`~/.config/oxidgene`) | what the user writes by hand: custom themes |
//! | state | `$XDG_STATE_HOME/oxidgene` (`~/.local/state/oxidgene`) | the desktop window's web profile: cookies, local storage |
//! | cache | `$XDG_CACHE_HOME/oxidgene` (`~/.cache/oxidgene`) | disposable work: job scratch, staged inputs, the web cache |
//!
//! The `dirs` crate reads the `XDG_*` variables and falls back to their
//! defaults, and gives the platform's equivalents on macOS and Windows. Those
//! platforms have no state directory; state then goes to the local data
//! directory, which they keep apart from roaming data.

use std::path::{Path, PathBuf};

/// The name of the application's directory in each base directory.
const APPLICATION: &str = "oxidgene";

/// The application's directories, one per kind of file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    /// The user's genealogy: irreplaceable.
    pub data: PathBuf,
    /// What the user configures or writes by hand.
    pub config: PathBuf,
    /// What the application remembers between runs and can rebuild only at
    /// the user's cost: a sign-in, a chosen language.
    pub state: PathBuf,
    /// What can be deleted at any time the application is not running.
    pub cache: PathBuf,
}

impl AppDirs {
    /// The directories of the current user, or `None` when the platform
    /// knows no home for them.
    #[must_use]
    pub fn resolve() -> Option<Self> {
        Some(Self::from_bases(
            dirs::data_dir()?,
            dirs::config_dir()?,
            state_base(dirs::state_dir(), dirs::data_local_dir())?,
            dirs::cache_dir()?,
        ))
    }

    /// The application's directories within these base directories.
    #[must_use]
    pub fn from_bases(data: PathBuf, config: PathBuf, state: PathBuf, cache: PathBuf) -> Self {
        Self {
            data: data.join(APPLICATION),
            config: config.join(APPLICATION),
            state: state.join(APPLICATION),
            cache: cache.join(APPLICATION),
        }
    }

    /// The SQLite database.
    #[must_use]
    pub fn database(&self) -> PathBuf {
        self.data.join("oxidgene.db")
    }

    /// The media store's root.
    #[must_use]
    pub fn media(&self) -> PathBuf {
        self.data.join("media")
    }

    /// The folder of custom themes.
    #[must_use]
    pub fn themes(&self) -> PathBuf {
        self.config.join("themes")
    }

    /// The desktop window's web profile.
    #[must_use]
    pub fn webview(&self) -> PathBuf {
        self.state.join("webview")
    }

    /// The root of the disposable working files (see [`crate::workdir`]).
    #[must_use]
    pub fn work(&self) -> &Path {
        &self.cache
    }
}

/// The base directory of state: the platform's, or its local data directory
/// where it has none (macOS, Windows).
fn state_base(state: Option<PathBuf>, data_local: Option<PathBuf>) -> Option<PathBuf> {
    state.or(data_local)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_gets_the_applications_directory_in_its_base() {
        let dirs = AppDirs::from_bases("/d".into(), "/c".into(), "/s".into(), "/k".into());
        assert_eq!(dirs.database(), Path::new("/d/oxidgene/oxidgene.db"));
        assert_eq!(dirs.media(), Path::new("/d/oxidgene/media"));
        assert_eq!(dirs.themes(), Path::new("/c/oxidgene/themes"));
        assert_eq!(dirs.webview(), Path::new("/s/oxidgene/webview"));
        assert_eq!(dirs.work(), Path::new("/k/oxidgene"));
    }

    #[test]
    fn state_falls_back_to_local_data_where_the_platform_has_none() {
        let local = PathBuf::from("/local");
        assert_eq!(
            state_base(Some("/state".into()), Some(local.clone())),
            Some(PathBuf::from("/state"))
        );
        assert_eq!(state_base(None, Some(local.clone())), Some(local));
        assert_eq!(state_base(None, None), None);
    }

    /// The variables and their defaults, read from the environment as the
    /// applications read them. The environment is the process's: nextest,
    /// which runs the suites, gives each test a process of its own.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_xdg_variables_choose_the_directories_and_home_gives_the_defaults() {
        const VARIABLES: [&str; 5] = [
            "HOME",
            "XDG_DATA_HOME",
            "XDG_CONFIG_HOME",
            "XDG_STATE_HOME",
            "XDG_CACHE_HOME",
        ];
        let saved: Vec<_> = VARIABLES.iter().map(std::env::var_os).collect();
        let root = tempfile::tempdir().expect("a directory for the bases");
        let base = |name: &str| root.path().join(name);
        let set = |name: &str, value: Option<&Path>| {
            // SAFETY: the test runs alone in its process (see above), on
            // one thread, and restores the variables.
            unsafe {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        };

        set("HOME", Some(&base("home")));
        for (variable, name) in [
            ("XDG_DATA_HOME", "data"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_CACHE_HOME", "cache"),
        ] {
            set(variable, Some(&base(name)));
        }
        let chosen = AppDirs::resolve();

        for variable in &VARIABLES[1..] {
            set(variable, None);
        }
        let defaults = AppDirs::resolve();

        for (variable, value) in VARIABLES.iter().zip(&saved) {
            set(variable, value.as_deref().map(Path::new));
        }

        assert_eq!(
            chosen,
            Some(AppDirs::from_bases(
                base("data"),
                base("config"),
                base("state"),
                base("cache"),
            ))
        );
        let home = base("home");
        assert_eq!(
            defaults,
            Some(AppDirs::from_bases(
                home.join(".local/share"),
                home.join(".config"),
                home.join(".local/state"),
                home.join(".cache"),
            ))
        );
    }
}
