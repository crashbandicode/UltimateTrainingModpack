//! Revision-2 owned identity and exact-tuple deduplication. Native memory access is deliberately absent.
pub const SLOT_SIZE: usize = 0x64340;
pub const CHARINFO_LEN: usize = 0x58;
/// create_id (0x00..0x10), 10-unit nickname (0x10..0x24), u16 terminator (0x24..0x26) per Eden char_info.h.
pub const CHARINFO_MASK_END: usize = 0x26;
pub const LOW40: u64 = 0xff_ffff_ffff;
/// Type tags in the top byte of the three tagged ids: `+0x130` outfit, `+0x138` hat, `+0x1a0` ui_chara.
pub const TAG_OUTFIT: u8 = 0x13;
pub const TAG_HAT: u8 = 0x5a;
pub const TAG_CHARA: u8 = 0xc1;
/// hash40 (len << 32 | crc32) of the ui_chara_db keys; verified on three saves (class-next).
pub const HASH40_UI_CHARA_MIIFIGHTER: u64 = 0x13_fdfd_fb2b;
pub const HASH40_UI_CHARA_MIISWORDSMAN: u64 = 0x15_d706_1f77;
pub const HASH40_UI_CHARA_MIIGUNNER: u64 = 0x12_57ab_1d87;
/// Canonical bytes: class kind (1) + masked CharInfo (0x58) + specials (4) + colour (1) + voice (1) + 3 x hash40 (5 each).
pub const CANONICAL_LEN: usize = 1 + CHARINFO_LEN + 4 + 1 + 1 + 15;
pub const CANONICAL_HEX_LEN: usize = CANONICAL_LEN * 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MiiClass {
    Brawler = 0x48,
    Swordfighter = 0x49,
    Gunner = 0x4a,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassError {
    BadTag(u8),
    UnknownLabel(u64),
}

impl MiiClass {
    pub fn kind(self) -> u32 {
        self as u32
    }

    pub fn from_kind(kind: u32) -> Option<Self> {
        match kind {
            0x48 => Some(Self::Brawler),
            0x49 => Some(Self::Swordfighter),
            0x4a => Some(Self::Gunner),
            _ => None,
        }
    }

    /// Class from the slot's tagged ui_chara id (+0x1a0). The row-hint bits 40..55 are ignored:
    /// the game's own lookups (0x32638e0, 0x3283fa0, 0x3288190) search the low 40 bits and use the
    /// hint only as a first guess.
    pub fn from_chara_id(tagged: TaggedId) -> Result<Self, ClassError> {
        if tagged.tag() != TAG_CHARA {
            return Err(ClassError::BadTag(tagged.tag()));
        }
        match tagged.hash40() {
            HASH40_UI_CHARA_MIIFIGHTER => Ok(Self::Brawler),
            HASH40_UI_CHARA_MIISWORDSMAN => Ok(Self::Swordfighter),
            HASH40_UI_CHARA_MIIGUNNER => Ok(Self::Gunner),
            other => Err(ClassError::UnknownLabel(other)),
        }
    }

    pub fn chara_hash40(self) -> u64 {
        match self {
            Self::Brawler => HASH40_UI_CHARA_MIIFIGHTER,
            Self::Swordfighter => HASH40_UI_CHARA_MIISWORDSMAN,
            Self::Gunner => HASH40_UI_CHARA_MIIGUNNER,
        }
    }
}

/// Tagged id as stored in a slot: bits 63..56 type tag, bits 55..40 cached row index (a lookup hint),
/// bits 39..0 hash40 of the database key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TaggedId(pub u64);

impl TaggedId {
    pub fn tag(self) -> u8 {
        (self.0 >> 56) as u8
    }
    pub fn row_hint(self) -> u16 {
        ((self.0 >> 40) & 0xffff) as u16
    }
    pub fn hash40(self) -> u64 {
        self.0 & LOW40
    }
}

/// The fields the inventory needs, copied out of one slot. Holding a `RawSlot` keeps no pointer
/// into game memory. The saved-file adapter produces this owned value from a bounded header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawSlot {
    pub index: usize,          // position in the database vector; tie-break only
    pub slot_id: u32,          // +0x00 (file number)
    pub name_len: u32,         // +0x114
    pub name_units: [u16; 10], // +0x118
    pub colour: u8,            // +0x140, outfit colour (controlled diff red->orange: 0->1; body request `extra`)
    pub voice: u8,             // +0x141, voice index (resource category 0x15; 37 = default path in all known saves)
    pub specials: [u8; 4],     // +0x142..0x145, zero-based N, S, Hi, Lw
    pub tag_outfit: TaggedId,  // +0x130 (tag 0x13; resource category 0x13 through the outfit table)
    pub tag_hat: TaggedId,     // +0x138 (tag 0x5a; resource category 0x14 through the hat table)
    pub tag_chara: TaggedId,   // +0x1a0
    pub charinfo: [u8; CHARINFO_LEN], // +0x148..0x1a0
}

/// Appearance + moves + colour + voice without the class. Tags are the low 40 bits only (hash40).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Appearance {
    pub charinfo_masked: [u8; CHARINFO_LEN],
    pub specials: [u8; 4],
    pub colour: u8,
    pub voice: u8,
    pub outfit: u64,
    pub hat: u64,
    pub chara: u64,
}

/// Identity = class + appearance. Compared by value; serialised by `to_bytes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Canonical {
    pub class: MiiClass,
    pub appearance: Appearance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalError {
    Length(usize),
    Hex,
    ClassByte(u8),
    ClassMismatch,
    SpecialOutOfRange(u8),
}

fn put40(out: &mut [u8], value: u64) {
    let b = (value & LOW40).to_le_bytes();
    out[..5].copy_from_slice(&b[..5]);
}

fn get40(input: &[u8]) -> u64 {
    let mut b = [0u8; 8];
    b[..5].copy_from_slice(&input[..5]);
    u64::from_le_bytes(b)
}

impl Canonical {
    /// Exact layout (offsets in the 0x6e-byte array):
    /// 0x00 class kind (0x48/0x49/0x4a); 0x01..0x59 masked CharInfo; 0x59..0x5d specials; 0x5d colour;
    /// 0x5e voice; 0x5f..0x64 outfit hash40 LE; 0x64..0x69 hat hash40 LE; 0x69..0x6e chara hash40 LE.
    pub fn to_bytes(&self) -> [u8; CANONICAL_LEN] {
        let mut out = [0u8; CANONICAL_LEN];
        out[0] = self.class.kind() as u8;
        out[1..1 + CHARINFO_LEN].copy_from_slice(&self.appearance.charinfo_masked);
        out[0x59..0x5d].copy_from_slice(&self.appearance.specials);
        out[0x5d] = self.appearance.colour;
        out[0x5e] = self.appearance.voice;
        put40(&mut out[0x5f..0x64], self.appearance.outfit);
        put40(&mut out[0x64..0x69], self.appearance.hat);
        put40(&mut out[0x69..0x6e], self.appearance.chara);
        out
    }

    pub fn from_bytes(bytes: &[u8; CANONICAL_LEN]) -> Result<Self, CanonicalError> {
        let class = MiiClass::from_kind(u32::from(bytes[0])).ok_or(CanonicalError::ClassByte(bytes[0]))?;
        let mut charinfo_masked = [0u8; CHARINFO_LEN];
        charinfo_masked.copy_from_slice(&bytes[1..1 + CHARINFO_LEN]);
        if charinfo_masked[..CHARINFO_MASK_END].iter().any(|b| *b != 0) {
            return Err(CanonicalError::Hex); // a masked CharInfo must have its identity prefix zeroed
        }
        let mut specials = [0u8; 4];
        specials.copy_from_slice(&bytes[0x59..0x5d]);
        if let Some(bad) = specials.iter().find(|s| **s > 2) {
            return Err(CanonicalError::SpecialOutOfRange(*bad));
        }
        let appearance = Appearance {
            charinfo_masked,
            specials,
            colour: bytes[0x5d],
            voice: bytes[0x5e],
            outfit: get40(&bytes[0x5f..0x64]),
            hat: get40(&bytes[0x64..0x69]),
            chara: get40(&bytes[0x69..0x6e]),
        };
        if appearance.chara != class.chara_hash40() {
            return Err(CanonicalError::ClassMismatch);
        }
        Ok(Self { class, appearance })
    }

    /// Lowercase hex, exactly `CANONICAL_HEX_LEN` (220) characters. This is the persisted selector.
    pub fn to_hex(&self) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let bytes = self.to_bytes();
        let mut s = String::with_capacity(CANONICAL_HEX_LEN);
        for b in bytes {
            s.push(DIGITS[(b >> 4) as usize] as char);
            s.push(DIGITS[(b & 15) as usize] as char);
        }
        s
    }

    pub fn from_hex(hex: &str) -> Result<Self, CanonicalError> {
        if hex.len() != CANONICAL_HEX_LEN {
            return Err(CanonicalError::Length(hex.len()));
        }
        let mut bytes = [0u8; CANONICAL_LEN];
        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let hi = (chunk[0] as char).to_digit(16).ok_or(CanonicalError::Hex)?;
            let lo = (chunk[1] as char).to_digit(16).ok_or(CanonicalError::Hex)?;
            bytes[i] = ((hi << 4) | lo) as u8;
        }
        Self::from_bytes(&bytes)
    }

    /// Lookup aid only. Equality is always decided by `to_bytes()` / `==`.
    pub fn hash(&self) -> u64 {
        fnv1a64(&self.to_bytes())
    }
}

impl serde::Serialize for Canonical {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> serde::Deserialize<'de> for Canonical {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::from_hex(&text).map_err(|e| serde::de::Error::custom(format!("invalid Mii identity: {e:?}")))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MiiConfig {
    pub slot_id: u32,
    pub slot_index: usize,
    pub name: String,                 // display only
    pub charinfo: [u8; CHARINFO_LEN], // unmasked, kept for the swap and for runtime cross-checks; never logged raw
    pub raw_tags: [TaggedId; 3],      // outfit (+0x130), hat (+0x138), chara (+0x1a0) as stored, with row hints
    pub canonical: Canonical,
    /// Slot ids collapsed into this representative (byte-equal identity, higher slot ids).
    pub copies: Vec<u32>,
}

impl MiiConfig {
    pub fn charinfo_hash(&self) -> u64 {
        fnv1a64(&self.charinfo)
    }
    /// Row hints the swap uses as tokens after validating them against the ui tables.
    pub fn outfit_hint(&self) -> u16 {
        self.raw_tags[0].row_hint()
    }
    pub fn hat_hint(&self) -> u16 {
        self.raw_tags[1].row_hint()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Finding {
    BadTag { slot_index: usize, slot_id: u32, field: &'static str, tag: u8 },
    UnknownClassLabel { slot_index: usize, slot_id: u32, hash40: u64 },
    SpecialOutOfRange { slot_index: usize, slot_id: u32, specials: [u8; 4] },
    NameLength { slot_index: usize, slot_id: u32, len: u32 },
    DuplicateCollapsed { slot_id: u32, representative: u32 },
    /// Native SelectData excludes every row whose saved byte +0x1a9 is nonzero.
    NativeExcluded { slot_index: usize, slot_id: u32, flag: u8 },
    Source { slot_id: Option<u32>, error: String },
    /// A mod-owned preset failure does not invalidate independently read native saves.
    PresetSource { error: String },
}

impl Finding {
    /// Only native-source defects invalidate the native configuration inventory.
    pub fn is_defect(&self) -> bool {
        !matches!(self, Finding::DuplicateCollapsed { .. } | Finding::NativeExcluded { .. } | Finding::PresetSource { .. })
    }
}

/// One inventory result: every valid configuration (deduplicated, lowest slot id first) and every
/// slot that is not in `configs` with the reason. A UI must present defects, never drop them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub configs: Vec<MiiConfig>,
    /// Mod-owned configurations have no native save ID. Native duplicates take precedence.
    pub presets: Vec<crate::draft::Preset>,
    pub findings: Vec<Finding>,
    pub slots_seen: usize,
}

impl Snapshot {
    pub fn of_class(&self, class: MiiClass) -> impl Iterator<Item = &MiiConfig> {
        self.configs.iter().filter(move |c| c.canonical.class == class)
    }
    pub fn defects(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| f.is_defect())
    }
    pub fn is_complete(&self) -> bool {
        self.defects().next().is_none()
    }
    pub fn with_library(mut self, library: &crate::draft::Library) -> Self {
        self.presets = library.presets().iter().filter(|preset|
            !self.configs.iter().any(|native| native.canonical == preset.canonical()))
            .cloned().collect();
        self
    }
    pub fn entries(&self, class: MiiClass) -> impl Iterator<Item = CatalogEntry<'_>> {
        self.of_class(class).map(|c| CatalogEntry {
            canonical: c.canonical, slot_id: Some(c.slot_id), name: &c.name, copies: c.copies.len(),
        }).chain(self.presets.iter().filter(move |p| p.canonical().class == class)
            .map(|p| CatalogEntry { canonical: p.canonical(), slot_id: None, name: p.name(), copies: 0 }))
    }
    pub fn configuration(&self, choice: crate::selection::Choice) -> Option<crate::lifetime::Configuration> {
        if !self.is_complete() { return None; }
        match choice.slot_id {
            Some(id) => self.configs.iter().find(|c| c.slot_id == id && c.canonical == choice.canonical)
                .and_then(|c| crate::lifetime::Configuration::from_saved(c).ok()),
            None => self.presets.iter().find(|p| p.canonical() == choice.canonical).map(|p| p.configuration()),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CatalogEntry<'a> {
    pub canonical: Canonical,
    pub slot_id: Option<u32>,
    pub name: &'a str,
    pub copies: usize,
}

pub fn classify(slot: &RawSlot) -> Result<MiiConfig, Finding> {
    let (slot_index, slot_id) = (slot.index, slot.slot_id);
    if slot.specials.iter().any(|s| *s > 2) {
        return Err(Finding::SpecialOutOfRange { slot_index, slot_id, specials: slot.specials });
    }
    if slot.tag_outfit.tag() != TAG_OUTFIT {
        return Err(Finding::BadTag { slot_index, slot_id, field: "+0x130", tag: slot.tag_outfit.tag() });
    }
    if slot.tag_hat.tag() != TAG_HAT {
        return Err(Finding::BadTag { slot_index, slot_id, field: "+0x138", tag: slot.tag_hat.tag() });
    }
    let class = match MiiClass::from_chara_id(slot.tag_chara) {
        Ok(class) => class,
        Err(ClassError::BadTag(tag)) => return Err(Finding::BadTag { slot_index, slot_id, field: "+0x1a0", tag }),
        Err(ClassError::UnknownLabel(hash40)) => return Err(Finding::UnknownClassLabel { slot_index, slot_id, hash40 }),
    };
    if slot.name_len == 0 || slot.name_len > 10 {
        return Err(Finding::NameLength { slot_index, slot_id, len: slot.name_len });
    }
    let mut masked = slot.charinfo;
    masked[..CHARINFO_MASK_END].fill(0);
    Ok(MiiConfig {
        slot_id,
        slot_index,
        name: String::from_utf16_lossy(&slot.name_units[..slot.name_len as usize]),
        charinfo: slot.charinfo,
        raw_tags: [slot.tag_outfit, slot.tag_hat, slot.tag_chara],
        canonical: Canonical {
            class,
            appearance: Appearance {
                charinfo_masked: masked,
                specials: slot.specials,
                colour: slot.colour,
                voice: slot.voice,
                outfit: slot.tag_outfit.hash40(),
                hat: slot.tag_hat.hash40(),
                chara: slot.tag_chara.hash40(),
            },
        },
        copies: Vec::new(),
    })
}

/// Pure: sort by slot id (database position breaks ties), classify, deduplicate on byte-equal
/// canonical identity keeping the lowest slot id, and report every excluded slot.
pub fn build_snapshot(mut slots: Vec<RawSlot>) -> Snapshot {
    let slots_seen = slots.len();
    slots.sort_by_key(|s| (s.slot_id, s.index));
    let mut findings = Vec::new();
    let mut valid = Vec::with_capacity(slots.len());
    for slot in &slots {
        match classify(slot) {
            Ok(cfg) => valid.push(cfg),
            Err(finding) => findings.push(finding),
        }
    }
    let (configs, dup_findings) = dedupe_with(valid, Canonical::hash);
    findings.extend(dup_findings);
    Snapshot { configs, findings, slots_seen, presets: Vec::new() }
}

/// Deduplicate already slot-id-sorted configurations. The hash only narrows candidates; the merge
/// decision is `kept.canonical == cfg.canonical` (byte equality, class included).
pub fn dedupe_with(configs: Vec<MiiConfig>, hash: fn(&Canonical) -> u64) -> (Vec<MiiConfig>, Vec<Finding>) {
    let mut out: Vec<MiiConfig> = Vec::with_capacity(configs.len());
    let mut findings = Vec::new();
    for cfg in configs {
        let h = hash(&cfg.canonical);
        if let Some(kept) = out.iter_mut().find(|k| hash(&k.canonical) == h && k.canonical == cfg.canonical) {
            kept.copies.push(cfg.slot_id);
            findings.push(Finding::DuplicateCollapsed { slot_id: cfg.slot_id, representative: kept.slot_id });
        } else {
            out.push(cfg);
        }
    }
    (out, findings)
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100000001b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTFIT_4155: u64 = 0x1300011bfef7a5f7;
    const HAT_DEFAULT: u64 = 0x5a00000fa9efa5ea;
    const CHARA_BRAWLER: u64 = 0xc1004c13fdfdfb2b;
    const CHARA_SWORD: u64 = 0xc1004d15d7061f77;
    const CHARA_GUNNER: u64 = 0xc1004e1257ab1d87;

    fn charinfo(create_id: u8, name: u8, face: u8) -> [u8; CHARINFO_LEN] {
        let mut c = [face; CHARINFO_LEN];
        c[..0x10].fill(create_id);
        c[0x10..0x24].fill(name);
        c[0x24] = 0;
        c[0x25] = 0;
        c
    }

    fn slot(index: usize, slot_id: u32, chara: u64, face: u8, create_id: u8, specials: [u8; 4], colour: u8) -> RawSlot {
        RawSlot {
            index,
            slot_id,
            name_len: 2,
            name_units: [b'A' as u16, b'B' as u16, 0, 0, 0, 0, 0, 0, 0, 0],
            colour,
            voice: 37,
            specials,
            tag_outfit: TaggedId(OUTFIT_4155),
            tag_hat: TaggedId(HAT_DEFAULT),
            tag_chara: TaggedId(chara),
            charinfo: charinfo(create_id, b'N', face),
        }
    }

    #[test]
    fn class_from_verified_chara_ids() {
        assert_eq!(MiiClass::from_chara_id(TaggedId(CHARA_BRAWLER)), Ok(MiiClass::Brawler));
        assert_eq!(MiiClass::from_chara_id(TaggedId(CHARA_SWORD)), Ok(MiiClass::Swordfighter));
        assert_eq!(MiiClass::from_chara_id(TaggedId(CHARA_GUNNER)), Ok(MiiClass::Gunner));
        assert_eq!(MiiClass::from_chara_id(TaggedId(0xc1000013fdfdfb2b)), Ok(MiiClass::Brawler));
        assert_eq!(MiiClass::from_chara_id(TaggedId(0xc2004c13fdfdfb2b)), Err(ClassError::BadTag(0xc2)));
        assert_eq!(MiiClass::from_chara_id(TaggedId(0xc1004c1300000001)), Err(ClassError::UnknownLabel(0x1300000001)));
    }

    #[test]
    fn copied_or_renamed_configurations_collapse_to_lowest_slot_id() {
        let a = slot(0, 4155, CHARA_BRAWLER, 7, 0x11, [1, 1, 2, 1], 1);
        let mut b = slot(1, 2622, CHARA_BRAWLER, 7, 0x22, [1, 1, 2, 1], 1);
        b.name_units[0] = b'Z' as u16;
        let snap = build_snapshot(vec![a, b]);
        assert_eq!(snap.configs.len(), 1);
        assert_eq!(snap.configs[0].slot_id, 2622);
        assert_eq!(snap.configs[0].copies, vec![4155]);
        assert_eq!(snap.findings, vec![Finding::DuplicateCollapsed { slot_id: 4155, representative: 2622 }]);
        assert!(snap.is_complete());
    }

    #[test]
    fn same_appearance_different_class_stays_separate() {
        let a = slot(0, 10, CHARA_BRAWLER, 7, 0x11, [0, 0, 0, 0], 5);
        let b = slot(1, 11, CHARA_GUNNER, 7, 0x11, [0, 0, 0, 0], 5);
        let snap = build_snapshot(vec![a, b]);
        assert_eq!(snap.configs.len(), 2);
        assert_eq!(snap.of_class(MiiClass::Brawler).count(), 1);
        assert_eq!(snap.of_class(MiiClass::Gunner).count(), 1);
        assert_ne!(snap.configs[0].canonical, snap.configs[1].canonical);
    }

    #[test]
    fn specials_colour_and_voice_are_identity() {
        let base = slot(0, 1, CHARA_SWORD, 3, 0x11, [0, 0, 0, 0], 5);
        let other_special = slot(1, 2, CHARA_SWORD, 3, 0x11, [1, 0, 0, 0], 5);
        let other_colour = slot(2, 3, CHARA_SWORD, 3, 0x11, [0, 0, 0, 0], 2);
        let mut other_voice = slot(3, 4, CHARA_SWORD, 3, 0x11, [0, 0, 0, 0], 5);
        other_voice.voice = 3;
        let snap = build_snapshot(vec![base, other_special, other_colour, other_voice]);
        assert_eq!(snap.configs.len(), 4, "a voice-only difference is a different configuration");
        assert!(snap.findings.is_empty());
    }

    #[test]
    fn outfit_and_hat_are_identity_with_correct_semantics() {
        let base = slot(0, 1, CHARA_BRAWLER, 3, 0x11, [0, 0, 0, 0], 0);
        let mut other_outfit = slot(1, 2, CHARA_BRAWLER, 3, 0x11, [0, 0, 0, 0], 0);
        other_outfit.tag_outfit = TaggedId(0x1300291bfef7a5f8);
        let mut other_hat = slot(2, 3, CHARA_BRAWLER, 3, 0x11, [0, 0, 0, 0], 0);
        other_hat.tag_hat = TaggedId(0x5a00030fa9efa5eb);
        let configs = build_snapshot(vec![base, other_outfit, other_hat]).configs;
        assert_eq!(configs.len(), 3);
        assert_eq!(configs[0].canonical.appearance.outfit, OUTFIT_4155 & LOW40);
        assert_eq!(configs[0].canonical.appearance.hat, HAT_DEFAULT & LOW40);
        assert_eq!(configs[1].canonical.appearance.outfit, 0x1bfef7a5f8);
        assert_eq!(configs[1].outfit_hint(), 0x29);
        assert_eq!(configs[2].canonical.appearance.hat, 0x0fa9efa5eb);
        assert_eq!(configs[2].hat_hint(), 3);
        // Swapped tags are reported against the right offsets.
        let mut swapped = slot(0, 9, CHARA_BRAWLER, 3, 0x11, [0, 0, 0, 0], 0);
        swapped.tag_outfit = TaggedId(HAT_DEFAULT);
        assert_eq!(classify(&swapped), Err(Finding::BadTag { slot_index: 0, slot_id: 9, field: "+0x130", tag: 0x5a }));
        let mut swapped_hat = slot(0, 9, CHARA_BRAWLER, 3, 0x11, [0, 0, 0, 0], 0);
        swapped_hat.tag_hat = TaggedId(OUTFIT_4155);
        assert_eq!(classify(&swapped_hat), Err(Finding::BadTag { slot_index: 0, slot_id: 9, field: "+0x138", tag: 0x13 }));
    }

    #[test]
    fn row_hint_bits_do_not_change_identity_but_raw_tags_keep_them() {
        let a = slot(0, 5, CHARA_BRAWLER, 9, 0x11, [2, 2, 2, 2], 0);
        let mut b = slot(1, 6, 0xc1000013fdfdfb2b, 9, 0x11, [2, 2, 2, 2], 0);
        b.tag_outfit = TaggedId(0x1300ff1bfef7a5f7);
        b.tag_hat = TaggedId(0x5a00010fa9efa5ea);
        let ca = classify(&a).unwrap();
        let cb = classify(&b).unwrap();
        assert_eq!(ca.canonical, cb.canonical);
        assert_eq!(ca.canonical.to_bytes(), cb.canonical.to_bytes());
        assert_ne!(ca.raw_tags, cb.raw_tags);
        let snap = build_snapshot(vec![a, b]);
        assert_eq!(snap.configs.len(), 1);
        assert_eq!(snap.configs[0].slot_id, 5);
    }

    #[test]
    fn deliberate_hash_collision_does_not_merge_different_bytes() {
        let a = classify(&slot(0, 20, CHARA_BRAWLER, 1, 0x11, [0, 0, 0, 0], 0)).unwrap();
        let b = classify(&slot(1, 21, CHARA_BRAWLER, 2, 0x11, [0, 0, 0, 0], 0)).unwrap();
        let (out, findings) = dedupe_with(vec![a, b], |_| 7);
        assert_eq!(out.len(), 2);
        assert!(findings.is_empty());
    }

    #[test]
    fn invalid_and_unknown_slots_are_reported_not_dropped() {
        let bad_special = slot(0, 30, CHARA_BRAWLER, 1, 0x11, [3, 0, 0, 0], 0);
        let bad_tag = slot(1, 31, 0xc2004c13fdfdfb2b, 1, 0x11, [0, 0, 0, 0], 0);
        let unknown = slot(2, 32, 0xc1004c1300000001, 1, 0x11, [0, 0, 0, 0], 0);
        let mut no_name = slot(3, 33, CHARA_GUNNER, 1, 0x11, [0, 0, 0, 0], 0);
        no_name.name_len = 0;
        let good = slot(4, 34, CHARA_GUNNER, 1, 0x11, [0, 0, 0, 0], 0);
        let snap = build_snapshot(vec![bad_special, bad_tag, unknown, no_name, good]);
        assert_eq!(snap.slots_seen, 5);
        assert_eq!(snap.configs.len(), 1);
        assert_eq!(snap.configs[0].slot_id, 34);
        assert_eq!(snap.defects().count(), 4);
        assert!(!snap.is_complete());
        assert!(snap.findings.contains(&Finding::SpecialOutOfRange { slot_index: 0, slot_id: 30, specials: [3, 0, 0, 0] }));
        assert!(snap.findings.contains(&Finding::BadTag { slot_index: 1, slot_id: 31, field: "+0x1a0", tag: 0xc2 }));
        assert!(snap.findings.contains(&Finding::UnknownClassLabel { slot_index: 2, slot_id: 32, hash40: 0x1300000001 }));
        assert!(snap.findings.contains(&Finding::NameLength { slot_index: 3, slot_id: 33, len: 0 }));
    }

    #[test]
    fn persisted_identity_round_trips_and_rejects_tampering_and_old_layout() {
        let cfg = classify(&slot(0, 4155, CHARA_BRAWLER, 7, 0x11, [1, 1, 2, 1], 1)).unwrap();
        let hex = cfg.canonical.to_hex();
        assert_eq!(hex.len(), 220);
        let back = Canonical::from_hex(&hex).unwrap();
        assert_eq!(back, cfg.canonical);
        assert_eq!(back.to_bytes(), cfg.canonical.to_bytes());
        let mut bytes = cfg.canonical.to_bytes();
        bytes[0] = 0x4a;
        assert_eq!(Canonical::from_bytes(&bytes), Err(CanonicalError::ClassMismatch));
        bytes[0] = 0x07;
        assert_eq!(Canonical::from_bytes(&bytes), Err(CanonicalError::ClassByte(0x07)));
        assert_eq!(Canonical::from_hex("abc"), Err(CanonicalError::Length(3)));
        // A revision-1 string (218 chars) is rejected by length, never reinterpreted.
        assert_eq!(Canonical::from_hex(&"0".repeat(218)), Err(CanonicalError::Length(218)));
        let mut odd = hex.clone();
        odd.replace_range(0..1, "z");
        assert_eq!(Canonical::from_hex(&odd), Err(CanonicalError::Hex));
    }

    #[test]
    fn canonical_layout_is_exact() {
        let mut raw = slot(0, 4155, CHARA_BRAWLER, 7, 0x11, [1, 1, 2, 1], 1);
        raw.voice = 37;
        let cfg = classify(&raw).unwrap();
        let b = cfg.canonical.to_bytes();
        assert_eq!(b.len(), 0x6e);
        assert_eq!(b[0], 0x48);
        assert!(b[1..1 + CHARINFO_MASK_END].iter().all(|x| *x == 0));
        assert_eq!(&b[0x59..0x5d], &[1, 1, 2, 1]);
        assert_eq!(b[0x5d], 1);
        assert_eq!(b[0x5e], 37);
        assert_eq!(get40(&b[0x5f..0x64]), OUTFIT_4155 & LOW40);
        assert_eq!(get40(&b[0x64..0x69]), HAT_DEFAULT & LOW40);
        assert_eq!(get40(&b[0x69..0x6e]), HASH40_UI_CHARA_MIIFIGHTER);
    }
}
