//! Named practice-setup library: runtime glue.
//!
//! The file format is `training_mod_consts::practice_setup`. This module
//! exports the selected save-state slot to a named file through the
//! payload-neutral keyboard draft in [`named_macros`], lists files as metadata
//! rows, loads only the chosen file, and hands a value-only
//! [`save_states::SlotBind`] to the save-state module, which writes the slot on
//! the game thread and optionally restores it through the ordinary
//! death/rebirth machinery with the user's mirroring, damage, item, charge and
//! partner handling. The exact Mii identity and the stage come from the saved
//! slot, never from a live fighter at export time. Nothing here stores a
//! native pointer; the only resident state is one pending portable record.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use smash::app::utility;
use smash::lib::lua_const::*;
use training_mod_consts::practice_setup::{FighterSetup, Role, SetupLibrary, SetupRecord};
use training_mod_consts::{TrioFighter, SETUP_LIBRARY_DIR};
use training_mod_tui::macro_panel::LibraryRow;

use super::named_macros::{self, NameConfirmation};
use super::save_states::{self, SaveState, SavedState, SlotBind};
use crate::common::consts::FighterId;
use crate::common::try_get_module_accessor;
use crate::training::ui::notifications::notification;

const TITLE: &str = "Setup Library";
const DEFAULT_NAME: &str = "Setup";

/// The one pending portable record: the selected slot's values awaiting a name.
struct PendingSave {
    token: u64,
    record: SetupRecord,
    summary: String,
}

static PENDING_SAVE: Mutex<Option<PendingSave>> = Mutex::new(None);
static NAME_TOKENS: AtomicU64 = AtomicU64::new(1);
/// Slot and save-state enable captured from the open menu. The later game-thread
/// bind uses this, not the menu committed on close.
static LIBRARY_TARGET: Mutex<Option<LibraryTarget>> = Mutex::new(None);

#[derive(Clone, Copy)]
struct LibraryTarget {
    slot: usize,
    /// `save_state_enable` toggle 0 is OnOff::ON.
    save_enabled: bool,
}

fn notify(message: &str) {
    notification(TITLE.to_owned(), message.to_owned(), 180);
}

fn library() -> SetupLibrary {
    SetupLibrary::new(SETUP_LIBRARY_DIR)
}

fn library_target() -> Option<LibraryTarget> {
    *LIBRARY_TARGET.lock().unwrap()
}

fn fighter_label(kind: i32) -> String {
    if kind < 0 {
        return "none".to_owned();
    }
    TrioFighter::from_kind(kind as u32)
        .map(|fighter| fighter.name().to_owned())
        .unwrap_or_else(|| format!("0x{kind:X}"))
}

fn pair_label(player_kind: i32, cpu_kind: i32) -> String {
    format!("{} vs {}", fighter_label(player_kind), fighter_label(cpu_kind))
}

/// Idle practice: both owned fighters present and no save, record, playback,
/// preview or quick reset in flight.
unsafe fn practice_idle() -> Result<(), &'static str> {
    if !super::practice::enabled() || !super::practice::acquire() {
        return Err("Wait for both practice fighters");
    }
    if crate::training::recording_or_playback()
        || save_states::is_loading()
        || save_states::quick_reset_active()
        || crate::replay_preview::occupied()
    {
        return Err("Finish the current practice action first");
    }
    Ok(())
}

fn fighter_setup(saved: &SavedState) -> FighterSetup {
    FighterSetup {
        fighter_kind: saved.fighter_kind,
        x: saved.x,
        y: saved.y,
        percent: saved.percent,
        lr: saved.lr,
        situation_kind: saved.situation_kind,
        charge: saved.charge,
        steve_state: saved.steve_state,
        held_item: saved.held_item,
        mii: saved.mii,
    }
}

/// Values only: the restore machine is idle and the stage is the record's, so
/// a bound slot carries the same provenance a fresh save would.
fn saved_state(setup: &FighterSetup, stage: Option<i32>) -> SavedState {
    SavedState {
        x: setup.x,
        y: setup.y,
        percent: setup.percent,
        lr: setup.lr,
        situation_kind: setup.situation_kind,
        state: SaveState::NoAction,
        fighter_kind: setup.fighter_kind,
        charge: setup.charge,
        steve_state: setup.steve_state,
        held_item: setup.held_item,
        saved_stage: stage,
        mii: setup.mii,
    }
}

/// Menu action: name the selected slot and store both halves as a setup file.
/// `slot` is the open menu's save-state row. The keyboard draft is the shared
/// payload-neutral one; the record waits here.
pub fn save_selected_slot(slot: Option<usize>) {
    if let Err(message) = unsafe { practice_idle() } {
        notify(message);
        return;
    }
    if named_macros::panel_visible() {
        notify("Close the open panel first");
        return;
    }
    if PENDING_SAVE.lock().unwrap().is_some() {
        notify("Finish naming the previous setup first");
        return;
    }
    let Some(slot) = slot else {
        notify("Choose a Save State Slot first");
        return;
    };
    let Some((player, cpu)) = save_states::slot_pair(slot) else {
        notify("Save a Training setup into this slot first");
        return;
    };
    let stage = match (player.saved_stage, cpu.saved_stage) {
        (Some(first), Some(second)) if first != second => {
            notify("Slot halves were saved on different stages");
            return;
        }
        (first, second) => first.or(second),
    };
    let record = match SetupRecord::new(
        DEFAULT_NAME,
        crate::common::SMASH_VERSION.clone(),
        stage,
        fighter_setup(&player),
        fighter_setup(&cpu),
    ) {
        Ok(record) => record,
        Err(err) => {
            notify(&err.to_string());
            return;
        }
    };
    let summary = format!(
        "Slot {} · {}",
        slot + 1,
        pair_label(player.fighter_kind, cpu.fighter_kind)
    );
    let token = NAME_TOKENS.fetch_add(1, Ordering::Relaxed);
    // Open the draft first so a same-frame service tick never sees a pending
    // record without its draft and drops it as cancelled.
    named_macros::offer_external_name(token, DEFAULT_NAME, &summary);
    *PENDING_SAVE.lock().unwrap() = Some(PendingSave { token, record, summary });
}

/// Menu action: list saved setups as metadata rows. Only the row the user
/// picks is read from disk, later, by `service`. `slot` and `save_enable` are
/// the open menu's rows (`save_enable` toggle 0 is On). They stay with this
/// list until the bind, including across menu close.
pub fn open_library(slot: Option<usize>, save_enable: Option<usize>) {
    if let Err(message) = unsafe { practice_idle() } {
        notify(message);
        return;
    }
    let Some(slot) = slot else {
        notify("Choose a Save State Slot first");
        return;
    };
    let listing = match library().list() {
        Ok(listing) => listing,
        Err(err) => {
            notify(&err.to_string());
            return;
        }
    };
    if listing.entries.is_empty() {
        notify(if listing.skipped.is_empty() {
            "No setups saved yet"
        } else {
            "No readable setups; check the files"
        });
        return;
    }
    let skipped = listing.skipped.len();
    let rows = listing
        .entries
        .into_iter()
        .map(|entry| LibraryRow {
            id: entry.id,
            label: format!("{} · {}", entry.name, pair_label(entry.player_kind, entry.cpu_kind)),
        })
        .collect();
    *LIBRARY_TARGET.lock().unwrap() = Some(LibraryTarget {
        slot,
        save_enabled: save_enable == Some(0),
    });
    named_macros::open_external_library(rows, skipped, TITLE, Some(format!("binds to Slot {}", slot + 1)));
}

/// Once per game frame from P1's callback: keyboard confirmations, list
/// actions, and the draft lifecycle of the pending record.
pub fn service() {
    if let Some(confirmation) = named_macros::take_confirmed_name() {
        on_name_confirmed(confirmation);
    }
    let choice = named_macros::take_list_choice();
    let action = named_macros::take_list_action();
    let idle = choice.is_none() && action.is_none();
    if let Some(id) = choice {
        load_and_restore(&id);
    }
    if let Some(id) = action {
        bind_only(&id);
    }
    // A closed list with nothing to apply drops the frozen slot. A same-lobby
    // suspension keeps the panel parked, so the slot stays with it.
    if idle && !named_macros::panel_visible() && !named_macros::suspended() {
        LIBRARY_TARGET.lock().unwrap().take();
    }
    // Cancel, B on the panel, or a new lobby closes the draft without a
    // confirmation; the pending record goes with it. A same-lobby interruption
    // keeps the draft parked, so the record stays as well.
    let stale = PENDING_SAVE
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|pending| !named_macros::external_name_open(pending.token));
    if stale {
        PENDING_SAVE.lock().unwrap().take();
    }
}

fn on_name_confirmed(confirmation: NameConfirmation) {
    let mut pending = PENDING_SAVE.lock().unwrap();
    let Some(save) = pending
        .as_mut()
        .filter(|save| save.token == confirmation.context.token)
    else {
        return;
    };
    let token = save.token;
    let summary = save.summary.clone();
    if let Err(err) = save.record.set_name(&confirmation.name) {
        drop(pending);
        notify(&err.to_string());
        named_macros::offer_external_name(token, &confirmation.name, &summary);
        return;
    }
    let result = library().save_new(&save.record);
    let name = save.record.name.clone();
    match result {
        Ok(_) => {
            *pending = None;
            drop(pending);
            notify(&format!("Saved {name}"));
        }
        Err(err) => {
            // Disk failure: the typed name stays in the record; reopen the same
            // draft under the same token so the user can retry or cancel.
            drop(pending);
            notify(&err.to_string());
            named_macros::offer_external_name(token, &name, &summary);
        }
    }
}

fn load_record(id: &str) -> Option<SetupRecord> {
    match library().load(id) {
        Ok(record) => Some(record),
        Err(err) => {
            notify(&err.to_string());
            None
        }
    }
}

unsafe fn situation_admitted(setup: &FighterSetup) -> bool {
    [*SITUATION_KIND_GROUND, *SITUATION_KIND_AIR, *SITUATION_KIND_CLIFF].contains(&setup.situation_kind)
}

/// Checks shared by A and Y before any slot mutation.
unsafe fn admit(record: &SetupRecord) -> Result<(), &'static str> {
    practice_idle()?;
    if library_target().is_none() {
        return Err("Choose a Save State Slot first");
    }
    if !situation_admitted(&record.player) || !situation_admitted(&record.cpu) {
        return Err("Setup has an unsupported fighter situation");
    }
    if save_states::bind_pending() {
        return Err("A setup is already being applied");
    }
    Ok(())
}

fn role_index(role: Role) -> usize {
    match role {
        Role::Player => 0,
        Role::Cpu => 1,
    }
}

/// Immediate restore needs the same stage, the same fighter kinds and the same
/// exact Mii identities as the saved pair; otherwise only Y (bind) applies.
unsafe fn restore_matches(record: &SetupRecord) -> Result<(), String> {
    restore_pair_matches(record.stage_id, &record.player, &record.cpu)
}

/// A queued menu action can wait for the panel to close. Recheck its native
/// context immediately before committing either slot, not only at selection.
pub(super) unsafe fn restore_bind_matches(bind: &SlotBind) -> Result<(), String> {
    restore_pair_matches(bind.player.saved_stage, &fighter_setup(&bind.player), &fighter_setup(&bind.cpu))
}

unsafe fn restore_pair_matches(stage: Option<i32>, player: &FighterSetup, cpu: &FighterSetup) -> Result<(), String> {
    let Some(stage) = stage else {
        return Err("Setup has no stage; use Y to bind".to_owned());
    };
    if stage != save_states::stage_id() {
        return Err("Different stage; use Y to bind".to_owned());
    }
    for (role, id, setup) in [(Role::Player, FighterId::Player, player), (Role::Cpu, FighterId::CPU, cpu)] {
        let Some(live) = try_get_module_accessor(id) else {
            return Err("Wait for both practice fighters".to_owned());
        };
        if utility::get_kind(&mut *live) != setup.fighter_kind {
            return Err(format!(
                "Setup is {}; use Y to bind",
                pair_label(player.fighter_kind, cpu.fighter_kind)
            ));
        }
        if crate::character_roster::current_mii_configuration(role_index(role)) != setup.mii {
            return Err(format!("{role} exact Mii differs; use Y to bind"));
        }
    }
    Ok(())
}

fn bind_only(id: &str) {
    let Some(record) = load_record(id) else { return };
    if let Err(message) = unsafe { admit(&record) } {
        notify(message);
        return;
    }
    queue(record, false);
}

fn load_and_restore(id: &str) {
    let Some(record) = load_record(id) else { return };
    if let Err(message) = unsafe { admit(&record) } {
        notify(message);
        return;
    }
    let Some(target) = library_target() else {
        notify("Choose a Save State Slot first");
        return;
    };
    if !target.save_enabled {
        notify("Enable save states first, or use Y to bind");
        return;
    }
    if let Err(message) = unsafe { restore_matches(&record) } {
        notify(&message);
        return;
    }
    queue(record, true);
}

fn queue(record: SetupRecord, restore: bool) {
    let Some(target) = library_target() else {
        notify("Choose a Save State Slot first");
        return;
    };
    let stage = record.stage_id;
    let bind = SlotBind {
        slot: target.slot,
        player: saved_state(&record.player, stage),
        cpu: saved_state(&record.cpu, stage),
        restore,
        name: record.name,
    };
    if let Err(message) = save_states::queue_bind(bind) {
        notify(message);
    }
}

/// Practice retirement. A same-lobby suspension keeps the name draft parked, so
/// the pending record stays with it; any other departure drops the record.
/// Queued binds and the load pin are cleared by `save_states::cancel_pending`.
pub fn clear_session() {
    if !named_macros::suspended() {
        PENDING_SAVE.lock().unwrap().take();
        LIBRARY_TARGET.lock().unwrap().take();
    }
}
