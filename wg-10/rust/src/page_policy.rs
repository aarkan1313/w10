//! Pure eviction bookkeeping for the GPU page pool (DESIGN §5.2). No `godot`
//! imports — this is the engine-agnostic policy that decides reuse/allocate/
//! evict, exhaustively headless-tested (the WG9-killer rules: protected pages are
//! never evicted; the budget is never exceeded). It owns KEYS and SLOTS, never
//! RIDs — `page_pool.rs` owns the actual textures and asks this what to do.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// Identifies a page in world space at a clipmap level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PageKey {
    pub level: i32,
    pub origin_x: i64,
    pub origin_z: i64,
}

/// What the pool should do for an `acquire` (no RIDs here — the pool maps a slot
/// index to its texture and acts on this).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Cache hit: the page is already resident in this slot; reuse it.
    Reuse(usize),
    /// Miss, a free slot was available; allocate into it (pool creates a texture).
    Allocate(usize),
    /// Miss at capacity; reuse this slot's texture for the new key, evicting `evicted`.
    AllocateEvicting { slot: usize, evicted: PageKey },
    /// Miss at capacity and EVERY slot is protected; cannot evict. (Pool must not
    /// free anything; the caller uses a coarser page — slice 3's concern.)
    Full,
}

/// Bounded LRU page policy with protected slots.
pub struct PagePolicy {
    capacity: usize,
    slots: Vec<Option<PageKey>>,   // slot -> occupying key (None = free)
    map: BTreeMap<PageKey, usize>, // key -> slot
    stamp: Vec<u64>,               // slot -> last-acquire stamp (LRU = smallest)
    protected: BTreeSet<usize>,    // protected slot indices
    clock: u64,
}

impl PagePolicy {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "PagePolicy capacity must be > 0");
        Self {
            capacity,
            slots: vec![None; capacity],
            map: BTreeMap::new(),
            stamp: vec![0; capacity],
            protected: BTreeSet::new(),
            clock: 0,
        }
    }

    pub fn capacity(&self) -> usize { self.capacity }

    pub fn resident_count(&self) -> usize { self.map.len() }

    /// All currently resident keys (read-only snapshot; no RIDs, no slots). The
    /// pool exposes this to the scheduler so it can diff coverage against reality.
    pub fn resident_keys(&self) -> Vec<PageKey> {
        self.map.keys().cloned().collect()
    }

    /// The slot holding `key` if it is currently resident, else None. Pure lookup — no
    /// LRU touch, no protect, no allocate/evict. Used by the read-only `get_resident_page`
    /// path so a CONSUMER (the view) can fetch an already-resident page's texture WITHOUT
    /// triggering a compute (only the scheduler's `acquire` may compute — the anti-WG9 rule:
    /// no synchronous page production on the render path).
    pub fn slot_of(&self, key: &PageKey) -> Option<usize> {
        self.map.get(key).copied()
    }

    pub fn is_protected(&self, key: &PageKey) -> bool {
        self.map.get(key).map_or(false, |s| self.protected.contains(s))
    }

    fn touch(&mut self, slot: usize) {
        self.clock += 1;
        self.stamp[slot] = self.clock;
        self.protected.insert(slot);
    }

    /// Acquire a page (marks it protected + most-recently-used). Returns the
    /// decision the pool should act on.
    pub fn acquire(&mut self, key: PageKey) -> Decision {
        // hit
        if let Some(&slot) = self.map.get(&key) {
            self.touch(slot);
            return Decision::Reuse(slot);
        }
        // miss: free slot?
        if let Some(slot) = self.slots.iter().position(|s| s.is_none()) {
            self.slots[slot] = Some(key);
            self.map.insert(key, slot);
            self.touch(slot);
            return Decision::Allocate(slot);
        }
        // miss at capacity: LRU unprotected slot
        let victim = (0..self.capacity)
            .filter(|s| !self.protected.contains(s))
            .min_by_key(|&s| self.stamp[s]);
        match victim {
            Some(slot) => {
                let evicted = self.slots[slot].expect("occupied slot");
                self.map.remove(&evicted);
                self.slots[slot] = Some(key);
                self.map.insert(key, slot);
                self.touch(slot);
                Decision::AllocateEvicting { slot, evicted }
            }
            None => Decision::Full,
        }
    }

    /// Roll back a just-decided allocation/eviction when the producer failed to
    /// fill the slot: remove `key` entirely (unmap + free the slot + unprotect),
    /// leaving the slot empty and reusable. Use when compute/texture_create fails
    /// AFTER acquire() already recorded the key, so policy state matches reality
    /// (no phantom-resident key, no stale mapping). Idempotent.
    pub fn rollback(&mut self, key: PageKey) {
        if let Some(slot) = self.map.remove(&key) {
            self.slots[slot] = None;
            self.protected.remove(&slot);
            // stamp left as-is; an empty slot is taken by the free-slot path before LRU.
        }
    }

    /// Release a page (unprotect; stays resident + LRU-eligible). Idempotent.
    pub fn release(&mut self, key: PageKey) {
        if let Some(&slot) = self.map.get(&key) {
            self.protected.remove(&slot);
        }
    }
}
