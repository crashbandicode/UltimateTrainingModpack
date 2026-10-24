//! Bounded, read-only native saved-file adapter. It is not called automatically.
//! Two matching reads detect change; they do not provide an atomic game-save snapshot.

use std::io::Read;
use std::path::Path;

use crate::inventory::{build_snapshot, Finding, RawSlot, Snapshot, TaggedId, CHARINFO_LEN, SLOT_SIZE};

pub const HEADER_LEN: usize = 0x1b0;
pub const MAX_FILES: usize = 99;
pub const MAX_DIR_ENTRIES: usize = 128;

/// Canonical filenames only: prevents a listed alias from opening a different synthesized path.
pub fn parse_name(name: &str) -> Option<u32> {
    let digits = name.strip_prefix("mii_")?.strip_suffix(".bin")?;
    if digits.is_empty() || digits.len() > 10 || !digits.bytes().all(|b| b.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
    {
        return None;
    }
    digits.parse().ok()
}

pub fn select_ids(names: impl Iterator<Item = Result<String, String>>) -> Result<Vec<u32>, String> {
    let mut ids = Vec::new();
    for (index, name) in names.enumerate() {
        if index >= MAX_DIR_ENTRIES {
            return Err(format!("directory entries exceed {MAX_DIR_ENTRIES}"));
        }
        let name = name.map_err(|e| format!("directory entry {index}: {e}"))?;
        if let Some(id) = parse_name(&name) {
            ids.push(id);
            if ids.len() > MAX_FILES {
                return Err(format!("saved files exceed {MAX_FILES}"));
            }
        }
    }
    ids.sort_unstable();
    Ok(ids)
}

/// Owned bytes only. Validation of class, tags, name length and specials is shared with inventory.
pub fn parse_header(id: u32, index: usize, bytes: &[u8; HEADER_LEN]) -> Result<RawSlot, String> {
    let u32_at = |off| u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
    let u64_at = |off| u64::from_le_bytes(bytes[off..off + 8].try_into().unwrap());
    let stored = u32_at(0);
    if stored != id {
        return Err(format!("filename id {id} differs from header id {stored}"));
    }
    let mut name_units = [0; 10];
    for (i, unit) in name_units.iter_mut().enumerate() {
        *unit = u16::from_le_bytes(bytes[0x118 + i * 2..0x11a + i * 2].try_into().unwrap());
    }
    let mut charinfo = [0; CHARINFO_LEN];
    charinfo.copy_from_slice(&bytes[0x148..0x1a0]);
    Ok(RawSlot {
        index, slot_id: id, name_len: u32_at(0x114), name_units,
        colour: bytes[0x140], voice: bytes[0x141],
        specials: bytes[0x142..0x146].try_into().unwrap(),
        tag_outfit: TaggedId(u64_at(0x130)), tag_hat: TaggedId(u64_at(0x138)),
        tag_chara: TaggedId(u64_at(0x1a0)), charinfo,
    })
}

fn check_reads(first: (u64, [u8; HEADER_LEN]), second: (u64, [u8; HEADER_LEN])) -> Result<[u8; HEADER_LEN], String> {
    if first.0 != SLOT_SIZE as u64 || second.0 != SLOT_SIZE as u64 {
        return Err(format!("unexpected file lengths {:#x}/{:#x}; expected {SLOT_SIZE:#x}", first.0, second.0));
    }
    if first.1 != second.1 {
        return Err("header changed between two reads".into());
    }
    Ok(first.1)
}

/// Only opens files for reading; each read is limited to HEADER_LEN. No native pointer is retained.
pub fn read_stable_header(dir: &Path, id: u32) -> Result<[u8; HEADER_LEN], String> {
    fn read(path: &Path) -> std::io::Result<(u64, [u8; HEADER_LEN])> {
        let mut file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        let mut header = [0; HEADER_LEN];
        file.read_exact(&mut header)?;
        Ok((len, header))
    }
    let path = dir.join(format!("mii_{id}.bin"));
    let first = read(&path).map_err(|e| format!("read1: {e}"))?;
    let second = read(&path).map_err(|e| format!("read2: {e}"))?;
    check_reads(first, second)
}

/// No partial result is silently labelled complete. Retains valid owned rows for diagnostics,
/// but selection rejects the entire snapshot while any source/validation defect remains.
pub fn catalog_from(
    names: impl Iterator<Item = Result<String, String>>,
    mut read: impl FnMut(u32) -> Result<[u8; HEADER_LEN], String>,
) -> Snapshot {
    let ids = match select_ids(names) {
        Ok(ids) => ids,
        Err(error) => return Snapshot { findings: vec![Finding::Source { slot_id: None, error }], ..Snapshot::default() },
    };
    let slots_seen = ids.len();
    let mut slots = Vec::with_capacity(slots_seen);
    let mut findings = Vec::new();
    for (index, id) in ids.into_iter().enumerate() {
        // Native SelectData: 13.0.5 main+0x19d1770 / +0x19d1850.
        // Keep source/ID validation before excluding this explicit native state.
        match read(id).and_then(|header| {
            let flag = header[0x1a9];
            parse_header(id, index, &header).map(|slot| (slot, flag))
        }) {
            Ok((slot, 0)) => slots.push(slot),
            Ok((slot, flag)) => findings.push(Finding::NativeExcluded {
                slot_index: slot.index, slot_id: id, flag,
            }),
            Err(error) => findings.push(Finding::Source { slot_id: Some(id), error }),
        }
    }
    let mut snapshot = build_snapshot(slots);
    snapshot.slots_seen = slots_seen;
    snapshot.findings.extend(findings);
    snapshot
}

/// Call only at an explicitly scheduled read boundary. This module installs no hooks or poller.
pub fn read_catalog(dir: &Path) -> Snapshot {
    match std::fs::read_dir(dir) {
        Ok(entries) => catalog_from(entries.map(|entry| {
            let entry = entry.map_err(|e| e.to_string())?;
            entry.file_name().into_string().map_err(|_| "non-UTF8 directory entry".into())
        }), |id| read_stable_header(dir, id)),
        Err(e) => Snapshot { findings: vec![Finding::Source { slot_id: None, error: format!("read_dir: {e}") }], ..Snapshot::default() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{MiiClass, HASH40_UI_CHARA_MIISWORDSMAN};
    use crate::selection::{resolve, PlayerOrigin, Preferences, Role};

    fn header(id: u32) -> [u8; HEADER_LEN] {
        let mut h = [0; HEADER_LEN];
        h[..4].copy_from_slice(&id.to_le_bytes());
        h[0x114..0x118].copy_from_slice(&1u32.to_le_bytes());
        h[0x118..0x11a].copy_from_slice(&(b'A' as u16).to_le_bytes());
        h[0x130..0x138].copy_from_slice(&0x1300291cce3b03f5u64.to_le_bytes());
        h[0x138..0x140].copy_from_slice(&0x5a00000fa9efa5eau64.to_le_bytes());
        h[0x1a0..0x1a8].copy_from_slice(&(0xc100000000000000u64 | HASH40_UI_CHARA_MIISWORDSMAN).to_le_bytes());
        h[0x141] = 37;
        h
    }

    #[test]
    fn native_header_keeps_owned_name_tokens_and_identity() {
        let snap = catalog_from([Ok("mii_7.bin".into())].into_iter(), |_| Ok(header(7)));
        assert!(snap.is_complete());
        let c = &snap.configs[0];
        assert_eq!((c.name.as_str(), c.slot_id, c.outfit_hint(), c.hat_hint()), ("A", 7, 41, 0));
        assert_eq!(c.canonical.class, MiiClass::Swordfighter);
        assert_eq!(c.canonical.appearance.voice, 37);
    }

    #[test]
    fn unreadable_or_invalid_file_makes_entire_catalog_unselectable() {
        for failure in [Err("read denied".into()), Ok(header(99))] {
            let snap = catalog_from([Ok("mii_7.bin".into()), Ok("mii_8.bin".into())].into_iter(), |id| {
                if id == 7 { Ok(header(7)) } else { failure.clone() }
            });
            assert_eq!(snap.configs.len(), 1);
            assert_eq!(snap.slots_seen, 2);
            assert!(!snap.is_complete());
            assert!(resolve(&snap, &Preferences::default(), Role::Cpu, MiiClass::Swordfighter, PlayerOrigin::NotMii).is_none());
        }
    }

    #[test]
    fn enumeration_errors_and_bounds_do_not_publish_partial_inventory() {
        let error = catalog_from([Ok("mii_7.bin".into()), Err("entry error".into())].into_iter(), |_| panic!("must not read incomplete enumeration"));
        assert!(!error.is_complete());
        assert!(select_ids((0..100).map(|id| Ok(format!("mii_{id}.bin")))).is_err());
        assert!(select_ids((0..129).map(|_| Ok("other".into()))).is_err());
        assert_eq!(parse_name("mii_07.bin"), None);
        assert_eq!(parse_name("mii_4294967296.bin"), None);
        assert_eq!(parse_name("mii_7.bin"), Some(7));
    }

    #[test]
    fn changed_or_wrong_length_headers_are_not_accepted() {
        let h = header(7);
        let mut changed = h; changed[0x141] = 3;
        assert!(check_reads((SLOT_SIZE as u64, h), (SLOT_SIZE as u64, changed)).is_err());
        assert!(check_reads((HEADER_LEN as u64, h), (SLOT_SIZE as u64, h)).is_err());
        assert!(check_reads((SLOT_SIZE as u64, h), (SLOT_SIZE as u64 - 1, h)).is_err());
        assert_eq!(check_reads((SLOT_SIZE as u64, h), (SLOT_SIZE as u64, h)), Ok(h));
    }

    #[test]
    fn real_file_adapter_reads_only_and_detects_truncation() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("mii-catalog-test-{}-{unique}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
        let _cleanup = Cleanup(dir.clone());
        let file = dir.join("mii_7.bin");
        let mut bytes = vec![0; SLOT_SIZE]; bytes[..HEADER_LEN].copy_from_slice(&header(7));
        std::fs::write(&file, &bytes).unwrap();
        assert!(read_catalog(&dir).is_complete());
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        std::fs::write(&file, header(7)).unwrap();
        assert!(!read_catalog(&dir).is_complete());
    }

    #[test]
    fn native_excluded_headers_do_not_poison_named_catalog() {
        use crate::inventory::{HASH40_UI_CHARA_MIIFIGHTER, HASH40_UI_CHARA_MIIGUNNER};
        let labels = [HASH40_UI_CHARA_MIIFIGHTER, HASH40_UI_CHARA_MIISWORDSMAN,
            HASH40_UI_CHARA_MIIGUNNER];
        let snap = catalog_from((7..=10).map(|id| Ok(format!("mii_{id}.bin"))), |id| {
            let mut h = header(id);
            if id >= 8 {
                h[0x114..0x118].fill(0);
                h[0x118..0x12e].fill(0);
                h[0x130..0x138].copy_from_slice(&0x13ffff0000000000u64.to_le_bytes());
                h[0x138..0x140].copy_from_slice(&0x5affff0000000000u64.to_le_bytes());
                h[0x1a0..0x1a8].copy_from_slice(&(0xc100000000000000u64 | labels[(id - 8) as usize]).to_le_bytes());
                h[0x1a9] = 1;
            }
            Ok(h)
        });
        assert!(snap.is_complete());
        assert_eq!(snap.slots_seen, 4, "physical file count is not reduced");
        assert_eq!(snap.configs.len(), 1);
        assert_eq!(snap.configs[0].slot_id, 7);
        for id in 8..=10 {
            assert!(snap.findings.contains(&Finding::NativeExcluded {
                slot_index: (id - 7) as usize, slot_id: id, flag: 1,
            }));
        }
    }

    #[test]
    fn unmarked_unnamed_header_remains_a_fatal_defect() {
        let snap = catalog_from([Ok("mii_7.bin".into())].into_iter(), |_| {
            let mut h = header(7);
            h[0x114..0x118].fill(0);
            Ok(h)
        });
        assert!(!snap.is_complete());
        assert_eq!(snap.findings, vec![Finding::NameLength { slot_index: 0, slot_id: 7, len: 0 }]);
    }

    #[test]
    fn native_exclusion_is_independent_of_the_name() {
        let snap = catalog_from([Ok("mii_7.bin".into())].into_iter(), |_| {
            let mut h = header(7); // valid nonempty name
            h[0x1a9] = 1;
            Ok(h)
        });
        assert!(snap.is_complete());
        assert!(snap.configs.is_empty());
        assert_eq!(snap.findings, vec![Finding::NativeExcluded { slot_index: 0, slot_id: 7, flag: 1 }]);
    }

    #[test]
    fn native_exclusion_does_not_hide_source_identity_failures() {
        let snap = catalog_from([Ok("mii_7.bin".into())].into_iter(), |_| {
            let mut h = header(99);
            h[0x1a9] = 1;
            Ok(h)
        });
        assert!(!snap.is_complete());
        assert!(matches!(&snap.findings[..], [Finding::Source { slot_id: Some(7), .. }]));
    }

    #[test]
    fn root_native_excluded_row_does_not_poison_catalog() {
        let snap = catalog_from((7..=8).map(|id| Ok(format!("mii_{id}.bin"))), |id| {
            let mut h = header(id);
            if id == 8 { h[0x114..0x118].fill(0); h[0x1a9] = 1; }
            Ok(h)
        });
        assert!(snap.is_complete(), "native excluded row poisoned catalog");
        assert_eq!(snap.configs.len(), 1);
        assert_eq!(snap.slots_seen, 2);
    }
}
