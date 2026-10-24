//! CPU-only diagnostic admission. All saved pointers below are comparison identities.
//! Native reads use the current manager map, current callback and current G critical section.
use super::*;
use std::convert::TryInto;
use values::CpuOwner;

pub(super) use crate::mii_native_access::cpu::{resolve, locked_snapshot, snapshot, unchanged, read_scene, helper_ready, Live, Snapshot};

pub(crate) unsafe fn complete(entry: *const u8, current: Context, expected: &Snapshot) -> (bool, serde_json::Value) {
    let result = snapshot(entry, current);
    let valid = result
        .as_ref()
        .is_ok_and(|fresh| unchanged(expected, fresh).is_ok());
    log_always(format_args!(
        "[MiiCpuAdapter] helpers_complete valid={valid} current={:x?}",
        result.as_ref().map(view)
    ));
    if !valid {
        ENDED.store(true, Ordering::Relaxed);
    }
    let evidence = match &result {
        Ok(s) => ack_view(s),
        Err(reason) => serde_json::json!({"error":reason}),
    };
    (valid, evidence)
}

fn resource(record: &[u8; values::RECORD_LEN]) -> [u64; 2] {
    std::array::from_fn(|i| {
        u64::from_le_bytes(
            record[CHARINFO_LEN + i * 8..CHARINFO_LEN + (i + 1) * 8]
                .try_into()
                .unwrap(),
        )
    })
}

// Owned, bounded diagnostic values; raw CharInfo is retained only for the exact comparison.
type HelperView = (values::Mirrors, u64, [u64; 2], usize, u32);
pub(crate) type View = ([CpuOwner; 4], [HelperView; 2]);
pub(crate) fn view(s: &Snapshot) -> View {
    (
        s.owners,
        s.helpers.map(|h| {
            (
                h.mirrors,
                fnv1a64(&h.record[..CHARINFO_LEN]),
                resource(&h.record),
                h.body,
                h.body_id,
            )
        }),
    )
}

fn ack_owners(owners: &[CpuOwner; 4]) -> serde_json::Value {
    serde_json::Value::Array(owners.iter().map(|o| serde_json::json!({
        "owner":o.owner,"kind":o.kind,"selector":o.selector,"index":o.index,"alias":o.alias,
        "entry":ack::hex(o.entry),"row":ack::hex(o.row),"information":ack::hex(o.information),
        "object":ack::hex(o.object)
    })).collect())
}

pub(super) fn ack_view(s: &Snapshot) -> serde_json::Value {
    let helpers: Vec<_> = s.helpers.iter().map(|h| {
        let m = h.mirrors;
        serde_json::json!({"owner":h.owner.owner,"index":m.index,
            "tokens":[m.tokens.outfit,m.tokens.hat,m.tokens.colour,m.tokens.voice],
            "specials":m.tokens.specials,"fe":m.fe,"d":m.d,"d_specials":m.d_specials,
            "owner_index":m.owner_index,"owner_tokens":m.owner,
            "record_fnv":ack::hash(fnv1a64(&h.record[..CHARINFO_LEN])),
            "resources":resource(&h.record).map(ack::hash),"body":ack::hex(h.body),"body_id":h.body_id})
    }).collect();
    serde_json::json!({"owners":ack_owners(&s.owners),"helpers":helpers})
}

pub(crate) fn fixture(
    s: &Snapshot,
    original: Alternate,
    alternate: Alternate,
) -> Result<(), &'static str> {
    if original.id != 2622
        || alternate.id != 13216
        || original.header.class as u32 != 0x49
        || alternate.header.class as u32 != 0x49
    {
        return Err("CPU adapter: only saved Sword2622 to Sword13216 is qualified");
    }
    for helper in s.helpers {
        if helper.record[..CHARINFO_LEN] != original.charinfo
            || helper
                .mirrors
                .validate(helper.owner.index, tokens(original.tokens))
                .is_err()
        {
            return Err(
                "CPU adapter: helper is not the unchanged original saved Sword configuration",
            );
        }
    }
    Ok(())
}

static ORDINARY_OBSERVED: AtomicBool = AtomicBool::new(false);

pub(crate) fn ordinary_pending() -> bool {
    !ORDINARY_OBSERVED.load(Ordering::Relaxed)
}

/// One ordinary P1 READY observation in this process, from its actual native callback.
/// Early not-ready shapes cause only cheap checks; no recurring G snapshots/logs/allocations.
pub(crate) unsafe fn observe_ordinary(entry: *const u8) {
    if ORDINARY_OBSERVED.load(Ordering::Relaxed) {
        return;
    }
    let symbol = training_mod_sync::read(&crate::common::FIGHTER_MANAGER_ADDR);
    if !aligned(symbol) {
        return;
    }
    let manager: usize = rd(symbol, 0);
    if !aligned(manager) {
        return;
    }
    let internal: usize = rd(manager, 0);
    if !aligned(internal) || rd::<usize>(internal, 0x20) != entry as usize {
        return;
    }
    let current: [usize; 4] = rd(internal, 0x20);
    let base = getRegionAddress(Region::Text) as usize;
    if !current.iter().all(|fe| aligned(*fe))
        || !crate::character_roster::mii_cpu_probe_visible_ready(entry, 0)
        || !crate::character_roster::mii_cpu_probe_visible_ready(current[1] as *const u8, 1)
        || !helper_ready(base, current[2], 2)
        || !helper_ready(base, current[3], 3)
    {
        return;
    }
    if ORDINARY_OBSERVED.swap(true, Ordering::Relaxed) {
        return;
    }
    let target = current[1] as *const u8;
    let result = resolve(target).and_then(|live| ordinary_snapshot(live, target));
    match result {
        Ok(row) => {
            log_always(format_args!(
                "[MiiCpuAdapter] ordinary_ready current_p1={:#x} owners={:x?} pool={:x?} indices={:x?} owner_initialized={:#x} selector_record_fnv={:016x} selector_resource={:x?}; observation only, no ordinary ownership inferred",
                entry as usize, row.owners, row.pool, row.indices, row.owner_initialized,
                fnv1a64(&row.record[..CHARINFO_LEN]), resource(&row.record)
            ));
            ack::ordinary_ready(target as usize, serde_json::json!({"current_p1":ack::hex(entry as usize),
                "owners":ack_owners(&row.owners),"pool":ack_context(row.pool),"indices":row.indices,
                "owner_initialized":row.owner_initialized,"selector_record_fnv":ack::hash(fnv1a64(&row.record[..CHARINFO_LEN])),
                "selector_resources":resource(&row.record).map(ack::hash),"ownership_inferred":false}));
        }
        Err(reason) => {
            log_always(format_args!("[MiiCpuAdapter] ordinary_ready refused current_p1={:#x} reason={reason}; no retry, no write", entry as usize));
            ack::terminal(ack::REFUSED, 0, "ordinary_ready_refused",
                serde_json::json!({"reason":reason,"current_p1":ack::hex(entry as usize),"writes_committed":false}));
        }
    }
}

struct Ordinary {
    owners: [CpuOwner; 4],
    pool: Context,
    indices: [u16; 4],
    owner_initialized: u32,
    record: [u8; values::RECORD_LEN],
}

unsafe fn ordinary_snapshot(live: Live, target: *const u8) -> Result<Ordinary, &'static str> {
    let current = capture(target)?;
    let base = getRegionAddress(Region::Text) as usize;
    let guard=PoolLock::acquire(base,current.binding.manager);
    let result = (|| {
        let fresh = context(
            base,
            target as usize,
            current.binding.row,
            Some(guard.g),
            current.thread,
        )?;
        if fresh != current {
            return Err("ordinary observation: pool context changed");
        }
        let owners = read_scene(base, live)?;
        let p1 = owners[0];
        let selector_binding = Binding {
            owner: 0,
            entry: p1.entry,
            row: p1.row,
            record_index: u16::from(p1.selector),
            ..fresh.binding
        };
        let at = values::record_address(selector_binding, selector_binding)
            .map_err(|_| "ordinary observation: selector outside current vector")?;
        let result = Ordinary {
            owners,
            pool: fresh,
            indices: [
                rd(p1.row, 0x1a8),
                rd(p1.entry, 0x46),
                rd(p1.information, 0x72),
                rd(p1.object, 0x68),
            ],
            owner_initialized: rd(p1.object, 0x4c),
            record: rd(at, 0),
        };
        if read_scene(base, live)? != owners {
            return Err("ordinary observation: current identities changed");
        }
        Ok(result)
    })();
    drop(guard);
    result
}
