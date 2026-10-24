//! Bounded development probe: reload preallocated inactive Training entry2 with
//! native non-activating flags. Never change CPU quantity or manually free bodies.
use super::{aligned, finish_install, read, ready_forms, Forms, InstallToken, BASE, INSTALL};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use smash::app::{self, lua_bind::*, sv_battle_object};
use smash::lib::lua_const::*;

const OWNER: u32 = 2;
const KIND: u32 = 0x0d; // Peach, matching the native fighter-kind roster.
const REPLACEMENT: u32 = 0x02; // Link; copy-only lifetime experiment, no item.
static BODY: AtomicU32 = AtomicU32::new(u32::MAX);
static LAST_COUNT: AtomicU32 = AtomicU32::new(u32::MAX);
static LAST_RATIO: AtomicU32 = AtomicU32::new(u32::MAX);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Resources { owner: usize, files: [u32; 3], directories: [u32; 3] }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PeachParameters {
    registry: usize, users: u32, typed: usize, control: usize,
    turnip: usize, powers: [u32; 4],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Consumer {
    body: u32, slot: i32, model: [usize; 2], motion: [usize; 2], typed: [usize; 2],
}
struct Probe {
    install: Option<InstallToken>, departed: bool,
    entry: usize, started: bool, finished: bool, ticks: u16, phase: u32,
    resources: Option<Resources>, quiet_ticks: u16,
    parameters: Option<PeachParameters>,
    kind: u32, replace_requested: bool,
    consumer: Option<Consumer>,
}
impl Probe {
    const fn new() -> Self {
        Self { install: None, departed: false,
            entry: 0, started: false, finished: false, ticks: 0, phase: u32::MAX,
            resources: None, parameters: None, quiet_ticks: 0,
            kind: KIND, replace_requested: false, consumer: None }
    }
}
static STATE: Mutex<Probe> = Mutex::new(Probe::new());

// Development-only numeric observations. No live accessor or AttackData pointer
// is retained, and no attack/copy/damage value is modified by this diagnostic.
struct CopyDiagnostic {
    status: [i32; 2],
    attacks: [Option<(u32, i32, [u32; 8])>; 32],
    next: usize,
    remaining: u32,
}
static COPY_DIAGNOSTIC: Mutex<CopyDiagnostic> = Mutex::new(CopyDiagnostic {
    status: [-1; 2], attacks: [None; 32], next: 0, remaining: 96,
});
static DAMAGE_LOGS: AtomicU32 = AtomicU32::new(0);

pub unsafe fn observe_copy(body: *mut app::BattleObjectModuleAccessor) {
    if body.is_null() || !crate::common::is_native_training_mode() { return; }
    let owner = WorkModule::get_int(body, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID);
    if !(0..2).contains(&owner) { return; }
    let kind = app::utility::get_kind(&mut *body);
    if kind != *FIGHTER_KIND_KIRBY && kind != *FIGHTER_KIND_PEACH { return; }
    let mut diag = COPY_DIAGNOSTIC.lock().unwrap();
    if diag.remaining == 0 { return; }
    let status = StatusModule::status_kind(body);
    if diag.status[owner as usize] != status {
        diag.status[owner as usize] = status;
        diag.remaining -= 1;
        crate::logging::diagnostic(format_args!(
            "[CopyDamageProbe] owner={} kind={} status=0x{:x} damage={} lock={} copy={}:{} slot={}",
            owner, kind, status, DamageModule::damage(body, 0), DamageModule::is_damage_lock(body),
            kind == *FIGHTER_KIND_KIRBY && WorkModule::is_flag(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_FLAG_COPY),
            if kind == *FIGHTER_KIND_KIRBY { WorkModule::get_int(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_INT_COPY_CHARA) } else { -1 },
            if kind == *FIGHTER_KIND_KIRBY { WorkModule::get_int(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_INT_COPY_CHARA_SLOT_NO) } else { -1 }));
    }
    let counter = if kind == *FIGHTER_KIND_KIRBY {
        matches!(status, value if value == *FIGHTER_KIRBY_STATUS_KIND_PEACH_SPECIAL_N
            || value == *FIGHTER_KIRBY_STATUS_KIND_PEACH_SPECIAL_N_HIT)
    } else {
        status == *FIGHTER_STATUS_KIND_SPECIAL_N || status == *FIGHTER_PEACH_STATUS_KIND_SPECIAL_N_HIT
    };
    if !counter { return; }
    // Kirby's native copied articles occupy the 24 documented reserve slots.
    // Native Peach has its own small article table. Inspect only active entries.
    let slots = if kind == *FIGHTER_KIND_KIRBY { 24 } else { 3 };
    for slot in 0..slots {
        if ArticleModule::get_active_num(body, slot) <= 0 { continue; }
        let article = ArticleModule::get_article(body, slot);
        if article.is_null() { continue; }
        let raw_id = Article::get_battle_object_id(article);
        if raw_id > u64::from(u32::MAX) { continue; }
        let id = raw_id as u32;
        if !sv_battle_object::is_active(id) { continue; }
        let weapon = sv_battle_object::module_accessor(id);
        if weapon.is_null() || app::utility::get_category(&mut *weapon) != *BATTLE_OBJECT_CATEGORY_WEAPON
            || app::utility::get_kind(&mut *weapon) != *WEAPON_KIND_PEACH_KINOPIOSPORE { continue; }
        for attack in 0..8 {
            if diag.remaining == 0 { return; }
            if !AttackModule::is_attack(weapon, attack, false) { continue; }
            let data = AttackModule::attack_data(weapon, attack, false);
            if data.is_null() { continue; }
            let values = [(*data).power, AttackModule::power_mul_status(weapon),
                AttackModule::power_add_status(weapon), AttackModule::power_mul(weapon),
                AttackModule::power_mul_2nd(weapon), AttackModule::power_mul_3rd(weapon),
                AttackModule::power_mul_4th(weapon), AttackModule::power_mul_5th(weapon)];
            let observation = (id, attack, values.map(f32::to_bits));
            if diag.attacks.contains(&Some(observation)) { continue; }
            let next = diag.next;
            diag.attacks[next] = Some(observation);
            diag.next = (next + 1) % diag.attacks.len();
            diag.remaining -= 1;
            crate::logging::diagnostic(format_args!(
                "[CopyDamageProbe] spore owner={} article_slot={} id=0x{:x} attack={} power/status_mul/status_add/muls={:?}",
                owner, slot, id, attack, values));
        }
    }
}

pub unsafe fn observe_damage(body: *mut app::BattleObjectModuleAccessor,
    requested: f32, before: f32) {
    if DAMAGE_LOGS.fetch_add(1, Ordering::Relaxed) >= 48 { return; }
    crate::logging::diagnostic(format_args!(
        "[CopyDamageProbe] victim id=0x{:x} kind={} requested={} before={} after={} lock={}",
        (*body).battle_object_id, app::utility::get_kind(&mut *body), requested,
        before, DamageModule::damage(body, 0), DamageModule::is_damage_lock(body)));
}

pub fn scene_changed() {
    *COPY_DIAGNOSTIC.lock().unwrap() = CopyDiagnostic {
        status: [-1; 2], attacks: [None; 32], next: 0, remaining: 96,
    };
    DAMAGE_LOGS.store(0, Ordering::Relaxed);
    BODY.store(u32::MAX, Ordering::Release);
    LAST_COUNT.store(u32::MAX, Ordering::Relaxed);
    LAST_RATIO.store(u32::MAX, Ordering::Relaxed);
    let mut state = STATE.lock().unwrap();
    if state.install.is_some() {
        state.departed = true;
        state.replace_requested = false;
        state.resources = None;
        state.parameters = None;
    } else {
        *state = Probe::new();
    }
}

/// Read-only rule observation, before and after native CPU-count changes.
/// The pinned binding declares the ratio as u64, but the native export returns
/// an f32 in s0 (ldr x8,[x0]; ldr s0,[x8,#0xf0]; ret). Use its actual ABI.
pub unsafe fn observe_rules() {
    if !crate::common::is_native_training_mode() || !crate::common::is_ready_go() {
        return;
    }
    let symbol = super::setting(&crate::common::FIGHTER_MANAGER_ADDR);
    if symbol == 0 { return; }
    let manager = std::ptr::read_volatile(symbol as *const *mut app::FighterManager);
    if !aligned(manager.cast::<u8>()) { return; }
    let ratio_fn: unsafe extern "C" fn(*mut app::FighterManager) -> f32 =
        std::mem::transmute(BASE.load(Ordering::Acquire) + 0x21410d0);
    let ratio = ratio_fn(manager);
    let count = FighterManager::entry_count(manager) as u32;
    let previous_count = LAST_COUNT.swap(count, Ordering::Relaxed);
    let previous_ratio = LAST_RATIO.swap(ratio.to_bits(), Ordering::Relaxed);
    if previous_count != count || previous_ratio != ratio.to_bits() {
        crate::logging::diagnostic(format_args!(
            "[SourceOwnerProbe] rules entries={} one_on_one_ratio={}", count, ratio));
    }
}

pub unsafe fn destroy(entry: *const u8) {
    let mut state = STATE.lock().unwrap();
    if state.entry == entry as usize {
        crate::logging::diagnostic(format_args!("[SourceOwnerProbe] native destroy owner=2"));
        if let Some(install) = state.install.take() { finish_install(install, "source destroyed"); }
        BODY.store(u32::MAX, Ordering::Release);
        *state = Probe::new();
    }
}

/// This feature reserves only native Training entry2, including its loading body.
pub unsafe fn reserved(body: &mut app::BattleObjectModuleAccessor) -> bool {
    super::sources::legacy_training_owner_reserved(body)
}

pub unsafe fn source(kind: u32) -> Option<*mut app::BattleObjectModuleAccessor> {
    if kind != KIND || !crate::common::is_native_training_mode() { return None; }
    let entry = current_entry(OWNER)?;
    if ready_forms(entry, OWNER) != Some((Forms::starting(KIND), KIND)) { return None; }
    let id = BODY.load(Ordering::Acquire);
    if id == u32::MAX || !sv_battle_object::is_active(id) { return None; }
    let body = sv_battle_object::module_accessor(id);
    if body.is_null() || !reserved(&mut *body)
        || app::utility::get_kind(&mut *body) as u32 != KIND { return None; }
    Some(body)
}

/// Read the current native resource table, never a cached owner pointer.
/// The file lookup writes a u32 file index; directory lookup is read-only.
unsafe fn resources(expected_kind: u32) -> Option<Resources> {
    let base = BASE.load(Ordering::Acquire);
    let context = std::ptr::read_volatile((base + 0x5324680) as *const *const u8);
    if !aligned(context) {
        crate::logging::diagnostic(format_args!("[SourceOwnerProbe] resource context invalid={:p}", context));
        return None;
    }
    let owner = read::<*mut u8>(context, 0xe8 + OWNER as usize * 8);
    if !aligned(owner) {
        crate::logging::diagnostic(format_args!("[SourceOwnerProbe] resource owner invalid={:p}", owner));
        return None;
    }
    let kind = read::<u32>(owner, 0x58);
    let ready: unsafe extern "C" fn(*mut u8) -> bool =
        std::mem::transmute(base + 0x17e2880);
    let loaded = ready(owner);
    crate::logging::diagnostic(format_args!(
        "[SourceOwnerProbe] resource owner={:p} kind=0x{:x} costume={} ready={}",
        owner, kind, read::<u32>(owner, 0x64), loaded));
    if kind != expected_kind || !loaded { return None; }
    let lookup: unsafe extern "C" fn(*mut u32, *mut u8, u32, u32) =
        std::mem::transmute(base + 0x17e8890);
    let directory: unsafe extern "C" fn(u32) -> u32 =
        std::mem::transmute(base + 0x353ea90);
    let mut result = Resources { owner: owner as usize,
        files: [0xffffff; 3], directories: [0xffffff; 3] };
    for index in 0..3 {
        lookup(&mut result.files[index], owner, expected_kind, 0x20 + index as u32);
        if result.files[index] != 0xffffff {
            result.directories[index] = directory(result.files[index]);
        }
    }
    crate::logging::diagnostic(format_args!("[SourceOwnerProbe] resource lookup={:?}", result));
    // Native copy_setup permits each optional copy-parameter file to be absent,
    // including all three. Peach's active owner is ready with all three absent.
    // Keep the owner/entry lifetime checks and reject an existing unloaded file.
    let valid = result.files.iter().zip(result.directories).all(|(file, directory)|
            *file == 0xffffff || directory != 0xffffff);
    valid.then_some(result)
}

/// Observe the exact native Peach parameter chain used by its turnip getters.
/// Compare values and identities from a freshly reacquired registry; never read
/// a saved pointer or manufacture a parameter object/count.
unsafe fn peach_parameters() -> Option<PeachParameters> {
    let registry = std::ptr::read_volatile(
        (BASE.load(Ordering::Acquire) + 0x52bc3b0) as *const *const u8);
    if !aligned(registry) { return None; }
    let record = registry.add(0x60 + KIND as usize * 0x38);
    let users = read::<u32>(record, 0);
    let typed = read::<*const u8>(record, 0x10);
    let control = read::<*const u8>(record, 0x18);
    if users == 0 || !aligned(typed) || !aligned(control) { return None; }
    let turnip = read::<*const u8>(typed, 0x158);
    if !aligned(turnip) { return None; }
    let mut powers = [0; 4];
    for (index, offset) in [0x20, 0x34, 0x38, 0x3c].iter().copied().enumerate() {
        let power = read::<f32>(turnip, offset);
        if !power.is_finite() || power <= 0.0 { return None; }
        powers[index] = power.to_bits();
    }
    Some(PeachParameters { registry: registry as usize, users,
        typed: typed as usize, control: control as usize, turnip: turnip as usize, powers })
}

/// Bounded inactive-owner item experiment, Peach only. Active-source behavior
/// is unchanged; the inactive path also requires the original parsed parameters.
pub unsafe fn item_source(kind: u32) -> bool {
    if source(kind).is_some() { return true; }
    if source_slot(kind).is_none() { return false; }
    let expected = STATE.lock().unwrap().parameters;
    let current = peach_parameters();
    crate::logging::diagnostic(format_args!(
        "[SourceOwnerProbe] inactive item parameters={:?} same={}",
        current, expected.is_some() && current == expected));
    expected.is_some() && current == expected
}

/// Development-only test of native non-activating loading.
/// Native Kirby copy setup uses the resource slot, not the source body accessor.
/// Require the same live entry and loaded resources observed on the ready body.
pub unsafe fn source_slot(kind: u32) -> Option<i32> {
    if source(kind).is_some() {
        let current = resources(KIND);
        if current.is_some() { STATE.lock().unwrap().resources = current; }
        return Some(OWNER as i32);
    }
    if kind != KIND || !crate::common::is_native_training_mode() { return None; }
    let (identity, expected) = {
        let state = STATE.lock().unwrap();
        if !state.finished || state.kind != KIND { return None; }
        (state.entry, state.resources?)
    };
    let entry = current_entry(OWNER)?;
    crate::logging::diagnostic(format_args!(
        "[SourceOwnerProbe] requested entry same={} outer={} loader={} guard={} installed={} kind=0x{:x}",
        entry as usize == identity, read::<u8>(entry, 0x5920), read::<u8>(entry, 0x591c),
        read::<u8>(entry, 0x5935), read::<u32>(entry, 0x4150), read::<u32>(entry, 0x18)));
    if entry as usize != identity || !inactive_body_ready(entry, KIND) { return None; }
    let current = resources(KIND);
    crate::logging::diagnostic(format_args!(
        "[SourceOwnerProbe] inactive source slot={} outer={} loader={} installed={} resources={:?} same={}",
        OWNER, read::<u8>(entry, 0x5920), read::<u8>(entry, 0x591c),
        read::<u32>(entry, 0x4150), current, current == Some(expected)));
    (current == Some(expected)).then_some(OWNER as i32)
}

unsafe fn current_entry(owner: u32) -> Option<*const u8> {
    let symbol = super::setting(&crate::common::FIGHTER_MANAGER_ADDR);
    if symbol == 0 { return None; }
    let manager = std::ptr::read_volatile(symbol as *const *mut app::FighterManager);
    if !aligned(manager.cast::<u8>()) { return None; }
    let entry_fn: unsafe extern "C" fn(*mut app::FighterManager, u32) -> *const u8 =
        std::mem::transmute(BASE.load(Ordering::Acquire) + 0x2140f90);
    let entry = entry_fn(manager, owner);
    (aligned(entry) && read::<u32>(entry, 0xe0) == owner).then_some(entry)
}

/// Read the actual native Training menu quantity; allocated entry count is not
/// the CPU quantity. Chain verified in native155c020, no writes or UI callbacks.
unsafe fn cpu_quantity() -> Option<u32> {
    let root = std::ptr::read_volatile(
        (BASE.load(Ordering::Acquire) + 0x52c3420) as *const *const u8);
    let menu = super::pointer_chain(root, &[8, 0, 0xb38, 0, 0x210, 8, 0x80])?;
    Some(read(menu, 0xb40))
}

/// Separate probe predicate: native quantity removal leaves inactive0; native
/// non-activating reload leaves inactive1. Both retain installed bodies. The
/// loaded resource and typed guards remain mandatory; production still needs6.
unsafe fn inactive_body_ready(entry: *const u8, kind: u32) -> bool {
    if !aligned(entry) || read::<u32>(entry, 0xe0) != OWNER
        || read::<u32>(entry, 0x14) != 1 || read::<u32>(entry, 0x18) != kind
        || read::<u32>(entry, 0x4150) != 1 || !matches!(read::<u8>(entry, 0x5920), 0 | 1)
        || read::<u8>(entry, 0x591c) != 14 || read::<u8>(entry, 0x5935) != 0
    { return false; }
    let body = read::<*const u8>(entry, 0x4160);
    aligned(body) && read::<usize>(body, 0) == BASE.load(Ordering::Acquire) + 0x4f80210
        && read::<u32>(body, 0xc) == kind && read::<u64>(body, 0x10) == u64::from(OWNER)
        && aligned(read::<*const u8>(body, 0x20))
}

/// Opt-in copy-only experiment: a second P1 shortcut after equipping Peach
/// queues Link on the SAME inactive helper. Test must never create an item.
/// No native copy flags or consumer reference counts are modified here.
pub unsafe fn request_replacement(body: *mut app::BattleObjectModuleAccessor) -> bool {
    if !crate::common::is_native_training_mode()
        || WorkModule::get_int(body, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID) != 0
        || !WorkModule::is_flag(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_FLAG_COPY)
        || WorkModule::is_flag(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_FLAG_COPY_ON_START)
        || WorkModule::get_int(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_INT_COPY_CHARA) != KIND as i32
        || ItemModule::is_have_item(body, 0)
    { return false; }
    let mut state = STATE.lock().unwrap();
    if !state.finished || state.kind != KIND || state.replace_requested { return false; }
    state.consumer = consumer_snapshot();
    if state.consumer.is_none() { return false; }
    state.replace_requested = true;
    crate::logging::diagnostic(format_args!(
        "[SourceOwnerProbe] replacement queued: Peach copy only; no live item allowed; consumer={:?}",
        state.consumer));
    true
}

/// Read-only, fresh consumer chains verified from the native copy-module
/// constructor and copy_setup→ba4090. Saved numeric pairs are compared only.
unsafe fn consumer_snapshot() -> Option<Consumer> {
    let body = crate::common::try_get_module_accessor(crate::common::consts::FighterId::Player)?;
    if app::utility::get_kind(&mut *body) != *FIGHTER_KIND_KIRBY
        || !WorkModule::is_flag(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_FLAG_COPY)
        || WorkModule::get_int(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_INT_COPY_CHARA) != KIND as i32
    { return None; }
    let raw = body.cast::<u8>();
    let model = super::pointer_chain(raw, &[0xf2c8]);
    let motion = super::pointer_chain(raw, &[0x88]);
    let work = super::pointer_chain(raw, &[0xf1c0]);
    crate::logging::diagnostic(format_args!(
        "[SourceOwnerProbe] consumer roots model={:?} motion={:?} work={:?}", model, motion, work));
    let (Some(model), Some(motion), Some(work)) = (model, motion, work) else { return None; };
    let work_kind = read::<u32>(work, 0x40);
    let base = BASE.load(Ordering::Acquire);
    let model_vtable = read::<usize>(model, 0);
    let motion_vtable = read::<usize>(motion, 0);
    crate::logging::diagnostic(format_args!(
        "[SourceOwnerProbe] consumer types work={} model_vtable=0x{:x} motion_vtable=0x{:x}",
        work_kind, model_vtable.wrapping_sub(base), motion_vtable.wrapping_sub(base)));
    if work_kind != 6 || model_vtable != base + 0x4f88d00
        || motion_vtable != base + 0x4f89198 { return None; }
    let typed = super::pointer_chain(work, &[0x38, 8]);
    crate::logging::diagnostic(format_args!("[SourceOwnerProbe] consumer typed={:?}", typed));
    let typed = typed?;
    let result = Consumer {
        body: (*body).battle_object_id,
        slot: WorkModule::get_int(body, *FIGHTER_KIRBY_INSTANCE_WORK_ID_INT_COPY_CHARA_SLOT_NO),
        model: [read(model, 0x10), read(model, 0x18)],
        motion: [read(motion, 0x158), read(motion, 0x160)],
        typed: [read(typed, 0x1b50), read(typed, 0x1b58)],
    };
    // Optional pairs may legitimately be null. We only compare their numeric
    // values; neither a nonnull pair nor equality certifies a working move.
    crate::logging::diagnostic(format_args!("[SourceOwnerProbe] consumer pairs={:?}", result));
    Some(result)
}

unsafe fn install(entry: *mut u8, kind: u32) {
    let target = Forms::starting(kind);
    let from = read::<u32>(entry, 0x18);
    let mut descriptor = [0u64; 14];
    std::ptr::copy_nonoverlapping(entry, descriptor.as_mut_ptr().cast::<u8>(), 0x70);
    let raw = descriptor.as_mut_ptr().cast::<u8>();
    std::ptr::write(raw.add(0x14).cast::<u32>(), target.count);
    for (index, kind) in target.kinds.iter().copied().enumerate() {
        std::ptr::write(raw.add(0x18 + index * 4).cast::<u32>(), kind);
    }
    std::ptr::write(raw.add(0x28).cast::<u32>(), target.actor_count);
    std::ptr::write(raw.add(0x43), 0);
    crate::logging::diagnostic(format_args!(
        "[SourceOwnerProbe] inactive install owner=2 from=0x{:x} to=0x{:x} quantity={:?} installed={} flags=0,0,0",
        from, kind, cpu_quantity(), read::<u32>(entry, 0x4150)));
    let install: unsafe extern "C" fn(*mut u8, *const u64, *const u8) =
        std::mem::transmute(BASE.load(Ordering::Acquire) + 0x653240);
    install(entry, descriptor.as_ptr(), [0u8, 0, 0].as_ptr());
}

/// Invoked exclusively by entry2's current native callback, never a saved pointer.
pub unsafe fn service(entry: *mut u8) {
    if !aligned(entry) { return; }
    let mut state = STATE.lock().unwrap();
    if state.entry != 0 && state.entry != entry as usize { return; }
    if state.departed {
        if inactive_body_ready(entry, state.kind) {
            if let Some(install) = state.install.take() {
                finish_install(install, "departed source completed");
                crate::logging::diagnostic(format_args!(
                    "[SourceOwnerProbe] departed install completed; source remains unavailable"));
            }
        }
        return;
    }
    if !crate::common::is_native_training_mode() { return; }
    if state.finished {
        if state.replace_requested && state.kind == KIND
            && crate::common::is_ready_go() && !super::menu::captures_input()
            && !super::lobby::native_ui_busy() && !super::training::save_states::is_loading()
            && !super::training::recording_or_playback() && !crate::replay_preview::occupied()
            && cpu_quantity() == Some(1) && inactive_body_ready(entry, KIND)
        {
            let Some(install_token) = INSTALL.try_begin(false) else { return; };
            state.install = Some(install_token);
            state.finished = false;
            state.kind = REPLACEMENT;
            state.ticks = 0;
            state.phase = u32::MAX;
            state.quiet_ticks = 0;
            state.resources = None;
            state.parameters = None;
            BODY.store(u32::MAX, Ordering::Release);
            drop(state);
            install(entry, REPLACEMENT);
            return;
        }
        let phase = u32::from(read::<u8>(entry, 0x5920))
            | (u32::from(read::<u8>(entry, 0x591c)) << 8)
            | (u32::from(read::<u8>(entry, 0x5935)) << 16);
        if phase != state.phase {
            state.phase = phase;
            crate::logging::diagnostic(format_args!(
                "[SourceOwnerProbe] retained phase=0x{:x} quantity={:?} body_ready={}",
                phase, cpu_quantity(), inactive_body_ready(entry, state.kind)));
        }
        if state.quiet_ticks < 300 {
            state.quiet_ticks += 1;
            if state.quiet_ticks == 300 {
                crate::logging::diagnostic(format_args!(
                    "[SourceOwnerProbe] quiet outer={} loader={} guard={} quantity={:?} body_ready={}",
                    read::<u8>(entry, 0x5920), read::<u8>(entry, 0x591c),
                    read::<u8>(entry, 0x5935), cpu_quantity(), inactive_body_ready(entry, state.kind)));
                if state.kind == REPLACEMENT {
                    let current = consumer_snapshot();
                    crate::logging::diagnostic(format_args!(
                        "[SourceOwnerProbe] replacement quiet consumer={:?} same={} Peach_parameters={:?}",
                        current, current.is_some() && current == state.consumer, peach_parameters()));
                }
            }
        }
        return;
    }
    if state.started {
        state.ticks = state.ticks.saturating_add(1);
        let phase = u32::from(read::<u8>(entry, 0x5920))
            | (u32::from(read::<u8>(entry, 0x591c)) << 8)
            | (u32::from(read::<u8>(entry, 0x5935)) << 16);
        if phase != state.phase {
            state.phase = phase;
            crate::logging::diagnostic(format_args!(
                "[SourceOwnerProbe] phase=0x{:x} tick={}", phase, state.ticks));
        }
        if inactive_body_ready(entry, state.kind) {
            state.resources = resources(state.kind);
            state.parameters = peach_parameters();
            if state.resources.is_none() || (state.kind == KIND && state.parameters.is_none()) {
                if state.ticks < 600 { return; }
                state.finished = true;
                if let Some(install) = state.install.take() { finish_install(install, "source native completion without plugin readiness"); }
                crate::logging::diagnostic(format_args!(
                    "[SourceOwnerProbe] timeout: inactive body without resource/typed readiness"));
                return;
            }
            let body = read::<*const u8>(entry, 0x4160);
            let id = read::<u32>(body, 8);
            BODY.store(id, Ordering::Release);
            crate::logging::diagnostic(format_args!(
                "[SourceOwnerProbe] ready resources={:?}", state.resources));
            crate::logging::diagnostic(format_args!(
                "[SourceOwnerProbe] ready parameters={:?}", state.parameters));
            state.finished = true;
            if let Some(install) = state.install.take() { finish_install(install, "source ready"); }
            crate::logging::diagnostic(format_args!(
                "[SourceOwnerProbe] inactive ready owner=2 kind=0x{:x} id=0x{:x} ticks={} body_state={} quantity={:?}",
                state.kind, id, state.ticks, read::<u8>(body, 0x3a), cpu_quantity()));
            if state.kind == REPLACEMENT {
                let current = consumer_snapshot();
                crate::logging::diagnostic(format_args!(
                    "[SourceOwnerProbe] replacement ready consumer={:?} same={} Peach_parameters={:?}",
                    current, current.is_some() && current == state.consumer, state.parameters));
            }
        } else if state.ticks == 600 {
            crate::logging::diagnostic(format_args!(
                "[SourceOwnerProbe] timeout; install ownership retained until native completion/destruction"));
        }
        return;
    }
    if !crate::common::is_ready_go() || super::menu::captures_input()
        || super::lobby::native_ui_busy() || super::training::save_states::is_loading()
        || super::training::recording_or_playback() || crate::replay_preview::occupied()
        || read::<u8>(entry, 0x5920) != 0 || read::<u8>(entry, 0x5935) != 0
        || cpu_quantity() != Some(1)
        || current_entry(0).and_then(|entry| ready_forms(entry, 0)).is_none()
        || current_entry(1).and_then(|entry| ready_forms(entry, 1)).is_none()
    { return; }
    let Some(install_token) = INSTALL.try_begin(false) else { return; };
    state.install = Some(install_token);
    state.entry = entry as usize;
    state.started = true;
    drop(state);
    install(entry, KIND);
}
