//! Global tree cache — survives navigation between sibling routes.
//!
//! Provided once via [`use_init_tree_cache`] in the Layout component.
//! Consumed in child pages via [`use_tree_cache`].

use dioxus::prelude::*;
use uuid::Uuid;

use crate::api::{ApiClient, ApiError, TreeSnapshot};
use oxidgene_core::types::Tree;

// ─── Public context type ────────────────────────────────────────────

/// Shared tree cache stored in Dioxus context.
///
/// Holds the most recently loaded tree metadata + snapshot.  When a page
/// needs the data for the same `tree_id` that is already cached, it can
/// read from the signals immediately (no HTTP round-trip, no loading
/// spinner).
#[derive(Clone, Copy)]
pub struct TreeCache {
    tree_id: Signal<Option<Uuid>>,
    tree: Signal<Option<Tree>>,
    snapshot: Signal<Option<TreeSnapshot>>,
    /// Monotonically increasing counter — bump to force a re-fetch.
    generation: Signal<u64>,
}

impl TreeCache {
    /// Return the cached tree if it matches `tid`, otherwise `None`.
    pub fn tree(&self, tid: Uuid) -> Option<Tree> {
        if *self.tree_id.read() == Some(tid) {
            self.tree.read().clone()
        } else {
            None
        }
    }

    /// Return the cached snapshot if it matches `tid`, otherwise `None`.
    pub fn snapshot(&self, tid: Uuid) -> Option<TreeSnapshot> {
        if *self.tree_id.read() == Some(tid) {
            self.snapshot.read().clone()
        } else {
            None
        }
    }

    /// Store freshly fetched data into the cache.
    pub fn store_tree(&self, tid: Uuid, tree: Tree) {
        let mut id = self.tree_id;
        let mut t = self.tree;
        id.set(Some(tid));
        t.set(Some(tree));
    }

    /// Store freshly fetched snapshot into the cache.
    pub fn store_snapshot(&self, tid: Uuid, snapshot: TreeSnapshot) {
        let mut id = self.tree_id;
        let mut s = self.snapshot;
        id.set(Some(tid));
        s.set(Some(snapshot));
    }

    /// Invalidate the cache and bump the generation counter so that
    /// any reactive `use_resource` that reads `generation()` will re-run.
    pub fn invalidate(&self) {
        let mut id = self.tree_id;
        let mut t = self.tree;
        let mut s = self.snapshot;
        let mut generation = self.generation;
        id.set(None);
        t.set(None);
        s.set(None);
        let next = *generation.peek() + 1;
        generation.set(next);
    }

    /// Current generation — include this in `use_resource` dependencies
    /// so the resource re-runs after [`invalidate`].
    pub fn generation(&self) -> u64 {
        *self.generation.read()
    }
}

// ─── Hooks ──────────────────────────────────────────────────────────

/// Call once in the root Layout to provide the cache context.
pub fn use_init_tree_cache() -> TreeCache {
    let cache = TreeCache {
        tree_id: use_context_provider(|| Signal::new(None)),
        tree: use_context_provider(|| Signal::new(None)),
        snapshot: use_context_provider(|| Signal::new(None)),
        generation: use_context_provider(|| Signal::new(0u64)),
    };
    use_context_provider(|| cache);
    cache
}

/// Consume the tree cache from any child component.
pub fn use_tree_cache() -> TreeCache {
    use_context::<TreeCache>()
}

// ─── Pedigree view state cache ──────────────────────────────────────

/// Saved pedigree view state for a specific tree, so navigating away
/// and back does not reset pan / zoom / depth.
#[derive(Clone, Debug)]
pub struct PedigreeViewState {
    pub tree_id: Uuid,
    pub offset_x: f64,
    pub offset_y: f64,
    pub scale: f64,
    pub ancestor_levels: usize,
    pub descendant_levels: usize,
    pub selected_root: Option<Uuid>,
}

/// Global signal holding the last pedigree view state.
#[derive(Clone, Copy)]
pub struct ViewStateCache {
    state: Signal<Option<PedigreeViewState>>,
}

impl ViewStateCache {
    /// Get the saved state if it matches the given tree_id.
    pub fn get(&self, tid: Uuid) -> Option<PedigreeViewState> {
        self.state.read().as_ref().and_then(|s| {
            if s.tree_id == tid {
                Some(s.clone())
            } else {
                None
            }
        })
    }

    /// Save the current view state.
    pub fn save(&self, state: PedigreeViewState) {
        let mut sig = self.state;
        sig.set(Some(state));
    }
}

/// Call once in the root Layout.
pub fn use_init_view_state_cache() -> ViewStateCache {
    let cache = ViewStateCache {
        state: use_context_provider(|| Signal::new(None)),
    };
    use_context_provider(|| cache);
    cache
}

/// Consume from any child component.
pub fn use_view_state_cache() -> ViewStateCache {
    use_context::<ViewStateCache>()
}

// ─── Fetch helpers ──────────────────────────────────────────────────

/// Fetch the tree, using the cache when possible.
pub async fn fetch_tree_cached(
    api: &ApiClient,
    cache: &TreeCache,
    tid: Uuid,
) -> Result<Tree, ApiError> {
    if let Some(t) = cache.tree(tid) {
        return Ok(t);
    }
    let t = api.get_tree(tid).await?;
    cache.store_tree(tid, t.clone());
    Ok(t)
}

/// Fetch the snapshot, using the cache when possible.
pub async fn fetch_snapshot_cached(
    api: &ApiClient,
    cache: &TreeCache,
    tid: Uuid,
) -> Result<TreeSnapshot, ApiError> {
    if let Some(s) = cache.snapshot(tid) {
        return Ok(s);
    }
    let s = api.get_tree_snapshot(tid).await?;
    cache.store_snapshot(tid, s.clone());
    Ok(s)
}
