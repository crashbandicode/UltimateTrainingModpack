//! Same pristine native code contract as the qualified fullconfig adapter; no patching.
use crate::mii_native_access::*;
const ROW_LOOKUP_CODE: [(usize, usize, u64); 3] = [
    (OUTFIT_ROW, 0xf4, 0xca258ff0092b5bec),
    (HAT_ROW, 0xf4, 0xe5191f06609380db),
    (0x3269eb0, 0x314, 0x9a4141b62bf2864e),
];
const LOCK: usize = 0x39c19c0;
const UNLOCK: usize = 0x39c19d0;
const INSTALL: usize = 0x653240;

/// Exact first four instruction words at every address this module hooks or calls (13.0.5 `main`).
/// Any mismatch disables the probe before anything is installed.
const SITES: [(usize, [u32; 4], &str); 10] = [
    (
        0x66ddfc,
        [0x290da688, 0x290eae8a, 0x3901f28e, 0x3901f68f],
        "E->O token copy (stp w8, w9, [x20, #0x6c])",
    ),
    (
        0x17e69b4,
        [0x97ffe42b, 0xf9479380, 0x394983ec, 0x340002cc],
        "body request bl 0x17dfa60 (owner load step)",
    ),
    (
        0x17e71a0,
        [0x97ffe230, 0xf9479380, 0x394983ec, 0x340002cc],
        "voice request bl 0x17dfa60 (owner load step)",
    ),
    (
        0x657e18,
        [0x94461f12, 0xf00266db, 0xf9479360, 0x394003ec],
        "voice request bl 0x17dfa60 (setup path)",
    ),
    (
        0x67ead0,
        [0xd10343ff, 0xa90b4ff4, 0xa90c7bfd, 0x910303fd],
        "face texture function entry",
    ),
    (
        LOCK,
        [0xd000c6f0, 0xf945e211, 0x912f0210, 0xd61f0220],
        "pool lock import stub",
    ),
    (
        UNLOCK,
        [0xd000c6f0, 0xf945e611, 0x912f2210, 0xd61f0220],
        "pool unlock import stub",
    ),
    (
        INSTALL,
        [0xd101c3ff, 0xf90013f9, 0xa9035ff8, 0xa90457f6],
        "descriptor install entry",
    ),
    (
        OUTFIT_ROW,
        [0xf81e0ff3, 0xa9017bfd, 0x910043fd, 0xf9401008],
        "outfit row tagged-id getter",
    ),
    (
        HAT_ROW,
        [0xf81e0ff3, 0xa9017bfd, 0x910043fd, 0xf9401008],
        "hat row tagged-id getter",
    ),
];
/// The canonical applier insert (`0x14ebbac..0x14ebc60`) takes the same lock around its raw record
/// copy: `bl 0x39c19c0` at `0x14ebbbc`, `bl 0x39c19d0` at `0x14ebc4c`. Checked as a cross-reference.
const APPLIER_LOCK_SITES: [(usize, u32); 2] = [(0x14ebbbc, 0x94935781), (0x14ebc4c, 0x94935761)];
/// `D` token mirrors: index, outfit, hat, colour.
const D_MIRROR_COPY: (usize, [u32; 13]) = (
    0x654eec,
    [
        0x79408e69, 0xf9407e6b, 0x7900e569, 0x79409269, 0xf9407e6b, 0x7900e969, 0x79409669,
        0xf9407e6b, 0x7900ed69, 0x79409a69, 0xf9407e6b, 0x7900f169, 0xb9403269,
    ],
);
pub(super) fn valid(base: usize) -> bool {
    unsafe {
        SITES
            .iter()
            .all(|(at, words, _)| std::slice::from_raw_parts((base + at) as *const u32, 4) == words)
            && APPLIER_LOCK_SITES
                .iter()
                .all(|(at, word)| rd::<u32>(base, *at) == *word)
            && ROW_LOOKUP_CODE.iter().all(|(at, n, hash)| {
                let bytes = std::slice::from_raw_parts((base + at) as *const u8, *n);
                bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
                    (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
                }) == *hash
            })
            && std::slice::from_raw_parts((base + 0x654eec) as *const u32, D_MIRROR_COPY.1.len())
                == D_MIRROR_COPY.1
    }
}
