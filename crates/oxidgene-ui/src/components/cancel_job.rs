//! The Cancel button of a running background job — an import or a GEDZIP
//! export — and the confirmation it asks for.
//!
//! The button only asks the server to stop the job. Whoever follows the job
//! keeps polling it, sees `cancelling` while it unwinds and `cancelled` once
//! it has, and shows that outcome itself; a request that arrives after the
//! job ended changes nothing, and the poll then reads its real end.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{ApiClient, is_job_over};
use crate::components::confirm_dialog::ConfirmDialog;
use crate::i18n::use_i18n;

/// Which kind of job the button stops, and so what the confirmation says
/// is undone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    /// An import, which rolls back everything it wrote.
    Import,
    /// A GEDZIP export, which produces no archive and changes nothing.
    Export,
}

/// The translation keys of one kind's button and confirmation.
struct CancelKeys {
    button: &'static str,
    title: &'static str,
    message: &'static str,
    confirm: &'static str,
    dismiss: &'static str,
}

impl JobKind {
    const fn keys(self) -> CancelKeys {
        match self {
            Self::Import => CancelKeys {
                button: "import.cancel",
                title: "import.cancel_title",
                message: "import.cancel_message",
                confirm: "import.cancel_confirm",
                dismiss: "import.cancel_keep",
            },
            Self::Export => CancelKeys {
                button: "settings.export_cancel",
                title: "settings.export_cancel_title",
                message: "settings.export_cancel_message",
                confirm: "settings.export_cancel_confirm",
                dismiss: "settings.export_cancel_keep",
            },
        }
    }
}

/// Whether a job that last reported `phase` can still be asked to cancel:
/// not once it has been asked, nor once it has ended.
#[must_use]
pub fn can_cancel(phase: &str) -> bool {
    phase != "cancelling" && !is_job_over(phase)
}

/// A Cancel button for job `job_id` of tree `tree_id`, which asks for
/// confirmation before stopping it.
///
/// Render it only while the job can be cancelled ([`can_cancel`]); it hides
/// itself once the request went through, before the next poll says so.
#[component]
pub fn CancelJobButton(kind: JobKind, tree_id: Uuid, job_id: Uuid) -> Element {
    let api = use_context::<ApiClient>();
    let i18n = use_i18n();
    let keys = kind.keys();
    let mut confirming = use_signal(|| false);
    let mut requesting = use_signal(|| false);
    let mut requested = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let confirm = move |()| {
        if requesting() {
            return;
        }
        requesting.set(true);
        error.set(None);
        let api = api.clone();
        spawn(async move {
            let outcome = match kind {
                JobKind::Import => api.cancel_file_import(tree_id, job_id).await.map(|_| ()),
                JobKind::Export => api.cancel_export_job(tree_id, job_id).await.map(|_| ()),
            };
            requesting.set(false);
            match outcome {
                Ok(()) => {
                    requested.set(true);
                    confirming.set(false);
                }
                Err(e) => error.set(Some(format!("{e}"))),
            }
        });
    };

    if requested() {
        return rsx! {};
    }

    rsx! {
        button {
            class: "btn btn-outline",
            r#type: "button",
            onclick: move |_| {
                error.set(None);
                confirming.set(true);
            },
            {i18n.t(keys.button)}
        }
        if confirming() {
            ConfirmDialog {
                title: i18n.t(keys.title),
                message: i18n.t(keys.message),
                confirm_label: i18n.t(keys.confirm),
                dismiss_label: i18n.t(keys.dismiss),
                error: error(),
                busy: requesting(),
                on_confirm: confirm,
                on_cancel: move |_| confirming.set(false),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_can_be_cancelled_only_while_it_runs_unasked() {
        for running in [
            "queued",
            "staging",
            "parsing",
            "media",
            "database",
            "projections",
        ] {
            assert!(can_cancel(running), "{running}");
        }
        // Asked already, or over: a second request would change nothing.
        for over in ["cancelling", "cancelled", "completed", "failed"] {
            assert!(!can_cancel(over), "{over}");
        }
    }
}
