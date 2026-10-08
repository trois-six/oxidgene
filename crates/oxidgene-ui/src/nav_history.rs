//! The in-app back and forward history.
//!
//! [`NavHistory`] is the reader's path through the application as a browser
//! keeps it: the pages visited, where the reader stands among them, what
//! each page was about and the view each page left behind. [`AppHistory`]
//! keeps it in step with the router by standing in front of the platform's
//! own history, which still does the navigating: the browser's on the web,
//! so the in-app buttons and the browser's move through one stack, and the
//! renderer's in-memory one on the desktop, which has no browser chrome.

use std::any::Any;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use dioxus::history::History;
use dioxus::prelude::*;

use crate::router::Route;

/// How many pages the history remembers; going further drops the oldest.
pub const MAX_ENTRIES: usize = 50;

/// One page of the history.
pub struct Entry<R> {
    id: u64,
    route: R,
    /// What the page is about — a person, a couple, a dictionary entry, a
    /// tree — as the page itself named it from the data it shows.
    subject: Option<String>,
    /// The view the page left behind, for it to take up again on return.
    state: Option<Rc<dyn Any>>,
}

impl<R> Entry<R> {
    /// Identifies the entry for as long as it is in the history.
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn route(&self) -> &R {
        &self.route
    }

    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }
}

/// The pages visited, bounded to [`MAX_ENTRIES`], and the one shown.
///
/// A navigation the router makes is a [`push`](Self::push) or a
/// [`replace`](Self::replace); a move through the history is announced
/// with [`expect`](Self::expect) and confirmed by [`sync`](Self::sync) once
/// the platform has made it, since the browser makes it asynchronously.
pub struct NavHistory<R> {
    entries: VecDeque<Entry<R>>,
    index: usize,
    next_id: u64,
    /// The move announced and not yet seen, as a signed number of steps.
    expected: Option<isize>,
}

impl<R: PartialEq> NavHistory<R> {
    /// A history holding only the page shown.
    pub fn new(route: R) -> Self {
        let mut history = Self {
            entries: VecDeque::new(),
            index: 0,
            next_id: 0,
            expected: None,
        };
        let entry = history.entry(route);
        history.entries.push_back(entry);
        history
    }

    fn entry(&mut self, route: R) -> Entry<R> {
        self.next_id += 1;
        Entry {
            id: self.next_id,
            route,
            subject: None,
            state: None,
        }
    }

    pub fn current(&self) -> &Entry<R> {
        &self.entries[self.index]
    }

    pub fn can_go_back(&self) -> bool {
        self.index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.index + 1 < self.entries.len()
    }

    /// The pages before the current one, nearest first, each with the
    /// number of steps leading to it (`-1`, `-2`…).
    pub fn back_entries(&self) -> impl Iterator<Item = (isize, &Entry<R>)> {
        (0..self.index)
            .rev()
            .map(move |i| (i as isize - self.index as isize, &self.entries[i]))
    }

    /// The pages after the current one, nearest first, each with the
    /// number of steps leading to it (`1`, `2`…).
    pub fn forward_entries(&self) -> impl Iterator<Item = (isize, &Entry<R>)> {
        (self.index + 1..self.entries.len())
            .map(move |i| (i as isize - self.index as isize, &self.entries[i]))
    }

    /// Whether `delta` steps from the current page stays in the history.
    pub fn can_go(&self, delta: isize) -> bool {
        delta != 0 && self.target(delta).is_some()
    }

    fn target(&self, delta: isize) -> Option<usize> {
        self.index
            .checked_add_signed(delta)
            .filter(|&target| target < self.entries.len())
    }

    /// A new page: the pages ahead of the current one are dropped, and the
    /// oldest once the history is full. Opening the page already shown
    /// adds nothing, as the router's histories do.
    pub fn push(&mut self, route: R) {
        self.expected = None;
        if self.current().route == route {
            return;
        }
        self.entries.truncate(self.index + 1);
        let entry = self.entry(route);
        self.entries.push_back(entry);
        if self.entries.len() > MAX_ENTRIES {
            self.entries.pop_front();
        }
        self.index = self.entries.len() - 1;
    }

    /// The current page now shows `route`. What the page was about and
    /// the view it left are forgotten unless it still shows the same.
    pub fn replace(&mut self, route: R) {
        self.expected = None;
        let current = &mut self.entries[self.index];
        if current.route != route {
            current.route = route;
            current.subject = None;
            current.state = None;
        }
    }

    /// Announces a move of `delta` steps, which [`sync`](Self::sync)
    /// prefers when the page reached could be found at several places.
    pub fn expect(&mut self, delta: isize) {
        self.expected = Some(delta);
    }

    /// Moves `delta` steps, when the history reaches that far.
    pub fn go(&mut self, delta: isize) -> bool {
        match self.target(delta) {
            Some(target) => {
                self.index = target;
                true
            }
            None => false,
        }
    }

    /// The platform now shows `route`: the history moves to it — by the
    /// move announced if it leads there, else to the nearest page showing
    /// it, the previous ones first. A page it does not hold is a navigation
    /// it did not see, recorded as a new page.
    pub fn sync(&mut self, route: R) {
        if self.current().route == route {
            return;
        }
        let expected = self.expected.take().filter(|&delta| {
            self.target(delta)
                .is_some_and(|t| self.entries[t].route == route)
        });
        if let Some(delta) = expected.or_else(|| self.nearest(&route)) {
            self.go(delta);
        } else {
            self.push(route);
        }
    }

    fn nearest(&self, route: &R) -> Option<isize> {
        (1..self.entries.len() as isize)
            .flat_map(|step| [-step, step])
            .find(|&delta| {
                self.target(delta)
                    .is_some_and(|t| self.entries[t].route == *route)
            })
    }

    /// Names what the current page is about, when it still shows `route`.
    /// Returns whether that changed anything.
    pub fn set_subject(&mut self, route: &R, subject: Option<String>) -> bool {
        let current = &mut self.entries[self.index];
        if current.route != *route || current.subject == subject {
            return false;
        }
        current.subject = subject;
        true
    }

    /// Keeps the view the page of entry `id` left, if it is still held.
    pub fn set_state(&mut self, id: u64, state: Rc<dyn Any>) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.state = Some(state);
        }
    }

    /// The view the current page left when it was last shown, if it is of
    /// type `T`.
    pub fn current_state<T: Clone + 'static>(&self) -> Option<T> {
        self.current()
            .state
            .as_ref()
            .and_then(|state| state.downcast_ref::<T>())
            .cloned()
    }
}

/// What a browser tab's history is saved as, so a reload finds it again.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SavedHistory {
    /// Each page's route and subject, oldest first.
    pub entries: Vec<(String, Option<String>)>,
    pub index: usize,
}

impl<R: PartialEq + ToString + std::str::FromStr> NavHistory<R> {
    pub fn saved(&self) -> SavedHistory {
        SavedHistory {
            entries: self
                .entries
                .iter()
                .map(|entry| (entry.route.to_string(), entry.subject.clone()))
                .collect(),
            index: self.index,
        }
    }

    /// Takes up `saved` in place of a history that has not moved since it
    /// started, when both stand on the same page: the browser reloaded the
    /// page and kept its own history, which the saved one mirrors.
    pub fn restore(&mut self, saved: SavedHistory) -> bool {
        let routes: Option<Vec<R>> = saved
            .entries
            .iter()
            .map(|(route, _)| route.parse().ok())
            .collect();
        let Some(routes) = routes else {
            return false;
        };
        let fits = self.entries.len() == 1
            && saved.entries.len() <= MAX_ENTRIES
            && routes.get(saved.index) == Some(&self.current().route);
        if !fits {
            return false;
        }
        let subject = self.entries[0].subject.take();
        self.entries.clear();
        for (route, (_, saved_subject)) in routes.into_iter().zip(saved.entries) {
            let mut entry = self.entry(route);
            entry.subject = saved_subject;
            self.entries.push_back(entry);
        }
        self.index = saved.index;
        if subject.is_some() {
            self.entries[self.index].subject = subject;
        }
        true
    }
}

/// The platform's history with the application's [`NavHistory`] kept in
/// step with it.
///
/// Provided above the router in place of the platform's own, so every push,
/// replace and move the router makes goes through it. On the web, the
/// browser's buttons, shortcuts and history menu move without the router:
/// the browser tells the router, which reads the route again, and reading
/// it is when the history catches up.
pub struct AppHistory {
    inner: Rc<dyn History>,
    model: RefCell<NavHistory<Route>>,
    /// The platform's route as last read, so it is parsed once per change.
    seen: RefCell<String>,
    /// Re-renders what reads the route, as the platform does after a move.
    updater: RefCell<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl AppHistory {
    pub fn new(inner: Rc<dyn History>) -> Self {
        let seen = inner.current_route();
        let route = parse_route(&seen);
        Self {
            inner,
            model: RefCell::new(NavHistory::new(route)),
            seen: RefCell::new(seen),
            updater: RefCell::new(None),
        }
    }

    /// The history, caught up with the platform's current route.
    pub fn read<T>(&self, f: impl FnOnce(&NavHistory<Route>) -> T) -> T {
        self.observe();
        f(&self.model.borrow())
    }

    /// Moves `delta` steps through the history: one step as the platform's
    /// back and forward buttons do, several at once from the history menu.
    pub fn go(&self, delta: isize) {
        self.observe();
        if !self.model.borrow().can_go(delta) {
            return;
        }
        self.model.borrow_mut().expect(delta);
        self.move_platform(delta);
    }

    /// The browser moves itself and tells the router when it has.
    #[cfg(target_arch = "wasm32")]
    fn move_platform(&self, delta: isize) {
        match delta {
            -1 => self.inner.go_back(),
            1 => self.inner.go_forward(),
            _ => {
                document::eval(&format!("history.go({delta});"));
            }
        }
    }

    /// The in-memory history moves at once, a step at a time; the router is
    /// then told, as the browser would.
    #[cfg(not(target_arch = "wasm32"))]
    fn move_platform(&self, delta: isize) {
        for _ in 0..delta.unsigned_abs() {
            if delta < 0 {
                self.inner.go_back();
            } else {
                self.inner.go_forward();
            }
        }
        self.observe();
        self.notify();
    }

    /// Names what the current page, showing `route`, is about.
    pub fn set_subject(&self, route: &Route, subject: Option<String>) {
        self.observe();
        if self.model.borrow_mut().set_subject(route, subject) {
            self.save();
        }
    }

    pub fn current_id(&self) -> u64 {
        self.read(|history| history.current().id())
    }

    pub fn current_state<T: Clone + 'static>(&self) -> Option<T> {
        self.read(NavHistory::current_state)
    }

    pub fn set_state(&self, id: u64, state: Rc<dyn Any>) {
        self.model.borrow_mut().set_state(id, state);
    }

    /// Takes up the history saved before the browser reloaded the page.
    pub fn restore(&self, saved: SavedHistory) {
        self.observe();
        if self.model.borrow_mut().restore(saved) {
            self.notify();
        }
    }

    /// Catches up with the platform's route, which the browser may have
    /// changed on its own.
    fn observe(&self) {
        let raw = self.inner.current_route();
        if *self.seen.borrow() == raw {
            return;
        }
        self.model.borrow_mut().sync(parse_route(&raw));
        *self.seen.borrow_mut() = raw;
        self.save();
    }

    fn notify(&self) {
        let updater = self.updater.borrow().clone();
        if let Some(updater) = updater {
            updater();
        }
    }

    /// A browser tab keeps its history across a reload, so the titles and
    /// the place in it are kept with the tab, in its session storage.
    #[cfg(target_arch = "wasm32")]
    fn save(&self) {
        let saved = self.model.borrow().saved();
        let Ok(json) = serde_json::to_string(&saved) else {
            return;
        };
        let Ok(literal) = serde_json::to_string(&json) else {
            return;
        };
        document::eval(&format!(
            "try {{ sessionStorage.setItem('{SESSION_KEY}', {literal}); }} catch (e) {{}}"
        ));
    }

    /// The desktop history lives as long as the window, as this one does.
    #[cfg(not(target_arch = "wasm32"))]
    fn save(&self) {}
}

/// Where a browser tab keeps its history (see [`AppHistory`]).
#[cfg(target_arch = "wasm32")]
const SESSION_KEY: &str = "oxidgene.history";

fn parse_route(raw: &str) -> Route {
    raw.parse().unwrap_or(Route::Home {})
}

impl History for AppHistory {
    fn current_route(&self) -> String {
        self.observe();
        self.inner.current_route()
    }

    fn current_prefix(&self) -> Option<String> {
        self.inner.current_prefix()
    }

    fn can_go_back(&self) -> bool {
        self.read(NavHistory::can_go_back)
    }

    fn can_go_forward(&self) -> bool {
        self.read(NavHistory::can_go_forward)
    }

    fn go_back(&self) {
        self.go(-1);
    }

    fn go_forward(&self) {
        self.go(1);
    }

    fn push(&self, route: String) {
        self.observe();
        self.inner.push(route.clone());
        self.model.borrow_mut().push(parse_route(&route));
        self.observe();
        self.save();
    }

    fn replace(&self, path: String) {
        self.observe();
        self.inner.replace(path.clone());
        self.model.borrow_mut().replace(parse_route(&path));
        self.observe();
        self.save();
    }

    fn external(&self, url: String) -> bool {
        self.inner.external(url)
    }

    fn updater(&self, callback: Arc<dyn Fn() + Send + Sync>) {
        *self.updater.borrow_mut() = Some(callback.clone());
        self.inner.updater(callback);
    }

    fn include_prevent_default(&self) -> bool {
        self.inner.include_prevent_default()
    }
}

/// Installs the application's history in front of the platform's, for the
/// router below and for the pages; on the web, takes up what the tab's
/// history was before a reload; on the desktop, listens for the back and
/// forward shortcuts the browser would otherwise handle.
pub fn use_init_app_history() -> Rc<AppHistory> {
    let history = use_hook(|| {
        let history = Rc::new(AppHistory::new(dioxus::history::history()));
        provide_context(history.clone());
        dioxus::history::provide_history_context(history.clone());
        history
    });
    use_restored_history(history.clone());
    use_history_shortcuts(history.clone());
    history
}

#[cfg(target_arch = "wasm32")]
fn use_restored_history(history: Rc<AppHistory>) {
    use_future(move || {
        let history = history.clone();
        async move {
            let stored = document::eval(&format!(
                "try {{ return sessionStorage.getItem('{SESSION_KEY}'); }} catch (e) {{ return null; }}"
            ))
            .join::<Option<String>>()
            .await;
            let saved = stored
                .ok()
                .flatten()
                .and_then(|json| serde_json::from_str::<SavedHistory>(&json).ok());
            if let Some(saved) = saved {
                history.restore(saved);
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn use_restored_history(_history: Rc<AppHistory>) {}

/// The browser handles Alt+Left, Alt+Right and the mouse's back and forward
/// buttons itself, and must not move twice: only the desktop, whose window
/// has no browser around it, listens for them (`history_keys.js`).
#[cfg(not(target_arch = "wasm32"))]
fn use_history_shortcuts(history: Rc<AppHistory>) {
    use_future(move || {
        let history = history.clone();
        async move {
            let mut keys = document::eval(include_str!("history_keys.js"));
            while let Ok(direction) = keys.recv::<String>().await {
                match direction.as_str() {
                    "back" => history.go(-1),
                    "forward" => history.go(1),
                    _ => {}
                }
            }
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn use_history_shortcuts(_history: Rc<AppHistory>) {}

/// The application's history, when the page runs inside the shell.
pub fn use_app_history() -> Option<Rc<AppHistory>> {
    try_use_context::<Rc<AppHistory>>()
}

/// Names what the page is about in its history entry — a person's name, a
/// couple, a dictionary entry, a tree — from data the page already shows.
pub fn use_history_subject(subject: Option<String>) {
    let route = use_route::<Route>();
    if let Some(history) = use_app_history() {
        history.set_subject(&route, subject);
    }
}

/// The view a page left in its history entry, when the reader comes back
/// to it through the history; `None` on a page newly opened.
///
/// The page keeps its view there with [`use_saved_view`], so going back
/// finds the tab, filters and open entry the reader left rather than the
/// page's defaults.
pub fn use_restored_view<T: Clone + 'static>() -> Option<T> {
    use_hook(|| {
        try_consume_context::<Rc<AppHistory>>().and_then(|history| history.current_state::<T>())
    })
}

/// Keeps `view()` as the view of the history entry the page opened on,
/// whenever what it reads changes.
pub fn use_saved_view<T: 'static>(view: impl Fn() -> T + 'static) {
    let history = use_app_history();
    let id = use_hook(|| history.as_ref().map(|history| history.current_id()));
    use_effect(move || {
        let view = view();
        if let (Some(history), Some(id)) = (history.as_ref(), id) {
            history.set_state(id, Rc::new(view));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn routes(history: &NavHistory<&'static str>) -> Vec<&'static str> {
        history.entries.iter().map(|entry| entry.route).collect()
    }

    fn visited(pages: &[&'static str]) -> NavHistory<&'static str> {
        let mut history = NavHistory::new(pages[0]);
        for page in &pages[1..] {
            history.push(page);
        }
        history
    }

    #[test]
    fn a_new_history_goes_nowhere() {
        let history = NavHistory::new("/");
        assert!(!history.can_go_back());
        assert!(!history.can_go_forward());
        assert!(!history.can_go(-1));
        assert!(!history.can_go(0));
    }

    #[test]
    fn pushing_moves_forward_and_opens_the_way_back() {
        let history = visited(&["/", "/trees/a", "/trees/a/dictionary"]);
        assert_eq!(routes(&history), ["/", "/trees/a", "/trees/a/dictionary"]);
        assert_eq!(*history.current().route(), "/trees/a/dictionary");
        assert!(history.can_go_back());
        assert!(!history.can_go_forward());
    }

    #[test]
    fn opening_the_page_shown_adds_nothing() {
        let mut history = visited(&["/", "/trees/a"]);
        history.push("/trees/a");
        assert_eq!(routes(&history), ["/", "/trees/a"]);
    }

    #[test]
    fn going_back_and_forward_moves_one_page_at_a_time() {
        let mut history = visited(&["/", "/trees/a", "/trees/a/dictionary"]);
        assert!(history.go(-1));
        assert_eq!(*history.current().route(), "/trees/a");
        assert!(history.can_go_forward());
        assert!(history.go(1));
        assert_eq!(*history.current().route(), "/trees/a/dictionary");
        assert!(!history.go(1));
        assert_eq!(*history.current().route(), "/trees/a/dictionary");
    }

    #[test]
    fn a_jump_moves_several_pages_and_never_past_either_end() {
        let mut history = visited(&["/", "/a", "/b", "/c"]);
        assert!(history.can_go(-3));
        assert!(!history.can_go(-4));
        assert!(history.go(-3));
        assert_eq!(*history.current().route(), "/");
        assert!(!history.go(-1));
        assert!(history.go(2));
        assert_eq!(*history.current().route(), "/b");
        assert!(!history.go(2));
    }

    #[test]
    fn the_lists_run_nearest_first_with_their_steps() {
        let mut history = visited(&["/", "/a", "/b", "/c", "/d"]);
        history.go(-2);
        let back: Vec<_> = history
            .back_entries()
            .map(|(d, e)| (d, *e.route()))
            .collect();
        assert_eq!(back, [(-1, "/a"), (-2, "/")]);
        let forward: Vec<_> = history
            .forward_entries()
            .map(|(d, e)| (d, *e.route()))
            .collect();
        assert_eq!(forward, [(1, "/c"), (2, "/d")]);
    }

    #[test]
    fn a_new_page_after_going_back_drops_the_pages_ahead() {
        let mut history = visited(&["/", "/a", "/b", "/c"]);
        history.go(-2);
        history.push("/z");
        assert_eq!(routes(&history), ["/", "/a", "/z"]);
        assert!(!history.can_go_forward());
    }

    #[test]
    fn the_history_keeps_only_the_latest_pages() {
        let mut history = NavHistory::new(0);
        for page in 1..=(MAX_ENTRIES + 10) {
            history.push(page);
        }
        assert_eq!(history.entries.len(), MAX_ENTRIES);
        assert_eq!(*history.current().route(), MAX_ENTRIES + 10);
        assert_eq!(*history.entries[0].route(), 11);
        assert!(history.can_go(1 - MAX_ENTRIES as isize));
        assert!(!history.can_go(-(MAX_ENTRIES as isize)));
    }

    #[test]
    fn replacing_keeps_the_place_and_forgets_another_pages_subject() {
        let mut history = visited(&["/", "/a", "/b"]);
        history.go(-1);
        assert!(history.set_subject(&"/a", Some("Alpha".into())));
        history.replace("/a");
        assert_eq!(history.current().subject(), Some("Alpha"));
        history.replace("/a2");
        assert_eq!(routes(&history), ["/", "/a2", "/b"]);
        assert_eq!(history.current().subject(), None);
        assert!(history.can_go_forward());
    }

    #[test]
    fn a_subject_is_given_only_to_the_page_that_names_it() {
        let mut history = visited(&["/", "/a"]);
        assert!(!history.set_subject(&"/", Some("Home".into())));
        assert!(history.set_subject(&"/a", Some("Alpha".into())));
        assert!(!history.set_subject(&"/a", Some("Alpha".into())));
        assert_eq!(history.current().subject(), Some("Alpha"));
    }

    #[test]
    fn sync_follows_the_announced_move_first() {
        // The same page on both sides: only the announcement tells them apart.
        let mut history = visited(&["/p", "/d", "/p"]);
        history.go(-1);
        history.expect(1);
        history.sync("/p");
        assert_eq!(history.index, 2);

        history.go(-1);
        history.expect(-1);
        history.sync("/p");
        assert_eq!(history.index, 0);
    }

    #[test]
    fn sync_keeps_the_announcement_until_the_platform_moves() {
        let mut history = visited(&["/p", "/d", "/p"]);
        history.go(-1);
        history.expect(1);
        // The browser has not moved yet: the route read is still the same.
        history.sync("/d");
        history.sync("/p");
        assert_eq!(history.index, 2);
    }

    #[test]
    fn sync_finds_a_move_made_by_the_browser_alone() {
        let mut history = visited(&["/", "/a", "/b", "/c"]);
        history.sync("/a");
        assert_eq!(*history.current().route(), "/a");
        history.sync("/c");
        assert_eq!(*history.current().route(), "/c");
        assert_eq!(history.entries.len(), 4);
    }

    #[test]
    fn sync_records_a_page_it_never_saw() {
        let mut history = visited(&["/", "/a"]);
        history.sync("/elsewhere");
        assert_eq!(routes(&history), ["/", "/a", "/elsewhere"]);
    }

    #[test]
    fn a_view_belongs_to_its_entry() {
        let mut history = visited(&["/", "/d"]);
        let id = history.current().id();
        history.set_state(id, Rc::new(3_u8));
        assert_eq!(history.current_state::<u8>(), Some(3));
        assert_eq!(history.current_state::<String>(), None);
        history.push("/p");
        assert_eq!(history.current_state::<u8>(), None);
        history.go(-1);
        assert_eq!(history.current_state::<u8>(), Some(3));
        // A new page in its place starts afresh.
        history.go(-1);
        history.push("/d");
        assert_eq!(history.current_state::<u8>(), None);
    }

    /// On the desktop, the window's in-memory history and this one move as
    /// one, whether the router or the history buttons move them.
    #[test]
    fn the_platform_history_and_the_model_move_together() {
        use dioxus::history::MemoryHistory;
        let history = AppHistory::new(Rc::new(MemoryHistory::default()));
        for route in ["/trees/t", "/trees/t/dictionary", "/trees/t/persons/p"] {
            History::push(&history, route.to_string());
        }
        History::replace(&history, "/trees/t/persons/q".to_string());
        assert_eq!(history.current_route(), "/trees/t/persons/q");
        history.go(-2);
        assert_eq!(history.current_route(), "/trees/t");
        assert!(History::can_go_back(&history) && History::can_go_forward(&history));
        let forward: Vec<_> = history.read(|h| {
            h.forward_entries()
                .map(|(_, entry)| entry.route().to_string())
                .collect()
        });
        assert_eq!(forward, ["/trees/t/dictionary", "/trees/t/persons/q"]);
        History::go_forward(&history);
        assert_eq!(history.current_route(), "/trees/t/dictionary");
        // Out of reach: nothing moves.
        history.go(5);
        assert_eq!(history.current_route(), "/trees/t/dictionary");
        History::push(&history, "/trees/t/tools".to_string());
        assert!(!History::can_go_forward(&history));
        history.go(-3);
        assert_eq!(history.current_route(), "/");
    }

    #[test]
    fn a_reload_takes_up_the_saved_history_where_it_stood() {
        let mut before = NavHistory::new("/".to_string());
        before.push("/trees/a".into());
        before.set_subject(&"/trees/a".into(), Some("Tree A".into()));
        before.push("/trees/a/dictionary".into());
        before.go(-1);
        let saved = before.saved();

        let mut reloaded = NavHistory::new("/trees/a".to_string());
        assert!(reloaded.restore(saved.clone()));
        assert_eq!(reloaded.saved(), saved);
        assert!(reloaded.can_go_back() && reloaded.can_go_forward());

        // Somewhere else, or after moving, the saved history does not apply.
        let mut elsewhere = NavHistory::new("/trees/b".to_string());
        assert!(!elsewhere.restore(saved.clone()));
        let mut moved = NavHistory::new("/".to_string());
        moved.push("/trees/a".into());
        assert!(!moved.restore(saved));
    }
}
