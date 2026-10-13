//! Revision 29 UI anchors for Smash 13.0.5 (see `docs/REVISION29_DRAW_DECOUPLING.md`).
//!
//! The Training Modpack no longer hooks `nn::ui2d::Layout`'s `+0x98` entry (main+0x4b620).
//! It runs its UI work from two inline anchors inside Smash's UI manager, right before the
//! game's own virtual calculate call on a holder's layout. This module holds the
//! version-specific words and needles, and the pure checks the runtime applies before
//! installing; the Skyline side lives in the product crate.
//!
//! Each anchor is located by a *locator* needle that starts one word after the site. A plugin
//! that patched the site's first word therefore still locates it, and the five-word pin can
//! report that case precisely instead of failing the search.

/// One inline anchor: where it is on 13.0.5, the words it must hold, and how to find it.
#[derive(Clone, Copy, Debug)]
pub struct AnchorSite {
    /// Stable name used in the install log and in panic messages.
    pub name: &'static str,
    /// Offset from the main module's text base on 13.0.5 (documentation and tests only;
    /// the runtime locates the site with [`AnchorSite::locator`]).
    pub offset: usize,
    /// The first five instruction words at the site.
    pub pin: [u32; 5],
    /// Needle that begins [`LOCATOR_DELTA`] bytes after the site.
    pub locator: &'static [u8],
}

/// Bytes from an anchor site to the start of its locator needle.
pub const LOCATOR_DELTA: usize = 4;

/// Per-frame UI pass `0x375e260`, before `blr` to the holder layout's `+0x98` (x23 = holder).
#[rustfmt::skip]
pub const UI_PASS_CALCULATE: AnchorSite = AnchorSite {
    name: "ui_pass_calculate",
    offset: 0x375efa8,
    pin: [0xf9422ee0, 0xf9400008, 0xf9404d08, 0xaa1303e1, 0x2a1f03e2],
    locator: &[
        0x08, 0x00, 0x40, 0xf9, // ldr x8, [x0]
        0x08, 0x4d, 0x40, 0xf9, // ldr x8, [x8, #0x98]
        0xe1, 0x03, 0x13, 0xaa, // mov x1, x19
        0xe2, 0x03, 0x1f, 0x2a, // mov w2, wzr
        0x00, 0x01, 0x3f, 0xd6, // blr x8
        0xe0, 0x03, 0x13, 0xaa, // mov x0, x19
        0x4f, 0x97, 0x23, 0x97, // bl 0x44d00
    ],
};

/// Manager insert `0x37849c0`, initial calculate of a newly registered holder (x21 = holder).
#[rustfmt::skip]
pub const UI_REGISTER_CALCULATE: AnchorSite = AnchorSite {
    name: "ui_register_calculate",
    offset: 0x3784ad4,
    pin: [0xf9422ea0, 0xf9400008, 0xf9404d08, 0x320003e2, 0xaa1303e1],
    locator: &[
        0x08, 0x00, 0x40, 0xf9, // ldr x8, [x0]
        0x08, 0x4d, 0x40, 0xf9, // ldr x8, [x8, #0x98]
        0xe2, 0x03, 0x00, 0x32, // orr w2, wzr, #0x1
        0xe1, 0x03, 0x13, 0xaa, // mov x1, x19
        0x00, 0x01, 0x3f, 0xd6, // blr x8
        0xe0, 0x03, 0x13, 0xaa, // mov x0, x19
        0x84, 0x00, 0x23, 0x97, // bl 0x44d00
    ],
};

/// Prologue of the UI pass `0x375e260`; its `adrp`/`ldr` pair names the manager global.
#[rustfmt::skip]
pub const UI_MANAGER_PROLOGUE: &[u8] = &[
    0xff, 0x83, 0x02, 0xd1, // sub sp, sp, #0xa0
    0xfc, 0x6f, 0x04, 0xa9, // stp x28, x27, [sp, #0x40]
    0xfa, 0x67, 0x05, 0xa9, // stp x26, x25, [sp, #0x50]
    0xf8, 0x5f, 0x06, 0xa9, // stp x24, x23, [sp, #0x60]
    0xf6, 0x57, 0x07, 0xa9, // stp x22, x21, [sp, #0x70]
    0xf4, 0x4f, 0x08, 0xa9, // stp x20, x19, [sp, #0x80]
    0xfd, 0x7b, 0x09, 0xa9, // stp x29, x30, [sp, #0x90]
    0xfd, 0x43, 0x02, 0x91, // add x29, sp, #0x90
    0xfc, 0x0e, 0x01, 0xb0, // adrp x28, 0x593b000
    0x88, 0x4b, 0x45, 0xf9, // ldr x8, [x28, #0xa90]
];
/// Offset of the pass function on 13.0.5 (documentation and tests only).
pub const UI_MANAGER_PROLOGUE_OFFSET: usize = 0x375e260;
/// Bytes from the prologue start to its `adrp`; the `ldr` follows it.
pub const UI_MANAGER_ADRP_DELTA: usize = 0x20;
/// The manager-global slot that the pair decodes to on 13.0.5 (documentation and tests only).
pub const UI_MANAGER_SLOT_OFFSET: usize = 0x593ba90;

/// Bytes from the prologue start to [`UI_MANAGER_LOCATOR`]: past the 16 bytes a Skyline function hook
/// rewrites at a function entry (`ldr x17, #8; br x17; .quad target`).
pub const UI_MANAGER_LOCATOR_DELTA: usize = 0x14;
const _: () = assert!(UI_MANAGER_LOCATOR_DELTA >= 16, "the locator must clear a 16-byte function-hook stub");
/// Revision 30 locator for the UI pass: prologue words 5–9 plus the first body word. It has exactly one
/// hit in the 13.0.5 text and survives another plugin's function hook on the entry `0x375e260` (Deluxe
/// 0.3.1 hooks that entry for its dispatcher idle backoff and runs its `main` before ours). It contains
/// the `adrp`/`ldr` pair at [`UI_MANAGER_ADRP_DELTA`] from the prologue start.
#[rustfmt::skip]
pub const UI_MANAGER_LOCATOR: &[u8] = &[
    0xf4, 0x4f, 0x08, 0xa9, // stp x20, x19, [sp, #0x80]
    0xfd, 0x7b, 0x09, 0xa9, // stp x29, x30, [sp, #0x90]
    0xfd, 0x43, 0x02, 0x91, // add x29, sp, #0x90
    0xfc, 0x0e, 0x01, 0xb0, // adrp x28, 0x593b000
    0x88, 0x4b, 0x45, 0xf9, // ldr x8, [x28, #0xa90]
    0x09, 0x01, 0x40, 0xf9, // ldr x9, [x8]
];

/// The first five prologue words, for the informational `ui_manager_prologue` report (the runtime reads
/// only the adrp/ldr pair, so a patched entry is reported with [`verify_site`]'s vocabulary, never fatal).
pub fn ui_manager_prologue_pin() -> [u32; 5] {
    let prologue = words(UI_MANAGER_PROLOGUE);
    [prologue[0], prologue[1], prologue[2], prologue[3], prologue[4]]
}

/// `Holder*` -> `nn::ui2d::Layout*`.
pub const HOLDER_LAYOUT: usize = 0x458;
/// Manager -> `std::vector<Holder*>` begin; the end pointer follows it.
pub const MANAGER_HOLDERS: usize = 0x0;

/// Outcome of the five-word pin check, in the `ssbu-dispatcher-backoff` install-log vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteVerdict {
    Installed,
    /// The first word differs and the other four match: another plugin patched the site.
    OwnedByAnotherPlugin,
    WordsDiffer,
}

impl SiteVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            SiteVerdict::Installed => "installed",
            SiteVerdict::OwnedByAnotherPlugin => "site_owned_by_another_plugin",
            SiteVerdict::WordsDiffer => "site_words_differ",
        }
    }
}

pub fn verify_site(found: [u32; 5], expected: [u32; 5]) -> SiteVerdict {
    if found == expected {
        SiteVerdict::Installed
    } else if found[0] != expected[0] && found[1..] == expected[1..] {
        SiteVerdict::OwnedByAnotherPlugin
    } else {
        SiteVerdict::WordsDiffer
    }
}

/// Little-endian instruction words of a needle (trailing bytes that do not fill a word are ignored).
pub fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect()
}

/// Address loaded by `adrp xN, page` at `pc` followed by `ldr xM, [xN, #imm]` (64-bit, unsigned
/// offset). Returns `None` unless both words have exactly that form and the same base register.
pub fn decode_adrp_ldr(pc: u64, adrp: u32, ldr: u32) -> Option<u64> {
    if adrp & 0x9f00_0000 != 0x9000_0000 || ldr & 0xffc0_0000 != 0xf940_0000 {
        return None;
    }
    if adrp & 0x1f != (ldr >> 5) & 0x1f {
        return None;
    }
    let immlo = u64::from((adrp >> 29) & 0x3);
    let immhi = u64::from((adrp >> 5) & 0x7_ffff);
    // Sign-extend the 21-bit page count.
    let pages = (((immhi << 2) | immlo) << 43) as i64 >> 43;
    let page = (pc & !0xfff).wrapping_add_signed(pages << 12);
    Some(page + u64::from((ldr >> 10) & 0xfff) * 8)
}

/// First entry whose name equals `name`, in registry (priority) order. Entries without a
/// readable name are skipped.
pub fn find_by_name<'a, T>(entries: impl IntoIterator<Item = (Option<&'a [u8]>, T)>, name: &str) -> Option<T> {
    entries
        .into_iter()
        .find(|(entry, _)| *entry == Some(name.as_bytes()))
        .map(|(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locator_starts_with_pin_tail(site: &AnchorSite) {
        let locator = words(site.locator);
        assert_eq!(site.locator.len() % 4, 0, "{}", site.name);
        assert_eq!(&locator[..4], &site.pin[1..], "{}", site.name);
    }

    #[test]
    fn locators_begin_with_the_last_four_pin_words() {
        locator_starts_with_pin_tail(&UI_PASS_CALCULATE);
        locator_starts_with_pin_tail(&UI_REGISTER_CALCULATE);
        assert_eq!(UI_PASS_CALCULATE.locator.len(), 28);
        assert_eq!(UI_REGISTER_CALCULATE.locator.len(), 28);
    }

    #[test]
    fn pins_load_the_holder_layout_and_its_plus_0x98_entry() {
        // ldr x0, [x23|x21, #0x458]; ldr x8, [x0]; ldr x8, [x8, #0x98]
        for (site, holder) in [(UI_PASS_CALCULATE, 23), (UI_REGISTER_CALCULATE, 21)] {
            let first = site.pin[0];
            assert_eq!(first & 0xffc0_0000, 0xf940_0000, "{}", site.name);
            assert_eq!(((first >> 10) & 0xfff) as usize * 8, HOLDER_LAYOUT, "{}", site.name);
            assert_eq!((first >> 5) & 0x1f, holder, "{}", site.name);
            assert_eq!(first & 0x1f, 0, "{}", site.name);
            assert_eq!(site.pin[1], 0xf9400008);
            assert_eq!(site.pin[2], 0xf9404d08);
        }
    }

    #[test]
    fn verdicts_follow_the_backoff_install_log() {
        let pin = UI_PASS_CALCULATE.pin;
        assert_eq!(verify_site(pin, pin), SiteVerdict::Installed);
        let mut patched = pin;
        patched[0] = 0x1400_0000 | 0x1234; // b <trampoline>
        assert_eq!(verify_site(patched, pin), SiteVerdict::OwnedByAnotherPlugin);
        let mut other = pin;
        other[3] = 0xd503_201f;
        assert_eq!(verify_site(other, pin), SiteVerdict::WordsDiffer);
        patched[4] = 0xd503_201f;
        assert_eq!(verify_site(patched, pin), SiteVerdict::WordsDiffer);
        assert_eq!(SiteVerdict::Installed.as_str(), "installed");
        assert_eq!(SiteVerdict::OwnedByAnotherPlugin.as_str(), "site_owned_by_another_plugin");
        assert_eq!(SiteVerdict::WordsDiffer.as_str(), "site_words_differ");
    }

    #[test]
    fn manager_slot_decodes_from_the_pass_prologue() {
        let prologue = words(UI_MANAGER_PROLOGUE);
        let index = UI_MANAGER_ADRP_DELTA / 4;
        let pc = (UI_MANAGER_PROLOGUE_OFFSET + UI_MANAGER_ADRP_DELTA) as u64;
        assert_eq!(decode_adrp_ldr(pc, prologue[index], prologue[index + 1]),
            Some(UI_MANAGER_SLOT_OFFSET as u64));
        // Position independence: the same words at another load address keep the delta.
        let base = 0x80_0400_0000u64;
        assert_eq!(decode_adrp_ldr(base + pc, prologue[index], prologue[index + 1]),
            Some(base + UI_MANAGER_SLOT_OFFSET as u64));
    }

    #[test]
    fn manager_locator_is_the_prologue_tail_past_a_function_hook_stub() {
        let prologue = words(UI_MANAGER_PROLOGUE);
        let locator = words(UI_MANAGER_LOCATOR);
        assert_eq!(UI_MANAGER_LOCATOR_DELTA, 0x14); // >= 16 is a compile-time assertion beside the constant
        assert_eq!(UI_MANAGER_LOCATOR.len(), 24);
        let skip = UI_MANAGER_LOCATOR_DELTA / 4;
        assert_eq!(&locator[..prologue.len() - skip], &prologue[skip..]);
        assert_eq!(locator[prologue.len() - skip], 0xf940_0109); // ldr x9, [x8]
        // The adrp/ldr pair sits inside the locator at the documented delta and still names the slot.
        let index = (UI_MANAGER_ADRP_DELTA - UI_MANAGER_LOCATOR_DELTA) / 4;
        let pc = (UI_MANAGER_PROLOGUE_OFFSET + UI_MANAGER_ADRP_DELTA) as u64;
        assert_eq!(decode_adrp_ldr(pc, locator[index], locator[index + 1]), Some(UI_MANAGER_SLOT_OFFSET as u64));
        assert_eq!(ui_manager_prologue_pin(), [0xd102_83ff, 0xa904_6ffc, 0xa905_67fa, 0xa906_5ff8, 0xa907_57f6]);
        // Another plugin's entry hook: a near `b` reads as owned, a far 16-byte stub as differing words;
        // both are reports only.
        let mut near = ui_manager_prologue_pin();
        near[0] = 0x1400_0000 | 0x1234;
        assert_eq!(verify_site(near, ui_manager_prologue_pin()), SiteVerdict::OwnedByAnotherPlugin);
        let mut far = ui_manager_prologue_pin();
        far[..4].copy_from_slice(&[0x5800_0051, 0xd61f_0220, 0x0000_1000, 0x0000_0020]);
        assert_eq!(verify_site(far, ui_manager_prologue_pin()), SiteVerdict::WordsDiffer);
    }

    #[test]
    fn decode_rejects_other_forms() {
        let (adrp, ldr) = (0xb0010efc, 0xf9454b88);
        assert_eq!(decode_adrp_ldr(0x1000, 0x90000000 | 0x1c, 0xf9400388), Some(0x1000));
        assert_eq!(decode_adrp_ldr(0, 0x10010efc, ldr), None); // adr, not adrp
        assert_eq!(decode_adrp_ldr(0, adrp, 0xb9454b88), None); // 32-bit ldr
        assert_eq!(decode_adrp_ldr(0, adrp, 0xf9454b68), None); // base x27, not x28
        // Negative page delta.
        assert_eq!(decode_adrp_ldr(0x10_0000, 0xf0ff_fffc, 0xf9400388), Some(0x10_0000 - 0x1000));
    }

    #[test]
    fn find_by_name_takes_the_first_named_match_in_order() {
        let entries = [
            (None, 1),
            (Some(&b"info_melee"[..]), 2),
            (Some(&b"info_training"[..]), 3),
            (Some(&b"info_training"[..]), 4),
        ];
        assert_eq!(find_by_name(entries, "info_training"), Some(3));
        assert_eq!(find_by_name(entries, "info_melee"), Some(2));
        assert_eq!(find_by_name(entries, "info_waiting"), None);
        assert_eq!(find_by_name(entries, ""), None);
        assert_eq!(find_by_name(entries, "info_trainin"), None);
    }

    /// Run with the owned dump: `TRAINING_MOD_MAIN_ELF=<main-13.0.5.elf> cargo test -- --ignored`.
    #[test]
    #[ignore = "needs the owned 13.0.5 main ELF in TRAINING_MOD_MAIN_ELF"]
    fn elf_pins_match_owned_13_0_5_dump() {
        let path = std::env::var("TRAINING_MOD_MAIN_ELF").expect("TRAINING_MOD_MAIN_ELF");
        let elf = std::fs::read(path).expect("read main ELF");
        let u16_at = |o: usize| u16::from_le_bytes([elf[o], elf[o + 1]]) as usize;
        let u64_at = |o: usize| u64::from_le_bytes(elf[o..o + 8].try_into().unwrap()) as usize;
        let (phoff, phentsize, phnum) = (u64_at(0x20), u16_at(0x36), u16_at(0x38));
        // First executable PT_LOAD: the text segment, vaddr 0.
        let (vaddr, offset, size) = (0..phnum)
            .map(|i| phoff + i * phentsize)
            .find(|&p| u32::from_le_bytes(elf[p..p + 4].try_into().unwrap()) == 1 && elf[p + 4] & 1 == 1)
            .map(|p| (u64_at(p + 0x10), u64_at(p + 8), u64_at(p + 0x20)))
            .expect("text PT_LOAD");
        let text = &elf[offset..offset + size];
        let hits = |needle: &[u8]| -> Vec<usize> {
            text.windows(needle.len()).enumerate().filter(|(_, w)| *w == needle).map(|(i, _)| vaddr + i).collect()
        };
        for site in [UI_PASS_CALCULATE, UI_REGISTER_CALCULATE] {
            assert_eq!(hits(site.locator), vec![site.offset + LOCATOR_DELTA], "{}", site.name);
            let found = words(&text[site.offset - vaddr..site.offset - vaddr + 20]);
            assert_eq!(verify_site(found.try_into().unwrap(), site.pin), SiteVerdict::Installed, "{}", site.name);
        }
        assert_eq!(hits(UI_MANAGER_PROLOGUE), vec![UI_MANAGER_PROLOGUE_OFFSET]);
        assert_eq!(hits(UI_MANAGER_LOCATOR), vec![UI_MANAGER_PROLOGUE_OFFSET + UI_MANAGER_LOCATOR_DELTA]);
        // The removed hook's needle still identifies main+0x4b620 on this dump (documentation).
        let draw: &[u8] = &[0x08, 0x0c, 0x40, 0xf9, 0xc8, 0x03, 0x00, 0xb4, 0xff, 0x83, 0x01, 0xd1, 0xf5, 0x1b, 0x00, 0xf9,
            0xf4, 0x4f, 0x04, 0xa9, 0xfd, 0x7b, 0x05, 0xa9, 0xfd, 0x43, 0x01, 0x91, 0xf4, 0x03, 0x00, 0xaa];
        assert_eq!(hits(draw), vec![0x4b620]);
    }
}
