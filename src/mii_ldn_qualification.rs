//! Default-off, read-only, bounded Mii G/selector qualification capture.
//!
//! Answers one native fact for the exact local-wireless waiting map 0/4/5/6 (and the
//! Training map 0/1/2/3 as a reader control): the current FE/E/D/O identities, descriptor
//! +04/+46 words, E+a7 selector and E+1a8 index, D/O index mirrors, O alias/header, the
//! existing G holder/manager/vector extent, and only the selected/destination record
//! identities (hash + resource-word presence), each bounds-checked through
//! `native_values::record_address` before any read. One full sample per scene generation,
//! one bounded confirmation, refusals once per code. No slot allocation, native write,
//! catalog/resource/flag mutation or vector growth. G is held only for raw copies; no
//! manager lookup, Rust lock, formatting or logging happens under it. Raw CharInfo never
//! leaves `capture`. SourceBook remains the sole lifetime owner; this grants no authority.
use super::mii_ordinary_player::native::span;
use super::{lobby, native_source, BASE, INSTALL, STATES};
use crate::common::{self, game_mode};
use crate::mii_native_access::{aligned, rd, PoolLock, OWNER_TABLE, POOL_HOLDER};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use training_mod_consts::mii::{
    lifetime::Binding,
    native_values::{self as values, RECORD_LEN},
};

const CHARINFO_LEN: usize = 0x58;
const ROWS: usize = 0x52c5730;
const ROW_STRIDE: usize = 0x260;
const FE_SPAN: usize = 0x5a88;
const D_SPAN: usize = 0x3a0;
const O_SPAN: usize = 0xb0;
const POLL_LIMIT: u16 = 900;
const STABLE_TARGET: u16 = 30;

static ARMED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    Training,
    LdnWaiting,
}

impl Layout {
    fn owners(self) -> [usize; 4] {
        match self {
            Self::Training => [0, 1, 2, 3],
            Self::LdnWaiting => [0, 4, 5, 6],
        }
    }
}

/// Record identity only: index, whether the existing vector contains it, CharInfo hash and
/// whether the native shared_ptr data/control words are non-zero. Never raw bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RecordRef {
    index: u16,
    in_bounds: bool,
    hash: u64,
    data: bool,
    control: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DPart {
    d: usize,
    mirrors: [u16; 4],
    specials: [u32; 4],
    rebuild: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OPart {
    o: usize,
    init: u8,
    header: [u32; 5],
    costume: u32,
    index: u16,
    tokens: [u32; 4],
    task: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OwnerSample {
    id: u8,
    fe: usize,
    e: usize,
    fe_owner: u32,
    fe_selector_word: u32,
    fe_count: u32,
    fe_kinds: [u32; 3],
    fe_tokens: [u16; 5],
    fe_phase: [u8; 3],
    e_kind: i8,
    e_owner: u32,
    e_control: [u8; 7],
    e_index: u16,
    e_tokens: [u16; 4],
    e_specials: [u8; 4],
    alias: u8,
    d: Option<DPart>,
    o: Option<OPart>,
    current: RecordRef,
    destination: RecordRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pool {
    holder: usize,
    g: usize,
    begin: usize,
    end: usize,
    capacity: usize,
    records: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Roots {
    symbol: usize,
    manager: usize,
    internal: usize,
    count: u32,
    primary: [usize; 8],
    backup_null: bool,
    table: usize,
    selection: [(u8, u8, u8); 8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sample {
    layout: Layout,
    thread: usize,
    lobby_active: bool,
    roots: Roots,
    pool: Pool,
    owners: [OwnerSample; 4],
}

struct State {
    generation: u64,
    layout: Option<Layout>,
    entries: [usize; 8],
    polls: u16,
    refused: u32,
    emitted: u8,
    stable: Option<(Sample, u16)>,
    done: bool,
}

impl State {
    const fn new(generation: u64) -> Self {
        Self {
            generation,
            layout: None,
            entries: [0; 8],
            polls: 0,
            refused: 0,
            emitted: 0,
            stable: None,
            done: false,
        }
    }
    fn reset(&mut self) {
        *self = Self::new(self.generation.wrapping_add(1));
    }
}

static STATE: Mutex<State> = Mutex::new(State::new(0));

type Refusal = (u8, &'static str, usize, usize);

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}

fn ldn_waiting() -> bool {
    matches!(game_mode::current(), Some(mode)
        if mode.initialized && mode.melee == 0x0d && mode.mode == 0x02010000)
}

/// Exact LDN waiting with lobby practice active is the same admission the Ldn resource
/// backend uses; native Training is the known-good reader control.
unsafe fn current_layout() -> Option<Layout> {
    if common::is_native_training_mode() {
        Some(Layout::Training)
    } else if ldn_waiting() && lobby::active() {
        Some(Layout::LdnWaiting)
    } else {
        None
    }
}

fn refuse(generation: u64, (code, reason, first, second): Refusal) {
    let mut state = STATE.lock().unwrap();
    if state.generation != generation {
        return;
    }
    state.stable = None;
    let bit = 1u32 << code;
    let log = state.refused & bit == 0;
    state.refused |= bit;
    drop(state);
    if log {
        crate::logging::diagnostic(format_args!(
            "[MiiLdnQualification] refusal generation={} code={} reason={} detail=0x{:x}/0x{:x}; observation_only=true",
            generation, code, reason, first, second));
    }
}

/// Existing private state is queried with no native lock held.
unsafe fn idle() -> Result<(), Refusal> {
    if !crate::character_lifecycle::native_hooks_ready() || BASE.load(Ordering::Acquire) == 0 {
        return Err((1, "native lifecycle hooks/base not ready", 0, 0));
    }
    if !common::is_ready_go() {
        return Err((2, "scene not ready-go", 0, 0));
    }
    if INSTALL.active() {
        return Err((3, "native install active", 0, 0));
    }
    let busy = STATES.iter().any(|state| {
        let state = state.lock().unwrap();
        state.active.is_some() || state.pending.is_some()
    });
    if busy {
        return Err((4, "roster replacement pending/active", 0, 0));
    }
    Ok(())
}

/// Current roots from the native manager, reacquired on every call. The callback owner's
/// entry must be the current primary for that owner.
unsafe fn current_roots(layout: Layout, entry: usize, owner: u32) -> Result<Roots, Refusal> {
    let base = BASE.load(Ordering::Acquire);
    let symbol = training_mod_sync::read(&common::FIGHTER_MANAGER_ADDR);
    if !aligned(symbol) || !span(symbol, 8, false) {
        return Err((5, "fighter manager symbol unavailable", symbol, 0));
    }
    let manager: usize = rd(symbol, 0);
    if !aligned(manager) || !span(manager, 8, false) {
        return Err((5, "fighter manager unavailable", manager, 0));
    }
    let internal: usize = rd(manager, 0);
    if !aligned(internal) || !span(internal, 0xa4, false) {
        return Err((5, "internal manager unavailable", internal, 0));
    }
    let entries: [usize; 8] = std::array::from_fn(|id| {
        smash::app::lua_bind::FighterManager::get_fighter_entry(
            manager as *mut smash::app::FighterManager,
            smash::app::FighterEntryID(id as i32),
        ) as usize
    });
    let primary: [usize; 8] = rd(internal, 0x20);
    let backup: [usize; 8] = rd(internal, 0x60);
    let count: u32 = rd(internal, 0xa0);
    if primary != entries {
        return Err((6, "manager getter and primary map differ", primary[0], entries[0]));
    }
    if owner >= 8 || primary[owner as usize] != entry {
        return Err((6, "callback entry is not the current primary", owner as usize, entry));
    }
    match layout {
        Layout::Training => {
            if count != 4 || primary[..4].contains(&0) || primary[4..] != [0; 4] {
                return Err((7, "Training four-owner shape unavailable", count as usize, 0));
            }
        }
        Layout::LdnWaiting => {
            if !native_source::exact_waiting_layout(primary[5] as *const u8, 5)
                || !native_source::exact_waiting_layout(primary[6] as *const u8, 6)
            {
                return Err((7, "exact 0/4/5/6 waiting layout unavailable", primary[5], primary[6]));
            }
        }
    }
    let table: usize = rd(base, OWNER_TABLE);
    if !aligned(table) || !span(table, 0x130, false) {
        return Err((8, "owner table span unavailable", table, 0));
    }
    Ok(Roots {
        symbol,
        manager,
        internal,
        count,
        primary,
        backup_null: backup == [0; 8],
        table,
        selection: std::array::from_fn(|i| {
            let row = base + ROWS + i * ROW_STRIDE;
            (rd(row, 0xa), rd(row, 0x79), rd(row, 0xa7))
        }),
    })
}

/// Pool identity outside G; the same words are re-read under G and must agree.
unsafe fn pool_identity(base: usize) -> Result<(usize, usize), Refusal> {
    let holder: usize = rd(base, POOL_HOLDER);
    if !aligned(holder) || !span(holder, 8, false) {
        return Err((9, "pool holder unavailable", holder, 0));
    }
    let g: usize = rd(holder, 0);
    if !aligned(g) || !span(g, 0x24e8, false) {
        return Err((9, "pool manager unavailable", g, 0));
    }
    Ok((holder, g))
}

/// Raw owned copy of one existing record, or None when the exact bounds reader refuses or
/// the span is unreadable. Called only with G held; no logging or allocation here.
unsafe fn record_copy(binding: Binding) -> Option<[u8; RECORD_LEN]> {
    let at = values::record_address(binding, binding).ok()?;
    if !span(at, RECORD_LEN, false) {
        return None;
    }
    let mut raw = [0u8; RECORD_LEN];
    for (offset, byte) in raw.iter_mut().enumerate() {
        *byte = rd::<u8>(at, offset);
    }
    Some(raw)
}

fn record_ref(index: u16, raw: Option<[u8; RECORD_LEN]>) -> RecordRef {
    match raw {
        Some(raw) => RecordRef {
            index,
            in_bounds: true,
            hash: fnv(&raw[..CHARINFO_LEN]),
            data: raw[CHARINFO_LEN..CHARINFO_LEN + 8].iter().any(|b| *b != 0),
            control: raw[CHARINFO_LEN + 8..RECORD_LEN].iter().any(|b| *b != 0),
        },
        None => RecordRef { index, in_bounds: false, hash: 0, data: false, control: false },
    }
}

unsafe fn capture(layout: Layout, roots: Roots) -> Result<Sample, Refusal> {
    let base = BASE.load(Ordering::Acquire);
    let owners = layout.owners();
    let mut e_rows = [0usize; 4];
    for (slot, id) in owners.iter().copied().enumerate() {
        let fe = roots.primary[id];
        let e = base + ROWS + id * ROW_STRIDE;
        if !aligned(fe) || !span(fe, FE_SPAN, false) || !span(e, ROW_STRIDE, false) {
            return Err((6, "owner FE/E span unavailable", id, fe));
        }
        if rd::<u32>(fe, 0xe0) != id as u32 {
            return Err((6, "owner FE identity differs", id, fe));
        }
        e_rows[slot] = e;
    }
    let (holder, g) = pool_identity(base)?;
    // Raw copies under G only. Record indices are re-read inside the lock so the copied
    // record is the one the current E row names at that instant.
    let mut raw_current = [None; 4];
    let mut raw_destination = [None; 4];
    let mut indices = [(0u16, 0u8); 4];
    let (begin, end, capacity) = {
        let lock = PoolLock::acquire(base, g);
        if rd::<usize>(base, POOL_HOLDER) != holder || rd::<usize>(holder, 0) != lock.g {
            drop(lock);
            return Err((9, "pool rebound during lock acquisition", holder, g));
        }
        let begin = rd::<usize>(g, 0x24d0);
        let end = rd::<usize>(g, 0x24d8);
        let capacity = rd::<usize>(g, 0x24e0);
        for (slot, id) in owners.iter().copied().enumerate() {
            let e = e_rows[slot];
            let index: u16 = rd(e, 0x1a8);
            let selector: u8 = rd(e, 0xa7);
            indices[slot] = (index, selector);
            let binding = Binding {
                epoch: 0,
                owner: id as u32,
                entry: roots.primary[id],
                row: e,
                holder,
                manager: g,
                vector_begin: begin,
                vector_end: end,
                record_index: index,
            };
            raw_current[slot] = record_copy(binding);
            raw_destination[slot] = record_copy(Binding {
                record_index: u16::from(selector),
                ..binding
            });
        }
        drop(lock);
        (begin, end, capacity)
    };
    let length = end.checked_sub(begin).ok_or((10, "vector end precedes begin", begin, end))?;
    if !aligned(begin) || !length.is_multiple_of(RECORD_LEN) || capacity < end {
        return Err((10, "vector begin/end/capacity shape invalid", begin, end));
    }
    let pool = Pool { holder, g, begin, end, capacity, records: length / RECORD_LEN };
    let mut samples = [None; 4];
    for (slot, id) in owners.iter().copied().enumerate() {
        let fe = roots.primary[id];
        let e = e_rows[slot];
        let d: usize = rd(fe, 0xf8);
        let d = (aligned(d) && span(d, D_SPAN, false)).then(|| DPart {
            d,
            mirrors: rd(d, 0x72),
            specials: rd(d, 0x390),
            rebuild: rd(d, 0x94),
        });
        let o: usize = rd(roots.table, 0xe8 + id * 8);
        let o = (aligned(o) && span(o, O_SPAN, false)).then(|| OPart {
            o,
            init: rd(o, 0x4c),
            header: rd(o, 0x54),
            costume: rd(o, 0x64),
            index: rd(o, 0x68),
            tokens: rd(o, 0x6c),
            task: rd(o, 0xa8),
        });
        let (index, selector) = indices[slot];
        samples[slot] = Some(OwnerSample {
            id: id as u8,
            fe,
            e,
            fe_owner: rd(fe, 0xe0),
            fe_selector_word: rd(fe, 4),
            fe_count: rd(fe, 0x14),
            fe_kinds: rd(fe, 0x18),
            fe_tokens: rd(fe, 0x46),
            fe_phase: [rd(fe, 0x5920), rd(fe, 0x591c), rd(fe, 0x5935)],
            e_kind: rd(e, 8),
            e_owner: rd(e, 0x14),
            e_control: [0xa, 0x6b, 0x79, 0x78, 0x19, 0x1a, 0xa7].map(|offset| rd(e, offset)),
            e_index: index,
            e_tokens: rd(e, 0x1aa),
            e_specials: rd(e, 0x1b2),
            alias: rd(roots.table, 0x128 + id),
            d,
            o,
            current: record_ref(index, raw_current[slot]),
            destination: record_ref(u16::from(selector), raw_destination[slot]),
        });
    }
    Ok(Sample {
        layout,
        thread: skyline::nn::os::GetCurrentThread() as usize,
        lobby_active: lobby::active(),
        roots,
        pool,
        owners: samples.map(Option::unwrap),
    })
}

/// Pure value summary of the one question: per owner selector/index/mirrors/bounds, and
/// whether selectors and indices are pairwise distinct. It decides nothing.
fn summary(sample: &Sample) -> String {
    use std::fmt::Write;
    let mut line = String::with_capacity(512);
    let mut selectors = [0u8; 4];
    let mut indices = [0u16; 4];
    for (slot, owner) in sample.owners.iter().enumerate() {
        selectors[slot] = owner.e_control[6];
        indices[slot] = owner.e_index;
        let d_index = owner.d.map(|d| d.mirrors[0]);
        let o_index = owner.o.map(|o| o.index);
        let mirrors_agree = owner.fe_tokens[0] == owner.e_index
            && d_index == Some(owner.e_index)
            && o_index == Some(owner.e_index);
        let _ = write!(
            line,
            " owner{}:sel={} idx={} fe46={} d72={:?} o68={:?} mirrors_agree={} dest_in_bounds={} cur_in_bounds={} alias={} d={} o={} mii_kind={}",
            owner.id, owner.e_control[6], owner.e_index, owner.fe_tokens[0], d_index, o_index,
            mirrors_agree, owner.destination.in_bounds, owner.current.in_bounds, owner.alias,
            owner.d.is_some(), owner.o.is_some(), (0x48..=0x4a).contains(&owner.fe_kinds[0])
        );
    }
    let distinct = |values: &[u16]| {
        values.iter().enumerate().all(|(i, v)| !values[..i].contains(v))
    };
    let _ = write!(
        line,
        " distinct_selectors={} distinct_indices={} records={} g5_g6_present={}",
        distinct(&selectors.map(u16::from)), distinct(&indices), sample.pool.records,
        sample.pool.records > 6
    );
    line
}

fn emit(generation: u64, sample: &Sample, confirmation: Option<u16>) {
    let tag = match confirmation {
        Some(stable) if stable >= STABLE_TARGET => "stable",
        Some(_) => "changed",
        None => "first",
    };
    if tag == "stable" {
        // The first sample already carries every field; confirm identity in one line.
        crate::logging::diagnostic(format_args!(
            "[MiiLdnQualification] end kind=stable layout={:?} generation={} stable={:?} lines=1 unchanged_since_first=true observation_only=true",
            sample.layout, generation, confirmation));
        return;
    }
    crate::logging::diagnostic(format_args!(
        "[MiiLdnQualification] begin kind={} layout={:?} generation={} thread=0x{:x} lobby_active={} roots={:x?}",
        tag, sample.layout, generation, sample.thread, sample.lobby_active, sample.roots));
    crate::logging::diagnostic(format_args!(
        "[MiiLdnQualification] pool kind={} generation={} {:x?} record_len=0x{:x}",
        tag, generation, sample.pool, RECORD_LEN));
    for owner in sample.owners.iter() {
        crate::logging::diagnostic(format_args!(
            "[MiiLdnQualification] owner kind={} generation={} {:x?}", tag, generation, owner));
    }
    crate::logging::diagnostic(format_args!(
        "[MiiLdnQualification] summary kind={} generation={}{}", tag, generation, summary(sample)));
    crate::logging::diagnostic(format_args!(
        "[MiiLdnQualification] end kind={} generation={} stable={:?} lines=8 observation_only=true no_allocation=true no_native_write=true quantity_not_applicable=true",
        tag, generation, confirmation));
}

pub(super) fn scene_changed() {
    STATE.lock().unwrap().reset();
}

/// Numeric identities only: destruction of any bound owner entry ends this generation.
pub(super) fn on_destroy(entry: *const u8, owner: u32) {
    let mut state = STATE.lock().unwrap();
    if owner < 8 && state.entries[owner as usize] == entry as usize && entry as usize != 0 {
        state.reset();
    }
}

/// Runs from the already installed native service callback, before existing adapters.
/// Returning early never suppresses the native call.
pub(super) unsafe fn service(entry: *const u8, owner: u32) {
    if !ARMED.load(Ordering::Acquire) {
        return;
    }
    let Some(layout) = current_layout() else { return; };
    if !layout.owners().contains(&(owner as usize)) {
        return;
    }
    let generation = {
        let mut state = STATE.lock().unwrap();
        if state.layout.is_some_and(|previous| previous != layout) {
            state.reset();
        }
        state.layout = Some(layout);
        if state.entries[owner as usize] != 0 && state.entries[owner as usize] != entry as usize {
            state.reset();
            state.layout = Some(layout);
        }
        state.entries[owner as usize] = entry as usize;
        if state.done {
            return;
        }
        state.polls += 1;
        if state.polls > POLL_LIMIT {
            state.done = true;
            let detail = (state.polls as usize, state.emitted as usize);
            let generation = state.generation;
            drop(state);
            refuse(generation, (12, "bounded observation budget exhausted", detail.0, detail.1));
            return;
        }
        state.generation
    };
    if let Err(reason) = idle() {
        refuse(generation, reason);
        return;
    }
    let roots = match current_roots(layout, entry as usize, owner) {
        Ok(roots) => roots,
        Err(reason) => {
            refuse(generation, reason);
            return;
        }
    };
    let sample = match capture(layout, roots) {
        Ok(sample) => sample,
        Err(reason) => {
            refuse(generation, reason);
            return;
        }
    };
    // Reacquire roots after the locked copies; retained addresses are compared, never followed.
    if current_layout() != Some(layout) || idle().is_err()
        || current_roots(layout, entry as usize, owner).ok() != Some(roots)
    {
        refuse(generation, (11, "identity/idle changed during observation", owner as usize, entry as usize));
        return;
    }
    let mut state = STATE.lock().unwrap();
    if state.generation != generation || state.done {
        return;
    }
    // Bind every current owner identity so any exact destructor ends this generation.
    for id in layout.owners() {
        state.entries[id] = roots.primary[id];
    }
    let same = state.stable.is_some_and(|(before, _)| before == sample);
    let count = if same { state.stable.unwrap().1 + 1 } else { 1 };
    state.stable = Some((sample, count));
    let decision = if state.emitted == 0 {
        state.emitted = 1;
        Some(None)
    } else if same && count >= STABLE_TARGET {
        state.done = true;
        Some(Some(count))
    } else if !same && state.emitted == 1 {
        state.emitted = 2;
        Some(Some(1))
    } else if !same {
        // A second change after the one permitted confirmation ends the bounded run.
        state.done = true;
        None
    } else {
        None
    };
    drop(state);
    if let Some(confirmation) = decision {
        emit(generation, &sample, confirmation);
    }
}

/// After the roster's own fingerprints and BASE store. Reuses the saved-alpha pristine code
/// contract (pool lock/unlock stubs, applier lock sites, row getters); no new fingerprints.
pub(super) unsafe fn init(base: usize) {
    if !super::mii_saved_fingerprints::valid(base) {
        crate::logging::diagnostic(format_args!(
            "[MiiLdnQualification] armed=false reason=saved-alpha fingerprint mismatch"));
        return;
    }
    ARMED.store(true, Ordering::Release);
    crate::logging::diagnostic(format_args!(
        "[MiiLdnQualification] armed=true layouts=Training0/1/2/3,LdnWaiting0/4/5/6 readonly=true one_sample_per_generation=true poll_limit={} stable_target={}",
        POLL_LIMIT, STABLE_TARGET));
}
