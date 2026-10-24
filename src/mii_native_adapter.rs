//! Optional regression adapter, called only by the existing fullconfig probe while its native
//! Training/current target/ready gate and INSTALL token are held. Not production admission or a G lifetime pin.
//! The caller supplies the current service FE; stored pointers are comparison identities only.

use super::*;
#[cfg(feature = "mii-cpu-adapter-probe")]
#[path = "mii_cpu_adapter.rs"]
pub(super) mod cpu;
const TARGET_OWNER: u32 = if cfg!(feature = "mii-cpu-adapter-probe") {
    1
} else {
    0
};
#[cfg(feature="mii-cpu-adapter-probe")]
use training_mod_consts::mii::lifetime::Binding;
use training_mod_consts::mii::native_values::{self as values, RecordUpdate, RowIds};

pub(crate) use crate::mii_native_access::Context;
use crate::mii_native_access::{copy_update, PoolLock, tokens};

/// Current native registration and original scope, before taking the nonrecursive pool mutex.
unsafe fn bind(entry: *const u8) -> Result<crate::mii_identity_binding::BoundEntry, &'static str> {
    let bound = bind_saved_entry(entry).ok_or("adapter: current saved-row binding failed")?;
    if bound.id != TARGET_OWNER
        || bound.kind != 0x49
        || rd::<u32>(entry as usize, 0xe0) != TARGET_OWNER
    {
        return Err("adapter: only the feature-selected original saved Sword owner is qualified");
    }
    #[cfg(not(feature = "mii-cpu-adapter-probe"))]
    if other_mii_entry(entry as usize).is_some() {
        return Err("adapter: other Mii entry or unavailable manager");
    }
    #[cfg(feature = "mii-cpu-adapter-probe")]
    cpu::resolve(entry)?;
    Ok(bound)
}

unsafe fn context(base:usize,fe:usize,e:usize,locked_g:Option<usize>,thread:usize)->Result<Context,&'static str> {
    crate::mii_native_access::context(base,fe,e,locked_g,thread,TARGET_OWNER,0)
}
pub(super) unsafe fn capture(entry: *const u8) -> Result<Context, &'static str> {
    let bound = bind(entry)?;
    context(
        getRegionAddress(Region::Text) as usize,
        entry as usize,
        bound.e,
        None,
        skyline::nn::os::GetCurrentThread() as usize,
    )
}

unsafe fn rows(base: usize, alt: Alternate) -> Result<RowIds, &'static str> {
    let result = RowIds {
        outfit: lookup_row_id(base, OUTFIT_TABLE, alt.tokens.outfit)?,
        hat: lookup_row_id(base, HAT_TABLE, alt.tokens.hat)?,
    };
    result
        .validate_tokens(tokens(alt.tokens), alt.outfit_hash40, alt.hat_hash40)
        .map_err(|_| "adapter: native row hint/hash identity mismatch")?;
    Ok(result)
}

#[cfg(feature = "mii-cpu-adapter-probe")]
pub(super) fn ack_context(c: Context) -> serde_json::Value {
    let b = c.binding;
    serde_json::json!({"epoch":b.epoch,"owner":b.owner,"entry":ack::hex(b.entry),
        "row":ack::hex(b.row),"holder":ack::hex(b.holder),"manager":ack::hex(b.manager),
        "vector_begin":ack::hex(b.vector_begin),"vector_end":ack::hex(b.vector_end),
        "record_index":b.record_index,"thread":ack::hex(c.thread)})
}

fn event(stage: &str, current: Context, previous: Context, caller: usize) {
    let base = unsafe { getRegionAddress(Region::Text) as usize };
    log_always(format_args!(
        "[MiiNativeAdapter] {stage} current={current:x?} expected={previous:x?} identity_equal={} service_entry_lr={caller:#x} service_entry_lr_main={:#x?}",
        current == previous, caller.checked_sub(base)
    ));
}

/// All fallible checks precede the first native write. Native getter calls and current manager
/// binding are outside G's mutex; no Rust state mutex is held. While G is locked, rederive its
/// holder/vector/index and fresh D/O, compare exact owned copies, then copy only CharInfo. Unlock
/// before the existing token writes and caller's identical descriptor INSTALL. No timer cleanup.
pub(super) unsafe fn apply(
    entry: *mut u8,
    expected: Context,
    before: Alternate,
    after: Alternate,
    caller: usize,
    #[cfg(feature = "mii-cpu-adapter-probe")] cpu_expected: &cpu::Snapshot,
) -> Result<u64, String> {
    let base = getRegionAddress(Region::Text) as usize;
    let bound = bind(entry)?;
    if before.header.class as u32 != 0x49 || after.header.class as u32 != 0x49 {
        return Err("adapter: configuration class changed".into());
    }
    let previous_rows = rows(base, before)?;
    let target_rows = rows(base, after)?;
    if before.tokens.hat != after.tokens.hat || previous_rows.hat != target_rows.hat {
        return Err("adapter: hat changed".into());
    }
    let current = context(
        base,
        entry as usize,
        bound.e,
        None,
        skyline::nn::os::GetCurrentThread() as usize,
    )?;
    if current != expected {
        return Err("adapter: pre-lock pool/entry/thread identity changed".into());
    }
    #[cfg(feature = "mii-cpu-adapter-probe")]
    let cpu_live = cpu::resolve(entry)?;
    event("before", current, expected, caller);
    #[cfg(feature = "mii-cpu-adapter-probe")]
    ack::prepared(PHASE.load(Ordering::Relaxed) + 1, serde_json::json!({
        "current":ack_context(current),"expected":ack_context(expected),"identity_equal":current == expected,
        "service_entry_lr":ack::hex(caller),"service_entry_lr_main":caller.checked_sub(base).map(ack::hex),
        "before_saved_id":before.id,"after_saved_id":after.id,
        "before_tokens":ack_tokens(before.tokens),"after_tokens":ack_tokens(after.tokens),
        "before_fnv":ack::hash(fnv1a64(&before.charinfo)),"after_fnv":ack::hash(fnv1a64(&after.charinfo)),
        "native_row_ids":[ack::hash(previous_rows.outfit),ack::hash(previous_rows.hat),
                          ack::hash(target_rows.outfit),ack::hash(target_rows.hat)],
        "helpers":cpu::ack_view(cpu_expected),"writes_committed":false
    }))?;
    let guard = PoolLock::acquire(base,current.binding.manager);
    // Inner error values are static; String formatting and all event logs occur after unlock.
    let result = locked_apply(
        base,
        entry,
        bound.e,
        expected,
        guard.g,
        [before, after],
        #[cfg(feature = "mii-cpu-adapter-probe")]
        (cpu_live, cpu_expected),
    );
    drop(guard);
    let (targets, committed, written) = result.map_err(str::to_string)?;
    write_tokens(&targets, after.tokens);
    event("committed", committed, expected, caller);
    #[cfg(feature = "mii-cpu-adapter-probe")]
    ack::committed(serde_json::json!({"phase":PHASE.load(Ordering::Relaxed) + 1,
        "current":ack_context(committed),"expected":ack_context(expected),
        "identity_equal":committed == expected,"written_fnv":ack::hash(fnv1a64(&written)),
        "tokens":ack_tokens(after.tokens),"service_entry_lr":ack::hex(caller),
        "service_entry_lr_main":caller.checked_sub(base).map(ack::hex)}));
    Ok(fnv1a64(&written))
}

/// A new binding from the current completion callback, never a dereference of saved identities.
pub(super) unsafe fn complete(
    entry: *const u8,
    expected: Context,
    caller: usize,
    #[cfg(feature = "mii-cpu-adapter-probe")] cpu_expected: &cpu::Snapshot,
) {
    match capture(entry) {
        Ok(current) => {
            event("complete", current, expected, caller);
            #[cfg(feature = "mii-cpu-adapter-probe")]
            {
                let (valid, helpers) = cpu::complete(entry, current, cpu_expected);
                ack::observation(serde_json::json!({"valid":valid && current == expected,
                    "current":ack_context(current),"expected":ack_context(expected),
                    "identity_equal":current == expected,"helpers_valid":valid,"helpers":helpers,
                    "service_entry_lr":ack::hex(caller),
                    "service_entry_lr_main":caller.checked_sub(getRegionAddress(Region::Text) as usize).map(ack::hex)}));
            }
            if current != expected {
                ENDED.store(true, Ordering::Relaxed);
                log_always(format_args!("[MiiNativeAdapter] identity changed across native reload; further shots refused"));
            }
        }
        Err(reason) => {
            #[cfg(feature = "mii-cpu-adapter-probe")]
            ack::observation(serde_json::json!({"valid":false,"reason":reason}));
            ENDED.store(true, Ordering::Relaxed);
            log_always(format_args!(
                "[MiiNativeAdapter] completion binding refused: {reason}; further shots refused"
            ));
        }
    }
}

/// Called only with the current G mutex held by `apply`; no callbacks or fallible work after copy.
unsafe fn locked_apply(
    base: usize,
    entry: *mut u8,
    e: usize,
    expected: Context,
    g: usize,
    configurations: [Alternate; 2],
    #[cfg(feature = "mii-cpu-adapter-probe")] cpu_state: (cpu::Live, &cpu::Snapshot),
) -> Result<(Targets, Context, [u8; CHARINFO_LEN]), &'static str> {
    let [before, after] = configurations;
    let fresh = context(base, entry as usize, e, Some(g), expected.thread)?;
    if fresh != expected {
        return Err("adapter: locked pool/entry/thread identity changed");
    }
    let record = values::record_address(expected.binding, fresh.binding)
        .map_err(|_| "adapter: locked record address changed")?;
    let d: usize = rd(entry as usize, 0xf8);
    let o = owner_object(base, TARGET_OWNER).ok_or("adapter: current owner object unavailable")?;
    if !aligned(d) {
        return Err("adapter: current information block unavailable");
    }
    let mirrors = values::Mirrors::decode(
        rd(e, 0x1a8),
        rd(entry as usize, 0x46),
        rd(d, 0x72),
        rd(d, 0x390),
        rd(o, 0x68),
    );
    mirrors
        .validate(fresh.binding.record_index, tokens(before.tokens))
        .map_err(|_| "adapter: locked E/FE/D/O mirrors differ from expected")?;
    let copy: [u8; values::RECORD_LEN] = rd(record, 0);
    let update =
        RecordUpdate::compare_record(before.charinfo, after.charinfo, tokens(after.tokens), &copy)
            .map_err(|_| "adapter: locked CharInfo differs from expected")?;
    #[cfg(feature = "mii-cpu-adapter-probe")]
    {
        let current_cpu = cpu::locked_snapshot(base, cpu_state.0, fresh)?;
        cpu::unchanged(cpu_state.1, &current_cpu)?;
    }
    // No fallible operation after this point. Shared_ptr bytes +58..68 are untouched.
    copy_update(record,&update);
    Ok((
        Targets {
            e,
            fe: entry as usize,
            d,
            o,
        },
        fresh,
        rd(record, 0),
    ))
}
