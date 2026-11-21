//! Toasts (`docs/ui-common.md` §4.21): the transient outcome of an
//! operation — a record saved, a tree deleted, the network back — said in a
//! corner of the window and gone by itself.
//!
//! One queue for the application, created by the shell
//! ([`use_init_toasts`]); any component raises a toast through the handle
//! [`use_toasts`] gives. What a form needs to be completed is never a
//! toast: a failed save keeps its inline error and the user's input.

use std::sync::atomic::{AtomicU64, Ordering};

use dioxus::prelude::*;

use crate::i18n::use_i18n;

/// The most toasts shown at once: one more pushes the oldest out.
pub const MAX_TOASTS: usize = 4;

/// Whether the operation succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Error,
}

impl ToastKind {
    /// How long the toast stays, in milliseconds: a failure longer, since
    /// it is the one the reader has to act on.
    const fn lifetime_ms(self) -> u32 {
        match self {
            Self::Success => 5_000,
            Self::Error => 10_000,
        }
    }

    const fn class(self) -> &'static str {
        match self {
            Self::Success => "toast-success",
            Self::Error => "toast-error",
        }
    }
}

/// One toast on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    /// What happened, already translated.
    pub text: String,
}

/// The toasts on screen, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToastQueue {
    next_id: u64,
    toasts: Vec<Toast>,
}

impl ToastQueue {
    /// Add a toast and return its id. The same outcome said again replaces
    /// its earlier toast rather than stacking under it, and the oldest
    /// toasts leave once there are more than [`MAX_TOASTS`].
    pub fn push(&mut self, kind: ToastKind, text: String) -> u64 {
        self.toasts
            .retain(|toast| toast.kind != kind || toast.text != text);
        let id = self.next_id;
        self.next_id += 1;
        self.toasts.push(Toast { id, kind, text });
        let excess = self.toasts.len().saturating_sub(MAX_TOASTS);
        self.toasts.drain(..excess);
        id
    }

    /// Remove toast `id`; whether it was still there.
    pub fn dismiss(&mut self, id: u64) -> bool {
        let before = self.toasts.len();
        self.toasts.retain(|toast| toast.id != id);
        self.toasts.len() != before
    }

    pub fn toasts(&self) -> &[Toast] {
        &self.toasts
    }

    fn contains(&self, id: u64) -> bool {
        self.toasts.iter().any(|toast| toast.id == id)
    }
}

/// The handle a component raises toasts through.
#[derive(Clone, Copy)]
pub struct Toasts {
    queue: Signal<ToastQueue>,
    /// The outlets mounted, in the order they were: the last one is the
    /// dialog opened last, or the shell's when no dialog is open.
    outlets: Signal<Vec<u64>>,
}

impl Toasts {
    /// Say that an operation succeeded; `text` is already translated.
    pub fn success(self, text: impl Into<String>) {
        self.push(ToastKind::Success, text.into());
    }

    /// Say that an operation failed; `text` is already translated.
    pub fn error(self, text: impl Into<String>) {
        self.push(ToastKind::Error, text.into());
    }

    fn push(mut self, kind: ToastKind, text: String) {
        let id = self.queue.write().push(kind, text);
        // On the application's own scope: the component that raised the
        // toast is often a dialog closing as it does, and its tasks end
        // with it.
        dioxus::core::spawn_forever(async move {
            crate::utils::sleep_ms(kind.lifetime_ms()).await;
            self.dismiss(id);
        });
    }

    /// Take toast `id` off the screen.
    pub fn dismiss(mut self, id: u64) {
        if self.queue.peek().contains(id) {
            self.queue.write().dismiss(id);
        }
    }
}

/// Create the application's toast queue: called once, by the shell.
pub fn use_init_toasts() -> Toasts {
    use_context_provider(|| Toasts {
        queue: Signal::new(ToastQueue::default()),
        outlets: Signal::new(Vec::new()),
    })
}

/// The handle to raise a toast with.
pub fn use_toasts() -> Toasts {
    use_context()
}

/// Say that an operation succeeded, from an event handler or a task, where
/// no hook can be called; `text` is already translated. Nothing is said
/// below no shell.
pub fn success(text: impl Into<String>) {
    if let Some(toasts) = try_consume_context::<Toasts>() {
        toasts.success(text);
    }
}

/// Say that an operation failed, from an event handler or a task; `text` is
/// already translated.
pub fn error(text: impl Into<String>) {
    if let Some(toasts) = try_consume_context::<Toasts>() {
        toasts.error(text);
    }
}

/// Where the toasts are drawn: an outlet in the shell, and one in every
/// open dialog (`Modal`), since everything outside an open dialog is inert
/// and painted under its backdrop. The outlet mounted last draws the queue
/// — the dialog opened last, or the shell's when none is open — and the
/// others stay empty.
///
/// The list is a polite live region that stays mounted while empty, so a
/// toast added to it is announced; a failure is an `alert` of its own.
#[component]
pub fn ToastOutlet() -> Element {
    static NEXT_OUTLET: AtomicU64 = AtomicU64::new(0);
    // Absent only below no shell, where nothing raises a toast either.
    let toasts = try_use_context::<Toasts>();
    let i18n = use_i18n();
    let id = use_hook(|| NEXT_OUTLET.fetch_add(1, Ordering::Relaxed));
    // Registered once mounted, as the dialog opens; the shell's outlet goes
    // with the application, whose queue may be gone before it.
    use_effect(move || {
        if let Some(mut toasts) = toasts {
            toasts.outlets.write().push(id);
        }
    });
    use_drop(move || {
        if let Some(mut toasts) = toasts
            && let Ok(mut outlets) = toasts.outlets.try_write()
        {
            outlets.retain(|outlet| *outlet != id);
        }
    });
    let Some(toasts) = toasts else {
        return rsx! {};
    };
    let dismiss_label = i18n.t("toast.dismiss");
    let queue = toasts.queue.read();
    let shown: &[Toast] = if toasts.outlets.read().last() == Some(&id) {
        queue.toasts()
    } else {
        &[]
    };
    rsx! {
        div {
            class: "toast-outlet",
            role: "status",
            "aria-live": "polite",
            // A press on a toast drawn in a dialog is not one on its backdrop.
            onmousedown: move |event| event.stop_propagation(),
            for toast in shown.iter() {
                div {
                    key: "{toast.id}",
                    class: "toast {toast.kind.class()}",
                    role: if toast.kind == ToastKind::Error { "alert" },
                    span { class: "toast-mark", "aria-hidden": "true" }
                    span { class: "toast-text", "{toast.text}" }
                    button {
                        r#type: "button",
                        class: "toast-dismiss",
                        "aria-label": "{dismiss_label}",
                        title: "{dismiss_label}",
                        onclick: {
                            let id = toast.id;
                            move |_| toasts.dismiss(id)
                        },
                        "×"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(queue: &ToastQueue) -> Vec<&str> {
        queue
            .toasts()
            .iter()
            .map(|toast| toast.text.as_str())
            .collect()
    }

    #[test]
    fn the_queue_is_bounded_and_drops_its_oldest_toast() {
        let mut queue = ToastQueue::default();
        for n in 0..=MAX_TOASTS {
            queue.push(ToastKind::Success, format!("saved {n}"));
        }
        assert_eq!(queue.toasts().len(), MAX_TOASTS);
        assert_eq!(texts(&queue), ["saved 1", "saved 2", "saved 3", "saved 4"]);
    }

    #[test]
    fn the_same_outcome_said_again_replaces_its_toast() {
        let mut queue = ToastQueue::default();
        let first = queue.push(ToastKind::Error, "unreachable".to_string());
        queue.push(ToastKind::Success, "saved".to_string());
        let again = queue.push(ToastKind::Error, "unreachable".to_string());
        assert_ne!(first, again);
        assert_eq!(texts(&queue), ["saved", "unreachable"]);
        // The first toast's timer finds nothing left to remove.
        assert!(!queue.dismiss(first));
        assert_eq!(queue.toasts().len(), 2);
        // The same words for the other outcome are another toast.
        queue.push(ToastKind::Success, "unreachable".to_string());
        assert_eq!(queue.toasts().len(), 3);
    }

    #[test]
    fn dismissing_removes_one_toast_and_ids_are_never_reused() {
        let mut queue = ToastQueue::default();
        let first = queue.push(ToastKind::Success, "one".to_string());
        let second = queue.push(ToastKind::Success, "two".to_string());
        assert!(queue.dismiss(first));
        assert!(!queue.dismiss(first));
        assert_eq!(texts(&queue), ["two"]);
        let third = queue.push(ToastKind::Success, "three".to_string());
        assert!(third > second);
    }

    #[test]
    fn a_failure_stays_longer_than_a_success() {
        assert!(ToastKind::Error.lifetime_ms() > ToastKind::Success.lifetime_ms());
    }
}
