//! How the process gives back the memory its media work used.
//!
//! An import reads files of several megabytes and decodes images of tens of
//! megabytes, on a dozen threads at once. glibc's `malloc` maps a block that
//! large on its own at first, but each time one is freed it raises that
//! threshold to the block's size (up to 32 MiB), and the threshold at which
//! it trims an arena's heap with it. From then on such buffers come out of
//! the per-thread arenas, and what they free stays there: `malloc_trim`
//! hands back free pages inside an arena's heap, but never the free space at
//! the top of a thread's own arena. On a reference tree of 10,000 people and
//! 632 pictures, that is what made a Geneanet import peak at 1.8 to 2.0 GiB
//! and leave the process holding 1.2 to 1.4 GiB once it had finished.
//!
//! [`tune`] pins the threshold, which also pins the trimming one at its
//! 128 KiB default: every block of [`MMAP_THRESHOLD`] or more is mapped when
//! allocated and unmapped when freed, and an arena's free top goes back as it
//! frees. [`release_free_memory`] then hands back what the arenas still hold
//! after a background job. The same import peaks under 600 MiB and leaves
//! 70 MiB. Elsewhere — macOS, Windows, musl — both do nothing: those
//! allocators return large blocks on their own.

/// Allocations of this size or more are mapped, and unmapped when freed.
///
/// A mebibyte is far above what the database layer and the request handlers
/// allocate, so their small allocations keep the arenas' speed, and below a
/// photograph's file or its decoded pixels, which no longer outlive their
/// use. Mapping and trimming cost system calls and fresh pages: on the
/// reference tree, a few seconds of system time over a whole import and a
/// GEDZIP round trip, within the noise of their wall time. A threshold of
/// 4 MiB, or a trimming threshold of 8 MiB, saved some of those seconds but
/// let the import peak 75 to 150 MiB higher.
pub const MMAP_THRESHOLD: usize = 1024 * 1024;

/// Pin the allocator's mapping threshold at [`MMAP_THRESHOLD`]. Each binary
/// calls it first thing in `main`; whether it applied.
pub fn tune() -> bool {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        let threshold = libc::c_int::try_from(MMAP_THRESHOLD).unwrap_or(libc::c_int::MAX);
        // SAFETY: `mallopt` only sets an allocator parameter, under the
        // allocator's own lock; it touches no memory of the caller's.
        unsafe { libc::mallopt(libc::M_MMAP_THRESHOLD, threshold) == 1 }
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        false
    }
}

/// Return the free memory the allocator's arenas hold to the system.
///
/// Called after each background job, which is when a process has just
/// stopped needing the most memory it will hold. It costs a walk over the
/// arenas, milliseconds against a job's seconds.
pub fn release_free_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: `malloc_trim` only releases pages the allocator holds free,
    // under its own locks; it touches no memory of the caller's.
    unsafe {
        libc::malloc_trim(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    fn glibc_takes_the_mapping_threshold() {
        assert!(tune());
        release_free_memory();
    }
}
