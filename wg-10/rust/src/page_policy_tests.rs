use crate::page_policy::{PagePolicy, Decision, PageKey};

fn k(level: i32, x: i64, z: i64) -> PageKey { PageKey { level, origin_x: x, origin_z: z } }

#[test]
fn miss_allocates_into_free_slots_within_budget() {
    let mut p = PagePolicy::new(2);
    assert_eq!(p.acquire(k(0,0,0)), Decision::Allocate(0));
    assert_eq!(p.acquire(k(0,1,0)), Decision::Allocate(1));
    assert_eq!(p.resident_count(), 2);
    assert!(p.resident_count() <= p.capacity());
}

#[test]
fn hit_reuses_same_slot() {
    let mut p = PagePolicy::new(2);
    let a = p.acquire(k(0,0,0));
    let slot = match a { Decision::Allocate(s) => s, _ => panic!("expected Allocate") };
    p.release(k(0,0,0));
    // re-acquire the same key -> Reuse SAME slot (RID stability)
    assert_eq!(p.acquire(k(0,0,0)), Decision::Reuse(slot));
}

#[test]
fn at_capacity_evicts_lru_unprotected() {
    let mut p = PagePolicy::new(2);
    p.acquire(k(0,0,0)); p.release(k(0,0,0)); // slot 0, now unprotected, LRU
    p.acquire(k(0,1,0)); p.release(k(0,1,0)); // slot 1, unprotected, more-recent
    // both unprotected; slot 0 is LRU -> evicting it
    match p.acquire(k(0,2,0)) {
        Decision::AllocateEvicting { slot, evicted } => {
            assert_eq!(slot, 0);
            assert_eq!(evicted, k(0,0,0));
        }
        other => panic!("expected AllocateEvicting slot0/key(0,0,0), got {other:?}"),
    }
    assert!(p.resident_count() <= 2);
}

#[test]
fn protected_is_never_evicted() {
    let mut p = PagePolicy::new(2);
    p.acquire(k(0,0,0));            // slot 0, PROTECTED (not released)
    p.acquire(k(0,1,0)); p.release(k(0,1,0)); // slot 1, unprotected
    // acquiring a 3rd key must evict the UNPROTECTED slot 1, never the protected slot 0
    match p.acquire(k(0,2,0)) {
        Decision::AllocateEvicting { slot, evicted } => {
            assert_eq!(slot, 1, "must evict the unprotected slot, not the protected one");
            assert_eq!(evicted, k(0,1,0));
        }
        other => panic!("expected eviction of unprotected slot1, got {other:?}"),
    }
}

#[test]
fn all_protected_yields_full_no_eviction() {
    let mut p = PagePolicy::new(2);
    p.acquire(k(0,0,0)); // protected
    p.acquire(k(0,1,0)); // protected
    // both protected; a new key cannot evict -> Full, state unchanged
    assert_eq!(p.acquire(k(0,2,0)), Decision::Full);
    assert_eq!(p.resident_count(), 2);
    // the new key is NOT resident
    assert_eq!(p.acquire(k(0,0,0)), Decision::Reuse(0)); // existing still there
}

#[test]
fn release_makes_slot_evictable() {
    let mut p = PagePolicy::new(1);
    p.acquire(k(0,0,0)); // slot 0, protected, capacity 1 full
    assert_eq!(p.acquire(k(0,9,0)), Decision::Full); // can't evict protected
    p.release(k(0,0,0));
    // now slot 0 is evictable
    match p.acquire(k(0,9,0)) {
        Decision::AllocateEvicting { slot, evicted } => {
            assert_eq!(slot, 0); assert_eq!(evicted, k(0,0,0));
        }
        other => panic!("expected eviction after release, got {other:?}"),
    }
}

#[test]
fn reacquire_reprotects() {
    let mut p = PagePolicy::new(2);
    p.acquire(k(0,0,0)); p.release(k(0,0,0)); // resident, unprotected
    p.acquire(k(0,0,0));                      // re-acquire -> re-PROTECTED
    p.acquire(k(0,1,0)); p.release(k(0,1,0)); // slot 1 unprotected
    // acquiring a 3rd key must evict slot 1 (unprotected), not the re-protected slot 0
    match p.acquire(k(0,2,0)) {
        Decision::AllocateEvicting { slot, .. } => assert_eq!(slot, 1),
        other => panic!("expected eviction of unprotected slot1, got {other:?}"),
    }
}

#[test]
fn deterministic_sequence() {
    let seq = |p: &mut PagePolicy| vec![
        p.acquire(k(0,0,0)), p.acquire(k(0,1,0)),
        { p.release(k(0,0,0)); p.acquire(k(0,2,0)) },
    ];
    let mut a = PagePolicy::new(2);
    let mut b = PagePolicy::new(2);
    assert_eq!(seq(&mut a), seq(&mut b));
}

#[test]
fn release_unknown_key_is_noop() {
    let mut p = PagePolicy::new(2);
    p.release(k(0,5,5)); // never acquired -> no panic
    assert_eq!(p.resident_count(), 0);
}

#[test]
fn rollback_frees_the_slot_no_phantom() {
    let mut p = PagePolicy::new(1);
    let d = p.acquire(k(0,0,0));                 // Allocate(0), protected
    assert!(matches!(d, Decision::Allocate(0)));
    p.rollback(k(0,0,0));                         // producer "failed" -> remove it
    assert_eq!(p.resident_count(), 0);           // slot is empty again
    // re-acquiring the SAME key is a fresh Allocate (NOT a phantom Reuse)
    assert_eq!(p.acquire(k(0,0,0)), Decision::Allocate(0));
    // and a different key also allocates cleanly into the freed slot after rollback
    let mut q = PagePolicy::new(1);
    q.acquire(k(0,0,0)); q.rollback(k(0,0,0));
    assert_eq!(q.acquire(k(0,9,0)), Decision::Allocate(0));
}

#[test]
fn rollback_unknown_key_is_noop() {
    let mut p = PagePolicy::new(2);
    p.rollback(k(0,1,1));  // never acquired -> no panic, no change
    assert_eq!(p.resident_count(), 0);
}

#[test]
fn resident_keys_lists_all_resident_pages() {
    let mut p = PagePolicy::new(3);
    p.acquire(k(0, 0, 0));
    p.acquire(k(0, 1000, 0));
    p.acquire(k(1, 0, 0));
    let mut got = p.resident_keys();
    got.sort(); // PageKey is Ord
    let mut want = vec![k(0,0,0), k(0,1000,0), k(1,0,0)];
    want.sort();
    assert_eq!(got, want);
    assert_eq!(got.len(), p.resident_count());
}
