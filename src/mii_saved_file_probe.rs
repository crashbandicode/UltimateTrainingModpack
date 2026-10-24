//! Development-only, default-off, read-only saved-file probe (feature `mii-saved-file-probe`, implies
//! `mii-identity-probe`). This is an opt-in development probe, not production inventory.
//!
//! Once per process, from the first service callback whose owner-0 entry is a ready single-kind Mii,
//! it enumerates `save:/save_data/mii/mii_<decimal id>.bin`, reads the first 0x1b0 bytes of each
//! (at most 99 files, at most 128 directory entries; more is an explicit failure), validates the
//! header (file-name id == header id, specials <= 2, tag bytes 0x13/0x5a/0xC1, known class label,
//! name length 1..=10) and logs counts, ids, class, colour, specials, CharInfo hashes and IO errors.
//! It never logs raw CharInfo or name bytes, never reads the game's database pointers, installs no
//! hooks, takes no locks, calls no loader, writes nothing.
//!
//! What it establishes: whether the `save:` mount is readable from the plugin and what identity the
//! files carry at that moment (compare with the identity probe's H1 record hash). What it does not
//! establish: snapshot atomicity. Two bounded reads of a header that agree can detect a change
//! between them; they are not a proof that no concurrent write happened.
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use skyline::hooks::{getRegionAddress, Region};
use training_mod_consts::character_forms::EntryState;

pub const DIR: &str = "save:/save_data/mii";
pub const HEADER_LEN: usize = 0x1b0;
pub const SLOT_LEN: u64 = 0x64340;
/// Native loader bound (0x026C01C0 / 0x64340). More matching files is reported, never clamped.
pub const MAX_FILES: usize = 99;
/// Enumeration bound; a directory with more entries is reported as overlong, not processed.
pub const MAX_DIR_ENTRIES: usize = 128;
const LINE_CAP: usize = 120;

static FIRED: AtomicBool = AtomicBool::new(false);
static LINES: AtomicUsize = AtomicUsize::new(0);

fn log(args: std::fmt::Arguments<'_>) {
    if LINES.fetch_add(1, Ordering::Relaxed) < LINE_CAP {
        crate::logging::diagnostic(args);
    }
}

// ------------------------------------------------------------------------------------------ parsing

pub const LOW40: u64 = 0xff_ffff_ffff;
/// `+0x130` (tag 0x13) is the outfit/body id and `+0x138` (tag 0x5a) the hat id: dispatcher
/// category 0x13 resolves `+0x130` through the outfit table, 0x14 resolves `+0x138` through the hat
/// table (artifacts/enhancements/mii-fable/fullconfig-resource-next). Earlier names were swapped.
pub const TAG_OUTFIT: u8 = 0x13;
pub const TAG_HAT: u8 = 0x5a;
pub const TAG_CHARA: u8 = 0xc1;
pub const HASH40_UI_CHARA_MIIFIGHTER: u64 = 0x13_fdfd_fb2b;
pub const HASH40_UI_CHARA_MIISWORDSMAN: u64 = 0x15_d706_1f77;
pub const HASH40_UI_CHARA_MIIGUNNER: u64 = 0x12_57ab_1d87;
pub const CHARINFO_OFF: usize = 0x148;
pub const CHARINFO_LEN: usize = 0x58;
pub const CHARINFO_MASK_END: usize = 0x26;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MiiClass {
    Brawler = 0x48,
    Swordfighter = 0x49,
    Gunner = 0x4a,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderError {
    IdMismatch { name_id: u32, header_id: u32 },
    SpecialOutOfRange([u8; 4]),
    BadTag { field: &'static str, tag: u8 },
    UnknownClassLabel(u64),
    NameLength(u32),
}

/// Parsed header fields. Hashes only; no CharInfo or name bytes are retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub id: u32,
    /// Observed head words (+0x04, +0x08, +0x14): 5, id+1, 0x10b in every known save. Logged, not enforced.
    pub head: [u32; 3],
    pub class: MiiClass,
    pub colour: u8,
    /// `+0x141`: voice index (dispatcher category 0x15; 0..35 named, 37 in all known saves = default path).
    pub voice: u8,
    pub specials: [u8; 4],
    /// `+0x130` outfit id hash40 (tag 0x13).
    pub outfit: u64,
    /// `+0x138` hat id hash40 (tag 0x5a).
    pub hat: u64,
    pub chara: u64,
    pub name_len: u32,
    pub charinfo_fnv: u64,
    pub charinfo_masked_fnv: u64,
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100000001b3))
}

fn le_u32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

fn le_u64(b: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(a)
}

fn class_from_chara(tagged: u64) -> Result<MiiClass, HeaderError> {
    let tag = (tagged >> 56) as u8;
    if tag != TAG_CHARA {
        return Err(HeaderError::BadTag { field: "+0x1a0", tag });
    }
    match tagged & LOW40 {
        HASH40_UI_CHARA_MIIFIGHTER => Ok(MiiClass::Brawler),
        HASH40_UI_CHARA_MIISWORDSMAN => Ok(MiiClass::Swordfighter),
        HASH40_UI_CHARA_MIIGUNNER => Ok(MiiClass::Gunner),
        other => Err(HeaderError::UnknownClassLabel(other)),
    }
}

pub fn parse_header(name_id: u32, bytes: &[u8; HEADER_LEN]) -> Result<Header, HeaderError> {
    let id = le_u32(bytes, 0);
    if id != name_id {
        return Err(HeaderError::IdMismatch { name_id, header_id: id });
    }
    let specials = [bytes[0x142], bytes[0x143], bytes[0x144], bytes[0x145]];
    if specials.iter().any(|s| *s > 2) {
        return Err(HeaderError::SpecialOutOfRange(specials));
    }
    let outfit = le_u64(bytes, 0x130);
    let hat = le_u64(bytes, 0x138);
    let chara = le_u64(bytes, 0x1a0);
    if (outfit >> 56) as u8 != TAG_OUTFIT {
        return Err(HeaderError::BadTag { field: "+0x130", tag: (outfit >> 56) as u8 });
    }
    if (hat >> 56) as u8 != TAG_HAT {
        return Err(HeaderError::BadTag { field: "+0x138", tag: (hat >> 56) as u8 });
    }
    let class = class_from_chara(chara)?;
    let name_len = le_u32(bytes, 0x114);
    if name_len == 0 || name_len > 10 {
        return Err(HeaderError::NameLength(name_len));
    }
    let charinfo = &bytes[CHARINFO_OFF..CHARINFO_OFF + CHARINFO_LEN];
    let mut masked = [0u8; CHARINFO_LEN];
    masked.copy_from_slice(charinfo);
    masked[..CHARINFO_MASK_END].fill(0);
    Ok(Header {
        id,
        head: [le_u32(bytes, 4), le_u32(bytes, 8), le_u32(bytes, 0x14)],
        class,
        colour: bytes[0x140],
        voice: bytes[0x141],
        specials,
        outfit: outfit & LOW40,
        hat: hat & LOW40,
        chara: chara & LOW40,
        name_len,
        charinfo_fnv: fnv1a64(charinfo),
        charinfo_masked_fnv: fnv1a64(&masked),
    })
}

/// Strict canonical `mii_<decimal>.bin`: 1..=10 digits, value fits u32, no leading zero, nothing
/// else. Canonical form keeps `inspect_file`'s `mii_{id}.bin` path identical to the listed name.
pub fn parse_name(name: &str) -> Option<u32> {
    let digits = name.strip_prefix("mii_")?.strip_suffix(".bin")?;
    if digits.is_empty() || digits.len() > 10 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return None;
    }
    digits.parse::<u32>().ok()
}

#[derive(Debug, PartialEq, Eq)]
pub enum EnumError {
    /// A directory entry could not be read: enumeration is incomplete, so nothing is inspected.
    Entry { index: usize, error: String },
    TooManyEntries(usize),
    TooManyFiles(usize),
}

/// Select matching ids from directory entries with explicit bounds. Returns ascending ids.
pub fn select_ids<I: Iterator<Item = Result<String, String>>>(names: I) -> Result<(Vec<u32>, usize), EnumError> {
    let mut ids = Vec::new();
    let mut entries = 0usize;
    for name in names {
        entries += 1;
        if entries > MAX_DIR_ENTRIES {
            return Err(EnumError::TooManyEntries(entries));
        }
        let name = name.map_err(|error| EnumError::Entry { index: entries - 1, error })?;
        if let Some(id) = parse_name(&name) {
            ids.push(id);
        }
    }
    if ids.len() > MAX_FILES {
        return Err(EnumError::TooManyFiles(ids.len()));
    }
    ids.sort_unstable();
    Ok((ids, entries))
}

// ----------------------------------------------------------------------------------- bounded IO

#[derive(Debug)]
pub struct FileReport {
    pub id: u32,
    pub len: Option<u64>,
    pub header: Option<Result<Header, HeaderError>>,
    /// Two bounded reads agreed (change detection between the reads only; not atomicity).
    pub stable: Option<bool>,
    pub io_error: Option<String>,
}

fn read_header(path: &std::path::Path) -> std::io::Result<[u8; HEADER_LEN]> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = [0u8; HEADER_LEN];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

pub fn inspect_file(dir: &str, id: u32) -> FileReport {
    let path = std::path::PathBuf::from(format!("{dir}/mii_{id}.bin"));
    let mut report = FileReport { id, len: None, header: None, stable: None, io_error: None };
    let reads = std::fs::metadata(&path).map_err(|e| format!("metadata: {e}")).and_then(|m| {
        report.len = Some(m.len());
        let first = read_header(&path).map_err(|e| format!("read1: {e}"))?;
        let second = read_header(&path).map_err(|e| format!("read2: {e}"))?;
        Ok((first, second))
    });
    match reads {
        Ok((first, second)) => {
            report.header = Some(parse_header(id, &first));
            report.stable = Some(first == second);
        }
        Err(error) => report.io_error = Some(error),
    }
    report
}

/// Two bounded reads that must agree (change detection between the reads only; not atomicity) of a
/// full-length slot file. Used by the full-configuration probe, which needs the bytes (row hints,
/// CharInfo) rather than the parsed summary.
#[cfg(any(feature = "mii-fullconfig-probe", test))]
pub fn read_stable_header(dir: &str, id: u32) -> Result<[u8; HEADER_LEN], String> {
    let path = std::path::PathBuf::from(format!("{dir}/mii_{id}.bin"));
    let len = std::fs::metadata(&path).map_err(|e| format!("metadata: {e}"))?.len();
    if len != SLOT_LEN {
        return Err(format!("unexpected length {len:#x} (expected {SLOT_LEN:#x})"));
    }
    let first = read_header(&path).map_err(|e| format!("read1: {e}"))?;
    let second = read_header(&path).map_err(|e| format!("read2: {e}"))?;
    if first != second {
        return Err("header changed between two reads".to_string());
    }
    Ok(first)
}

/// Enumerate once and read each header twice. Synchronous and bounded: development probe only.
/// The directory handle is consumed and dropped by `select_ids` before any file is opened.
pub fn inspect_dir(dir: &str) -> Result<(Vec<FileReport>, usize), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir {dir}: {e}"))?;
    let names = entries.map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()).map_err(|e| e.to_string()));
    let (ids, count) = select_ids(names).map_err(|e| format!("{e:?}"))?;
    Ok((ids.into_iter().map(|id| inspect_file(dir, id)).collect(), count))
}

// --------------------------------------------------------------------------------------- trigger

unsafe fn rd<T: Copy>(p: *const u8, off: usize) -> T {
    std::ptr::read_volatile(p.add(off).cast::<T>())
}

fn aligned(p: *const u8) -> bool {
    !p.is_null() && (p as usize).is_multiple_of(8)
}

/// Ready, single-kind Mii entry for owner 0: the same `EntryState::ready` and body checks the roster
/// applies, without the roster catalog admission. Reads only the live entry handed to the callback.
unsafe fn ready_mii_owner0(entry: *const u8) -> Option<u32> {
    if !aligned(entry) || rd::<u32>(entry, 0xe0) != 0 {
        return None;
    }
    let state = EntryState {
        count: rd(entry, 0x14),
        kinds: [rd(entry, 0x18), rd(entry, 0x1c), rd(entry, 0x20)],
        actor_count: rd(entry, 0x28),
        installed: rd(entry, 0x4150),
        index: rd(entry, 0x5918),
        previous_or_next: rd(entry, 0x5919),
        pending: rd(entry, 0x591a),
        flags: rd(entry, 0x591f),
        outer: rd(entry, 0x5920),
        loader: rd(entry, 0x591c),
        changing: rd(entry, 0x5934),
        guard: rd(entry, 0x5935),
    };
    if !(0x48..=0x4a).contains(&state.kinds[0]) {
        return None;
    }
    let (forms, kind) = state.ready()?;
    if forms.count != 1 {
        return None;
    }
    let body = rd::<*const u8>(entry, 0x4160);
    let base = getRegionAddress(Region::Text) as usize;
    if !aligned(body)
        || rd::<usize>(body, 0) != base + 0x4f80210
        || rd::<u32>(body, 0xc) != kind
        || rd::<u64>(body, 0x10) != 0
        || rd::<u8>(body, 0x3a) != forms.body_state(0, state.index)
    {
        return None;
    }
    Some(kind)
}

/// Called from `character_roster::service` after the owner filter. One shot per process.
pub unsafe fn on_service(entry: *const u8, owner: u32) {
    if owner != 0 || FIRED.load(Ordering::Relaxed) {
        return;
    }
    let Some(kind) = ready_mii_owner0(entry) else {
        return;
    };
    if FIRED.swap(true, Ordering::Relaxed) {
        return;
    }
    run_once(kind);
}

fn run_once(kind: u32) {
    let start = Instant::now();
    match inspect_dir(DIR) {
        Err(error) => {
            log(format_args!(
                "[MiiFileProbe] dir={DIR} error={error} kind={kind:#x} elapsed_us={}",
                start.elapsed().as_micros()
            ));
        }
        Ok((reports, entries)) => {
            let (mut valid, mut invalid, mut io_errors, mut bad_len) = (0usize, 0usize, 0usize, 0usize);
            let (mut brawler, mut sword, mut gunner) = (0usize, 0usize, 0usize);
            for r in &reports {
                match (&r.header, &r.io_error) {
                    (Some(Ok(h)), None) => {
                        valid += 1;
                        match h.class {
                            MiiClass::Brawler => brawler += 1,
                            MiiClass::Swordfighter => sword += 1,
                            MiiClass::Gunner => gunner += 1,
                        }
                        log(format_args!(
                            "[MiiFileProbe] file id={} len={:#x?} class={:?} colour={} voice={} specials={:?} outfit={:010x} hat={:010x} chara={:010x} charinfo_fnv={:016x} masked_fnv={:016x} name_len={} head=[{},{},{:#x}] stable={:?}",
                            h.id, r.len, h.class, h.colour, h.voice, h.specials, h.outfit, h.hat, h.chara,
                            h.charinfo_fnv, h.charinfo_masked_fnv, h.name_len, h.head[0], h.head[1], h.head[2], r.stable
                        ));
                    }
                    (Some(Err(e)), None) => {
                        invalid += 1;
                        log(format_args!("[MiiFileProbe] file id={} len={:#x?} invalid={:?} stable={:?}", r.id, r.len, e, r.stable));
                    }
                    (_, Some(error)) => {
                        io_errors += 1;
                        log(format_args!("[MiiFileProbe] file id={} len={:#x?} io_error={}", r.id, r.len, error));
                    }
                    (None, None) => {}
                }
                if r.len.is_some_and(|l| l != SLOT_LEN) {
                    bad_len += 1;
                    log(format_args!("[MiiFileProbe] file id={} unexpected_len={:#x?} (expected {:#x})", r.id, r.len, SLOT_LEN));
                }
            }
            // Uncapped: per-file lines may exhaust LINE_CAP, the summary must still appear.
            crate::logging::diagnostic(format_args!(
                "[MiiFileProbe] summary dir_entries={entries} files={} valid={valid} invalid={invalid} io_errors={io_errors} bad_len={bad_len} brawler={brawler} sword={sword} gunner={gunner} suppressed={} fired_kind={kind:#x} elapsed_us={}",
                reports.len(),
                LINES.load(Ordering::Relaxed).saturating_sub(LINE_CAP),
                start.elapsed().as_micros()
            ));
        }
    }
}

pub fn init() {
    crate::logging::diagnostic(format_args!(
        "[MiiFileProbe] compiled: one-shot header read of {DIR} on the first ready owner-0 Mii entry (max {MAX_FILES} files, {HEADER_LEN:#x} bytes each)"
    ));
}

// ------------------------------------------------------------------------------------------ tests
// Pure parsing and bounded-IO tests on synthetic fixtures. Not run in this pass (no cargo while a
// foreign Eden is active); root runs them in a build window.
#[cfg(test)]
mod tests {
    use super::*;

    const CHARA_BRAWLER: u64 = 0xc1004c13fdfdfb2b;
    const CHARA_SWORD: u64 = 0xc1004d15d7061f77;
    const OUTFIT: u64 = 0x1300011bfef7a5f7;
    const HAT: u64 = 0x5a00000fa9efa5ea;

    fn fixture(id: u32, chara: u64, specials: [u8; 4], colour: u8, name_len: u32, prefix: u8) -> [u8; HEADER_LEN] {
        let mut b = [0u8; HEADER_LEN];
        b[0..4].copy_from_slice(&id.to_le_bytes());
        b[4..8].copy_from_slice(&5u32.to_le_bytes());
        b[8..12].copy_from_slice(&(id + 1).to_le_bytes());
        b[0x14..0x18].copy_from_slice(&0x10bu32.to_le_bytes());
        b[0x114..0x118].copy_from_slice(&name_len.to_le_bytes());
        b[0x130..0x138].copy_from_slice(&OUTFIT.to_le_bytes());
        b[0x138..0x140].copy_from_slice(&HAT.to_le_bytes());
        b[0x140] = colour;
        b[0x141] = 37;
        b[0x142..0x146].copy_from_slice(&specials);
        for (i, byte) in b[CHARINFO_OFF..CHARINFO_OFF + CHARINFO_LEN].iter_mut().enumerate() {
            *byte = if i < CHARINFO_MASK_END { prefix } else { 0x40 + (i as u8 % 7) };
        }
        b[0x1a0..0x1a8].copy_from_slice(&chara.to_le_bytes());
        b
    }

    #[test]
    fn parses_known_brawler_header() {
        let h = parse_header(4155, &fixture(4155, CHARA_BRAWLER, [1, 1, 2, 1], 1, 2, 0x11)).unwrap();
        assert_eq!(h.id, 4155);
        assert_eq!(h.class, MiiClass::Brawler);
        assert_eq!(h.colour, 1);
        assert_eq!(h.specials, [1, 1, 2, 1]);
        assert_eq!(h.outfit, OUTFIT & LOW40);
        assert_eq!(h.hat, HAT & LOW40);
        assert_eq!(h.voice, 37);
        assert_eq!(h.chara, HASH40_UI_CHARA_MIIFIGHTER);
        assert_eq!(h.head, [5, 4156, 0x10b]);
        assert_eq!(h.name_len, 2);
    }

    #[test]
    fn masked_hash_ignores_create_id_and_name() {
        let a = parse_header(1, &fixture(1, CHARA_SWORD, [0; 4], 5, 2, 0x11)).unwrap();
        let b = parse_header(1, &fixture(1, CHARA_SWORD, [0; 4], 5, 2, 0x22)).unwrap();
        assert_ne!(a.charinfo_fnv, b.charinfo_fnv);
        assert_eq!(a.charinfo_masked_fnv, b.charinfo_masked_fnv);
    }

    #[test]
    fn rejects_invalid_headers() {
        assert_eq!(parse_header(2622, &fixture(2623, CHARA_SWORD, [0; 4], 5, 2, 0)), Err(HeaderError::IdMismatch { name_id: 2622, header_id: 2623 }));
        assert_eq!(parse_header(1, &fixture(1, CHARA_SWORD, [3, 0, 0, 0], 5, 2, 0)), Err(HeaderError::SpecialOutOfRange([3, 0, 0, 0])));
        assert_eq!(parse_header(1, &fixture(1, 0xc2004c13fdfdfb2b, [0; 4], 5, 2, 0)), Err(HeaderError::BadTag { field: "+0x1a0", tag: 0xc2 }));
        assert_eq!(parse_header(1, &fixture(1, 0xc1004c1300000001, [0; 4], 5, 2, 0)), Err(HeaderError::UnknownClassLabel(0x1300000001)));
        assert_eq!(parse_header(1, &fixture(1, CHARA_SWORD, [0; 4], 5, 0, 0)), Err(HeaderError::NameLength(0)));
        assert_eq!(parse_header(1, &fixture(1, CHARA_SWORD, [0; 4], 5, 11, 0)), Err(HeaderError::NameLength(11)));
        let mut bad_outfit = fixture(1, CHARA_SWORD, [0; 4], 5, 2, 0);
        bad_outfit[0x137] = 0x14;
        assert_eq!(parse_header(1, &bad_outfit), Err(HeaderError::BadTag { field: "+0x130", tag: 0x14 }));
        let mut bad_hat = fixture(1, CHARA_SWORD, [0; 4], 5, 2, 0);
        bad_hat[0x13f] = 0x5b;
        assert_eq!(parse_header(1, &bad_hat), Err(HeaderError::BadTag { field: "+0x138", tag: 0x5b }));
    }

    #[test]
    fn strict_file_names() {
        assert_eq!(parse_name("mii_4155.bin"), Some(4155));
        assert_eq!(parse_name("mii_0.bin"), Some(0));
        assert_eq!(parse_name("Mii_1.bin"), None);
        assert_eq!(parse_name("mii_x.bin"), None);
        assert_eq!(parse_name("mii_1.bin.bak"), None);
        assert_eq!(parse_name("mii_.bin"), None);
        assert_eq!(parse_name("mii_99999999999.bin"), None);
        assert_eq!(parse_name("mii_01.bin"), None);
        assert_eq!(parse_name("mii_00.bin"), None);
    }

    #[test]
    fn enumeration_bounds_are_explicit_failures() {
        let ok: Vec<String> = (0..99).map(|i| format!("mii_{i}.bin")).chain(["readme.txt".to_string()]).collect();
        let (ids, entries) = select_ids(ok.into_iter().map(Ok)).unwrap();
        assert_eq!(ids.len(), 99);
        assert_eq!(entries, 100);
        assert!(ids.windows(2).all(|w| w[0] < w[1]));
        let too_many: Vec<String> = (0..100).map(|i| format!("mii_{i}.bin")).collect();
        assert_eq!(select_ids(too_many.into_iter().map(Ok)).unwrap_err(), EnumError::TooManyFiles(100));
        let overlong: Vec<String> = (0..129).map(|i| format!("other_{i}")).collect();
        assert_eq!(select_ids(overlong.into_iter().map(Ok)).unwrap_err(), EnumError::TooManyEntries(129));
    }

    #[test]
    fn entry_error_fails_enumeration() {
        let names = [Ok("mii_1.bin".to_string()), Err("entry io".to_string()), Ok("mii_2.bin".to_string())];
        assert_eq!(
            select_ids(IntoIterator::into_iter(names)).unwrap_err(),
            EnumError::Entry { index: 1, error: "entry io".to_string() }
        );
    }

    #[test]
    fn reads_headers_from_temp_fixture_files() {
        let dir = std::env::temp_dir().join(format!("mii-files-reviewed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut full = vec![0u8; SLOT_LEN as usize];
        full[..HEADER_LEN].copy_from_slice(&fixture(2622, CHARA_SWORD, [0; 4], 5, 2, 0x11));
        std::fs::write(dir.join("mii_2622.bin"), &full).unwrap();
        full[..HEADER_LEN].copy_from_slice(&fixture(4155, CHARA_BRAWLER, [1, 1, 2, 1], 1, 2, 0x33));
        std::fs::write(dir.join("mii_4155.bin"), &full).unwrap();
        std::fs::write(dir.join("mii_7.bin"), [0u8; 0x100]).unwrap(); // too short: IO error, no header
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        let (reports, entries) = inspect_dir(dir.to_str().unwrap()).unwrap();
        assert_eq!(entries, 4);
        assert_eq!(reports.iter().map(|r| r.id).collect::<Vec<_>>(), vec![7, 2622, 4155]);
        assert!(reports[0].io_error.as_deref().is_some_and(|e| e.starts_with("read1: ")) && reports[0].header.is_none());
        assert_eq!(reports[0].len, Some(0x100));
        let stable = read_stable_header(dir.to_str().unwrap(), 2622).unwrap();
        assert_eq!(parse_header(2622, &stable).unwrap().class, MiiClass::Swordfighter);
        assert!(read_stable_header(dir.to_str().unwrap(), 7).unwrap_err().starts_with("unexpected length"));
        assert!(read_stable_header(dir.to_str().unwrap(), 9).unwrap_err().starts_with("metadata: "));
        let missing = inspect_file(dir.to_str().unwrap(), 9);
        assert!(missing.io_error.as_deref().is_some_and(|e| e.starts_with("metadata: ")) && missing.len.is_none());
        let sword = reports[1].header.as_ref().unwrap().as_ref().unwrap();
        assert_eq!((sword.class, sword.colour), (MiiClass::Swordfighter, 5));
        assert_eq!(reports[1].stable, Some(true));
        let brawler = reports[2].header.as_ref().unwrap().as_ref().unwrap();
        assert_eq!((brawler.class, brawler.specials), (MiiClass::Brawler, [1, 1, 2, 1]));
        assert_eq!(reports[2].len, Some(SLOT_LEN));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
