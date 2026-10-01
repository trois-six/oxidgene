//! Read-only data shared by handle between the components that read it.

use std::sync::Arc;

/// A value shared between a page, the handlers that read it and the
/// components that draw it, compared by identity.
///
/// Large read models — a pedigree, the portraits of its persons, the SOSA
/// root's ancestor set — are read from many closures and passed down as
/// props. Handing each of them an owned copy meant rebuilding all of it on
/// every render, including the renders that only opened a context menu.
///
/// Equality is identity, which is what makes it a usable prop: assembling the
/// value again produces a new handle and redraws what reads it, while a render
/// that changed nothing passes the same handle and does not. Comparing the
/// contents instead would cost as much as copying them.
#[derive(Debug, Default)]
pub struct Shared<T>(Arc<T>);

impl<T> Shared<T> {
    #[must_use]
    pub fn new(value: T) -> Self {
        Self(Arc::new(value))
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T> PartialEq for Shared<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> std::ops::Deref for Shared<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> From<T> for Shared<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::Shared;

    #[test]
    fn equality_is_identity_not_content() {
        let a = Shared::new(vec![1, 2, 3]);
        let b = Shared::new(vec![1, 2, 3]);
        assert!(a == a.clone());
        assert!(a != b);
        assert_eq!(*a, *b);
    }
}
