//! Shared exact native CPU1 / helper2,3 association readers; no diagnostic policy.
use super::*;
use values::{CpuHelper, CpuOwner};
#[derive(Clone, Copy)]
pub(crate) struct Live {
    symbol: usize,
    manager: usize,
    internal: usize,
    entries: [usize; 8],
    kind: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub(crate) owners: [CpuOwner; 4],
    pub(crate) helpers: [CpuHelper; 2],
}

/// Manager lookup and all Rust synchronization occur before taking the native G mutex.
#[cfg(feature = "mii-cpu-adapter-probe")]
pub(crate) unsafe fn resolve(target: *const u8) -> Result<Live, &'static str> {
    resolve_for_kind(target, 0x49)
}

pub(crate) unsafe fn resolve_for_kind(target: *const u8, kind: u32) -> Result<Live, &'static str> {
    if !(0x48..=0x4a).contains(&kind) { return Err("CPU adapter: saved class required"); }
    if !crate::common::is_native_training_mode() {
        return Err("CPU adapter: native Training required");
    }
    if !crate::character_roster::mii_cpu_probe_roster_idle() {
        return Err("CPU adapter: original one-CPU roster is not idle");
    }
    let symbol = training_mod_sync::read(&crate::common::FIGHTER_MANAGER_ADDR);
    if !aligned(symbol) {
        return Err("CPU adapter: manager symbol unavailable");
    }
    let manager: usize = rd(symbol, 0);
    if !aligned(manager) {
        return Err("CPU adapter: manager unavailable");
    }
    let internal: usize = rd(manager, 0);
    if !aligned(internal) {
        return Err("CPU adapter: internal manager unavailable");
    }
    let entries = std::array::from_fn(|id| {
        smash::app::lua_bind::FighterManager::get_fighter_entry(
            manager as *mut smash::app::FighterManager,
            smash::app::FighterEntryID(id as i32),
        ) as usize
    });
    if entries[1] != target as usize {
        return Err("CPU adapter: target is not current CPU1");
    }
    let live = Live {
        symbol,
        manager,
        internal,
        entries,
        kind,
    };
    read_scene(getRegionAddress(Region::Text) as usize, live)?;
    Ok(live)
}

/// Exact native inactive single-form body shape, as in native_source::parked_ready, without
/// its selectable-source catalog admission (native saved Miis are intentionally not in it).
#[cfg(feature = "mii-cpu-adapter-probe")]
pub(crate) unsafe fn helper_ready(base: usize, fe: usize, id: u32) -> bool {
    helper_ready_for_kind(base, fe, id, 0x49)
}

pub(crate) unsafe fn helper_ready_for_kind(base: usize, fe: usize, id: u32, kind: u32) -> bool {
    if rd::<u32>(fe, 0x14) != 1
        || rd::<[u32; 3]>(fe, 0x18) != [kind, u32::MAX, u32::MAX]
        || rd::<u32>(fe, 0x28) != 1
        || rd::<u64>(fe, 0x4150) != 1
        || !matches!(rd::<u8>(fe, 0x5920), 0 | 1)
        || rd::<u8>(fe, 0x591c) != 14
        || rd::<u8>(fe, 0x5935) != 0
    {
        return false;
    }
    let body: usize = rd(fe, 0x4160);
    aligned(body)
        && rd::<usize>(body, 0) == base + 0x4f80210
        && rd::<u32>(body, 0xc) == kind
        && rd::<u64>(body, 0x10) == u64::from(id)
        && rd::<u8>(body, 0x3a) == 3
        && aligned(rd(body, 0x20))
}

/// Only raw current native reads and value checks; safe to invoke with G held. In particular,
/// never reacquire a Rust state lock, call the native manager getter, or trust an old O/D pointer.
pub(crate) unsafe fn read_scene(base: usize, live: Live) -> Result<[CpuOwner; 4], &'static str> {
    let manager: usize = rd(live.symbol, 0);
    if manager != live.manager || !aligned(manager) {
        return Err("CPU adapter: manager changed");
    }
    let internal: usize = rd(manager, 0);
    if internal != live.internal || !aligned(internal) || rd::<u32>(internal, 0xa0) != 4 {
        return Err("CPU adapter: native layout changed");
    }
    let current: [usize; 8] = rd(internal, 0x20);
    if current != live.entries
        || rd::<[usize; 8]>(internal, 0x60) != [0; 8]
        || current[4..].iter().any(|p| *p != 0)
    {
        return Err("CPU adapter: primary/backup map changed");
    }
    let table: usize = rd(base, OWNER_TABLE);
    if !aligned(table) {
        return Err("CPU adapter: owner table unavailable");
    }
    let mut owners = [None; 4];
    for (id, fe) in current.iter().copied().take(4).enumerate() {
        if !aligned(fe) || rd::<u32>(fe, 0xe0) != id as u32 {
            return Err("CPU adapter: current entry owner mismatch");
        }
        let e = base + 0x52c5730 + id * 0x260;
        let kind: u32 = rd(fe, 0x18);
        let d: usize = rd(fe, 0xf8);
        let object: usize = rd(table, 0xe8 + id * 8);
        if !aligned(d)
            || !aligned(object)
            || rd::<u32>(e, 0x14) != id as u32
            || i64::from(rd::<i8>(e, 8)) != i64::from(kind)
            || rd::<u16>(e, 0x1a8) != rd::<u16>(fe, 0x46)
        {
            return Err("CPU adapter: current E/FE/D/O association mismatch");
        }
        if id < 2 {
            if !crate::character_roster::mii_saved_cpu_visible_ready(fe as *const u8, id as u32, live.kind) {
                return Err("CPU adapter: visible native entry not ready");
            }
        } else if !helper_ready_for_kind(base, fe, id as u32, live.kind) {
            return Err("CPU adapter: native Mii helper not parked ready");
        }
        let index = rd(e, 0x1a8);
        if id != 0 {
            mirrors(e, fe, d, object)
                .validate(index, tokens(Tokens::from_row(e)))
                .map_err(|_| "CPU adapter: current Mii mirrors differ")?;
        }
        owners[id] = Some(CpuOwner {
            owner: id as u32,
            kind,
            selector: rd(e, 0xa7),
            index,
            entry: fe,
            row: e,
            information: d,
            object,
            alias: rd(table, 0x128 + id),
        });
    }
    let owners = owners.map(Option::unwrap);
    values::qualified_saved_cpu_scope(&owners, false, live.kind)
        .map_err(|_| "CPU adapter: owner/index/alias scope")?;
    Ok(owners)
}

unsafe fn mirrors(e: usize, fe: usize, d: usize, o: usize) -> values::Mirrors {
    values::Mirrors::decode(
        rd(e, 0x1a8),
        rd(fe, 0x46),
        rd(d, 0x72),
        rd(d, 0x390),
        rd(o, 0x68),
    )
}

/// Current G is locked by the caller. No native or Rust synchronization calls here.
pub(crate) unsafe fn locked_snapshot(
    base: usize,
    live: Live,
    current: Context,
) -> Result<Snapshot, &'static str> {
    let owners = read_scene(base, live)?;
    if owners[1].entry != current.binding.entry || owners[1].row != current.binding.row {
        return Err("CPU adapter: callback target changed");
    }
    let mut helpers = [None; 2];
    for (i, owner) in owners[2..].iter().copied().enumerate() {
        let binding = Binding {
            owner: owner.owner,
            entry: owner.entry,
            row: owner.row,
            record_index: owner.index,
            ..current.binding
        };
        let at = values::record_address(binding, binding)
            .map_err(|_| "CPU adapter: helper record out of bounds")?;
        let body: usize = rd(owner.entry, 0x4160);
        helpers[i] = Some(CpuHelper {
            owner,
            mirrors: mirrors(owner.row, owner.entry, owner.information, owner.object),
            record: rd(at, 0),
            body,
            body_id: rd(body, 8),
        });
    }
    if read_scene(base, live)? != owners {
        return Err("CPU adapter: identities changed during snapshot");
    }
    Ok(Snapshot {
        owners,
        helpers: helpers.map(Option::unwrap),
    })
}

#[cfg(feature = "mii-cpu-adapter-probe")]
pub(crate) unsafe fn snapshot(
    entry: *const u8,
    current: Context,
) -> Result<Snapshot, &'static str> {
    let live = resolve(entry)?;
    let base = getRegionAddress(Region::Text) as usize;
    let guard = PoolLock::acquire(base, current.binding.manager);
    let fresh = context(
        base,
        entry as usize,
        current.binding.row,
        Some(guard.g),
        current.thread,
        current.binding.owner,
        current.binding.epoch,
    );
    let result = match fresh {
        Ok(fresh) if fresh == current => locked_snapshot(base, live, fresh),
        _ => Err("CPU adapter: locked context changed"),
    };
    drop(guard);
    result
}

#[cfg(feature = "mii-cpu-adapter-probe")]
pub(crate) fn unchanged(expected: &Snapshot, current: &Snapshot) -> Result<(), &'static str> {
    if expected.owners != current.owners {
        return Err("CPU adapter: current owner associations changed");
    }
    values::preserved_cpu_helpers(&expected.helpers, &current.helpers)
        .map_err(|_| "CPU adapter: native helper configuration/resource identity changed")
}
