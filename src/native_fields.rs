//! Structural checks for fields borrowed from a live native object.
//!
//! These functions never own, retain, or free game memory. Null/overflow checks
//! do not prove liveness: the caller must still hold the native callback/lock
//! that makes every non-null object valid. Do not store a borrowed field between
//! callbacks or turn it into a Rust reference/Box.
//!
//! Prefer `read`, `write` and `chain` at nullable boundaries. The explicitly
//! unchecked operations are for required fields inside an already-qualified
//! native callback/transaction: repeating structural checks there neither pins
//! memory nor supplies a valid recovery path halfway through a native mutation.
//! They preserve the original volatile access and add no allocation or logging.

use std::mem::{align_of, size_of};

/// Structural native-object check only; does not establish mapped memory or life.
#[inline]
pub(crate) fn object(address: usize) -> bool {
    address != 0 && address.is_multiple_of(align_of::<usize>())
}

/// Read a required field after the caller has qualified its native owner.
///
/// # Safety
/// The complete `base + offset .. + size_of::<T>()` range must not wrap and must
/// be live, readable, aligned and contain a valid `T`. The caller's current
/// native callback or lock must prevent invalidation for this access. This is
/// the unchecked fast path, not a fallback for a failed checked read.
#[inline]
pub(crate) unsafe fn read_unchecked<T: Copy>(base: usize, offset: usize) -> T {
    std::ptr::read_volatile((base + offset) as *const T)
}

/// Write a required field within the caller's already-qualified transaction.
///
/// # Safety
/// `read_unchecked`'s address/lifetime/alignment requirements apply, and the
/// complete field must be writable. The caller must preserve the native
/// ownership, locking and publication protocol. This does not run destructors.
#[inline]
pub(crate) unsafe fn write_unchecked<T: Copy>(base: usize, offset: usize, value: T) {
    std::ptr::write_volatile((base + offset) as *mut T, value);
}

fn address<T>(base: usize, offset: usize) -> Option<usize> {
    if base == 0 { return None; }
    let at = base.checked_add(offset)?;
    at.checked_add(size_of::<T>())?;
    at.is_multiple_of(align_of::<T>()).then_some(at)
}

/// Read one field; absent or structurally invalid parents return `None`.
///
/// # Safety
/// For a non-null base, the caller guarantees the field is live, readable, has
/// a valid `T` representation and cannot be concurrently invalidated. Volatile
/// access is not synchronization and does not catch use-after-free.
#[inline]
pub(crate) unsafe fn read<T: Copy>(base: usize, offset: usize) -> Option<T> {
    address::<T>(base, offset).map(|at| std::ptr::read_volatile(at as *const T))
}

/// Write one field, preserving all native ownership and destruction rules.
///
/// # Safety
/// The non-null field must be live and writable under the caller's native
/// lifetime/locking authority. No pointer validation beyond null, alignment and
/// address arithmetic is performed.
#[inline]
#[cfg(any(test, feature = "native-unlink-guard"))]
pub(crate) unsafe fn write<T: Copy>(base: usize, offset: usize, value: T) -> bool {
    let Some(at) = address::<T>(base, offset) else { return false; };
    std::ptr::write_volatile(at as *mut T, value);
    true
}

/// Follow a pointer-sized, aligned child without dereferencing the child.
///
/// # Safety
/// The parent field must satisfy `read`'s requirements. The returned child is
/// only an address; a later dereference still requires native lifetime authority.
#[inline]
pub(crate) unsafe fn child(base: usize, offset: usize) -> Option<usize> {
    let child = read::<usize>(base, offset)?;
    object(child).then_some(child)
}

/// Follow nullable child fields, checking every parent and the final address.
/// An empty chain returns its parent only when structurally valid.
///
/// # Safety
/// Every non-null intermediate object must remain live and readable under the
/// same native authority, as for `child`. No pointer escapes with a lifetime.
#[inline]
pub(crate) unsafe fn chain(mut base: usize, offsets: &[usize]) -> Option<usize> {
    for &offset in offsets {
        if !object(base) { return None; }
        base = child(base, offset)?;
    }
    object(base).then_some(base)
}

/// Mirror a native 32-bit acquire load; volatile access cannot replace this.
///
/// # Safety
/// The address must identify a live, aligned native atomic word for this load.
/// The caller prevents destruction and incompatible non-atomic access.
#[inline]
#[cfg(any(test, feature = "mii-ordinary-player-alpha"))]
pub(crate) unsafe fn load_acquire_u32(address: usize) -> u32 {
    (&*(address as *const std::sync::atomic::AtomicU32))
        .load(std::sync::atomic::Ordering::Acquire)
}

/// Mirror a native 64-bit acquire load under the same contract as the u32 form.
///
/// # Safety
/// The address must identify a live, aligned native atomic word for this load.
/// The caller prevents destruction and incompatible non-atomic access.
#[inline]
#[cfg(any(test, feature = "mii-editor-alpha"))]
pub(crate) unsafe fn load_acquire_u64(address: usize) -> u64 {
    (&*(address as *const std::sync::atomic::AtomicU64))
        .load(std::sync::atomic::Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_parent_or_child_ends_the_chain_without_a_dereference() {
        assert_eq!(unsafe { read::<usize>(0, 8) }, None);
        assert!(!unsafe { write::<usize>(0, 16, 7) });
        assert_eq!(unsafe { child(0, 0xa0) }, None);
        let empty = [0usize; 2];
        assert_eq!(unsafe { child(empty.as_ptr() as usize, size_of::<usize>()) }, None);
    }

    #[test]
    fn rejects_wrapping_or_misaligned_field_addresses() {
        assert_eq!(address::<usize>(usize::MAX - 7, 8), None);
        assert_eq!(address::<usize>(usize::MAX - 7, 0), None);
        assert_eq!(address::<usize>(1, 0), None);
    }

    #[test]
    fn live_fields_preserve_reads_and_writes_without_taking_ownership() {
        let mut value = [11usize, 22];
        let parent = [value.as_mut_ptr() as usize];
        let child = unsafe { child(parent.as_ptr() as usize, 0) }.unwrap();
        assert_eq!(unsafe { read::<usize>(child, size_of::<usize>()) }, Some(22));
        assert!(unsafe { write(child, size_of::<usize>(), 33usize) });
        assert_eq!(value, [11, 33]);
    }

    #[test]
    fn qualified_required_fields_preserve_the_complete_value() {
        let mut words = [0x1122334455667788u64, 0xfedcba9876543210];
        let base = words.as_mut_ptr() as usize;
        unsafe { write_unchecked(base, 8, 0x13579bdf2468ace0u64); }
        assert_eq!(unsafe { read_unchecked::<u64>(base, 0) }, words[0]);
        assert_eq!(unsafe { read_unchecked::<u64>(base, 8) }, 0x13579bdf2468ace0);
    }

    #[test]
    fn chains_preserve_legacy_nullable_walk_results() {
        unsafe fn legacy(mut p: usize, offsets: &[usize]) -> Option<usize> {
            for &offset in offsets {
                if p == 0 || !p.is_multiple_of(8) { return None; }
                p = std::ptr::read_volatile((p + offset) as *const usize);
            }
            (p != 0 && p.is_multiple_of(8)).then_some(p)
        }
        let leaf = [0usize; 2];
        let leaf_address = leaf.as_ptr() as usize;
        for child_address in [0, leaf_address + 1, leaf_address] {
            let branch = [0usize, child_address];
            let root = [branch.as_ptr() as usize];
            let base = root.as_ptr() as usize;
            for offsets in [&[][..], &[0][..], &[0, size_of::<usize>()][..]] {
                assert_eq!(unsafe { chain(base, offsets) }, unsafe { legacy(base, offsets) });
            }
        }
        for base in [0, leaf_address + 1] {
            assert_eq!(unsafe { chain(base, &[0]) }, None);
            assert_eq!(unsafe { chain(base, &[]) }, None);
        }
    }

    #[test]
    fn native_atomic_words_keep_their_width_and_acquire_ordering() {
        use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
        let word = AtomicU32::new(0);
        let wide = AtomicU64::new(0);
        word.store(0x89abcdef, Ordering::Release);
        wide.store(0xfedcba9876543210, Ordering::Release);
        assert_eq!(unsafe { load_acquire_u32(&word as *const _ as usize) }, 0x89abcdef);
        assert_eq!(unsafe { load_acquire_u64(&wide as *const _ as usize) }, 0xfedcba9876543210);
    }
}
