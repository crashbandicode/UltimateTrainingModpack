//! Experimental containment of the observed null-parent unlink failure.
//! The writer is unknown. This does not validate arbitrary native pointers or
//! replace native traversal, link clearing, resource destruction, or freeing.

pub const SKIPPED_NULL_STORE: u64 = 1;
pub const ADVANCED_ROOT: u64 = 2;
pub const UPDATED_ROOT_LAST: u64 = 4;
pub const ABSENT_HEADER_FIELD: u64 = 8;

/// Execute one displaced native link store, except a write through null+8/16.
///
/// Safety: caller must supply the exact native store's live node/address/value.
/// For the header-first store, owner is the live destructor owner. Its root is
/// the same root the native loop just read; never derive a parent from history.
/// All non-null store destinations retain the native routine's preconditions.
pub unsafe fn store(address: u64, value: u64, node: u64, owner: u64, header_first: bool) -> u64 {
    let mut result = 0;
    if header_first && crate::native_fields::read::<u64>(node as usize, 0) == Some(0) {
        let root = crate::native_fields::child(owner as usize, 0xa0);
        // The native header loop obtains node from root.first. Keep that loop
        // progressing if this one backlink disappeared. No guessed parent and
        // no write through a pointer retrieved from diagnostic history.
        if let Some(root) = root {
            if crate::native_fields::read::<u64>(root, 8) == Some(node) {
                if let (Some(next), Some(previous)) = (
                    crate::native_fields::read::<u64>(node as usize, 0x20),
                    crate::native_fields::read::<u64>(node as usize, 0x18),
                ) {
                    if crate::native_fields::write(root, 8, next) { result |= ADVANCED_ROOT; }
                    if crate::native_fields::read::<u64>(root, 0x10) == Some(node)
                        && crate::native_fields::write(root, 0x10, previous) {
                        result |= UPDATED_ROOT_LAST;
                    }
                } else { result |= ABSENT_HEADER_FIELD; }
            }
        } else { result |= ABSENT_HEADER_FIELD; }
    }
    if matches!(address, 8 | 16) {
        result | SKIPPED_NULL_STORE
    } else {
        std::ptr::write_volatile(address as *mut u64, value);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pointer<T>(value: &mut T) -> u64 { value as *mut T as u64 }

    #[test]
    fn valid_native_store_has_identical_effect_and_no_guard_action() {
        let mut node = [0u64; 6];
        let mut destination = 17u64;
        let address = pointer(&mut destination);
        assert_eq!(unsafe { store(address, 29, pointer(&mut node), 0, false) }, 0);
        assert_eq!(destination, 29);
    }

    #[test]
    fn both_null_parent_offsets_are_skipped_without_touching_node() {
        let mut node = [0u64, 0, 0, 0x123400, 0, 0x567800];
        let before = node;
        for address in [8, 16] {
            assert_eq!(unsafe { store(address, 91, pointer(&mut node), 0, false) }, SKIPPED_NULL_STORE);
        }
        assert_eq!(node, before);
    }

    #[test]
    fn absent_header_owner_or_root_preserves_the_remaining_cleanup() {
        let mut node = [0u64; 6];
        let mut owner = [0u64; 0x1f8 / 8];
        for o in [0, pointer(&mut owner)] {
            assert_eq!(unsafe { store(8, 0, pointer(&mut node), o, true) },
                SKIPPED_NULL_STORE | ABSENT_HEADER_FIELD);
            let mut destination = 3u64;
            assert_eq!(unsafe { store(pointer(&mut destination), 7, pointer(&mut node), o, true) },
                ABSENT_HEADER_FIELD);
            assert_eq!(destination, 7);
        }
    }

    #[test]
    fn lost_header_backlink_advances_root_until_empty() {
        let mut owner = [0u64; 0x1f8 / 8];
        let mut root = [0u64; 6];
        let mut first = [0u64; 6];
        let mut second = [0u64; 6];
        let a = pointer(&mut first);
        let b = pointer(&mut second);
        owner[0xa0 / 8] = pointer(&mut root);
        let o = pointer(&mut owner);
        root[1] = a; root[2] = b; first[4] = b; second[3] = a;
        assert_eq!(unsafe { store(8, b, a, o, true) }, SKIPPED_NULL_STORE | ADVANCED_ROOT);
        assert_eq!((root[1], root[2]), (b, b));
        // The untouched native second store repairs the next sibling, then
        // native code clears first's links. No allocation is released here.
        unsafe { store(b + 0x18, 0, a, o, false); }
        first[..5].fill(0);
        assert_eq!(second[3], 0);
        assert_eq!(unsafe { store(8, 0, b, o, true) }, SKIPPED_NULL_STORE | ADVANCED_ROOT | UPDATED_ROOT_LAST);
        assert_eq!((root[1], root[2]), (0, 0));
        assert_eq!(unsafe { store(16, 0, b, o, false) }, SKIPPED_NULL_STORE);
    }

    #[test]
    fn unrelated_root_is_not_rewritten_and_valid_parent_path_is_unchanged() {
        let mut owner = [0u64; 0x1f8 / 8];
        let mut root = [0u64, 0x123400, 0x123400, 0, 0, 0];
        let mut node = [0u64; 6];
        owner[0xa0 / 8] = pointer(&mut root);
        let expected = root;
        assert_eq!(unsafe { store(8, 77, pointer(&mut node), pointer(&mut owner), true) }, SKIPPED_NULL_STORE);
        assert_eq!(root, expected);
        node[0] = pointer(&mut root);
        let mut destination = 1u64;
        assert_eq!(unsafe { store(pointer(&mut destination), 77, pointer(&mut node), 0, true) }, 0);
        assert_eq!(destination, 77);
        assert_eq!(root, expected);
    }
}
