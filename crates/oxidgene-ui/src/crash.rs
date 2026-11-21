//! What the reader is told when the application stops on a bug
//! (docs/cross-cutting.md §6.5).
//!
//! A panic ends the application: the release profile aborts, and a browser
//! has no unwinding either. Nothing of the interface can draw after it, so
//! the message is written by other means — plain DOM on the web
//! ([`SCREEN_JS`], run by the web binary's panic hook), a native dialog on
//! the desktop ([`install_native_hook`]) — and its text is kept here, in the
//! reader's language, where a panic hook can reach it without the renderer.

use std::sync::Mutex;

use crate::i18n::{I18n, Language};

/// The crash screen, as the body of a JavaScript function of `title`,
/// `message`, `action` and `report`: it replaces the page with the first
/// three and logs the fourth.
pub const SCREEN_JS: &str = include_str!("crash_screen.js");

/// The text of the crash message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashText {
    pub title: String,
    pub message: String,
    /// The label of the action that starts the application again.
    pub action: String,
}

impl CrashText {
    fn of(i18n: &I18n) -> Self {
        Self {
            title: i18n.t("crash.title"),
            message: i18n.t(if cfg!(target_arch = "wasm32") {
                "crash.message_web"
            } else {
                "crash.message_desktop"
            }),
            action: i18n.t("crash.reload"),
        }
    }
}

/// The text in the language the interface was last drawn in.
static TEXT: Mutex<Option<CrashText>> = Mutex::new(None);

/// Keep the crash message in the language of `i18n`: called by the
/// application shell whenever the language changes.
pub fn remember(i18n: &I18n) {
    if let Ok(mut text) = TEXT.lock() {
        *text = Some(CrashText::of(i18n));
    }
}

/// The crash message: in the language last remembered, in English before the
/// interface drew anything. Never waits: a panic hook calls it, possibly
/// while the lock is held.
pub fn text() -> CrashText {
    TEXT.try_lock()
        .ok()
        .and_then(|text| text.clone())
        .unwrap_or_else(|| CrashText::of(&I18n::new(Language::english())))
}

/// Whether a panic on the thread named `thread` ends the process: always
/// when panics abort, as in a release build; when they unwind, only on the
/// main thread, which holds the window — a request handler's panic is
/// answered as an error and a worker's loop is started again.
#[cfg(not(target_arch = "wasm32"))]
fn is_fatal(thread: Option<&str>, aborts: bool) -> bool {
    aborts || thread == Some("main")
}

/// Say so in a native dialog when a panic ends the desktop application,
/// after whatever the hook already in place does with it. The window is
/// gone or about to be: the dialog is the operating system's own.
#[cfg(not(target_arch = "wasm32"))]
pub fn install_native_hook() {
    use std::sync::atomic::{AtomicBool, Ordering};

    static SHOWN: AtomicBool = AtomicBool::new(false);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        let fatal = is_fatal(std::thread::current().name(), cfg!(panic = "abort"));
        if !fatal || SHOWN.swap(true, Ordering::SeqCst) {
            return;
        }
        let text = text();
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title(text.title)
            .set_description(text.message)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_is_english_until_a_language_is_remembered_and_then_follows_it() {
        let english = CrashText::of(&I18n::new(Language::english()));
        let french = CrashText::of(&I18n::new(Language::try_from_code("fr").unwrap()));
        assert_ne!(english, french);
        for text in [&english, &french] {
            for part in [&text.title, &text.message, &text.action] {
                assert!(
                    !part.is_empty() && !part.starts_with("crash."),
                    "untranslated: {part}"
                );
            }
        }

        // One test, since the remembered text is the process's.
        assert_eq!(text(), english);
        remember(&I18n::new(Language::try_from_code("fr").unwrap()));
        assert_eq!(text(), french);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_panic_is_fatal_when_it_aborts_or_takes_the_main_thread() {
        assert!(is_fatal(Some("tokio-runtime-worker"), true));
        assert!(is_fatal(Some("main"), false));
        assert!(!is_fatal(Some("tokio-runtime-worker"), false));
        assert!(!is_fatal(None, false));
    }
}
