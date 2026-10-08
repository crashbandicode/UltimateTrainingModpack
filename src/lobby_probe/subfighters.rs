//! Owned 13.0.5 FighterEntry observations, reached only after the parent fingerprint.
//! No cached native objects or mutable native calls; constructed body metadata only.

use std::sync::atomic::{AtomicUsize, Ordering};

use skyline::nn::ro::LookupSymbol;
use smash::app::{self, lua_bind::*, utility, BattleObjectModuleAccessor};
use smash::lib::lua_const::*;

static MANAGER_SYMBOL: AtomicUsize = AtomicUsize::new(0);
const DISPATCH_CODE: [u32; 8] = [
    0xd10283ff, 0xa9046ffc, 0xa90567fa, 0xa9065ff8, 0xa90757f6, 0xa9084ff4, 0xa9097bfd, 0x910243fd,
];
const EPOCH_LIMIT: u16 = 256;
const TOTAL_LIMIT: u16 = 512;
const STATUS_INTERVAL: u16 = 30;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Slots {
    configured: u32,
    kinds: [u32; 3],
    installed: u64,
    bodies: u8,
    controls: u8,
    active: u8,
    pending: u8,
    flags: u8,
    loader: u8,
    outer: u8,
    native_num: u64,
    previous: u8,
    request: u8,
    change: u8,
    commit: [u8; 4],
    extra: [u8; 2],
    value41e4: u32,
    flag41e8: u8,
    auxiliary: u8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Fighter {
    id: u32,
    kind: i32,
    status: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct BodySample {
    id: u32,
    kind: u32,
    lifecycle: u8,
    slot: u8,
    live: Option<(i32, u64, bool)>,
}

#[derive(Default)]
pub(super) struct Observer {
    slots: [Option<Slots>; 2],
    fighters: [Option<Fighter>; 2],
    status_ticks: [u16; 2],
    epoch_logs: u16,
    total_logs: u16,
    dispatch_slots: [Option<Slots>; 2],
    dispatch_calls: [u64; 2],
    dispatch_last: [u64; 2],
    dispatch_epoch_logs: u16,
    dispatch_total_logs: u16,
    body_keys: [Option<(u32, u32)>; 32],
    telemetry: [[Option<BodySample>; 3]; 2],
    telemetry_active: [u8; 2],
    telemetry_calls: [u64; 2],
    telemetry_last: [u64; 2],
    telemetry_samples: [u16; 2],
    telemetry_total: u16,
}

impl Observer {
    pub(super) const fn new() -> Self {
        Self {
            slots: [None; 2],
            fighters: [None; 2],
            status_ticks: [0; 2],
            epoch_logs: 0,
            total_logs: 0,
            dispatch_slots: [None; 2],
            dispatch_calls: [0; 2],
            dispatch_last: [0; 2],
            dispatch_epoch_logs: 0,
            dispatch_total_logs: 0,
            body_keys: [None; 32],
            telemetry: [[None; 3]; 2],
            telemetry_active: [u8::MAX; 2],
            telemetry_calls: [0; 2],
            telemetry_last: [0; 2],
            telemetry_samples: [0; 2],
            telemetry_total: 0,
        }
    }

    pub(super) fn reset_epoch(&mut self) {
        self.slots = [None; 2];
        self.fighters = [None; 2];
        self.status_ticks = [0; 2];
        self.epoch_logs = 0;
        self.dispatch_slots = [None; 2];
        self.dispatch_epoch_logs = 0;
        self.body_keys = [None; 32];
        self.telemetry = [[None; 3]; 2];
        self.telemetry_active = [u8::MAX; 2];
        // Lifetime cap deliberately survives scene changes.
    }

    pub(super) unsafe fn observe(&mut self, accessor: &mut BattleObjectModuleAccessor, epoch: u64) {
        let entry_id = WorkModule::get_int(accessor, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID);
        if !(0..=1).contains(&entry_id) {
            return;
        }
        let symbol = MANAGER_SYMBOL.load(Ordering::Relaxed);
        if symbol == 0 {
            return;
        }
        let manager = std::ptr::read_volatile(symbol as *const *mut app::FighterManager);
        if manager.is_null() {
            return;
        }
        // Lookup afresh on this valid fighter callback; never retain native pointers.
        let entry = FighterManager::get_fighter_entry(manager, app::FighterEntryID(entry_id))
            as *mut app::FighterEntry;
        if entry.is_null() {
            return;
        }
        let raw = entry as *const u8;
        let mut slots = snapshot(raw);
        slots.native_num = FighterEntry::fighter_num(entry);
        self.observe_bodies(raw, slots, entry_id, epoch);
        self.observe_telemetry(raw, slots, entry_id, epoch, accessor);
        if self.epoch_logs >= EPOCH_LIMIT || self.total_logs >= TOTAL_LIMIT {
            return;
        }
        let fighter = Fighter {
            id: accessor.battle_object_id,
            kind: utility::get_kind(accessor),
            status: StatusModule::status_kind(accessor),
        };
        let index = entry_id as usize;
        self.status_ticks[index] = self.status_ticks[index].saturating_add(1);
        let slot_change = self.slots[index] != Some(slots);
        let fighter_change =
            self.fighters[index] != Some(fighter) && self.status_ticks[index] >= STATUS_INTERVAL;
        if !slot_change && !fighter_change {
            return;
        }
        self.slots[index] = Some(slots);
        self.fighters[index] = Some(fighter);
        self.status_ticks[index] = 0;
        self.epoch_logs += 1;
        self.total_logs += 1;
        println!(
            "[SubfighterProbe] epoch={} n={} entry={} role={} callback_id={} kind=0x{:x} status={} configured={} native_num={} kinds={:x?} installed={} bodies=0b{:03b} controls=0b{:05b} active={} pending={} flags=0x{:02x} loader={} outer={}",
            epoch, self.total_logs, entry_id, if entry_id == 0 { "P1" } else { "CPU" },
            fighter.id, fighter.kind, fighter.status, slots.configured, slots.native_num,
            slots.kinds, slots.installed, slots.bodies, slots.controls, slots.active,
            slots.pending, slots.flags, slots.loader, slots.outer,
        );
        log_extra("live", epoch, entry_id, slots);
        if self.epoch_logs == EPOCH_LIMIT || self.total_logs == TOTAL_LIMIT {
            println!(
                "[SubfighterProbe] observation cap reached epoch={} total={}",
                epoch, self.total_logs
            );
        }
    }

    pub(super) unsafe fn dispatch(
        &mut self,
        entry: *const u8,
        epoch: u64,
        phase: &str,
        argument: u64,
    ) {
        if entry.is_null() || !(entry as usize).is_multiple_of(8) {
            return;
        }
        let owner: u32 = field(entry, 0xe0);
        if owner > 1 {
            return;
        }
        let i = owner as usize;
        if phase == "before" {
            self.dispatch_calls[i] = self.dispatch_calls[i].saturating_add(1);
        }
        if self.dispatch_epoch_logs >= 512 || self.dispatch_total_logs >= 1024 {
            return;
        }
        let s = snapshot(entry); // Raw entry reads only, no FighterEntry queries here.
        if self.dispatch_slots[i] == Some(s) {
            return;
        }
        let delta = self.dispatch_calls[i] - self.dispatch_last[i];
        self.dispatch_last[i] = self.dispatch_calls[i];
        self.dispatch_slots[i] = Some(s);
        self.dispatch_epoch_logs += 1;
        self.dispatch_total_logs += 1;
        println!("[SubfighterDispatch] epoch={} entry={} phase={} argument={} calls={} delta_calls={} n={} configured={} kinds={:x?} installed={} bodies={:03b} controls={:05b} active={} pending={} flags=0x{:02x} loader={} outer={} previous={} request={} change={} commit={:?} extra={:?} value41e4=0x{:x} flag41e8={} auxiliary={:02b}", epoch, owner, phase, argument, self.dispatch_calls[i], delta, self.dispatch_total_logs, s.configured, s.kinds, s.installed, s.bodies, s.controls, s.active, s.pending, s.flags, s.loader, s.outer, s.previous, s.request, s.change, s.commit, s.extra, s.value41e4, s.flag41e8, s.auxiliary);
        if self.dispatch_epoch_logs == 512 || self.dispatch_total_logs == 1024 {
            println!(
                "[SubfighterDispatch] cap reached epoch={} total={}",
                epoch, self.dispatch_total_logs
            );
        }
    }

    unsafe fn observe_bodies(&mut self, entry: *const u8, s: Slots, owner: i32, epoch: u64) {
        if s.outer != 6 || !(1..=3).contains(&s.installed) {
            return;
        }
        let base = super::BASE.load(Ordering::Relaxed);
        for i in 0..s.installed as usize {
            // Installed array in outer6 owns constructed bodies. Never use raw out-of-count slots.
            let body: usize = field(entry, 0x4160 + i * 8);
            if body == 0 || !body.is_multiple_of(8) {
                continue;
            }
            let id: u32 = field(body as *const u8, 8);
            let kind = s.kinds[i];
            if self.body_keys.contains(&Some((kind, id))) {
                continue;
            }
            let Some(key) = self.body_keys.iter_mut().find(|key| key.is_none()) else {
                return;
            };
            *key = Some((kind, id));
            let vtable: usize = field(body as *const u8, 0);
            if !allowed_vtable(base, vtable) {
                println!("[SubfighterBody] epoch={} entry={} slot={} kind=0x{:x} id={} rejected=vtable_bounds vptr=0x{:x} main=0x{:x} kind_raw=0x{:x} owner_raw=0x{:x} lifecycle3a={} body_slot3c={}", epoch, owner, i, kind, id, vtable, base, field::<u32>(body as *const u8, 0xc), field::<u64>(body as *const u8, 0x10), field::<u8>(body as *const u8, 0x3a), field::<u8>(body as *const u8, 0x3c));
                continue;
            }
            let activate: usize = field(vtable as *const u8, 0x38);
            let deactivate: usize = field(vtable as *const u8, 0x48);
            if [activate, deactivate]
                .iter()
                .any(|p| p % 4 != 0 || *p < base || *p >= base + 0x39c8440)
            {
                println!("[SubfighterBody] epoch={} entry={} slot={} kind=0x{:x} id={} rejected=text_bounds", epoch, owner, i, kind, id);
                continue;
            }
            println!("[SubfighterBody] epoch={} entry={} slot={} kind=0x{:x} id={} vtable=main+0x{:x} activate38=main+0x{:x} deactivate48=main+0x{:x}", epoch, owner, i, kind, id, vtable-base, activate-base, deactivate-base);
        }
    }

    unsafe fn observe_telemetry(
        &mut self,
        entry: *const u8,
        s: Slots,
        owner: i32,
        epoch: u64,
        callback: &mut BattleObjectModuleAccessor,
    ) {
        let index = owner as usize;
        if s.outer != 6
            || !(1..=3).contains(&s.installed)
            || self.telemetry_samples[index] >= 64
            || self.telemetry_total >= 128
        {
            return;
        }
        self.telemetry_calls[index] = self.telemetry_calls[index].saturating_add(1);
        let base = super::BASE.load(Ordering::Relaxed);
        let mut samples = [None; 3];
        let mut geometry = [None; 3];
        for i in 0..s.installed as usize {
            let body: usize = field(entry, 0x4160 + i * 8);
            if body == 0 || !body.is_multiple_of(8) {
                continue;
            }
            let raw = body as *const u8;
            let vptr: usize = field(raw, 0);
            if !allowed_vtable(base, vptr) {
                continue;
            }
            let activate: usize = field(vptr as *const u8, 0x38);
            let deactivate: usize = field(vptr as *const u8, 0x48);
            if [activate, deactivate]
                .iter()
                .any(|p| !p.is_multiple_of(4) || *p < base || *p >= base + 0x39c8440)
            {
                continue;
            }
            // Pinned BattleObject: +08 ID,+0c kind,+10 owner,+20 module accessor.
            let id: u32 = field(raw, 8);
            let kind: u32 = field(raw, 0xc);
            if field::<u64>(raw, 0x10) != owner as u64 || kind != s.kinds[i] {
                continue;
            }
            let module: *mut BattleObjectModuleAccessor = field(raw, 0x20);
            let mut sample = BodySample {
                id,
                kind,
                lifecycle: field(raw, 0x3a),
                slot: field(raw, 0x3c),
                live: None,
            };
            // Only the actual current callback establishes fully live getter safety.
            // Installed inactive bodies may have reset/deactivated modules: no getters.
            if !module.is_null()
                && (module as usize).is_multiple_of(8)
                && std::ptr::eq(module, callback)
                && callback.battle_object_id == id
                && utility::get_kind(callback) == kind as i32
                && WorkModule::get_int(callback, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID) == owner
            {
                sample.live = Some((
                    StatusModule::status_kind(module),
                    MotionModule::motion_kind(module),
                    VisibilityModule::is_visible(module),
                ));
                geometry[i] = Some((
                    MotionModule::frame(module),
                    PostureModule::pos_x(module),
                    PostureModule::pos_y(module),
                    PostureModule::pos_z(module),
                ));
            }
            samples[i] = Some(sample);
        }
        let changed = self.telemetry[index] != samples || self.telemetry_active[index] != s.active;
        let delta = self.telemetry_calls[index] - self.telemetry_last[index];
        if !changed && delta < 60 {
            return;
        }
        self.telemetry[index] = samples;
        self.telemetry_active[index] = s.active;
        self.telemetry_last[index] = self.telemetry_calls[index];
        self.telemetry_samples[index] += 1;
        self.telemetry_total += 1;
        for i in 0..s.installed as usize {
            if let Some(b) = samples[i] {
                println!("[SubfighterTelemetry] epoch={} entry={} sample={} total={} valid_calls={} active={} slot={} id={} kind=0x{:x} lifecycle3a={} body_slot3c={} live_status_motion_visible={:?} live_frame_xyz={:?} getters={}", epoch, owner, self.telemetry_samples[index], self.telemetry_total, self.telemetry_calls[index], s.active, i, b.id, b.kind, b.lifecycle, b.slot, b.live, geometry[i], if b.live.is_some() { "callback_validated" } else { "omitted_inactive_lifetime_unproven" });
            } else {
                println!("[SubfighterTelemetry] epoch={} entry={} sample={} active={} slot={} rejected=metadata_validation", epoch, owner, self.telemetry_samples[index], s.active, i);
            }
        }
        if self.telemetry_samples[index] == 64 || self.telemetry_total == 128 {
            println!(
                "[SubfighterTelemetry] cap reached entry={} samples={} lifetime={}",
                owner, self.telemetry_samples[index], self.telemetry_total
            );
        }
    }
}

fn allowed_vtable(base: usize, vptr: usize) -> bool {
    vptr.is_multiple_of(8)
        && (vptr == base + 0x4f80210
            || vptr == base + 0x51863b8
            || (base + 0x39c9000..=base + 0x4f16f60 - 0x50).contains(&vptr))
}

unsafe fn snapshot(raw: *const u8) -> Slots {
    Slots {
        configured: field(raw, 0x14),
        kinds: [field(raw, 0x18), field(raw, 0x1c), field(raw, 0x20)],
        installed: field(raw, 0x4150),
        bodies: presence(raw, &[0x4160, 0x4168, 0x4170]),
        controls: presence(raw, &[0x4178, 0x4180, 0x4188, 0x4190, 0x4198]),
        active: field(raw, 0x5918),
        pending: field(raw, 0x5919),
        flags: field(raw, 0x591f),
        loader: field(raw, 0x591c),
        outer: field(raw, 0x5920),
        native_num: 0,
        previous: field(raw, 0x5910),
        request: field(raw, 0x591a),
        change: field(raw, 0x591b),
        commit: [
            field(raw, 0x5931),
            field(raw, 0x5932),
            field(raw, 0x5933),
            field(raw, 0x5934),
        ],
        extra: [field(raw, 0x5ab9), field(raw, 0x5aba)],
        value41e4: field(raw, 0x41e4),
        flag41e8: field(raw, 0x41e8),
        auxiliary: presence(raw, &[0x5a88, 0x5a90]),
    }
}

fn log_extra(source: &str, epoch: u64, entry: i32, s: Slots) {
    println!("[SubfighterExtra] source={} epoch={} entry={} previous={} request={} change={} commit={:?} extra={:?} value41e4=0x{:x} flag41e8={} auxiliary={:02b}", source, epoch, entry, s.previous, s.request, s.change, s.commit, s.extra, s.value41e4, s.flag41e8, s.auxiliary);
}

pub(super) unsafe fn dispatcher_supported(base: usize) -> bool {
    std::slice::from_raw_parts((base + 0x6556b0) as *const u32, DISPATCH_CODE.len())
        == DISPATCH_CODE
}

unsafe fn field<T: Copy>(entry: *const u8, offset: usize) -> T {
    std::ptr::read_volatile(entry.add(offset) as *const T)
}

unsafe fn presence(entry: *const u8, offsets: &[usize]) -> u8 {
    offsets.iter().enumerate().fold(0, |mask, (index, offset)| {
        mask | (u8::from(field::<usize>(entry, *offset) != 0) << index)
    })
}

pub(super) unsafe fn init() {
    let mut symbol = 0;
    LookupSymbol(
        &mut symbol,
        b"_ZN3lib9SingletonIN3app14FighterManagerEE9instance_E\0".as_ptr(),
    );
    MANAGER_SYMBOL.store(symbol, Ordering::Relaxed);
    println!("[SubfighterProbe] manager_lookup={} epoch_cap={} total_cap={} status_interval_callbacks={}", symbol != 0, EPOCH_LIMIT, TOTAL_LIMIT, STATUS_INTERVAL);
}
