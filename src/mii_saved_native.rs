//! Current-callback adapter for saved-original same-class alpha. No scheduler, hooks, pool allocation or IO.
use crate::mii_native_access::*;
use skyline::hooks::{getRegionAddress, Region};
use training_mod_consts::mii::{
    inventory::Snapshot,
    lifetime::{Configuration, Observed},
    native_values::{self as values, RowIds},
    role_adapter::{Commit, Current, Scope},
    selection::Role,
};

/// Resolved outside G. All numeric identities are rechecked from the current map under G.
#[derive(Clone, Copy)]
enum Authority {
    Player {
        symbol: usize,
        manager: usize,
        internal: usize,
        entries: [usize; 8],
    },
    Cpu(cpu::Live),
}

unsafe fn authority(entry: *const u8, role: Role) -> Result<Authority, &'static str> {
    if !crate::common::is_native_training_mode() {
        return Err("native Training required");
    }
    if role == Role::Cpu {
        return cpu::resolve_for_kind(entry, rd(entry as usize, 0x18)).map(Authority::Cpu);
    }
    let symbol = training_mod_sync::read(&crate::common::FIGHTER_MANAGER_ADDR);
    if !aligned(symbol) {
        return Err("manager symbol unavailable");
    }
    let manager: usize = rd(symbol, 0);
    if !aligned(manager) {
        return Err("manager unavailable");
    }
    let internal: usize = rd(manager, 0);
    if !aligned(internal) {
        return Err("internal manager unavailable");
    }
    let entries = std::array::from_fn(|id| {
        smash::app::lua_bind::FighterManager::get_fighter_entry(
            manager as *mut smash::app::FighterManager,
            smash::app::FighterEntryID(id as i32),
        ) as usize
    });
    if entries[0] != entry as usize {
        return Err("not current Player0");
    }
    Ok(Authority::Player {
        symbol,
        manager,
        internal,
        entries,
    })
}

unsafe fn scope(base: usize, a: Authority, c: Context) -> Result<Scope, &'static str> {
    match a {
        Authority::Cpu(live) => {
            let s = cpu::locked_snapshot(base, live, c)?;
            Ok(Scope::cpu(s.owners, s.helpers, false))
        }
        Authority::Player {
            symbol,
            manager,
            internal,
            entries,
        } => {
            if rd::<usize>(symbol, 0) != manager
                || rd::<usize>(manager, 0) != internal
                || rd::<[usize; 8]>(internal, 0x20) != entries
                || entries[0] != c.binding.entry
            {
                return Err("Player map changed");
            }
            let kinds = entries.map(|fe| aligned(fe).then(|| rd::<u32>(fe, 0x18)));
            values::qualified_saved_scope(c.binding, rd::<u32>(c.binding.entry, 0x18), kinds)
                .map_err(|_| "other Mii or invalid Player scope")?;
            Ok(Scope::player(kinds))
        }
    }
}

unsafe fn rows(base: usize, c: Configuration) -> Result<RowIds, &'static str> {
    let t = c.tokens();
    let ids = RowIds {
        outfit: lookup_row_id(base, OUTFIT_TABLE, t.outfit)?,
        hat: lookup_row_id(base, HAT_TABLE, t.hat)?,
    };
    ids.validate(c).map_err(|_| "saved hint/hash mismatch")?;
    Ok(ids)
}

/// Pure/raw reads only with G held. Row getters and manager calls already ran outside G.
unsafe fn locked_current(
    base: usize,
    entry: *const u8,
    expected: Context,
    a: Authority,
    configuration: Configuration,
    row_ids: RowIds,
) -> Result<Current, &'static str> {
    let c = context_for_kind(
        base,
        entry as usize,
        expected.binding.row,
        Some(expected.binding.manager),
        expected.thread,
        (expected.binding.owner, super::current_generation(expected.binding.owner)),
        configuration.canonical().class as u32,
    )?;
    if c != expected {
        return Err("locked binding/thread changed");
    }
    let d: usize = rd(entry as usize, 0xf8);
    let o = owner_object(base, c.binding.owner).ok_or("owner object unavailable")?;
    if !aligned(d) {
        return Err("information block unavailable");
    }
    let m = values::Mirrors::decode(
        rd(c.binding.row, 0x1a8),
        rd(entry as usize, 0x46),
        rd(d, 0x72),
        rd(d, 0x390),
        rd(o, 0x68),
    );
    m.validate(c.binding.record_index, configuration.tokens())
        .map_err(|_| "current mirrors differ")?;
    let at = values::record_address(c.binding, c.binding).map_err(|_| "record bounds")?;
    let record: [u8; values::RECORD_LEN] = rd(at, 0);
    if record[..CHARINFO_LEN] != *configuration.charinfo() {
        return Err("current CharInfo differs");
    }
    let scope = scope(base, a, c)?;
    Ok(Current {
        live: Observed {
            binding: c.binding,
            configuration,
        },
        thread: c.thread,
        information: d,
        object: o,
        scope,
        mirrors: m,
        row_ids,
        record,
    })
}

unsafe fn prepare(
    entry: *const u8,
    role: Role,
    epoch: u64,
) -> Result<(usize, Context, Authority), &'static str> {
    let owner = if role == Role::Player { 0 } else { 1 };
    let bound =
        crate::mii_identity_binding::bind_saved_entry(entry).ok_or("fresh saved-row binding")?;
    if bound.id != owner
        || !(0x48..=0x4a).contains(&bound.kind)
        || epoch == 0
        || super::current_generation(owner) != epoch
    {
        return Err("saved Mii role/generation required");
    }
    let a = authority(entry, role)?;
    let base = getRegionAddress(Region::Text) as usize;
    let c = context_for_kind(
        base,
        entry as usize,
        bound.e,
        None,
        skyline::nn::os::GetCurrentThread() as usize,
        (owner, epoch),
        bound.kind,
    )?;
    if c.thread == 0 {
        return Err("native thread unavailable");
    }
    Ok((base, c, a))
}

pub(super) unsafe fn current(
    entry: *const u8,
    role: Role,
    epoch: u64,
    c: Configuration,
) -> Result<Current, &'static str> {
    let ids = rows(getRegionAddress(Region::Text) as usize, c)?;
    let (base, context, a) = prepare(entry, role, epoch)?;
    let guard = PoolLock::acquire(base, context.binding.manager);
    let result = locked_current(base, entry, context, a, c, ids);
    drop(guard);
    result
}

/// Initial original capture uses exact unmasked bytes and all tokens, never name/hash alone.
/// Catalog is owned and complete; native reads are bounded to the one assigned record.
pub(super) unsafe fn original(
    entry: *const u8,
    role: Role,
    epoch: u64,
    catalog: &Snapshot,
) -> Result<Current, &'static str> {
    if !catalog.is_complete() {
        return Err("incomplete catalog");
    }
    let kind = rd::<u32>(entry as usize, 0x18);
    let (base, c, _a) = prepare(entry, role, epoch)?;
    let guard = PoolLock::acquire(base, c.binding.manager);
    let fresh = context_for_kind(
        base,
        entry as usize,
        c.binding.row,
        Some(guard.g),
        c.thread,
        (c.binding.owner, super::current_generation(c.binding.owner)),
        kind,
    );
    let raw = match fresh {
        Ok(fresh) if fresh == c => values::record_address(c.binding, fresh.binding)
            .map(|at| {
                (
                    rd::<[u8; values::RECORD_LEN]>(at, 0),
                    tokens(Tokens::from_row(c.binding.row)),
                )
            })
            .map_err(|_| "initial record bounds"),
        _ => Err("initial locked identity changed"),
    };
    drop(guard);
    let (record, tokens) = raw?;
    let configuration = catalog
        .configs
        .iter()
        .filter_map(|s| Configuration::from_saved(s).ok())
        .find(|s| s.canonical().class as u32 == kind
            && s.tokens() == tokens && *s.charinfo() == record[..CHARINFO_LEN])
        .ok_or("no exact original saved configuration")?;
    let ids = rows(base, configuration)?;
    let (base, fresh, a) = prepare(entry, role, epoch)?;
    if fresh != c {
        return Err("original binding changed across row validation");
    }
    let guard = PoolLock::acquire(base, fresh.binding.manager);
    let result = locked_current(base, entry, c, a, configuration, ids);
    drop(guard);
    result
}

/// Called only after the caller owns INSTALL and has submitted its pure lifetime ticket.
/// Nothing fallible follows the first record write. Unlock precedes token writes and INSTALL.
pub(super) unsafe fn apply(
    entry: *mut u8,
    role: Role,
    epoch: u64,
    commit: Commit,
) -> Result<(), &'static str> {
    let tx = commit.transaction();
    let base = getRegionAddress(Region::Text) as usize;
    let before = rows(base, tx.before())?;
    let after = rows(base, tx.after())?;
    let (base, c, a) = prepare(entry, role, epoch)?;
    let guard = PoolLock::acquire(base, c.binding.manager);
    let result = (|| {
        let now = locked_current(base, entry, c, a, tx.before(), before)?;
        let targets = Targets {
            e: now.live.binding.row,
            fe: entry as usize,
            d: now.information,
            o: now.object,
        };
        let record = values::record_address(now.live.binding, now.live.binding)
            .map_err(|_| "record changed")?;
        let update = commit
            .prepare_write(now, after)
            .map_err(|_| "owned transaction comparison refused")?;
        copy_update(record, &update);
        Ok(targets)
    })();
    drop(guard);
    let targets = result?;
    write_tokens(&targets, tx.after().tokens().into());
    Ok(())
}
