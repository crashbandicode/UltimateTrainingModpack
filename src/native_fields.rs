//! Structural checks for fields borrowed from a live native object.
//!
//! These functions never own, retain, or free game memory. Null/overflow checks
//! do not prove liveness: the caller must still hold the native callback/lock
//! that makes every non-null object valid. Do not store a borrowed field between
//! callbacks or turn it into a Rust reference/Box.

use std::mem::{align_of, size_of};

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
    (child != 0 && child.is_multiple_of(align_of::<usize>())).then_some(child)
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
}
