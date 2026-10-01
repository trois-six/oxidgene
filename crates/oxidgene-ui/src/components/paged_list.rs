//! A cursor-paginated list read a page at a time: the first page as soon as
//! it is shown, the next ones when the reader asks for more.

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use dioxus::prelude::*;
use oxidgene_core::types::{Connection, PageInfo};

use crate::ui_observability::use_ui_resource;

type PageFuture<T> = Pin<Box<dyn Future<Output = Result<Connection<T>, String>>>>;

/// A list whose first page loads with the view and whose next pages load on
/// demand ("Load more"), as [`use_paged_list`] builds it.
pub struct PagedList<T: 'static> {
    /// The first page: `None` while it loads, its error if it failed.
    pub first: Resource<Result<Connection<T>, String>>,
    more: Signal<Vec<T>>,
    next_cursor: Signal<Option<String>>,
    loading_more: Signal<bool>,
    fetch: Rc<dyn Fn(Option<String>) -> PageFuture<T>>,
}

impl<T: 'static> Clone for PagedList<T> {
    fn clone(&self) -> Self {
        Self {
            first: self.first,
            more: self.more,
            next_cursor: self.next_cursor,
            loading_more: self.loading_more,
            fetch: Rc::clone(&self.fetch),
        }
    }
}

impl<T: Clone + 'static> PagedList<T> {
    /// Every item loaded so far: the first page's, then the next pages'.
    pub fn items(&self) -> Vec<T> {
        match &*self.first.read() {
            Some(Ok(page)) => page
                .edges
                .iter()
                .map(|edge| edge.node.clone())
                .chain(self.more.read().iter().cloned())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Whether a page is left to load.
    pub fn has_more(&self) -> bool {
        self.next_cursor.read().is_some()
    }

    /// Whether the next page is on its way.
    pub fn loading_more(&self) -> bool {
        (self.loading_more)()
    }

    /// Loads the next page, once at a time; a failure leaves the list as it
    /// was, the button there to try again.
    pub fn load_more(&self) {
        let Some(cursor) = self.next_cursor.peek().clone() else {
            return;
        };
        if *self.loading_more.peek() {
            return;
        }
        let (mut more, mut next_cursor, mut loading_more) =
            (self.more, self.next_cursor, self.loading_more);
        let page = (self.fetch)(Some(cursor));
        loading_more.set(true);
        spawn(async move {
            if let Ok(page) = page.await {
                next_cursor.set(page_end(&page.page_info));
                more.write()
                    .extend(page.edges.into_iter().map(|edge| edge.node));
            }
            loading_more.set(false);
        });
    }
}

/// The cursor of the page after `page_info`'s, if there is one.
fn page_end(page_info: &PageInfo) -> Option<String> {
    page_info
        .has_next_page
        .then(|| page_info.end_cursor.clone())
        .flatten()
}

/// Reads a list a page at a time through `fetch`, which takes the cursor of
/// the page wanted (`None` for the first).
///
/// The first page is a resource named `name` in the page's load trace: the
/// signals `fetch` reads before its future starts are its dependencies, and
/// a new first page starts the list over.
pub fn use_paged_list<T, F, Fut>(name: &'static str, fetch: F) -> PagedList<T>
where
    T: Clone + 'static,
    F: Fn(Option<String>) -> Fut + 'static,
    Fut: Future<Output = Result<Connection<T>, String>> + 'static,
{
    let fetch: Rc<dyn Fn(Option<String>) -> PageFuture<T>> =
        use_hook(|| Rc::new(move |cursor| Box::pin(fetch(cursor)) as PageFuture<T>));
    let mut more = use_signal(Vec::<T>::new);
    let mut next_cursor = use_signal(|| None::<String>);
    let loading_more = use_signal(|| false);
    let first_fetch = Rc::clone(&fetch);
    let first = use_ui_resource(name, move || first_fetch(None));
    use_effect(move || {
        if let Some(Ok(page)) = &*first.read() {
            more.set(Vec::new());
            next_cursor.set(page_end(&page.page_info));
        }
    });
    PagedList {
        first,
        more,
        next_cursor,
        loading_more,
        fetch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_page_starts_where_a_page_says_more_follow() {
        let page = |has_next_page, end_cursor: Option<&str>| PageInfo {
            has_next_page,
            end_cursor: end_cursor.map(str::to_string),
        };
        assert_eq!(page_end(&page(true, Some("c"))), Some("c".to_string()));
        assert_eq!(page_end(&page(false, Some("c"))), None);
        assert_eq!(page_end(&page(true, None)), None);
    }
}
