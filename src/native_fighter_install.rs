//! The native fighter-entry install at main+0x653240, and its staged descriptor.
//!
//! The native routine synchronously copies a 0x70-byte, 8-byte-aligned descriptor and
//! three flag bytes; it retains neither. It is reached through its text address, which
//! `character_lifecycle::reload_trace` hooks inline, never through a trampoline. This
//! module holds values only: no hook, lock, log, allocation or retained native address.
//! It depends on `std` alone so `rustc --test` can check it outside the NRO.

use std::mem::{align_of, size_of};

pub(crate) const DESCRIPTOR_LEN: usize = 0x70;
const INSTALL_ENTRY: usize = 0x653240;
const COUNT: usize = 0x14;
const KINDS: usize = 0x18;
const ACTOR_COUNT: usize = 0x28;
const STARTING_FORM: usize = 0x43;

/// Local staging copy of a native fighter-entry descriptor. Same size and alignment as
/// the `[u64; 14]` buffer the native install reads.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Descriptor([u8; DESCRIPTOR_LEN]);

const _: () = assert!(size_of::<Descriptor>() == DESCRIPTOR_LEN && align_of::<Descriptor>() == 8);

/// Canonical group written over a copied descriptor; the fields of `Forms`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Group {
    pub count: u32,
    pub kinds: [u32; 3],
    pub actor_count: u32,
}

/// The flag bytes the existing callers pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flags {
    /// Practice player/CPU reload of the requested group: `[1, 0, 0]`.
    Reload,
    /// Non-activating helper/source install: `[0, 0, 0]`.
    Parked,
}

impl Flags {
    #[inline]
    fn bytes(self) -> [u8; 3] {
        match self {
            Self::Reload => [1, 0, 0],
            Self::Parked => [0, 0, 0],
        }
    }
}

impl Descriptor {
    /// Copy the complete descriptor at the start of a native fighter entry.
    ///
    /// # Safety
    /// `entry` is the current callback's live native entry, readable for 0x70 bytes.
    #[inline]
    pub(crate) unsafe fn copy_from(entry: *const u8) -> Self {
        let mut descriptor = Self([0; DESCRIPTOR_LEN]);
        std::ptr::copy_nonoverlapping(entry, descriptor.0.as_mut_ptr(), DESCRIPTOR_LEN);
        descriptor
    }

    /// Re-stage a descriptor value captured earlier.
    #[cfg_attr(
        not(any(
            feature = "mii-ordinary-player-alpha",
            feature = "mii-ordinary-cpu-alpha",
            feature = "mii-managed-helper-alpha"
        )),
        allow(dead_code)
    )]
    #[inline]
    pub(crate) fn from_bytes(bytes: &[u8; DESCRIPTOR_LEN]) -> Self {
        Self(*bytes)
    }

    /// Replace the complete canonical group and request `kinds[0]` as the starting form.
    /// Every other byte, including 0x24..0x28, keeps its copied value.
    #[inline]
    pub(crate) fn request_group(&mut self, group: Group) {
        self.put(COUNT, group.count);
        for (index, kind) in group.kinds.iter().copied().enumerate() {
            self.put(KINDS + index * 4, kind);
        }
        self.put(ACTOR_COUNT, group.actor_count);
        self.0[STARTING_FORM] = 0;
    }

    #[cfg_attr(
        not(any(feature = "mii-ordinary-player-alpha", feature = "mii-ordinary-cpu-alpha")),
        allow(dead_code)
    )]
    #[inline]
    pub(crate) fn bytes(&self) -> &[u8; DESCRIPTOR_LEN] {
        &self.0
    }

    #[inline]
    fn put(&mut self, offset: usize, value: u32) {
        self.0[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
    }

    #[inline]
    fn as_native(&self) -> *const u64 {
        (self as *const Self).cast()
    }
}

/// Submit a staged descriptor to the native install.
///
/// # Safety
/// `base` is the calling module's published, fingerprint-verified main text base. The
/// caller runs inside `entry`'s native loader callback, owns the shared install gate, and
/// has completed every validation, phase, trace and state step ordered before the call.
/// This function adds no check, lock, early return or log.
#[inline]
pub(crate) unsafe fn install(base: usize, entry: *mut u8, descriptor: &Descriptor, flags: Flags) {
    let native: unsafe extern "C" fn(*mut u8, *const u64, *const u8) =
        std::mem::transmute(base + INSTALL_ENTRY);
    let flags = flags.bytes();
    native(entry, descriptor.as_native(), flags.as_ptr());
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUPS: [Group; 3] = [
        Group { count: 1, kinds: [0x1a, 0, 0], actor_count: 1 },
        Group { count: 3, kinds: [0x24, 0x25, 0x26], actor_count: 3 },
        Group { count: 2, kinds: [0x5a, 0x5b, u32::MAX], actor_count: 0x1122_3344 },
    ];

    fn source() -> [u8; DESCRIPTOR_LEN] {
        let mut bytes = [0; DESCRIPTOR_LEN];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = (index as u8).wrapping_mul(37).wrapping_add(11);
        }
        bytes
    }

    // Oracle for the group sites: the replaced pointer-write staging. Keep it independent
    // of `Descriptor`; it must not be rewritten in terms of the code under test.
    unsafe fn legacy_group(entry: *const u8, group: Group) -> [u64; 14] {
        let mut descriptor = [0u64; 14];
        std::ptr::copy_nonoverlapping(entry, descriptor.as_mut_ptr().cast::<u8>(), 0x70);
        let raw = descriptor.as_mut_ptr().cast::<u8>();
        std::ptr::write(raw.add(0x14).cast::<u32>(), group.count);
        for (index, kind) in group.kinds.iter().copied().enumerate() {
            std::ptr::write(raw.add(0x18 + index * 4).cast::<u32>(), kind);
        }
        std::ptr::write(raw.add(0x28).cast::<u32>(), group.actor_count);
        std::ptr::write(raw.add(0x43), 0);
        descriptor
    }

    // Oracle for the copy-only sites, under the same independence rule.
    unsafe fn legacy_copy(source: *const u8) -> [u64; 14] {
        let mut descriptor = [0u64; 14];
        std::ptr::copy_nonoverlapping(source, descriptor.as_mut_ptr().cast::<u8>(), 0x70);
        descriptor
    }

    fn bytes_of(words: &[u64; 14]) -> [u8; DESCRIPTOR_LEN] {
        let mut bytes = [0; DESCRIPTOR_LEN];
        for (chunk, word) in bytes.chunks_exact_mut(8).zip(words.iter()) {
            chunk.copy_from_slice(&word.to_ne_bytes());
        }
        bytes
    }

    fn patched(offset: usize) -> bool {
        (0x14..0x24).contains(&offset) || (0x28..0x2c).contains(&offset) || offset == 0x43
    }

    #[test]
    fn layout_matches_the_legacy_u64_buffer() {
        // Type-check the native boundary without calling a game address on the
        // host. This also keeps standalone `rustc --test -D warnings` strict.
        let _: unsafe fn(usize, *mut u8, &Descriptor, Flags) = install;
        assert_eq!(size_of::<Descriptor>(), DESCRIPTOR_LEN);
        assert_eq!(size_of::<Descriptor>(), size_of::<[u64; 14]>());
        assert_eq!(align_of::<Descriptor>(), 8);
        assert_eq!(align_of::<Descriptor>() % align_of::<[u64; 14]>(), 0);
    }

    #[test]
    fn copy_only_staging_is_byte_identical() {
        let source = source();
        let legacy = bytes_of(&unsafe { legacy_copy(source.as_ptr()) });
        let copied = unsafe { Descriptor::copy_from(source.as_ptr()) };
        let restaged = Descriptor::from_bytes(&source);
        assert_eq!(legacy, source);
        assert_eq!(copied.bytes(), &legacy);
        assert_eq!(restaged.bytes(), &legacy);
        assert_eq!(copied, restaged);
    }

    #[test]
    fn group_request_matches_legacy_writes_and_keeps_other_bytes() {
        let source = source();
        assert_ne!(source[STARTING_FORM], 0);
        for group in GROUPS.iter().copied() {
            let legacy = bytes_of(&unsafe { legacy_group(source.as_ptr(), group) });
            let mut staged = unsafe { Descriptor::copy_from(source.as_ptr()) };
            staged.request_group(group);
            let bytes = staged.bytes();
            assert_eq!(bytes, &legacy, "{:?}", group);
            for (offset, (now, before)) in bytes.iter().zip(source.iter()).enumerate() {
                if !patched(offset) {
                    assert_eq!(now, before, "untouched byte {:#x} for {:?}", offset, group);
                }
            }
            assert_eq!(&bytes[0x24..0x28], &source[0x24..0x28]);
            assert_eq!(&bytes[0x14..0x18], &group.count.to_ne_bytes());
            for (index, kind) in group.kinds.iter().enumerate() {
                assert_eq!(&bytes[0x18 + index * 4..0x1c + index * 4], &kind.to_ne_bytes());
            }
            assert_eq!(&bytes[0x28..0x2c], &group.actor_count.to_ne_bytes());
            assert_eq!(bytes[STARTING_FORM], 0);
        }
    }

    #[test]
    fn native_argument_and_borrowed_bytes_are_the_same_staged_words() {
        let source = source();
        for group in GROUPS.iter().copied() {
            let legacy = unsafe { legacy_group(source.as_ptr(), group) };
            let mut staged = unsafe { Descriptor::copy_from(source.as_ptr()) };
            staged.request_group(group);
            let native = staged.as_native();
            assert_eq!(native as usize % 8, 0);
            assert_eq!(native as usize, staged.bytes().as_ptr() as usize);
            let words = unsafe { std::slice::from_raw_parts(native, 14) };
            assert_eq!(words, &legacy[..]);
        }
    }

    #[test]
    fn flags_are_the_existing_byte_arrays() {
        assert_eq!(Flags::Reload.bytes(), [1, 0, 0]);
        assert_eq!(Flags::Parked.bytes(), [0, 0, 0]);
    }
}
