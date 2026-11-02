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

/// The bytes the standard library's hash table of `T` entries allocates for
/// a reported `capacity`: one slot of `T` and one control byte per bucket,
/// plus a group of control bytes past the end. Its buckets are a power of
/// two holding at most seven eighths of them (or one less than their count
/// below eight), which is how `capacity` reports them. What a cache counts
/// a set or a map it keeps as.
#[must_use]
pub fn table_bytes<T>(capacity: usize) -> usize {
    /// The control bytes past the last bucket: one SIMD group.
    const GROUP: usize = 16;
    if capacity == 0 {
        return 0;
    }
    let buckets = if capacity < 8 {
        capacity + 1
    } else {
        capacity / 7 * 8
    };
    buckets * (size_of::<T>() + 1) + GROUP
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tables_bytes_follow_its_buckets() {
        assert_eq!(table_bytes::<u64>(0), 0);
        let set: std::collections::HashSet<u64> = (0..1000).collect();
        // 1,000 entries take 2,048 buckets, reported as a capacity of 1,792.
        let buckets = set.capacity() / 7 * 8;
        assert!(buckets.is_power_of_two(), "{}", set.capacity());
        assert_eq!(table_bytes::<u64>(set.capacity()), buckets * 9 + 16);
    }

    #[test]
    fn items_come_back_sorted_and_once() {
        assert_eq!(sorted_unique([3, 1, 2, 3, 1]), vec![1, 2, 3]);
        assert_eq!(sorted_unique(Vec::<u8>::new()), Vec::<u8>::new());
    }
}
