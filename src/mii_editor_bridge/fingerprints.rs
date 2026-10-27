//! Qualified pristine 13.0.5 ranges; checked before this adapter installs hooks.
const CODE: &[(usize, usize, u64)] = &[
    (0x13e8a60, 0x2d4, 0x2ac43d18f789c8b4),
    (0x13e6cb0, 0xe0, 0x1384682e6c1048dd),
    (0x13e6fe0, 0x44, 0x3d2d22e1af105bd4),
    (0x23ebbf0, 0x90, 0x94048fba53dc4b60),
    (0x22fa6d0, 0x64, 0xf7c0fa7ba3907cc7),
    (0x22fa450, 0x280, 0x52be5623deeee3a9),
    (0x22f85a0, 0xaa0, 0xd348d929e4afa0f),
    (0x22f9700, 0x1f0, 0xea651d2496cefe05),
    (0x180e13c, 0x20, 0x18ae7662a8922d5c),
    (0x180d1c0, 0x8f0, 0x2d9496d49d885450),
    (0x17bd3e0, 0x1b0, 0x7b503c214b5daaa0),
    (0x17bd170, 0x198, 0x1c427e8921794a88),
    (0x37266e0, 0x40c, 0x67051cfbf566067e),
    (0x3723a90, 0x284, 0x3841a101a085c74a),
    (0x3723e70, 0x140, 0xd6edd9eb32b5b156),
    (0x23ecdc0, 0x100, 0xcf1c1ffc043bbfd3),
    (0x180ea90, 0x74, 0xfd6278f35e6544b3),
    (0x32eafc0, 0x2ac, 0x6d4ad6174ef79ff1),
    (0x32e5a30, 0xd4, 0x905f343eebde16ef),
    (0x180fe98, 0x50, 0x37ff76f4685c5486),
    (0x22f7eac, 0x10, 0xb87e567e0906c70),
    (0x3302110, 0xa0, 0x5184e41d83ecdcd9),
    (0x3305a30, 0x270, 0x1de003a0c6f396b6),
    (0x3305ee0, 0x194, 0x43af110d5a53c7a3),
    (0x1815260, 0x178, 0x557768f4dce7c481),
    (0x180cb80, 0xc0, 0x8427e0e26ec67300),
    // Connected adapter: native owners, event peek, Test return and Cancel.
    (0x2310570, 0x340, 0x4160dca6143df07d),
    (0x1851480, 0x100, 0x7bd7a3fb02c6cb1c),
    (0x18512c0, 0x50, 0x7e4d66939125f247),
    (0x1851310, 0x30, 0x8165eba171e47d5c),
    (0x17f3810, 0xe4, 0x37d3d998a08bbf24),
    (0x22e0470, 0x214, 0xb89327149c58754c),
    (0x16bb300, 0x170, 0x2cd8ddb062c738b0),
    // Read-only native self slot and its member/slot lookup contracts.
    (0x1686610, 0x240, 0x96e6b12c71ca14bb),
    (0x16f6340, 0x160, 0x9cd7e957360ab61e),
    (0x16e4b20, 0x350, 0xab6cceddd48652ab),
    (0x180a194, 0x38, 0xfe2e0047c6cf8123),
    (0x180dab0, 0x30, 0x10153fe8bacf178e),
    (0x180c320, 0x820, 0x683cbea5aa14bffc),
    (0x2319200, 0x230, 0x1e31281a6107f660),
    // Native submenu pre-emption, completion, and non-save parent return.
    (0x17f3478, 0x150, 0xac99704229c22339),
    (0x3726af0, 0x150, 0x5a3453bfd3e1d133),
    (0x1809f70, 0x23b0, 0xa4e5e1e1be0387a3),
    (0x22f92d0, 0x204, 0xef81be18db0f38b8),
    // Native Join delivery and synchronous keyboard commit/cleanup contracts.
    (0x17f3650, 0x30, 0x2f531e0c2aa77abd),
    (0x17f3680, 0x188, 0x62bba6a87e1e786b),
    (0x3300780, 0x88, 0x4b66dc51fa478847),
    (0x3300a70, 0x1d0, 0x380d4e130df795a1),
    (0x22dd080, 0xe4, 0xeaff0d94a116d8c6),
    // Exact unfinished UI, protected Lua and outer Quickplay lifetime.
    (0x180a570, 0x540, 0x3985585ff0d953fb),
    (0x3392360, 0x100, 0x38ae03232d1bbbcf),
    (0x38f5d80, 0x1a4, 0x6326742a84ad8de),
    (0x38f5fe0, 0x88, 0x13a72c14b9867dda),
    (0x38f88f0, 0x20, 0x4056489657ade5aa),
    (0x38f5f30, 0x20, 0x8dd9dc1c18160cbc),
    (0x38f6b00, 0x50, 0x321363bd2cf385ff),
    (0x19dcc80, 0xb8, 0x28734a7fc80c3792),
    (0x3376860, 0x120, 0xf929fe905e2a7f0b),
    (0x3375938, 0x20, 0xdcfaaae843616997),
    (0x372cc40, 0x60, 0x8b586d22d867b665),
    (0x372d7ec, 0x30, 0x3f8de40a5ca7077),
    (0x38f6184, 0x7c, 0x71a3146c924e6a13),
    (0x236af60, 0x490, 0xb2de3678fec0e39f),
    (0x236b400, 0x64, 0x2464c236a1afa543),
    (0x236b470, 0x104, 0xa4a15f7db2dccf23),
    // Local Wireless parent lifetime, vtable 0x50b8028 slots +0x50/+0x58/+0x60.
    (0x2336c90, 0x45c, 0x29fe1b8d746382fb),
    (0x2337330, 0x180, 0x4d6b87b78c88df9),
    (0x23374b0, 0x100, 0x677be2cac0760653),
    (0x2369930, 0x20, 0x82ad67ca8ebd95e3),
    (0x180ce00, 0xc0, 0x5ed86c2953880871),
    (0x1809b9c, 0x28, 0x5898121c98d5327),
    (0x1809cf8, 0x28, 0x6d97a8e8cf6ec42b),
    (0x1809ff0, 0x580, 0xaf77951c12c63e29),
    (0x180a04c, 0x70, 0xcc3470bf06ae271d),
    (0x180b9e0, 0x150, 0x4737040d75a2cac5),
    (0x19ca590, 0x5d4, 0x60ae38853dd9301c),
    (0x32cabd0, 0x70, 0x8d5a9c0920169554),
    // Sequential native window-ticker and scene-framework passes.
    (0x374c9a4, 0x18, 0x705fa2a89d3b0847),
    (0x374cbb8, 0x18, 0x705fa2a89d3b0847),
    // Native new-slot Save and asynchronous writer completion.
    (0x180d3dc, 0xf0, 0xeb386dce895ce1bc),
    (0x17c84f0, 0x158, 0xeeff573e165e7dbc),
    (0x17c77c0, 0x3c0, 0x3bc298335c15b46c),
    (0x17bd310, 0xd0, 0xdb99873fe4e13d20),
    (0x17c6440, 0xb4, 0x38f7af7d49cb1339),
    (0x180cc40, 0x174, 0x2f5c38bb4ffc724b),
    (0x22d5dc0, 0x60, 0x7170baf4694f7be9),
    // Local Wireless round handshake, read only: the practice-owned consumer
    // predicate (resolved byte, then mask; window starts past its entry), the
    // guest acknowledgement store, and the commit function's early-exit head
    // plus its resolved-byte store tail.
    (0x1566f40, 0xe0, 0x634261521667fa0),
    (0x1699c10, 0x1c, 0xe4c4dbafc910dcd3),
    (0x169bf18, 0x24, 0x5ad49dddb8defeac),
    (0x169c3a8, 0x14, 0xa7b50e17d2e35ada),
];
pub(super) unsafe fn matches(base: usize) -> bool {
    let mut accepted = true;
    for &(offset, len, expected) in CODE {
        let bytes = std::slice::from_raw_parts((base + offset) as *const u8, len);
        let observed = training_mod_consts::mii::inventory::fnv1a64(bytes);
        if observed != expected {
            accepted = false;
            let words = std::slice::from_raw_parts((base + offset) as *const u32, len / 4);
            crate::logging::diagnostic(format_args!("[MiiEditor] code_mismatch offset={offset:#x} len={len:#x} expected={expected:#x} observed={observed:#x} words={words:x?}"));
        }
    }
    // ARCropolis Arcadia installs a normal Skyline hook at the scene queue.
    // Keep its complete native tail checked above and call through the hook;
    // never overwrite it or jump past its forwarding logic.
    let entry = std::slice::from_raw_parts((base + 0x37266d0) as *const u32, 4);
    let pristine = entry == [0xd10383ff, 0xf9005bf5, 0xa90c4ff4, 0xa90d7bfd];
    let target = u64::from(entry[2]) | (u64::from(entry[3]) << 32);
    let hooked = entry[..2] == [0x58000051, 0xd61f0220] && target != 0 && target.is_multiple_of(4);
    if !pristine && !hooked {
        crate::logging::diagnostic(format_args!("[MiiEditor] unsupported scene_queue entry words={entry:x?}"));
        accepted = false;
    } else if hooked {
        crate::logging::diagnostic(format_args!("[MiiEditor] scene_queue uses existing Skyline hook; native tail verified; target={target:#x}"));
    }
    accepted
}
