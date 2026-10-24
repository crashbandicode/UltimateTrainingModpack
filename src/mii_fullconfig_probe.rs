//! Development-only, default-off, two-shot same-class full Mii configuration probe (feature
//! `mii-fullconfig-probe`; implies `mii-identical-reload-probe` and `mii-saved-file-probe`; mutually
//! exclusive with `mii-special-change-probe`, see lib.rs). Native Training, P1 (owner 0), one
//! experiment per process, driven by `character_roster::mii_reload_probe` (chord, install token,
//! identical-descriptor reload, completion/destruction tracking):
//!
//! * shot 1 (L3+X): capture the live configuration, write the alternate same-class saved
//!   configuration (record CharInfo under the native pool lock; outfit, colour and voice tokens and
//!   the four specials in the saved row `E`, the live entry `FE` and the owner object `O`; outfit,
//!   colour and specials in the information block `D`, whose verified token copy excludes voice), then the
//!   identical reload;
//! * shot 2 (L3+X): on the same live entry, after a fresh table binding and byte-exact state checks,
//!   write every captured byte back in the same order, then the identical reload again.
//!
//! Never written: anything after entry destruction or scene departure (a pending change is logged,
//! not repaired), the pool vector size, the record's shared pointer (`+0x58/+0x60`), hat tokens
//! (the alternate must carry the same hat row), other owners, files. No native pointer captured at
//! shot 1 is dereferenced later: `E` is static and re-bound, `FE` is the live callback argument,
//! `D`, `O` and the record are re-derived from it on every shot. Evidence and acceptance notes:
//! artifacts/enhancements/mii-fable/fullconfig-probe-next/.
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::Mutex;

#[cfg(feature = "mii-native-adapter-probe")]
#[path = "mii_native_adapter.rs"]
mod native_adapter;
#[cfg(feature = "mii-cpu-adapter-probe")]
#[path = "mii_closed_ack.rs"]
pub(crate) mod ack;

#[cfg(feature = "mii-cpu-adapter-probe")]
pub(crate) fn ack_phase() -> u8 { PHASE.load(Ordering::Relaxed) }
#[cfg(feature = "mii-cpu-adapter-probe")]
fn ack_tokens(t: Tokens) -> serde_json::Value {
    serde_json::json!({"outfit":t.outfit,"hat":t.hat,"colour":t.colour,"voice":t.voice,"specials":t.specials})
}

use skyline::hooks::{getRegionAddress, InlineCtx, Region};

use crate::mii_identity_probe::bind_saved_entry;
use crate::mii_saved_file_probe::{
    fnv1a64, parse_header, read_stable_header, select_ids, Header, HeaderError, CHARINFO_LEN, CHARINFO_MASK_END,
    CHARINFO_OFF, DIR, HEADER_LEN,
};

/// Third arm file; the identical-reload arm file must exist too (`mii_reload_probe::init`).
#[cfg(not(feature = "mii-cpu-adapter-probe"))]
pub const ARM_FILE: &str = "sd:/ultimate/TrainingModpack/mii_fullconfig_probe.arm";
#[cfg(feature = "mii-cpu-adapter-probe")]
pub const ARM_FILE: &str = "sd:/ultimate/TrainingModpack/mii_cpu_adapter_probe.arm";
const LINE_CAP: usize = 160;
const RECORD_LEN: usize = 0x68;
/// `*(main+POOL_HOLDER)` is the singleton holder; its first word is the pool manager `G`
/// (`0x14ebbb0..0x14ebbb4`, `0x67eb34..0x67eb3c`). Records: `[G+0x24d0]..[G+0x24d8]`, 0x68 each.
const POOL_HOLDER: usize = 0x593b6a0;
/// `*(main+OWNER_TABLE)` is the owner table; `[table + 0xe8 + id*8]` is the owner Mii object `O`
/// (`0x66dda8..0x66ddb4`, `0x67eb1c..0x67eb28`, `0x653288..0x65329c`).
/// `[[main+UI_DB]+8]` is the ui database object; `+0x248` outfit table, `+0x238` hat table
/// (`0x17dfb78..0x17dfb84`; hat per the pass-15 resource report).
const OUTFIT_TABLE: usize = 0x248;
const HAT_TABLE: usize = 0x238;
const OUTFIT_ROW: usize = 0x3282b70;
const HAT_ROW: usize = 0x32870b0;
/// Complete read-only row getters and their shared leaf. Each getter takes [table+8], not table,
/// follows the named child of the PRC root map, bounds-checks that child's row count and returns a
/// tagged row id. The root map count and global key dictionary are not outfit/hat row arrays.
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
    (0x66ddfc, [0x290da688, 0x290eae8a, 0x3901f28e, 0x3901f68f], "E->O token copy (stp w8, w9, [x20, #0x6c])"),
    (0x17e69b4, [0x97ffe42b, 0xf9479380, 0x394983ec, 0x340002cc], "body request bl 0x17dfa60 (owner load step)"),
    (0x17e71a0, [0x97ffe230, 0xf9479380, 0x394983ec, 0x340002cc], "voice request bl 0x17dfa60 (owner load step)"),
    (0x657e18, [0x94461f12, 0xf00266db, 0xf9479360, 0x394003ec], "voice request bl 0x17dfa60 (setup path)"),
    (0x67ead0, [0xd10343ff, 0xa90b4ff4, 0xa90c7bfd, 0x910303fd], "face texture function entry"),
    (LOCK, [0xd000c6f0, 0xf945e211, 0x912f0210, 0xd61f0220], "pool lock import stub"),
    (UNLOCK, [0xd000c6f0, 0xf945e611, 0x912f2210, 0xd61f0220], "pool unlock import stub"),
    (INSTALL, [0xd101c3ff, 0xf90013f9, 0xa9035ff8, 0xa90457f6], "descriptor install entry"),
    (OUTFIT_ROW, [0xf81e0ff3, 0xa9017bfd, 0x910043fd, 0xf9401008], "outfit row tagged-id getter"),
    (HAT_ROW, [0xf81e0ff3, 0xa9017bfd, 0x910043fd, 0xf9401008], "hat row tagged-id getter"),
];
/// The canonical applier insert (`0x14ebbac..0x14ebc60`) takes the same lock around its raw record
/// copy: `bl 0x39c19c0` at `0x14ebbbc`, `bl 0x39c19d0` at `0x14ebc4c`. Checked as a cross-reference.
const APPLIER_LOCK_SITES: [(usize, u32); 2] = [(0x14ebbbc, 0x94935781), (0x14ebc4c, 0x94935761)];
/// `D` token mirrors: index, outfit, hat, colour.
const D_TOKENS: [usize; 4] = [0x72, 0x74, 0x76, 0x78];
/// Observed native writer of `D_TOKENS` (load step `0x654dc0`): four `ldrh w9, [x19, #FE]`,
/// `ldr x11, [x19, #0xf8]`, `strh w9, [x11, #D]` triples for `FE+0x46/0x48/0x4a/0x4c`, then
/// `ldr w9, [x19, #0x30]`. This copy excludes voice `FE+0x4e`; `D+0x7a` remains unmapped.
const D_MIRROR_COPY: (usize, [u32; 13]) = (
    0x654eec,
    [
        0x79408e69, 0xf9407e6b, 0x7900e569, 0x79409269, 0xf9407e6b, 0x7900e969, 0x79409669, 0xf9407e6b,
        0x7900ed69, 0x79409a69, 0xf9407e6b, 0x7900f169, 0xb9403269,
    ],
);

static ENABLED: AtomicBool = AtomicBool::new(false);
/// Terminal: no further shot is accepted (refusal after shot 1, destruction, departure, count change).
static ENDED: AtomicBool = AtomicBool::new(false);
/// 0 idle, 1 alternate applied (shot 1 issued), 2 restored (shot 2 issued).
static PHASE: AtomicU8 = AtomicU8::new(0);
static LINES: AtomicUsize = AtomicUsize::new(0);
static STATE: Mutex<Option<Snapshot>> = Mutex::new(None);

fn log(args: std::fmt::Arguments<'_>) {
    if LINES.fetch_add(1, Ordering::Relaxed) < LINE_CAP {
        crate::logging::diagnostic(args);
    }
}

/// Uncapped: decisions and refusals must always reach the log.
fn log_always(args: std::fmt::Arguments<'_>) {
    LINES.fetch_add(1, Ordering::Relaxed);
    crate::logging::diagnostic(args);
}

unsafe fn rd<T: Copy>(base: usize, offset: usize) -> T {
    std::ptr::read_volatile((base + offset) as *const T)
}

fn aligned(p: usize) -> bool {
    p != 0 && p.is_multiple_of(8)
}

fn is_mii_kind(kind: u32) -> bool {
    (0x48..=0x4a).contains(&kind)
}

fn le_u64(b: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(a)
}

/// Row hint of a tagged id: bits 55..40 (the game's first guess; validated against the table here).
pub fn row_hint(tagged: u64) -> u16 {
    ((tagged >> 40) & 0xffff) as u16
}

pub fn masked(charinfo: &[u8; CHARINFO_LEN]) -> [u8; CHARINFO_LEN] {
    let mut m = *charinfo;
    m[..CHARINFO_MASK_END].fill(0);
    m
}

// ------------------------------------------------------------------------------------------ tokens

/// The configuration tokens of a saved row: `u16 E+0x1aa` outfit row, `E+0x1ac` hat row, `E+0x1ae`
/// colour, `E+0x1b0` voice, `u8 E+0x1b2..0x1b5` specials. Mirrors: `FE+0x48/0x4a/0x4c/0x4e`,
/// `D+0x74/0x76/0x78` (no voice, see `D_MIRROR_COPY`) and `u32 D+0x390..0x39c`, `u32 O+0x6c/0x70/0x74/0x78`.
pub use crate::mii_native_access::Tokens;
#[cfg(test)]
use crate::mii_native_access::wr;

/// Live mirrors read for consistency checks; `fe[0]`/`d[0]` are the record index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mirrors {
    pub fe: [u16; 5],
    pub d: [u16; 4],
    pub d_specials: [u32; 4],
    pub o: [u32; 4],
}

impl Mirrors {
    pub unsafe fn read(fe: usize, d: usize, o: usize) -> Self {
        Self {
            fe: [rd(fe, 0x46), rd(fe, 0x48), rd(fe, 0x4a), rd(fe, 0x4c), rd(fe, 0x4e)],
            d: D_TOKENS.map(|offset| rd(d, offset)),
            d_specials: [rd(d, 0x390), rd(d, 0x394), rd(d, 0x398), rd(d, 0x39c)],
            o: [rd(o, 0x6c), rd(o, 0x70), rd(o, 0x74), rd(o, 0x78)],
        }
    }

    /// Every mirror agrees with the row tokens and the record index.
    pub fn agree(&self, idx: u16, t: Tokens) -> Result<(), String> {
        let row = [idx, t.outfit, t.hat, t.colour, t.voice];
        if self.fe != row {
            return Err(format!("FE+0x46..0x4f {:?} disagree with E {row:?}", self.fe));
        }
        let d_row = [idx, t.outfit, t.hat, t.colour];
        if self.d != d_row {
            return Err(format!("D+0x72..0x79 {:?} disagree with E {d_row:?}", self.d));
        }
        if self.d_specials != t.specials.map(u32::from) {
            return Err(format!("D+0x390..0x39c {:?} disagree with E+0x1b2..0x1b5 {:?}", self.d_specials, t.specials));
        }
        let o_row = [t.outfit, t.hat, t.colour, t.voice].map(u32::from);
        if self.o != o_row {
            return Err(format!("O+0x6c..0x7b {:?} disagree with E {o_row:?}", self.o));
        }
        Ok(())
    }
}

pub use crate::mii_native_access::{Targets, write_tokens};

// --------------------------------------------------------------------------------- saved alternates

/// One saved configuration with everything the swap needs. `charinfo` is never logged raw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Alternate {
    pub id: u32,
    pub header: Header,
    pub tokens: Tokens,
    pub outfit_hash40: u64,
    pub hat_hash40: u64,
    pub charinfo: [u8; CHARINFO_LEN],
}

/// Validates through `parse_header`, then takes the row hints of `+0x130` (outfit) and `+0x138`
/// (hat), colour `+0x140`, voice `+0x141`, specials `+0x142..0x145` and the CharInfo `+0x148..0x1a0`.
pub fn parse_alternate(id: u32, bytes: &[u8; HEADER_LEN]) -> Result<Alternate, HeaderError> {
    let header = parse_header(id, bytes)?;
    let outfit = le_u64(bytes, 0x130);
    let hat = le_u64(bytes, 0x138);
    let mut charinfo = [0u8; CHARINFO_LEN];
    charinfo.copy_from_slice(&bytes[CHARINFO_OFF..CHARINFO_OFF + CHARINFO_LEN]);
    Ok(Alternate {
        id,
        header,
        tokens: Tokens {
            outfit: row_hint(outfit),
            hat: row_hint(hat),
            colour: u16::from(bytes[0x140]),
            voice: u16::from(bytes[0x141]),
            specials: header.specials,
        },
        outfit_hash40: header.outfit,
        hat_hash40: header.hat,
        charinfo,
    })
}

/// The live original: class kind, row tokens and the masked record CharInfo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Live {
    pub kind: u32,
    pub tokens: Tokens,
    pub masked: [u8; CHARINFO_LEN],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectError {
    /// Counts of candidates passing each successive filter; the swap needs at least one past all.
    NoEligible { valid: usize, same_class: usize, equal_hat: usize, distinct: usize },
}

/// Same class, same hat row, different identity (masked CharInfo or any token); lowest slot id wins.
/// Copies of the original (byte-equal identity) are not alternatives. Returns the other eligible ids.
pub fn select_alternate(candidates: &[Alternate], live: &Live) -> Result<(Alternate, Vec<u32>), SelectError> {
    let same_class: Vec<&Alternate> = candidates.iter().filter(|a| a.header.class as u32 == live.kind).collect();
    let equal_hat: Vec<&Alternate> = same_class.iter().copied().filter(|a| a.tokens.hat == live.tokens.hat).collect();
    let mut distinct: Vec<&Alternate> = equal_hat
        .iter()
        .copied()
        .filter(|a| masked(&a.charinfo) != live.masked || a.tokens != live.tokens)
        .collect();
    distinct.sort_by_key(|a| a.id);
    match distinct.split_first() {
        Some((first, rest)) => Ok((**first, rest.iter().map(|a| a.id).collect())),
        None => Err(SelectError::NoEligible {
            valid: candidates.len(),
            same_class: same_class.len(),
            equal_hat: equal_hat.len(),
            distinct: 0,
        }),
    }
}

/// Bounded enumeration of `save:/save_data/mii`: ids through `select_ids` (explicit >99 failure), two
/// agreeing header reads per file, `parse_alternate`. Invalid files are returned, not dropped.
type AlternateScan = (Vec<Alternate>, Vec<(u32, String)>);

fn enumerate_alternates() -> Result<AlternateScan, String> {
    let entries = std::fs::read_dir(DIR).map_err(|e| format!("read_dir {DIR}: {e}"))?;
    let names = entries.map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()).map_err(|e| e.to_string()));
    let (ids, _) = select_ids(names).map_err(|e| format!("{e:?}"))?;
    let mut ok = Vec::new();
    let mut bad = Vec::new();
    for id in ids {
        match read_stable_header(DIR, id).and_then(|bytes| parse_alternate(id, &bytes).map_err(|e| format!("{e:?}"))) {
            Ok(alt) => ok.push(alt),
            Err(error) => bad.push((id, error)),
        }
    }
    Ok((ok, bad))
}

// ------------------------------------------------------------------------------------ native views

struct Pool {
    #[cfg(not(feature = "mii-native-adapter-probe"))]
    g: usize,
    begin: usize,
    count: usize,
}

/// `G = **(main+0x593b6a0)`; records `[G+0x24d0]..[G+0x24d8]` with stride 0x68 (checked exact).
unsafe fn pool(base: usize) -> Result<Pool, &'static str> {
    let holder: usize = rd(base, POOL_HOLDER);
    if !aligned(holder) {
        return Err("pool holder pointer null/misaligned");
    }
    let g: usize = rd(holder, 0);
    if !aligned(g) {
        return Err("pool manager pointer null/misaligned");
    }
    let begin: usize = rd(g, 0x24d0);
    let end: usize = rd(g, 0x24d8);
    if !aligned(begin) || end < begin || !(end - begin).is_multiple_of(RECORD_LEN) {
        return Err("record vector implausible (begin/end/stride)");
    }
    Ok(Pool { #[cfg(not(feature = "mii-native-adapter-probe"))] g, begin, count: (end - begin) / RECORD_LEN })
}

use crate::mii_native_access::owner_object;

/// Exact row identity, not a numeric field returned by a tagged-id-to-field accessor.
/// The native getters return tag|0xffff<<40 with a zero hash when the child row is unavailable.
/// A stale saved hint still refuses; this probe does not adopt the native fallback hash search.
fn check_row_id(table_off: usize, hint: u16, hash40: u64, actual: u64) -> Result<(), &'static str> {
    let tag = match table_off {
        OUTFIT_TABLE => 0x13u64,
        HAT_TABLE => 0x5au64,
        _ => return Err("unsupported ui table"),
    };
    if hint == u16::MAX || hash40 == 0 || hash40 & !0xff_ffff_ffff != 0 {
        return Err("invalid saved row identity");
    }
    let expected = (tag << 56) | (u64::from(hint) << 40) | hash40;
    if actual != expected {
        return Err("native row id does not match saved hint/hash");
    }
    Ok(())
}

/// Read-only native row lookup used by the resource dispatcher:
/// outfit 0x17dfc40..0x17dfc44; hat 0x17dfb28..0x17dfb34.
/// Both getters take data=[table+8], row:u32 -> tagged_id:u64 and only read PRC data.
/// Their complete code and shared leaf are fingerprinted before enabling the probe.
use crate::mii_native_access::lookup_row_id;

unsafe fn hint_resolves(base: usize, table_off: usize, hint: u16, hash40: u64) -> Result<(), &'static str> {
    let actual = lookup_row_id(base, table_off, hint)?;
    let result = check_row_id(table_off, hint, hash40, actual);
    log(format_args!(
        "[MiiFullProbe] row_lookup table={table_off:#x} hint={hint} saved_hash={hash40:010x} native_id={actual:016x} valid={}",
        result.is_ok()
    ));
    result
}

/// Conservative guard: no other live entry (ids 1..7) may be a Mii while the record is swapped.
#[cfg(not(feature = "mii-cpu-adapter-probe"))]
unsafe fn other_mii_entry(own: usize) -> Option<(u32, u32)> {
    let address = training_mod_sync::read(&crate::common::FIGHTER_MANAGER_ADDR) as *mut *mut smash::app::FighterManager;
    if address.is_null() || (*address).is_null() {
        return Some((u32::MAX, 0));
    }
    let mgr = *address;
    (1u32..8).find_map(|id| {
        let fe = smash::app::lua_bind::FighterManager::get_fighter_entry(mgr, smash::app::FighterEntryID(id as i32)) as usize;
        if fe == own || !aligned(fe) {
            return None;
        }
        let kind: u32 = rd(fe, 0x18);
        is_mii_kind(kind).then_some((id, kind))
    })
}

/// Raw 0x58-byte record copy under the native pool lock, exactly like the applier insert
/// (`0x14ebbbc` lock, copy, `0x14ebc4c` unlock). The shared pointer at `+0x58/+0x60` is untouched.
#[cfg(not(feature = "mii-native-adapter-probe"))]
unsafe fn write_record(base: usize, g: usize, record: usize, charinfo: &[u8; CHARINFO_LEN]) {
    let lock: unsafe extern "C" fn(usize) = std::mem::transmute(base + LOCK);
    let unlock: unsafe extern "C" fn(usize) = std::mem::transmute(base + UNLOCK);
    lock(g);
    std::ptr::copy_nonoverlapping(charinfo.as_ptr(), record as *mut u8, CHARINFO_LEN);
    unlock(g);
}

unsafe fn read_record(record: usize) -> [u8; CHARINFO_LEN] {
    let mut out = [0u8; CHARINFO_LEN];
    std::ptr::copy_nonoverlapping(record as *const u8, out.as_mut_ptr(), CHARINFO_LEN);
    out
}

// ------------------------------------------------------------------------------------------ shots

/// Shot-1 capture. Pointers are identities for comparison only, except `e` (static row, re-bound).
#[derive(Clone, Copy, Debug)]
struct Snapshot {
    fe: usize,
    id: u32,
    e: usize,
    kind: u32,
    idx: u16,
    count: usize,
    #[cfg(not(feature = "mii-native-adapter-probe"))]
    record: [u8; CHARINFO_LEN],
    #[cfg(not(feature = "mii-native-adapter-probe"))]
    original: Tokens,
    applied: Tokens,
    alt_charinfo: [u8; CHARINFO_LEN],
    alt_id: u32,
    e19: u8,
    fe41: u8,
    d51: u8,
    o_tex: [u32; 2],
    #[cfg(feature = "mii-native-adapter-probe")]
    adapter: native_adapter::Context,
    #[cfg(feature = "mii-cpu-adapter-probe")]
    cpu: crate::mii_native_access::cpu::Snapshot,
    #[cfg(feature = "mii-native-adapter-probe")]
    files: [Alternate; 2],
}

/// Called by `mii_reload_probe::service` after the install token is held and before the identical
/// descriptor copy. `shot` is 0 (apply) or 1 (restore). `false` means nothing was written.
pub unsafe fn step(entry: *mut u8, shot: usize, _service_entry_lr: usize) -> bool {
    if !ENABLED.load(Ordering::Relaxed) {
        log_always(format_args!("[MiiFullProbe] refused shot={shot}: hooks not installed (fingerprint mismatch at init)"));
        #[cfg(feature = "mii-cpu-adapter-probe")]
        ack::set_refusal("hooks not installed".into());
        return false;
    }
    if ENDED.load(Ordering::Relaxed) {
        log_always(format_args!("[MiiFullProbe] refused shot={shot}: experiment ended (phase={})", PHASE.load(Ordering::Relaxed)));
        #[cfg(feature = "mii-cpu-adapter-probe")]
        ack::set_refusal("experiment ended".into());
        return false;
    }
    let result = match shot {
        0 => shot_apply(entry, _service_entry_lr),
        1 => shot_restore(entry, _service_entry_lr),
        _ => Err("shot index out of range".to_string()),
    };
    match result {
        Ok(()) => true,
        Err(reason) => {
            log_always(format_args!("[MiiFullProbe] refused shot={shot} fe={:#x}: {reason}; nothing written", entry as usize));
            #[cfg(feature = "mii-cpu-adapter-probe")]
            ack::set_refusal(reason.clone());
            if shot == 1 {
                // The alternate stays applied in memory; the rule forbids any later write, so end here.
                ENDED.store(true, Ordering::Relaxed);
            }
            false
        }
    }
}

unsafe fn shot_apply(entry: *mut u8, _service_entry_lr: usize) -> Result<(), String> {
    if PHASE.load(Ordering::Relaxed) != 0 || STATE.lock().unwrap().is_some() {
        return Err("shot 1 already issued".into());
    }
    let base = getRegionAddress(Region::Text) as usize;
    let fe = entry as usize;
    let bound = bind_saved_entry(entry).ok_or("live entry could not be bound to a consistent saved row")?;
    if !is_mii_kind(bound.kind) {
        return Err(format!("bound kind {:#x} is not a Mii", bound.kind));
    }
    let owner_id: u32 = rd(fe, 0xe0);
    if owner_id != bound.id {
        return Err(format!("FE+0xe0 owner {owner_id} != bound entry id {}", bound.id));
    }
    #[cfg(not(feature = "mii-cpu-adapter-probe"))]
    if let Some((id, kind)) = other_mii_entry(fe) {
        return Err(format!("another live entry id={id} is a Mii (kind {kind:#x}) or the manager is unavailable"));
    }
    let d: usize = rd(fe, 0xf8);
    if !aligned(d) {
        return Err("information block pointer null/misaligned".into());
    }
    let pool = pool(base)?;
    let idx: u16 = rd(bound.e, 0x1a8);
    if usize::from(idx) >= pool.count {
        return Err(format!("record index {idx} not below pool count {}", pool.count));
    }
    let o = owner_object(base, bound.id).ok_or("owner Mii object null/misaligned")?;
    if rd::<u16>(o, 0x68) != idx {
        return Err(format!("owner object record index {} != row index {idx}", rd::<u16>(o, 0x68)));
    }
    let original = Tokens::from_row(bound.e);
    if original.specials.iter().any(|s| *s > 2) {
        return Err(format!("row specials {:?} out of range", original.specials));
    }
    Mirrors::read(fe, d, o).agree(idx, original)?;
    let record_ptr = pool.begin + usize::from(idx) * RECORD_LEN;
    let record = read_record(record_ptr);
    let live = Live { kind: bound.kind, tokens: original, masked: masked(&record) };

    let (candidates, invalid) = enumerate_alternates()?;
    for (id, error) in &invalid {
        log(format_args!("[MiiFullProbe] inventory id={id} invalid: {error}"));
    }
    // The applier gates its record copy on a native CharInfo check (0x39c2930/0x39c2940) that is not
    // called here: the live record and row must equal one saved file byte for byte before any
    // file-sourced CharInfo or token is written.
    let original_file = candidates
        .iter()
        .find(|a| a.header.class as u32 == bound.kind && a.charinfo == record && a.tokens == original)
        .ok_or_else(|| format!(
            "live record fnv={:016x} and row {original:?} match no saved file exactly; file-to-memory encoding unproven",
            fnv1a64(&record)
        ))?;
    hint_resolves(base, OUTFIT_TABLE, original.outfit, original_file.outfit_hash40)
        .map_err(|e| format!("original outfit hint {}: {e}", original.outfit))?;
    hint_resolves(base, HAT_TABLE, original.hat, original_file.hat_hash40)
        .map_err(|e| format!("original hat hint {}: {e}", original.hat))?;
    let (alt, others) = select_alternate(&candidates, &live).map_err(|e| format!("{e:?}"))?;
    hint_resolves(base, OUTFIT_TABLE, alt.tokens.outfit, alt.outfit_hash40).map_err(|e| format!("alternate outfit hint {}: {e}", alt.tokens.outfit))?;
    hint_resolves(base, HAT_TABLE, alt.tokens.hat, alt.hat_hash40).map_err(|e| format!("alternate hat hint {}: {e}", alt.tokens.hat))?;
    if alt.tokens.hat != original.hat {
        return Err("alternate hat row differs (selection invariant violated)".into());
    }

    #[cfg(feature = "mii-cpu-adapter-probe")]
    let cpu = {
        let current = native_adapter::capture(entry)?;
        let cpu = crate::mii_native_access::cpu::snapshot(entry, current)?;
        native_adapter::cpu::fixture(&cpu, *original_file, alt)?;
        log_always(format_args!("[MiiCpuAdapter] captured current={:x?}", native_adapter::cpu::view(&cpu)));
        cpu
    };
    let snapshot = Snapshot {
        fe,
        id: bound.id,
        e: bound.e,
        kind: bound.kind,
        idx,
        count: pool.count,
        #[cfg(not(feature = "mii-native-adapter-probe"))]
        record,
        #[cfg(not(feature = "mii-native-adapter-probe"))]
        original,
        applied: alt.tokens,
        alt_charinfo: alt.charinfo,
        alt_id: alt.id,
        e19: rd(bound.e, 0x19),
        fe41: rd(fe, 0x41),
        d51: rd(d, 0x51),
        o_tex: [rd(o, 0x90), rd(o, 0x94)],
        #[cfg(feature = "mii-native-adapter-probe")]
        adapter: native_adapter::capture(entry)?,
        #[cfg(feature = "mii-cpu-adapter-probe")]
        cpu,
        #[cfg(feature = "mii-native-adapter-probe")]
        files: [*original_file, alt],
    };
    log_always(format_args!(
        "[MiiFullProbe] shot1 id={} fe={fe:#x} e={:#x} d={d:#x} o={o:#x} kind={:#x} idx={idx} count={} record_fnv={:016x} masked_fnv={:016x} original={original:?} e19={} fe41={} d51={} o_tex={:08x?} -> alternate file id={} masked_fnv={:016x} applied={:?} others={others:?} invalid_files={}",
        bound.id, bound.e, bound.kind, pool.count, fnv1a64(&record), fnv1a64(&live.masked),
        snapshot.e19, snapshot.fe41, snapshot.d51, snapshot.o_tex, alt.id, alt.header.charinfo_masked_fnv, alt.tokens, invalid.len()
    ));
    #[cfg(feature = "mii-native-adapter-probe")]
    let written_fnv = native_adapter::apply(entry, snapshot.adapter, snapshot.files[0], snapshot.files[1], _service_entry_lr,
        #[cfg(feature = "mii-cpu-adapter-probe")] &snapshot.cpu)?;
    *STATE.lock().unwrap() = Some(snapshot);
    PHASE.store(1, Ordering::Relaxed);
    #[cfg(not(feature = "mii-native-adapter-probe"))]
    {
        // Original path: record (locked), then E, FE, D, O.
        write_record(base, pool.g, record_ptr, &alt.charinfo);
        write_tokens(&Targets { e: bound.e, fe, d, o }, alt.tokens);
    }
    #[cfg(not(feature = "mii-native-adapter-probe"))]
    let written_fnv = fnv1a64(&read_record(record_ptr));
    log_always(format_args!(
        "[MiiFullProbe] shot1 written: record idx={idx} fnv={:016x} tokens={:?}; identical reload follows",
        written_fnv, Tokens::from_row(bound.e)
    ));
    Ok(())
}

unsafe fn shot_restore(entry: *mut u8, _service_entry_lr: usize) -> Result<(), String> {
    let snap = (*STATE.lock().unwrap()).ok_or("no shot-1 record")?;
    if PHASE.load(Ordering::Relaxed) != 1 {
        return Err(format!("phase {} is not 'applied'", PHASE.load(Ordering::Relaxed)));
    }
    let base = getRegionAddress(Region::Text) as usize;
    let fe = entry as usize;
    if fe != snap.fe {
        return Err(format!("live entry {fe:#x} is not the shot-1 entry {:#x}", snap.fe));
    }
    let bound = bind_saved_entry(entry).ok_or("live entry could not be re-bound to a consistent saved row")?;
    if bound.id != snap.id || bound.e != snap.e || bound.kind != snap.kind {
        return Err(format!("binding changed: ({}, {:#x}, {:#x}) vs shot-1 ({}, {:#x}, {:#x})", bound.id, bound.e, bound.kind, snap.id, snap.e, snap.kind));
    }
    let owner_id: u32 = rd(fe, 0xe0);
    if owner_id != bound.id {
        return Err(format!("FE+0xe0 owner {owner_id} != bound entry id {}", bound.id));
    }
    let d: usize = rd(fe, 0xf8);
    if !aligned(d) {
        return Err("information block pointer null/misaligned".into());
    }
    let pool = pool(base)?;
    if pool.count != snap.count {
        return Err(format!("pool count changed {} -> {}", snap.count, pool.count));
    }
    let idx: u16 = rd(bound.e, 0x1a8);
    if idx != snap.idx {
        return Err(format!("row record index changed {} -> {idx}", snap.idx));
    }
    let o = owner_object(base, bound.id).ok_or("owner Mii object null/misaligned")?;
    if rd::<u16>(o, 0x68) != idx {
        return Err(format!("owner object record index {} != row index {idx}", rd::<u16>(o, 0x68)));
    }
    let record_ptr = pool.begin + usize::from(idx) * RECORD_LEN;
    let current = read_record(record_ptr);
    if current != snap.alt_charinfo {
        return Err(format!("record idx={idx} fnv={:016x} is not the applied alternate {:016x}", fnv1a64(&current), fnv1a64(&snap.alt_charinfo)));
    }
    let row = Tokens::from_row(bound.e);
    if row != snap.applied {
        return Err(format!("row tokens {row:?} are not the applied {:?}", snap.applied));
    }
    let mirrors = Mirrors::read(fe, d, o);
    let applied_row = [idx, snap.applied.outfit, snap.applied.hat, snap.applied.colour, snap.applied.voice];
    let applied_d = [idx, snap.applied.outfit, snap.applied.hat, snap.applied.colour];
    if mirrors.fe != applied_row || mirrors.d != applied_d || mirrors.d_specials != snap.applied.specials.map(u32::from) {
        return Err(format!("FE/D mirrors {:?}/{:?}/{:?} are not the applied {applied_row:?}/{:?}", mirrors.fe, mirrors.d, mirrors.d_specials, snap.applied.specials));
    }
    // O is re-derived live; its token words are the ones the native copy overwrites from E, so a
    // difference is logged (the 0x66ddfc hook explains it). The legacy path writes regardless;
    // the optional adapter instead requires exact current O tokens inside its locked preflight.
    let o_applied = [snap.applied.outfit, snap.applied.hat, snap.applied.colour, snap.applied.voice].map(u32::from);
    if mirrors.o != o_applied {
        log_always(format_args!("[MiiFullProbe] shot2 note: O+0x6c..0x7b {:?} differ from applied {o_applied:?} (o={o:#x})", mirrors.o));
    }
    #[cfg(feature = "mii-native-adapter-probe")]
    let written_fnv = native_adapter::apply(entry, snap.adapter, snap.files[1], snap.files[0], _service_entry_lr,
        #[cfg(feature = "mii-cpu-adapter-probe")] &snap.cpu)?;
    PHASE.store(2, Ordering::Relaxed);
    #[cfg(not(feature = "mii-native-adapter-probe"))]
    {
        write_record(base, pool.g, record_ptr, &snap.record);
        write_tokens(&Targets { e: bound.e, fe, d, o }, snap.original);
    }
    #[cfg(not(feature = "mii-native-adapter-probe"))]
    let written_fnv = fnv1a64(&read_record(record_ptr));
    log_always(format_args!(
        "[MiiFullProbe] shot2 id={} fe={fe:#x} e={:#x} restored record idx={idx} fnv={:016x} tokens={:?} (alternate file id={}); identical reload follows",
        bound.id, bound.e, written_fnv, Tokens::from_row(bound.e), snap.alt_id
    ));
    Ok(())
}

/// Read-only observation when `mii_reload_probe` sees the reload complete on the tracked entry.
pub unsafe fn on_complete(entry: *const u8, _service_entry_lr: usize) {
    let Some(snap) = *STATE.lock().unwrap() else {
        return;
    };
    let fe = entry as usize;
    if fe != snap.fe {
        return;
    }
    #[cfg(feature = "mii-native-adapter-probe")]
    native_adapter::complete(entry, snap.adapter, _service_entry_lr,
        #[cfg(feature = "mii-cpu-adapter-probe")] &snap.cpu);
    let base = getRegionAddress(Region::Text) as usize;
    let d: usize = rd(fe, 0xf8);
    let count = pool(base).map(|p| p.count).unwrap_or(usize::MAX);
    let record_fnv = pool(base).ok().filter(|p| usize::from(snap.idx) < p.count).map(|p| fnv1a64(&read_record(p.begin + usize::from(snap.idx) * RECORD_LEN)));
    let o = owner_object(base, snap.id);
    let o_fields = o.map(|o| ([rd::<u16>(o, 0x68), rd::<u16>(o, 0x6a)], [rd::<u32>(o, 0x6c), rd(o, 0x70), rd(o, 0x74), rd(o, 0x78)], [rd::<u32>(o, 0x90), rd(o, 0x94)]));
    let fe_tokens: [u16; 5] = [rd(fe, 0x46), rd(fe, 0x48), rd(fe, 0x4a), rd(fe, 0x4c), rd(fe, 0x4e)];
    // `D+0x7a` is logged raw: it is outside the native token copy and its meaning is unknown.
    let d_view = aligned(d).then(|| (D_TOKENS.map(|offset| rd::<u16>(d, offset)), rd::<u16>(d, 0x7a), [rd::<u32>(d, 0x390), rd(d, 0x394), rd(d, 0x398), rd(d, 0x39c)], rd::<u8>(d, 0x94)));
    let row_tokens = Tokens::from_row(snap.e);
    log_always(format_args!(
        "[MiiFullProbe] complete phase={} fe={fe:#x} row={:?} fe_tokens={fe_tokens:04x?} d={d_view:?} o={o:#x?} o_fields={o_fields:x?} record_fnv={record_fnv:016x?} count={count} (shot1 count {}) o_tex_before={:08x?}",
        PHASE.load(Ordering::Relaxed), row_tokens, snap.count, snap.o_tex
    ));
    #[cfg(feature = "mii-cpu-adapter-probe")]
    {
        let phase = ack_phase();
        let expected = snap.files[if phase == 1 { 1 } else { 0 }];
        let t = expected.tokens;
        let valid = matches!(phase, 1 | 2) && !ENDED.load(Ordering::Relaxed)
            && count == snap.count && row_tokens == t
            && record_fnv == Some(fnv1a64(&expected.charinfo))
            && fe_tokens == [snap.idx, t.outfit, t.hat, t.colour, t.voice]
            && d_view.is_some_and(|d| d.0 == [snap.idx, t.outfit, t.hat, t.colour]
                && d.2 == t.specials.map(u32::from))
            && o_fields.is_some_and(|o| o.0[0] == snap.idx
                && o.1 == [t.outfit, t.hat, t.colour, t.voice].map(u32::from));
        ack::extend_observation(serde_json::json!({"phase":phase,"entry":ack::hex(fe),
            "row":ack_tokens(row_tokens),"fe_tokens":fe_tokens,"d":d_view,"owner_object":o.map(ack::hex),
            "owner_fields":o_fields,"record_fnv":record_fnv.map(ack::hash),"count":count,
            "original_count":snap.count,"valid":valid}), valid);
    }
    if count != snap.count {
        ENDED.store(true, Ordering::Relaxed);
        log_always(format_args!("[MiiFullProbe] ended: pool count changed {} -> {count}; shot 2 refused by rule", snap.count));
    }
}

/// The shot-1 entry is being destroyed: end the experiment. Nothing is written (rule).
pub unsafe fn on_destroy(entry: *const u8) {
    let Some(snap) = STATE.lock().unwrap().take_if(|s| s.fe == entry as usize) else {
        return;
    };
    ENDED.store(true, Ordering::Relaxed);
    log_always(format_args!(
        "[MiiFullProbe] ended on entry destruction id={} fe={:#x} phase={} alternate_applied={}; no restoration at destruction by rule",
        snap.id, snap.fe, PHASE.load(Ordering::Relaxed), PHASE.load(Ordering::Relaxed) == 1
    ));
}

/// Scene departure: end the experiment; no live entry here, nothing is written.
pub fn scene_changed() {
    if let Some(snap) = *STATE.lock().unwrap() {
        ENDED.store(true, Ordering::Relaxed);
        log_always(format_args!(
            "[MiiFullProbe] ended on scene departure id={} fe={:#x} phase={}; rows left as they are (no write by rule)",
            snap.id, snap.fe, PHASE.load(Ordering::Relaxed)
        ));
    }
}

#[cfg(feature = "mii-cpu-adapter-probe")]
pub fn cpu_ordinary_pending() -> bool {
    ENABLED.load(Ordering::Relaxed) && !ENDED.load(Ordering::Relaxed)
        && native_adapter::cpu::ordinary_pending()
}

#[cfg(feature = "mii-cpu-adapter-probe")]
pub unsafe fn observe_cpu_ordinary(entry: *const u8) {
    if ENABLED.load(Ordering::Relaxed) && !ENDED.load(Ordering::Relaxed) {
        native_adapter::cpu::observe_ordinary(entry);
    }
}

pub fn armed() -> bool {
    std::path::Path::new(ARM_FILE).exists() && ENABLED.load(Ordering::Relaxed)
}

// ------------------------------------------------------------------- read-only instrumentation hooks

/// `main+0x66ddfc` (inside `0x66a700`): `x20` = O, `w21` = player id; the instruction about to run
/// stores outfit `w8` / hat `w9` to `O+0x6c/+0x70`, then `w10`/`w11` (colour, voice) to `+0x74/+0x78`
/// and `w13` (kind) to `+0x80`. Logs old and new values: proves whether the copy runs on the reload.
#[skyline::hook(offset = 0x66ddfc, inline)]
unsafe fn owner_copy(ctx: &mut InlineCtx) {
    let o = ctx.registers[20].x() as usize;
    if !aligned(o) {
        return;
    }
    let old: [u32; 4] = [rd(o, 0x6c), rd(o, 0x70), rd(o, 0x74), rd(o, 0x78)];
    log(format_args!(
        "[MiiFullProbe] owner_copy phase={} id={} o={o:#x} idx={} old={old:?} new=[{}, {}, {}, {}] kind={:#x}",
        PHASE.load(Ordering::Relaxed), ctx.registers[21].w(), rd::<u16>(o, 0x68),
        ctx.registers[8].w(), ctx.registers[9].w(), ctx.registers[10].w(), ctx.registers[11].w(), ctx.registers[13].w()
    ));
}

/// `main+0x17e69b4`: `bl 0x17dfa60(out, w1 kind, w2 = 0x13, w3 = O+0x6c outfit, w4 = O+0x74 colour)`, `x20` = O.
#[skyline::hook(offset = 0x17e69b4, inline)]
unsafe fn body_request(ctx: &mut InlineCtx) {
    log(format_args!(
        "[MiiFullProbe] body_request phase={} kind={:#x} category={:#x} outfit={} colour={} o={:#x}",
        PHASE.load(Ordering::Relaxed), ctx.registers[1].w(), ctx.registers[2].w(), ctx.registers[3].w(), ctx.registers[4].w(), ctx.registers[20].x()
    ));
}

/// `main+0x17e71a0`: `bl 0x17dfa60(out, w1 kind, w2 = 0x15, w3 = O+0x78 voice, w4 = 0)`, `x20` = O.
#[skyline::hook(offset = 0x17e71a0, inline)]
unsafe fn voice_request_step(ctx: &mut InlineCtx) {
    log(format_args!(
        "[MiiFullProbe] voice_request(step) phase={} kind={:#x} category={:#x} voice={} o={:#x}",
        PHASE.load(Ordering::Relaxed), ctx.registers[1].w(), ctx.registers[2].w(), ctx.registers[3].w(), ctx.registers[20].x()
    ));
}

/// `main+0x657e18`: setup-path `bl 0x17dfa60(out, w1 kind, w2 = 0x15, w3 = FE+0x4e voice, 0)` after `str w3, [x20, #0x78]`.
#[skyline::hook(offset = 0x657e18, inline)]
unsafe fn voice_request_setup(ctx: &mut InlineCtx) {
    log(format_args!(
        "[MiiFullProbe] voice_request(setup) phase={} kind={:#x} category={:#x} voice={} o={:#x}",
        PHASE.load(Ordering::Relaxed), ctx.registers[1].w(), ctx.registers[2].w(), ctx.registers[3].w(), ctx.registers[20].x()
    ));
}

/// `main+0x67ead0` entry: `w0` = player id, `w1` = kind, `x2` -> `u16` record index. Logs the owner
/// object's tokens and face texture handles before the function runs (`O+0x90/+0x94` recreated when zero).
#[skyline::hook(offset = 0x67ead0, inline)]
unsafe fn face_texture_entry(ctx: &mut InlineCtx) {
    let id = ctx.registers[0].w();
    let idx_ptr = ctx.registers[2].x() as *const u16;
    let idx = if idx_ptr.is_null() { u16::MAX } else { idx_ptr.read_volatile() };
    let base = getRegionAddress(Region::Text) as usize;
    let o = owner_object(base, id);
    let fields = o.map(|o| ([rd::<u16>(o, 0x68)], [rd::<u32>(o, 0x6c), rd(o, 0x70), rd(o, 0x74), rd(o, 0x78)], [rd::<u32>(o, 0x90), rd(o, 0x94)]));
    log(format_args!(
        "[MiiFullProbe] face_texture phase={} id={id} kind={:#x} idx={idx} o={o:#x?} o_fields={fields:x?}",
        PHASE.load(Ordering::Relaxed), ctx.registers[1].w()
    ));
}

pub fn init() {
    unsafe {
        let base = getRegionAddress(Region::Text) as usize;
        for (offset, words, what) in SITES {
            let actual = std::slice::from_raw_parts((base + offset) as *const u32, 4);
            if actual != words {
                crate::logging::diagnostic(format_args!(
                    "[MiiFullProbe] disabled: words at {offset:#x} ({what}) = {actual:08x?}, expected {words:08x?}"
                ));
                return;
            }
        }
        for (offset, len, expected) in ROW_LOOKUP_CODE {
            let code = std::slice::from_raw_parts((base + offset) as *const u8, len);
            let actual = fnv1a64(code);
            if actual != expected {
                crate::logging::diagnostic(format_args!(
                    "[MiiFullProbe] disabled: row lookup code at {offset:#x}+{len:#x} hash={actual:016x}, expected={expected:016x}"
                ));
                return;
            }
        }
        for (offset, word) in APPLIER_LOCK_SITES {
            let actual: u32 = rd(base, offset);
            if actual != word {
                crate::logging::diagnostic(format_args!(
                    "[MiiFullProbe] disabled: applier lock cross-check at {offset:#x} = {actual:08x}, expected {word:08x}"
                ));
                return;
            }
        }
        let (offset, words) = D_MIRROR_COPY;
        let actual = std::slice::from_raw_parts((base + offset) as *const u32, words.len());
        if actual != words {
            crate::logging::diagnostic(format_args!(
                "[MiiFullProbe] disabled: D token mirror copy at {offset:#x} = {actual:08x?}, expected {words:08x?}"
            ));
            return;
        }
        skyline::install_hooks!(owner_copy, body_request, voice_request_step, voice_request_setup, face_texture_entry);
        ENABLED.store(true, Ordering::Relaxed);
        crate::logging::diagnostic(format_args!(
            "[MiiFullProbe] installed: five read-only sites fingerprinted; two-shot swap armed by {ARM_FILE} (plus the identical-reload arm file)"
        ));
    }
}

// ------------------------------------------------------------------------------------------ tests
// Pure tests on synthetic buffers. Not run in this pass (artifact only); root runs them in a build window.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::mii_saved_file_probe::MiiClass;

    const CHARA_BRAWLER: u64 = 0xc1004c13fdfdfb2b;
    const CHARA_SWORD: u64 = 0xc1004d15d7061f77;
    const OUTFIT_HASH: u64 = 0x1bfef7a5f7;
    const HAT_DEFAULT: u64 = 0x5a00000fa9efa5ea;

    #[allow(clippy::too_many_arguments)]
    fn fixture(id: u32, chara: u64, outfit_hint: u16, hat_hint: u16, colour: u8, voice: u8, specials: [u8; 4], face: u8, create_id: u8) -> [u8; HEADER_LEN] {
        let mut b = [0u8; HEADER_LEN];
        b[0..4].copy_from_slice(&id.to_le_bytes());
        b[4..8].copy_from_slice(&5u32.to_le_bytes());
        b[8..12].copy_from_slice(&(id + 1).to_le_bytes());
        b[0x14..0x18].copy_from_slice(&0x10bu32.to_le_bytes());
        b[0x114..0x118].copy_from_slice(&2u32.to_le_bytes());
        let outfit = (0x13u64 << 56) | (u64::from(outfit_hint) << 40) | OUTFIT_HASH;
        let hat = (HAT_DEFAULT & !(0xffffu64 << 40)) | (u64::from(hat_hint) << 40);
        b[0x130..0x138].copy_from_slice(&outfit.to_le_bytes());
        b[0x138..0x140].copy_from_slice(&hat.to_le_bytes());
        b[0x140] = colour;
        b[0x141] = voice;
        b[0x142..0x146].copy_from_slice(&specials);
        for (i, byte) in b[CHARINFO_OFF..CHARINFO_OFF + CHARINFO_LEN].iter_mut().enumerate() {
            *byte = if i < CHARINFO_MASK_END { create_id } else { face };
        }
        b[0x1a0..0x1a8].copy_from_slice(&chara.to_le_bytes());
        b
    }

    fn live_from(alt: &Alternate) -> Live {
        Live { kind: alt.header.class as u32, tokens: alt.tokens, masked: masked(&alt.charinfo) }
    }

    #[test]
    fn parse_alternate_extracts_hints_tokens_and_charinfo() {
        let a = parse_alternate(4155, &fixture(4155, CHARA_BRAWLER, 0x29, 0, 1, 37, [1, 1, 2, 1], 0x40, 0x11)).unwrap();
        assert_eq!(a.header.class, MiiClass::Brawler);
        assert_eq!(a.tokens, Tokens { outfit: 0x29, hat: 0, colour: 1, voice: 37, specials: [1, 1, 2, 1] });
        assert_eq!(a.outfit_hash40, OUTFIT_HASH);
        assert_eq!(a.hat_hash40, HAT_DEFAULT & 0xff_ffff_ffff);
        assert!(a.charinfo[..CHARINFO_MASK_END].iter().all(|b| *b == 0x11));
        assert!(a.charinfo[CHARINFO_MASK_END..].iter().all(|b| *b == 0x40));
        assert!(masked(&a.charinfo)[..CHARINFO_MASK_END].iter().all(|b| *b == 0));
        assert_eq!(parse_alternate(4155, &fixture(4156, CHARA_BRAWLER, 0, 0, 0, 0, [0; 4], 0, 0)), Err(HeaderError::IdMismatch { name_id: 4155, header_id: 4156 }));
    }

    #[test]
    fn selection_requires_same_class_equal_hat_and_different_identity() {
        let original = parse_alternate(4155, &fixture(4155, CHARA_BRAWLER, 1, 0, 1, 37, [1, 1, 2, 1], 0x40, 0x11)).unwrap();
        let live = live_from(&original);
        // A renamed copy of the original (different create_id/name bytes only) is not an alternative.
        let copy = parse_alternate(5000, &fixture(5000, CHARA_BRAWLER, 1, 0, 1, 37, [1, 1, 2, 1], 0x40, 0x22)).unwrap();
        let sword = parse_alternate(2622, &fixture(2622, CHARA_SWORD, 0x29, 0, 5, 37, [0; 4], 0x40, 0x11)).unwrap();
        let other_hat = parse_alternate(6000, &fixture(6000, CHARA_BRAWLER, 2, 3, 2, 37, [0; 4], 0x41, 0x11)).unwrap();
        assert_eq!(
            select_alternate(&[original, copy, sword, other_hat], &live),
            Err(SelectError::NoEligible { valid: 4, same_class: 3, equal_hat: 2, distinct: 0 })
        );
        // Same face, one different special: eligible. Different voice only: eligible.
        let moves = parse_alternate(7000, &fixture(7000, CHARA_BRAWLER, 1, 0, 1, 37, [2, 1, 2, 1], 0x40, 0x33)).unwrap();
        let voice = parse_alternate(6500, &fixture(6500, CHARA_BRAWLER, 1, 0, 1, 3, [1, 1, 2, 1], 0x40, 0x33)).unwrap();
        let (chosen, others) = select_alternate(&[moves, original, voice, copy], &live).unwrap();
        assert_eq!(chosen.id, 6500);
        assert_eq!(others, vec![7000]);
    }

    #[test]
    fn write_tokens_touches_exactly_the_documented_offsets() {
        let mut e = vec![0xAAu8; 0x260];
        let mut fe = vec![0xBBu8; 0x100];
        let mut d = vec![0xCCu8; 0x3a0];
        let mut o = vec![0xDDu8; 0xa0];
        let tokens = Tokens { outfit: 0x29, hat: 0x7, colour: 5, voice: 37, specials: [0, 1, 2, 1] };
        unsafe {
            write_tokens(
                &Targets { e: e.as_mut_ptr() as usize, fe: fe.as_mut_ptr() as usize, d: d.as_mut_ptr() as usize, o: o.as_mut_ptr() as usize },
                tokens,
            );
        }
        let mut xe = vec![0xAAu8; 0x260];
        xe[0x1aa..0x1ac].copy_from_slice(&0x29u16.to_le_bytes());
        xe[0x1ae..0x1b0].copy_from_slice(&5u16.to_le_bytes());
        xe[0x1b0..0x1b2].copy_from_slice(&37u16.to_le_bytes());
        xe[0x1b2..0x1b6].copy_from_slice(&[0, 1, 2, 1]);
        assert_eq!(e, xe, "E: outfit, colour, voice, specials only (hat E+0x1ac untouched)");
        let mut xfe = vec![0xBBu8; 0x100];
        xfe[0x48..0x4a].copy_from_slice(&0x29u16.to_le_bytes());
        xfe[0x4c..0x4e].copy_from_slice(&5u16.to_le_bytes());
        xfe[0x4e..0x50].copy_from_slice(&37u16.to_le_bytes());
        assert_eq!(fe, xfe, "FE: +0x48, +0x4c, +0x4e only (record index +0x46 and hat +0x4a untouched)");
        let mut xd = vec![0xCCu8; 0x3a0];
        xd[0x74..0x76].copy_from_slice(&0x29u16.to_le_bytes());
        xd[0x78..0x7a].copy_from_slice(&5u16.to_le_bytes());
        for (i, s) in [0u32, 1, 2, 1].iter().enumerate() {
            xd[0x390 + i * 4..0x394 + i * 4].copy_from_slice(&s.to_le_bytes());
        }
        assert_eq!(d, xd, "D: +0x74, +0x78, +0x390..0x39c only");
        assert_eq!(d[0x7a..0x7c], [0xCC, 0xCC], "D+0x7a is not a voice mirror and is never written");
        let mut xo = vec![0xDDu8; 0xa0];
        xo[0x6c..0x70].copy_from_slice(&0x29u32.to_le_bytes());
        xo[0x74..0x78].copy_from_slice(&5u32.to_le_bytes());
        xo[0x78..0x7c].copy_from_slice(&37u32.to_le_bytes());
        assert_eq!(o, xo, "O: +0x6c, +0x74, +0x78 only (hat +0x70, flags +0x7c.., textures +0x90.. untouched)");
    }

    #[test]
    fn mirrors_agree_only_when_every_view_matches() {
        let t = Tokens { outfit: 1, hat: 0, colour: 1, voice: 37, specials: [1, 1, 2, 1] };
        let good = Mirrors { fe: [0, 1, 0, 1, 37], d: [0, 1, 0, 1], d_specials: [1, 1, 2, 1], o: [1, 0, 1, 37] };
        assert_eq!(good.agree(0, t), Ok(()));
        assert!(Mirrors { fe: [0, 2, 0, 1, 37], ..good }.agree(0, t).is_err());
        assert!(Mirrors { fe: [0, 1, 0, 1, 3], ..good }.agree(0, t).is_err());
        assert!(Mirrors { d: [0, 1, 0, 2], ..good }.agree(0, t).is_err());
        assert!(Mirrors { d: [3, 1, 0, 1], ..good }.agree(0, t).is_err());
        assert!(Mirrors { d_specials: [0, 1, 2, 1], ..good }.agree(0, t).is_err());
        assert!(Mirrors { o: [1, 0, 2, 37], ..good }.agree(0, t).is_err());
        assert!(good.agree(3, t).is_err());
    }

    #[test]
    fn d_mirror_copy_is_four_halfword_token_copies_without_voice() {
        let (_, words) = D_MIRROR_COPY;
        let regs = |w: u32| ((w >> 5) & 0x1f, w & 0x1f);
        let imm12 = |w: u32| ((w >> 10) & 0xfff) as usize;
        for (i, triple) in words[..12].chunks(3).enumerate() {
            assert_eq!(triple[0] & 0xffc0_0000, 0x7940_0000, "ldrh {i}");
            assert_eq!((imm12(triple[0]) * 2, regs(triple[0])), (0x46 + i * 2, (19, 9)));
            assert_eq!(triple[1] & 0xffc0_0000, 0xf940_0000, "ldr x {i}");
            assert_eq!((imm12(triple[1]) * 8, regs(triple[1])), (0xf8, (19, 11)));
            assert_eq!(triple[2] & 0xffc0_0000, 0x7900_0000, "strh {i}");
            assert_eq!((imm12(triple[2]) * 2, regs(triple[2])), (D_TOKENS[i], (11, 9)));
        }
        // A word load of FE+0x30 follows: there is no fifth (voice, FE+0x4e) copy.
        assert_eq!(words[12] & 0xffc0_0000, 0xb940_0000);
        assert_eq!((imm12(words[12]) * 4, regs(words[12])), (0x30, (19, 9)));
    }

    /// 8-aligned zeroed scratch for volatile field access.
    fn scratch(len: usize) -> Vec<u64> {
        vec![0; len.div_ceil(8)]
    }

    #[test]
    fn refused_shot1_shape_agrees_whatever_d7a_holds() {
        // Sword 2622 at the refused fullconfig06 shot 1: E = FE = [0, 41, 0, 5, 37], O = [41, 0, 5, 37].
        let t = Tokens { outfit: 41, hat: 0, colour: 5, voice: 37, specials: [0; 4] };
        let (mut fe_buf, mut o_buf) = (scratch(0x100), scratch(0xa0));
        let (fe, o) = (fe_buf.as_mut_ptr() as usize, o_buf.as_mut_ptr() as usize);
        unsafe {
            for (i, v) in [0u16, 41, 0, 5, 37].iter().copied().enumerate() {
                wr::<u16>(fe, 0x46 + i * 2, v);
            }
            for (i, v) in [41u32, 0, 5, 37].iter().copied().enumerate() {
                wr::<u32>(o, 0x6c + i * 4, v);
            }
        }
        for d7a in [0u16, 37, 0xcccc] {
            let mut d_buf = scratch(0x3a0);
            let d = d_buf.as_mut_ptr() as usize;
            unsafe {
                for (i, offset) in D_TOKENS.iter().copied().enumerate() {
                    wr::<u16>(d, offset, rd::<u16>(fe, 0x46 + i * 2));
                }
                wr::<u16>(d, 0x7a, d7a);
                let mirrors = Mirrors::read(fe, d, o);
                assert_eq!(mirrors.d, [0, 41, 0, 5]);
                assert_eq!(mirrors.agree(0, t), Ok(()), "D+0x7a = {d7a:#x}");
                wr::<u16>(d, 0x78, 4);
                assert!(Mirrors::read(fe, d, o).agree(0, t).is_err(), "a stale D colour still refuses");
            }
        }
    }

    #[test]
    fn native_row_identity_requires_the_saved_ordinal_and_hash() {
        // Saved Sword outfit row 43 is a child-array ordinal, independent of root map member count.
        let hash = 0x147bc59435;
        let id = (0x13u64 << 56) | (43u64 << 40) | hash;
        assert_eq!(check_row_id(OUTFIT_TABLE, 43, hash, id), Ok(()));
        assert_eq!(check_row_id(OUTFIT_TABLE, 41, 0x1cce3b03f5, 0x1300291cce3b03f5), Ok(()));
        assert!(check_row_id(OUTFIT_TABLE, 43, hash, 0x13ffff0000000000).is_err());
        assert!(check_row_id(OUTFIT_TABLE, 43, hash, id ^ 1).is_err());
        assert!(check_row_id(OUTFIT_TABLE, 41, hash, id).is_err());
        assert!(check_row_id(HAT_TABLE, 43, hash, id).is_err());
        assert!(check_row_id(OUTFIT_TABLE, 43, hash, 43).is_err(), "a numeric row field is not a tagged identity");
        assert_eq!(check_row_id(HAT_TABLE, 0, HAT_DEFAULT & 0xff_ffff_ffff, HAT_DEFAULT), Ok(()));
    }

    #[test]
    fn fingerprint_table_is_distinct_and_complete() {
        let mut offsets: Vec<usize> = SITES.iter().map(|s| s.0).collect();
        offsets.sort_unstable();
        offsets.dedup();
        assert_eq!(offsets.len(), SITES.len());
        for site in [0x66ddfc, 0x17e69b4, 0x17e71a0, 0x657e18, 0x67ead0, LOCK, UNLOCK, INSTALL, OUTFIT_ROW, HAT_ROW] {
            assert!(offsets.contains(&site));
        }
        assert_eq!(row_hint(0x1300291bfef7a5f7), 0x29);
        assert_eq!(row_hint(HAT_DEFAULT), 0);
    }
}
