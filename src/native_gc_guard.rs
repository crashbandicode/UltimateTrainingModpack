//! Experimental containment for a collectable Lua constant with a null object.
//! This does not establish why the constant was malformed or repair its value.

const LOOP: usize = 0x38ffcc0;
const EXPECTED: [u32; 15] = [
    0xf9401a89, 0x8b15012a, 0x3940214a, 0x3630010a, 0xf8756921,
    0x39402429, 0x7200053f, 0x54000080, 0xaa1303e0, 0x97fff997,
    0xb9401688, 0x910006d6, 0x910042b5, 0xeb28c2df, 0x54fffe4b,
];

// Same-sized loop, with the null check in space recovered by loading the
// 16-byte TValue's object and tag together. The native flag test, marking call,
// index advancement and subsequent child traversal remain unchanged.
// x9/x10 are dead after each skipped cell; x1 is caller-saved and is not used
// after a skipped cell. No hook callback, allocation, stack growth or polling.
const PATCHES: [(usize, u32); 3] = [
    (0x38ffcc4, 0x8b150129), // add x9, x9, x21: address of this TValue
    (0x38ffcc8, 0xa9402921), // ldp x1, x10, [x9]: object and tag/padding
    (0x38ffcd0, 0xb40000e1), // cbz x1, 0x38ffcec: advance to next constant
];

pub fn init() {
    let main = unsafe { skyline::hooks::getRegionAddress(skyline::hooks::Region::Text) as usize };
    // Validate the whole loop before any mutation; a different game build or
    // another mod's replacement must not be overwritten. Plugin init is quiescent.
    for (index, expected) in EXPECTED.iter().enumerate() {
        let actual = unsafe { std::ptr::read_volatile((main + LOOP + index * 4) as *const u32) };
        if actual != *expected {
            crate::logging::diagnostic(format_args!("[NativeGcGuard] refused modified loop at {:#x}", LOOP + index * 4));
            return;
        }
    }
    for (offset, word) in PATCHES {
        if skyline::patching::Patch::in_text(offset).data(word).is_err() {
            std::process::abort(); // Never execute a partially replaced loop.
        }
    }
    for (offset, word) in PATCHES {
        if unsafe { std::ptr::read_volatile((main + offset) as *const u32) } != word {
            std::process::abort();
        }
    }
    crate::logging::diagnostic(format_args!("[NativeGcGuard] null constant guard enabled; malformed-value writer unknown"));
}
