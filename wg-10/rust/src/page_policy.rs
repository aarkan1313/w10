//! Pure eviction bookkeeping for the GPU page pool (DESIGN §5.2). No `godot`
//! imports — this is the engine-agnostic policy that decides reuse/allocate/
//! evict, exhaustively headless-tested (the WG9-killer rules: protected pages are
//! never evicted; the budget is never exceeded). It owns KEYS and SLOTS, never
//! RIDs — `page_pool.rs` owns the actual textures and asks this what to do.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// Identifies a page in world space at a clipmap level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

    /// Release a page (unprotect; stays resident + LRU-eligible). Idempotent.
    pub fn release(&mut self, key: PageKey) {
        if let Some(&slot) = self.map.get(&key) {
            self.protected.remove(&slot);
        }
    }
}
