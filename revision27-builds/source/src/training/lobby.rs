//! Waiting-room practice support. Never change the game's own mode predicate.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use training_mod_consts::{Action, FighterId, LedgeOption, OnOff, MENU};
use training_mod_sync::read;

use crate::common::game_mode;

macro_rules! trace {
    ($($arg:tt)*) => { crate::logging::diagnostic(format_args!($($arg)*)) };
}

static NATIVE_UI_BUSY: AtomicBool = AtomicBool::new(false);
static UI_READY: AtomicBool = AtomicBool::new(false);
static WAS_ACTIVE: AtomicBool = AtomicBool::new(false);
static SEEN_ENTRIES: AtomicU32 = AtomicU32::new(0);
static LAST_MODE: AtomicU64 = AtomicU64::new(u64::MAX);
static CPU_OPTIONS_DIRTY: AtomicBool = AtomicBool::new(true);

pub fn native_ui_busy() -> bool {
    NATIVE_UI_BUSY.load(Ordering::Acquire)
}

/// Native pause/dialog ownership takes priority over an open editor or delayed input.
pub fn set_native_ui_busy(busy: bool) {
    if NATIVE_UI_BUSY.swap(busy, Ordering::AcqRel) != busy {
        if busy {
            crate::common::menu::clear_session_state();
            crate::common::button_config::clear_session_state();
            super::input_delay::clear_session_state();
        }
        trace!("[LobbyPractice] native_ui_busy={busy}");
    }
}

pub fn settings_changed() {
    CPU_OPTIONS_DIRTY.store(true, Ordering::Release);
}

/// Run only from a valid waiting dummy's simulation callback, never teardown/UI.
pub fn prepare_cpu_options() {
    if active() && CPU_OPTIONS_DIRTY.swap(false, Ordering::AcqRel) {
        super::sdi::roll_direction();
        super::shield_tilt::roll_direction();
    }
}

pub fn init() {
    trace!("[LobbyPractice] verified_13_0_5={}", game_mode::init());
}

pub fn is_waiting_scene() -> bool {
    // Shared PracticeScene setup (13.0.5, 0x1790020) explicitly selects melee 0x0d.
    // The normal 0x0701xxxx match mapper never returns that value. Keep the proven
    // local family exact; online activation also needs the actual waiting UI graft.
    // LDN is runtime-tested; the online family is supported by static call paths.
    matches!(game_mode::current(), Some(mode)
        if mode.initialized && mode.melee == 0x0d
            && (mode.mode == 0x02010000 || mode.mode & 0x7fff0000 == 0x07010000))
}

pub fn requested() -> bool {
    read(&MENU).lobby_practice == OnOff::ON && is_waiting_scene()
}

pub fn active() -> bool {
    requested() && UI_READY.load(Ordering::Acquire)
}

pub fn set_ui_ready(ready: bool) {
    UI_READY.store(ready, Ordering::Release);
}

pub fn entry_id(role: FighterId) -> i32 {
    if role == FighterId::CPU && is_waiting_scene() {
        4
    } else {
        role as i32
    }
}

pub fn permits_entry(entry: i32) -> bool {
    !is_waiting_scene() || matches!(entry, 0 | 4)
}

pub fn available_actions(mut actions: Action) -> Action {
    if is_waiting_scene() {
        actions.PLAYBACK_1 = 0;
        actions.PLAYBACK_2 = 0;
        actions.PLAYBACK_3 = 0;
        actions.PLAYBACK_4 = 0;
        actions.PLAYBACK_5 = 0;
    }
    actions
}

pub fn available_ledge_options(mut options: LedgeOption) -> LedgeOption {
    if is_waiting_scene() {
        options.PLAYBACK_1 = 0;
        options.PLAYBACK_2 = 0;
        options.PLAYBACK_3 = 0;
        options.PLAYBACK_4 = 0;
        options.PLAYBACK_5 = 0;
    }
    options
}

/// Called before input processing and UI dispatch. Cleanup only touches plugin state;
/// do not roll Smash RNG, dereference departing fighters or request a game status here.
pub fn update() {
    if let Some(mode) = game_mode::current() {
        let key = u64::from(mode.mode)
            | (u64::from(mode.melee) << 32)
            | (u64::from(mode.initialized) << 40);
        if LAST_MODE.swap(key, Ordering::Relaxed) != key {
            // A native Training departure need not change lobby::active(). Drop
            // preview values and pending injection at the actual scene boundary.
            if crate::replay_preview::occupied() {
                super::named_macros::clear_session();
                crate::replay_preview::clear_session();
                super::save_states::cancel_pending();
                unsafe { super::input_record::stop_playback(); }
            }
            crate::character_roster::scene_changed(mode);
            // A scene switch invalidates the previous layout even if the next
            // waiting room happens to use the same allocation address.
            UI_READY.store(false, Ordering::Release);
            NATIVE_UI_BUSY.store(false, Ordering::Release);
            super::ui::native_menu::reset();
            trace!(
                "[LobbyPractice] scene initialized={} melee=0x{:02x} mode=0x{:08x}",
                mode.initialized,
                mode.melee,
                mode.mode
            );
        }
    }
    if !requested() {
        UI_READY.store(false, Ordering::Release);
    }
    let now = active();
    let previous = WAS_ACTIVE.swap(now, Ordering::AcqRel);
    if now != previous {
        SEEN_ENTRIES.store(0, Ordering::Relaxed);
        super::clear_session_state();
        settings_changed();
        trace!(
            "[LobbyPractice] active={} mode={:?}",
            now,
            game_mode::current()
        );
    }
}

pub unsafe fn observe_fighter(accessor: &mut smash::app::BattleObjectModuleAccessor) {
    if !active() || !crate::common::is_fighter(accessor) {
        return;
    }
    let entry = smash::app::lua_bind::WorkModule::get_int(
        accessor,
        *smash::lib::lua_const::FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID,
    );
    if !(0..8).contains(&entry) {
        return;
    }
    let bit = 1 << entry;
    if SEEN_ENTRIES.fetch_or(bit, Ordering::Relaxed) & bit == 0 {
        trace!(
            "[LobbyPractice] fighter entry={} cpu={} ready_go={}",
            entry,
            crate::common::is_operation_cpu(accessor),
            crate::common::is_ready_go()
        );
    }
}
