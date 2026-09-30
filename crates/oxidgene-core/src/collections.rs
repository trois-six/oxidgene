//! Small collection helpers every layer reaches for.

/// The items in ascending order, each once.
///
/// What a batch read wants from ids gathered from several relations: a person
/// reached as a spouse and as a parent is asked for once, and the sorted order
/// keeps the queries and their results deterministic.
pub fn sorted_unique<T: Ord>(items: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut items: Vec<T> = items.into_iter().collect();
    items.sort_unstable();
    items.dedup();
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_come_back_sorted_and_once() {
        assert_eq!(sorted_unique([3, 1, 2, 3, 1]), vec![1, 2, 3]);
        assert_eq!(sorted_unique(Vec::<u8>::new()), Vec::<u8>::new());
    }
}
