//! Opening an archive that comes from outside within bounds.
//!
//! A ZIP reader learns what an archive holds from its directory, the list
//! of entries at its end, and keeps one record in memory per entry. An
//! archive can be all directory: a gigabyte of it describes twenty million
//! empty entries, and opening it takes gigabytes of memory before a single
//! entry is read. Checking the number of entries once the archive is open
//! is too late, and reading the count the archive declares ahead of the
//! reader is a second reader, which a crafted archive makes disagree with
//! the first.
//!
//! So the archive is opened through a [`Budgeted`] source, which lets the
//! reader take only so many bytes while it reads the directory: what
//! [`directory_budget`] allows for the entries the caller accepts. Whatever
//! the reader does with what it finds, it cannot have built more records
//! than those bytes describe. The caller then counts the entries exactly,
//! and lifts the budget before reading them.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The bytes of directory allowed per accepted entry: its fixed 46 bytes,
/// a name of some sixty bytes, and the extra fields writers add.
const DIRECTORY_BYTES_PER_ENTRY: u64 = 160;

/// What finding the directory may read besides: the end of the archive,
/// where its record and an archive comment of up to 64 KiB lie.
const DIRECTORY_SEARCH_BYTES: u64 = 256 * 1024;

/// The bytes opening an archive of at most `entries` entries may read.
///
/// An entry cannot be described in fewer than 47 bytes, so an archive
/// opened within this budget yields at most three and a half times
/// `entries` records before the caller counts them.
#[must_use]
pub fn directory_budget(entries: usize) -> u64 {
    u64::try_from(entries)
        .unwrap_or(u64::MAX)
        .saturating_mul(DIRECTORY_BYTES_PER_ENTRY)
        .saturating_add(DIRECTORY_SEARCH_BYTES)
}

/// How many bytes may still be read through a [`Budgeted`] source.
#[derive(Debug, Clone)]
pub struct Budget {
    /// `u64::MAX` once lifted.
    remaining: Arc<AtomicU64>,
    spent: Arc<AtomicBool>,
}

impl Budget {
    #[must_use]
    pub fn new(bytes: u64) -> Self {
        Self {
            remaining: Arc::new(AtomicU64::new(bytes)),
            spent: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Let the source be read without a bound from now on.
    pub fn lift(&self) {
        self.remaining.store(u64::MAX, Ordering::Relaxed);
    }

    /// Whether a read was refused for want of budget: why opening the
    /// archive failed, whatever error its reader made of it.
    #[must_use]
    pub fn is_spent(&self) -> bool {
        self.spent.load(Ordering::Relaxed)
    }
}

/// A source read within a [`Budget`]; seeking costs nothing.
#[derive(Debug)]
pub struct Budgeted<R> {
    inner: R,
    budget: Budget,
}

impl<R> Budgeted<R> {
    pub fn new(inner: R, budget: Budget) -> Self {
        Self { inner, budget }
    }
}

impl<R: Read> Read for Budgeted<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = self.budget.remaining.load(Ordering::Relaxed);
        if remaining == u64::MAX {
            return self.inner.read(buffer);
        }
        if remaining == 0 && !buffer.is_empty() {
            self.budget.spent.store(true, Ordering::Relaxed);
            return Err(io::Error::other(
                "the archive's directory is larger than the entries accepted",
            ));
        }
        let allowed = usize::try_from(remaining)
            .unwrap_or(usize::MAX)
            .min(buffer.len());
        let read = self.inner.read(&mut buffer[..allowed])?;
        self.budget
            .remaining
            .store(remaining - read as u64, Ordering::Relaxed);
        Ok(read)
    }
}

impl<R: Seek> Seek for Budgeted<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn a_source_is_read_within_its_budget_and_freely_once_lifted() {
        let budget = Budget::new(5);
        let mut source = Budgeted::new(Cursor::new(b"0123456789".to_vec()), budget.clone());
        let mut buffer = [0u8; 4];
        assert_eq!(source.read(&mut buffer).unwrap(), 4);
        // The last byte of the budget, then nothing more.
        assert_eq!(source.read(&mut buffer).unwrap(), 1);
        assert!(!budget.is_spent());
        assert!(source.read(&mut buffer).is_err());
        assert!(budget.is_spent());
        // Seeking is free, and a lifted budget bounds nothing.
        source.rewind().unwrap();
        budget.lift();
        let mut all = Vec::new();
        source.read_to_end(&mut all).unwrap();
        assert_eq!(all, b"0123456789");
    }

    #[test]
    fn the_budget_grows_with_the_entries_accepted() {
        assert_eq!(directory_budget(0), DIRECTORY_SEARCH_BYTES);
        assert_eq!(
            directory_budget(100_000),
            DIRECTORY_SEARCH_BYTES + 16_000_000
        );
        assert_eq!(directory_budget(usize::MAX), u64::MAX);
    }
}
