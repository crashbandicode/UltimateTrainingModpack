use std::cmp::Ordering;

use skyline::nn::ui2d::ResColor;
use smash::app::{lua_bind::*, utility, BattleObjectModuleAccessor};
use smash::lib::lua_const::*;

use InputRecordState::*;
use PossessionState::*;

use crate::common::consts::{FighterId, HitstunPlayback, OnOff, RecordTrigger};
use crate::common::input::*;
use crate::common::offsets::OFFSET_SET_CPU_CONTROLS;
use crate::common::button_config;
use super::practice;
use crate::common::{is_in_hitstun, is_in_shieldstun, try_get_module_accessor, MENU};
use crate::training::mash;
use crate::training::ui::notifications::{clear_notification, color_notification};
use crate::{error, warn};

use training_mod_sync::*;

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum InputRecordState {
    None,
    Pause,
    Record,
    Playback,
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum PossessionState {
    Player,
    Cpu,
    Lockout,
    Standby,
}

#[derive(PartialEq, Copy, Clone)]
pub enum StartingStatus {
    Aerial, // FIGHTER_STATUS_KIND_ATTACK_AIR TODO: This shouldn't happen without starting input recording in the air - when would we want this?
    // Probably should lock input recordings to either the ground or the air
    Airdodge, // FIGHTER_STATUS_KIND_ESCAPE_AIR, FIGHTER_STATUS_KIND_ESCAPE_AIR_SLIDE
    // Other statuses cannot be used to hitstun cancel via damage_fly_attack_frame/damage_fly_escape_frame
    // Some statuses can leave shield earlier though, so we should check for this
    SpecialHi,  // Up B: FIGHTER_STATUS_KIND_SPECIAL_HI,
    Jump,       // FIGHTER_STATUS_KIND_JUMP_SQUAT
    DoubleJump, //FIGHTER_STATUS_KIND_JUMP_AERIAL
    Spotdodge,  // FIGHTER_STATUS_KIND_ESCAPE
    Roll,       // FIGHTER_STATUS_KIND_ESCAPE_F, FIGHTER_STATUS_KIND_ESCAPE_B
    Grab,       // FIGHTER_STATUS_KIND_CATCH
    Other,
}

pub const STICK_NEUTRAL: f32 = 0.2;
pub const STICK_CLAMP_MULTIPLIER: f32 = 1.0 / 120.0; // 120.0 = CLAMP_MAX
const FINAL_RECORD_MAX: usize = 600; // Maximum length for input recording sequences (capacity)
const TOTAL_SLOT_COUNT: usize = 5; // Total number of input recording slots
pub static INPUT_RECORD: RwLock<InputRecordState> = RwLock::new(InputRecordState::None);
pub static INPUT_RECORD_FRAME: RwLock<usize> = RwLock::new(0);
pub static POSSESSION: RwLock<PossessionState> = RwLock::new(PossessionState::Player);
pub static LOCKOUT_FRAME: RwLock<usize> = RwLock::new(0);
pub static BUFFER_FRAME: RwLock<usize> = RwLock::new(0);
pub static RECORDED_LR: RwLock<f32> = RwLock::new(1.0); // The direction the CPU was facing before the current recording was recorded
pub static CURRENT_LR: RwLock<f32> = RwLock::new(1.0); // The direction the CPU was facing at the beginning of this playback
pub static STARTING_STATUS: RwLock<i32> = RwLock::new(0); // The first status entered in the recording outside of waits
                                                          //     used to calculate if the input playback should begin before hitstun would normally end (hitstun cancel, monado art?)
pub static CURRENT_RECORD_SLOT: RwLock<usize> = RwLock::new(0); // Which slot is being used for recording right now? Want to make sure this is synced with menu choices, maybe just use menu instead
pub static CURRENT_PLAYBACK_SLOT: RwLock<usize> = RwLock::new(0); // Which slot is being used for playback right now?
pub static CURRENT_FRAME_LENGTH: RwLock<usize> = RwLock::new(60);
pub static P1_FINAL_MAPPING: RwLock<[[MappedInputs; FINAL_RECORD_MAX]; TOTAL_SLOT_COUNT]> =
    RwLock::new([[{ MappedInputs::empty() }; FINAL_RECORD_MAX]; TOTAL_SLOT_COUNT]);
pub static P1_FRAME_LENGTH_MAPPING: RwLock<[usize; TOTAL_SLOT_COUNT]> =
    RwLock::new([60; TOTAL_SLOT_COUNT]);

/// Facing, status, and exact Mii taken when this slot's recording started.
/// Absent means the slot still uses the shared [`RECORDED_LR`] pair.
/// Export reads this snapshot. It does not read the live CPU again.
#[derive(Clone, Copy)]
struct SlotCapture {
    start: training_mod_macros::StartState,
    mii: Option<training_mod_consts::mii::lifetime::Configuration>,
    identity: training_mod_tui::macro_panel::MiiIdentity,
}

static SLOT_CAPTURE: RwLock<[Option<SlotCapture>; TOTAL_SLOT_COUNT]> =
    RwLock::new([Option::None; TOTAL_SLOT_COUNT]);
/// Shared facing/status parked while a snapshotted slot is playing.
static SLOT_PARK: RwLock<Option<(f32, i32)>> = RwLock::new(Option::None);
// pub static P1_STARTING_STATUSES: RwLock<[StartingStatus; TOTAL_SLOT_COUNT]> = RwLock::new([StartingStatus::Other; TOTAL_SLOT_COUNT]); // TODO! Not used currently

unsafe fn can_transition(module_accessor: *mut BattleObjectModuleAccessor) -> bool {
    let transition_term = into_transition_term(into_starting_status(read(&STARTING_STATUS)));
    WorkModule::is_enable_transition_term(module_accessor, transition_term)
}

unsafe fn should_mash_playback() {
    // Don't want to interrupt recording
    if is_recording() {
        return;
    }
    if !mash::is_playback_queued() {
        return;
    }
    // playback is queued, so we might want to begin this frame
    // if we're currently playing back, we don't want to interrupt (this may change for layered multislot playback, but for now this is fine)
    if is_playback() {
        return;
    }
    let mut should_playback = false;
    let Some(cpu_module_accessor) = try_get_module_accessor(FighterId::CPU) else { return; };
    // depending on our current status, we want to wait for different timings to begin playback

    // TODO: This isn't the best way to write this I'm sure, want to rewrite

    if is_in_hitstun(&mut *cpu_module_accessor) {
        // if we're in hitstun and want to enter the frame we start hitstop for SDI, start if we're in any damage status instantly
        if read(&MENU).hitstun_playback == HitstunPlayback::INSTANT {
            should_playback = true;
        }
        // if we want to wait until we exit hitstop and begin flying away for shield art etc, start if we're not in hitstop
        if read(&MENU).hitstun_playback == HitstunPlayback::HITSTOP
            && !StopModule::is_stop(cpu_module_accessor)
        {
            should_playback = true;
        }
        // if we're in hitstun and want to wait till FAF to act, then we want to match our starting status to the correct transition term to see if we can hitstun cancel
        if read(&MENU).hitstun_playback == HitstunPlayback::HITSTUN
            && can_transition(cpu_module_accessor)
        {
            should_playback = true;
        }
    } else if is_in_shieldstun(&mut *cpu_module_accessor) {
        // TODO: Add instant shieldstun toggle for shield art out of electric hitstun? Idk that's so specific
        if can_transition(cpu_module_accessor) {
            should_playback = true;
        }
    } else if can_transition(cpu_module_accessor) {
        should_playback = true;
    }

    // how do we deal with buffering motion inputs out of shield? You can't complete them in one frame, but they can definitely be buffered during shield drop
    // probably need a separate standby setting for grounded, aerial, shield, where shield starts once you let go of shield, and aerial keeps you in the air?

    if should_playback {
        playback(Some(mash::queued_playback_slot()));
    }
}

// TODO: set up a better match later and make this into one func

fn into_starting_status(status: i32) -> StartingStatus {
    if status == *FIGHTER_STATUS_KIND_ATTACK_AIR {
        return StartingStatus::Aerial;
    } else if (*FIGHTER_STATUS_KIND_ESCAPE_AIR..*FIGHTER_STATUS_KIND_ESCAPE_AIR_SLIDE)
        .contains(&status)
    {
        return StartingStatus::Airdodge;
    } else if status == *FIGHTER_STATUS_KIND_SPECIAL_HI {
        return StartingStatus::SpecialHi;
    } else if status == *FIGHTER_STATUS_KIND_JUMP_SQUAT {
        return StartingStatus::Jump;
    } else if status == *FIGHTER_STATUS_KIND_JUMP_AERIAL {
        return StartingStatus::DoubleJump;
    } else if status == *FIGHTER_STATUS_KIND_ESCAPE {
        return StartingStatus::Spotdodge;
    } else if (*FIGHTER_STATUS_KIND_ESCAPE_F..*FIGHTER_STATUS_KIND_ESCAPE_B).contains(&status) {
        return StartingStatus::Roll;
    } else if status == *FIGHTER_STATUS_KIND_CATCH {
        return StartingStatus::Grab;
    }
    StartingStatus::Other
}

fn into_transition_term(starting_status: StartingStatus) -> i32 {
    match starting_status {
        StartingStatus::Aerial => *FIGHTER_STATUS_TRANSITION_TERM_ID_CONT_ATTACK_AIR,
        StartingStatus::Airdodge => *FIGHTER_STATUS_TRANSITION_TERM_ID_CONT_ESCAPE_AIR,
        StartingStatus::Other => *FIGHTER_STATUS_TRANSITION_TERM_ID_CONT_SPECIAL_S, // placeholder - most likely don't want to use this in final build, and have a different set of checks
        StartingStatus::SpecialHi => *FIGHTER_STATUS_TRANSITION_TERM_ID_CONT_SPECIAL_HI,
        StartingStatus::Jump => *FIGHTER_STATUS_TRANSITION_TERM_ID_CONT_JUMP_SQUAT,
        StartingStatus::DoubleJump => *FIGHTER_STATUS_TRANSITION_TERM_ID_CONT_JUMP_AERIAL,
        StartingStatus::Spotdodge => *FIGHTER_STATUS_TRANSITION_TERM_ID_CONT_ESCAPE,
        StartingStatus::Roll => *FIGHTER_STATUS_TRANSITION_TERM_ID_CONT_ESCAPE_F,
        StartingStatus::Grab => *FIGHTER_STATUS_TRANSITION_TERM_ID_CONT_CATCH,
    }
}

pub unsafe fn handle_recording() {
    if !practice::enabled() { return; }
    if crate::replay_preview::occupied() {
        // Preview completion never consults the user's loop/slot/record-on-load
        // settings. Let the last emitted input run before ending injection.
        if read(&INPUT_RECORD) == Playback
            && read(&INPUT_RECORD_FRAME) >= read(&CURRENT_FRAME_LENGTH) {
            stop_playback();
            crate::replay_preview::complete();
        }
        return;
    }
    if let Some(player_module_accessor) = try_get_module_accessor(FighterId::Player) {
        if let Some(cpu_module_accessor) = try_get_module_accessor(FighterId::CPU) {
            handle_recording_for_fighter(&mut *player_module_accessor);
            handle_recording_for_fighter(&mut *cpu_module_accessor);
        }
    }
}

unsafe fn handle_recording_for_fighter(module_accessor: &mut BattleObjectModuleAccessor) {
    // Allow this because sometimes we want to make sure our NNSDK doesn't have
    // an erroneous definition
    #[allow(clippy::unnecessary_cast)]
    let entry_id_int =
        WorkModule::get_int(module_accessor, *FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID) as i32;
    let fighter_kind = utility::get_kind(module_accessor);
    let fighter_is_nana = fighter_kind == *FIGHTER_KIND_NANA;

    assign(
        &CURRENT_RECORD_SLOT,
        read(&MENU).recording_slot.into_idx().unwrap_or(0),
    );

    if entry_id_int == 0 && !fighter_is_nana {
        // Consume both queued shortcuts even during a state load. Otherwise a
        // held/overlapping playback chord can restart the recorder immediately
        // after save_states cancels it, moving the CPU during restoration.
        // Automatic record/playback-on-load remains owned by save_states.
        let playback_requested =
            button_config::combo_passes(button_config::ButtonCombo::InputPlayback);
        let record_requested = button_config::combo_passes(button_config::ButtonCombo::InputRecord);
        let state_loading = crate::training::save_states::is_loading();
        if state_loading && (playback_requested || record_requested) {
            crate::logging::diagnostic(format_args!(
                "[PracticeRecording] shortcut_ignored_during_state_load playback={} record={}",
                playback_requested, record_requested
            ));
        } else if playback_requested {
            crate::logging::diagnostic(format_args!(
                "[PracticeRecording] playback_shortcut player_kind=0x{:x}",
                fighter_kind
            ));
            if crate::training::named_macros::source_is_named() {
                crate::training::named_macros::request_playback();
            } else {
                playback(read(&MENU).playback_button_slots.get_random().into_idx());
            }
        } else if read(&MENU).record_trigger.contains(&RecordTrigger::COMMAND) && record_requested {
            if crate::training::named_macros::source_is_named() {
                if is_recording() || is_standby() {
                    finish_named_capture();
                } else if read(&INPUT_RECORD) == None {
                    start_named_lockout();
                }
            } else {
                lockout_record();
            }
        }
        let input_record = read(&INPUT_RECORD);
        if input_record == None {
            clear_notification("Input Recording");
        }
        // Handle recording end
        let input_record_frame = read(&INPUT_RECORD_FRAME);
        let last_frame = read(&CURRENT_FRAME_LENGTH)
            .saturating_sub(usize::from(!crate::training::named_macros::using_frames()));
        if (input_record == Record || input_record == Playback) && input_record_frame >= last_frame
        {
            let named_capture =
                input_record == Record && crate::training::named_macros::capture_active();
            let named_playback =
                input_record == Playback && crate::training::named_macros::playback_active();
            if named_capture {
                finish_named_capture();
            } else if named_playback {
                crate::logging::diagnostic(format_args!(
                    "[PracticeRecording] named_playback_complete emitted_frames={} length={}",
                    input_record_frame, read(&CURRENT_FRAME_LENGTH)
                ));
                assign(&POSSESSION, Player);
                if mash::is_playback_queued() {
                    mash::reset();
                }
                assign(&INPUT_RECORD_FRAME, 0);
                if read(&MENU).playback_loop == OnOff::ON {
                    playback_named();
                } else {
                    crate::training::named_macros::end_playback();
                    restore_named_legacy();
                    assign(&INPUT_RECORD, None);
                }
            } else {
                if input_record == Record {
                    crate::logging::diagnostic(format_args!(
                        "[PracticeRecording] recording_complete slot={} frames={}",
                        read(&CURRENT_RECORD_SLOT),
                        input_record_frame + 1
                    ));
                }
                assign(&POSSESSION, Player);
                if mash::is_playback_queued() {
                    mash::reset();
                }

                // If we need to crop the recording for neutral input
                // INPUT_RECORD_FRAME must be > 0 to prevent bounding errors
                if input_record == Record
                    && read(&MENU).recording_crop == OnOff::ON
                    && input_record_frame > 0
                {
                    let mut input_record_len = input_record_frame;
                    while input_record_len > 0 && is_input_neutral(input_record_len - 1) {
                        // Discard frames at the end of the recording until the last frame with input
                        input_record_len -= 1;
                    }
                    assign(&CURRENT_FRAME_LENGTH, input_record_len);
                    let mut p1_frame_length_mapping = lock_write(&P1_FRAME_LENGTH_MAPPING);
                    (*p1_frame_length_mapping)[read(&CURRENT_RECORD_SLOT)] = input_record_len;
                    drop(p1_frame_length_mapping);
                }

                assign(&INPUT_RECORD_FRAME, 0);

                if read(&MENU).playback_loop == OnOff::ON && input_record == Playback {
                    let playback_slot = read(&CURRENT_PLAYBACK_SLOT);
                    playback(Some(playback_slot));
                } else if input_record == Playback {
                    release_numbered_origin();
                    assign(&INPUT_RECORD, None);
                } else {
                    assign(&INPUT_RECORD, None);
                }
            }
        }
    }

    // Handle Possession Coloring
    let possession = read(&POSSESSION);
    let cpu_entry = super::lobby::entry_id(FighterId::CPU);
    if entry_id_int == cpu_entry && possession == Lockout {
        clear_notification("Input Recording");
        color_notification(
            "Input Recording".to_string(),
            "Lockout".to_owned(),
            60,
            ResColor {
                r: 8,
                g: 8,
                b: 200,
                a: 255,
            },
        );
        set_color_rgb_2(
            module_accessor,
            0.0,
            0.0,
            1.0,
            *MODEL_COLOR_TYPE_COLOR_BLEND,
        );
    } else if entry_id_int == cpu_entry && possession == Standby {
        clear_notification("Input Recording");
        color_notification(
            "Input Recording".to_string(),
            "Standby".to_owned(),
            60,
            ResColor {
                r: 200,
                g: 8,
                b: 200,
                a: 255,
            },
        );
        set_color_rgb_2(
            module_accessor,
            1.0,
            0.0,
            1.0,
            *MODEL_COLOR_TYPE_COLOR_BLEND,
        );
    } else if entry_id_int == cpu_entry && possession == Cpu {
        clear_notification("Input Recording");
        color_notification(
            "Input Recording".to_string(),
            "Recording".to_owned(),
            60,
            ResColor {
                r: 200,
                g: 8,
                b: 8,
                a: 255,
            },
        );
        set_color_rgb_2(
            module_accessor,
            1.0,
            0.0,
            0.0,
            *MODEL_COLOR_TYPE_COLOR_BLEND,
        );
    } else if entry_id_int == cpu_entry && possession == Player && read(&INPUT_RECORD) == Playback {
        // Need to re-read INPUT_RECORD instead of using the local variable because we might have assigned to it early
        // Displays if the inputs from the current frame were a result of playback
        let input_record_frame = read(&INPUT_RECORD_FRAME);
        if input_record_frame == 0 || input_record_frame == 1 {
            // can be either, seems like a thread issue
            clear_notification("Input Recording");
            color_notification(
                "Input Recording".to_string(),
                "Playback".to_owned(),
                read(&CURRENT_FRAME_LENGTH) as u32,
                ResColor {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                },
            );
        }
    }
}

pub unsafe fn start_named_lockout() {
    if !practice::enabled() || !practice::acquire() || crate::replay_preview::occupied() {
        return;
    }
    let Some(cpu_module_accessor) = try_get_module_accessor(FighterId::CPU) else { return; };
    release_numbered_origin();
    let start = crate::training::named_macros::snapshot_start(&mut *cpu_module_accessor);
    crate::training::named_macros::remember_legacy(read(&RECORDED_LR), read(&STARTING_STATUS));
    crate::training::named_macros::begin_capture(start);
    let recording_duration = training_mod_macros::MAX_FRAMES;
    crate::logging::diagnostic(format_args!(
        "[PracticeRecording] named_record_lockout cpu_kind=0x{:x} frames={}",
        start.fighter_kind, recording_duration
    ));
    assign(&CURRENT_FRAME_LENGTH, recording_duration);
    assign(&INPUT_RECORD, Pause);
    assign(&INPUT_RECORD_FRAME, 0);
    assign(&POSSESSION, Lockout);
    assign(&LOCKOUT_FRAME, 30);
    assign(&BUFFER_FRAME, 0);
    assign(&RECORDED_LR, start.facing);
    assign(&CURRENT_LR, start.facing);
}

unsafe fn finish_named_capture() {
    let crop = read(&MENU).recording_crop == OnOff::ON;
    assign(&POSSESSION, Player);
    assign(&INPUT_RECORD, None);
    assign(&INPUT_RECORD_FRAME, 0);
    crate::training::named_macros::finish_capture(crop);
    restore_named_legacy();
}

/// Finish before input delay, storage and CPU injection can consume the stop chord.
pub unsafe fn handle_named_stop_request(player_idx: i32) -> bool {
    if player_idx != 0 || !crate::training::named_macros::capture_active()
        || !button_config::combo_passes(button_config::ButtonCombo::InputRecord)
    {
        return false;
    }
    finish_named_capture();
    true
}

fn restore_named_legacy() {
    if let Some((facing, status)) = crate::training::named_macros::take_legacy() {
        assign(&RECORDED_LR, facing);
        assign(&STARTING_STATUS, status);
    }
}

fn release_numbered_origin() {
    let mut park = lock_write(&SLOT_PARK);
    if park.is_none() {
        return;
    }
    let shared = (read(&RECORDED_LR), read(&STARTING_STATUS));
    let (active, next) = training_mod_tui::macro_panel::release_slot_origin(shared, *park);
    *park = next;
    drop(park);
    assign(&RECORDED_LR, active.0);
    assign(&STARTING_STATUS, active.1);
}

fn apply_numbered_origin(slot: usize) {
    let origin = read(&SLOT_CAPTURE)[slot].map(|capture| (capture.start.facing, capture.start.status));
    let mut park = lock_write(&SLOT_PARK);
    let shared = (read(&RECORDED_LR), read(&STARTING_STATUS));
    let (active, next) = if origin.is_none() {
        training_mod_tui::macro_panel::release_slot_origin(shared, *park)
    } else {
        training_mod_tui::macro_panel::engage_slot_origin(shared, *park, origin)
    };
    *park = next;
    drop(park);
    assign(&RECORDED_LR, active.0);
    assign(&STARTING_STATUS, active.1);
}

fn frame_from_mapped(input: &MappedInputs) -> training_mod_macros::InputFrame {
    training_mod_macros::InputFrame {
        buttons: input.buttons.bits(),
        lstick_x: input.lstick_x,
        lstick_y: input.lstick_y,
        rstick_x: input.rstick_x,
        rstick_y: input.rstick_y,
    }
}

fn mapped_from_frame(frame: &training_mod_macros::InputFrame) -> MappedInputs {
    MappedInputs {
        buttons: Buttons::from_bits_truncate(frame.buttons),
        lstick_x: frame.lstick_x,
        lstick_y: frame.lstick_y,
        rstick_x: frame.rstick_x,
        rstick_y: frame.rstick_y,
    }
}

/// Slot snapshot from recording start. Errors name the refusal and write nothing.
pub fn export_named(
    slot: usize,
) -> Result<
    (
        Vec<training_mod_macros::InputFrame>,
        training_mod_macros::StartState,
        Option<training_mod_consts::mii::lifetime::Configuration>,
    ),
    &'static str,
> {
    if slot >= TOTAL_SLOT_COUNT {
        return Err("Record Slot is out of range");
    }
    let Some(capture) = read(&SLOT_CAPTURE)[slot] else {
        return Err("Record into this slot first");
    };
    if let Some(message) = training_mod_tui::macro_panel::export_mii_refusal(capture.identity) {
        return Err(message);
    }
    let len = read(&P1_FRAME_LENGTH_MAPPING)[slot];
    if len == 0 || len > FINAL_RECORD_MAX {
        return Err("Record into this slot first");
    }
    let mapping = read(&P1_FINAL_MAPPING);
    let frames = mapping[slot][..len]
        .iter()
        .map(frame_from_mapped)
        .collect();
    Ok((frames, capture.start, capture.mii))
}

/// Replaces one slot, or leaves every slot byte and [`RECORDED_LR`] alone.
pub fn import_named_slot(
    slot: usize,
    frames: &[training_mod_macros::InputFrame],
    start: training_mod_macros::StartState,
    mii: Option<training_mod_consts::mii::lifetime::Configuration>,
) -> Result<(), String> {
    if slot >= TOTAL_SLOT_COUNT {
        return Err("Record Slot is out of range. Nothing was changed.".to_owned());
    }
    let copied = training_mod_tui::macro_panel::copy_numbered_frames(frames).map_err(|admit| {
        match admit {
            training_mod_tui::macro_panel::SlotAdmit::Empty => {
                "Recording is empty. Nothing was changed.".to_owned()
            }
            training_mod_tui::macro_panel::SlotAdmit::OverLimit { frames, limit } => {
                format!("Recording is {frames} frames; a Record Slot holds {limit}. Nothing was changed.")
            }
            training_mod_tui::macro_panel::SlotAdmit::Ready => {
                "Recording was not admitted. Nothing was changed.".to_owned()
            }
        }
    })?;
    let mut mapping = lock_write(&P1_FINAL_MAPPING);
    let mut lengths = lock_write(&P1_FRAME_LENGTH_MAPPING);
    let mut captures = lock_write(&SLOT_CAPTURE);
    (*mapping)[slot] = [MappedInputs::empty(); FINAL_RECORD_MAX];
    for (index, frame) in copied.iter().enumerate() {
        (*mapping)[slot][index] = mapped_from_frame(frame);
    }
    (*lengths)[slot] = copied.len();
    let observed_kind = mii.as_ref().map(|configuration| configuration.canonical().class.kind());
    let identity = training_mod_tui::macro_panel::mii_identity(start.fighter_kind, observed_kind);
    let mii = match identity {
        training_mod_tui::macro_panel::MiiIdentity::Exact => mii,
        _ => Option::None,
    };
    (*captures)[slot] = Some(SlotCapture { start, mii, identity });
    Ok(())
}

pub unsafe fn lockout_record() {
    if !practice::enabled() || !practice::acquire() || crate::replay_preview::occupied() {
        return;
    }
    let Some(cpu_module_accessor) = try_get_module_accessor(FighterId::CPU) else { return; };
    let recording_duration = read(&MENU).recording_duration.into_frames();
    crate::logging::diagnostic(format_args!(
        "[PracticeRecording] record_lockout cpu_kind=0x{:x} frames={}",
        utility::get_kind(&mut *cpu_module_accessor),
        recording_duration
    ));
    let current_record_slot = read(&CURRENT_RECORD_SLOT);
    release_numbered_origin();
    let start = crate::training::named_macros::snapshot_start(&mut *cpu_module_accessor);
    let (identity, mii) = crate::training::named_macros::mii_at_lockout(start.fighter_kind);
    {
        let mut captures = lock_write(&SLOT_CAPTURE);
        (*captures)[current_record_slot] = Some(SlotCapture { start, mii, identity });
    }
    let mut p1_final_mapping = lock_write(&P1_FINAL_MAPPING);
    (*p1_final_mapping)[current_record_slot] = [{ MappedInputs::empty() }; FINAL_RECORD_MAX];
    drop(p1_final_mapping);
    let mut p1_frame_length_mapping = lock_write(&P1_FRAME_LENGTH_MAPPING);
    (*p1_frame_length_mapping)[current_record_slot] = recording_duration;
    drop(p1_frame_length_mapping);
    assign(&CURRENT_FRAME_LENGTH, recording_duration);
    assign(&INPUT_RECORD, Pause);
    assign(&INPUT_RECORD_FRAME, 0);
    assign(&POSSESSION, Lockout);
    assign(&LOCKOUT_FRAME, 30); // This needs to be this high or issues occur dropping shield - but does this cause problems when trying to record ledge?
    assign(&BUFFER_FRAME, 0);
    // Store the direction the CPU is facing when we initially record, so we can turn their inputs around if needed
    assign(&RECORDED_LR, start.facing);
    assign(&CURRENT_LR, start.facing);
}

pub unsafe fn playback_named() -> bool {
    if !practice::enabled() || !practice::acquire() || crate::replay_preview::occupied() {
        return false;
    }
    if read(&INPUT_RECORD) == Pause {
        warn!("Tried to playback during lockout!");
        return false;
    }
    let Some(cpu_module_accessor) = try_get_module_accessor(FighterId::CPU) else { return false; };
    let Some(info) = crate::training::named_macros::begin_playback() else {
        crate::training::ui::notifications::notification(
            "Named Macro".to_owned(),
            "Load a named macro and wait for its fighter".to_owned(),
            120,
        );
        return false;
    };
    release_numbered_origin();
    crate::training::named_macros::remember_legacy(read(&RECORDED_LR), read(&STARTING_STATUS));
    assign(&RECORDED_LR, info.facing);
    assign(&STARTING_STATUS, info.status);
    assign(&CURRENT_FRAME_LENGTH, info.len);
    assign(&INPUT_RECORD, Playback);
    assign(&POSSESSION, Player);
    assign(&INPUT_RECORD_FRAME, 0);
    assign(&BUFFER_FRAME, 0);
    assign(&CURRENT_LR, PostureModule::lr(cpu_module_accessor));
    true
}

pub unsafe fn playback_preview(recording: training_mod_macros::MacroRecording) -> bool {
    if !practice::enabled() || !practice::acquire() || recording.validate().is_err() { return false; }
    let Some(cpu) = try_get_module_accessor(FighterId::CPU) else { return false; };
    release_numbered_origin();
    let info = crate::training::named_macros::begin_preview(recording);
    crate::training::named_macros::remember_legacy(read(&RECORDED_LR), read(&STARTING_STATUS));
    assign(&RECORDED_LR, info.facing);
    assign(&STARTING_STATUS, info.status);
    assign(&CURRENT_FRAME_LENGTH, info.len);
    assign(&INPUT_RECORD, Playback);
    assign(&POSSESSION, Player);
    assign(&INPUT_RECORD_FRAME, 0);
    assign(&BUFFER_FRAME, 0);
    assign(&CURRENT_LR, PostureModule::lr(cpu));
    true
}

// Returns whether we did playback
pub unsafe fn playback(slot: Option<usize>) -> bool {
    if !practice::enabled() || !practice::acquire() || crate::replay_preview::occupied() {
        return false;
    }
    if read(&INPUT_RECORD) == Pause {
        warn!("Tried to playback during lockout!");
        return false;
    }
    if let Some(slot) = slot {
        if slot >= TOTAL_SLOT_COUNT { return false; }
        let Some(cpu_module_accessor) = try_get_module_accessor(FighterId::CPU) else { return false; };
        let frame_length = read(&P1_FRAME_LENGTH_MAPPING)[slot];
        if frame_length == 0 || frame_length > FINAL_RECORD_MAX { return false; }
        apply_numbered_origin(slot);
        assign(&CURRENT_FRAME_LENGTH, frame_length);
        assign(&CURRENT_PLAYBACK_SLOT, slot);
        assign(&INPUT_RECORD, Playback);
        assign(&POSSESSION, Player);
        assign(&INPUT_RECORD_FRAME, 0);
        assign(&BUFFER_FRAME, 0);
        assign(&CURRENT_LR, PostureModule::lr(cpu_module_accessor));
        true
    } else {
        warn!("Tried to playback without a slot selected!");
        false
    }
}

pub unsafe fn playback_ledge(slot: Option<usize>) {
    if !practice::enabled() {
        return;
    }
    let did_playback = playback(slot);
    if did_playback {
        let mut buffer_frame = lock_write(&BUFFER_FRAME);
        *buffer_frame = 5; // So we can make sure the option is buffered and won't get ledge trumped if delay is 0
                           // drop down from ledge can't be buffered on the same frame as jump/attack/roll/ngu so we have to do this
                           // Need to buffer 1 less frame for non-lassos
        let Some(cpu_module_accessor) = try_get_module_accessor(FighterId::CPU) else { return; };
        let status_kind = StatusModule::status_kind(cpu_module_accessor);
        if status_kind == *FIGHTER_STATUS_KIND_CLIFF_CATCH {
            *buffer_frame -= 1;
        }
    }
}

pub unsafe fn stop_playback() {
    let previous = read(&INPUT_RECORD);
    if previous != None {
        crate::logging::diagnostic(format_args!(
            "[PracticeRecording] stop state={:?} frame={}",
            previous,
            read(&INPUT_RECORD_FRAME)
        ));
    }
    crate::training::named_macros::cancel_motion();
    release_numbered_origin();
    restore_named_legacy();
    assign(&INPUT_RECORD, None);
    assign(&INPUT_RECORD_FRAME, 0);
    assign(&POSSESSION, Player);
    assign(&BUFFER_FRAME, 0);
    assign(&LOCKOUT_FRAME, 0);
}

/// Only verified native control fields; trailing "rstick" fields are rumble.
fn neutral_controls(controls: &mut ControlModuleInternal) {
    controls.buttons = Buttons::empty();
    controls.stick_x = 0.0;
    controls.stick_y = 0.0;
    controls.clamped_lstick_x = 0.0;
    controls.clamped_lstick_y = 0.0;
}

pub unsafe fn is_input_neutral(input_frame: usize) -> bool {
    // Returns whether we should be done with standby this frame (if any significant controller input has been made)
    let frame_input = if crate::training::named_macros::capture_active() {
        crate::training::named_macros::frame_at(input_frame)
    } else {
        let current_record_slot = read(&CURRENT_RECORD_SLOT);
        read(&P1_FINAL_MAPPING)[current_record_slot][input_frame]
    };

    let clamped_lstick_x =
        ((frame_input.lstick_x as f32) * STICK_CLAMP_MULTIPLIER).clamp(-1.0, 1.0);
    let clamped_lstick_y =
        ((frame_input.lstick_y as f32) * STICK_CLAMP_MULTIPLIER).clamp(-1.0, 1.0);
    let clamped_rstick_x =
        ((frame_input.rstick_x as f32) * STICK_CLAMP_MULTIPLIER).clamp(-1.0, 1.0);
    let clamped_rstick_y =
        ((frame_input.rstick_y as f32) * STICK_CLAMP_MULTIPLIER).clamp(-1.0, 1.0);

    // No buttons pressed or just flick jump-- if they really did a stick jump, we'd have lstick movement as well
    let buttons_pressed =
        !(frame_input.buttons.is_empty() || frame_input.buttons == Buttons::FLICK_JUMP);
    let lstick_movement =
        clamped_lstick_x.abs() >= STICK_NEUTRAL || clamped_lstick_y.abs() >= STICK_NEUTRAL;
    let rstick_movement =
        clamped_rstick_x.abs() >= STICK_NEUTRAL || clamped_rstick_y.abs() >= STICK_NEUTRAL;
    !(lstick_movement || rstick_movement || buttons_pressed)
}

pub unsafe fn handle_final_input_mapping(player_idx: i32, out: *mut MappedInputs) {
    if !practice::enabled() { return; }
    if player_idx == 0 {
        // if player 1
        if read(&INPUT_RECORD) == Record {
            let mut input_record_frame = lock_write(&INPUT_RECORD_FRAME);
            if crate::training::named_macros::capture_active() {
                if *input_record_frame == 0 {
                    if let Some(cpu) = try_get_module_accessor(FighterId::CPU) {
                        let start = crate::training::named_macros::capture_start(&mut *cpu);
                        assign(&RECORDED_LR, start.facing);
                        assign(&CURRENT_LR, start.facing);
                    }
                }
                crate::training::named_macros::store_frame(*input_record_frame, &*out);
                *out = MappedInputs::empty();
                return;
            }
            // check for standby before starting action:
            if read(&POSSESSION) == Standby && !is_input_neutral(0) {
                // last input made us start an action, so start recording and end standby.
                *input_record_frame += 1;
                assign(&POSSESSION, Cpu);
            }

            if *input_record_frame == 1 {
                // We're on the second frame of recording, grabbing the status should give us the status that resulted from the first frame of input
                // We'll want to save this status so that we use the correct TRANSITION TERM for hitstun cancelling out of damage fly
                let Some(cpu_module_accessor) = try_get_module_accessor(FighterId::CPU) else { return; };
                let status = StatusModule::status_kind(cpu_module_accessor);
                assign(&STARTING_STATUS, status);
                let slot = read(&CURRENT_RECORD_SLOT);
                let mut captures = lock_write(&SLOT_CAPTURE);
                if let Some(capture) = (*captures)[slot].as_mut() {
                    capture.start.status = status;
                }
                drop(captures);
                // TODO: Handle this based on slot later instead
                // let p1_starting_statuses = lock_write_rwlock(&P1_STARTING_STATUSES);
                // (*p1_starting_statuses)[read(&CURRENT_PLAYBACK_SLOT)] =
                //     into_starting_status(StatusModule::status_kind(cpu_module_accessor));
                // drop(p1_starting_statuses);
            }
            if crate::training::named_macros::capture_active() {
                crate::training::named_macros::store_frame(*input_record_frame, &*out);
            } else {
                let mut p1_final_mapping = lock_write(&P1_FINAL_MAPPING);
                let current_record_slot = read(&CURRENT_RECORD_SLOT);
                (*p1_final_mapping)[current_record_slot][*input_record_frame] = *out;
                drop(p1_final_mapping);
            }
            *out = MappedInputs::empty(); // don't control player while recording
        }
        // Don't allow for player input during Lockout
        if read(&POSSESSION) == Lockout {
            *out = MappedInputs::empty();
        }
    }
}

#[skyline::hook(offset = *OFFSET_SET_CPU_CONTROLS)] // After cpu controls are assigned from ai calls
unsafe fn set_cpu_controls(p_data: *mut *mut u8) {
    call_original!(p_data);
    #[cfg(feature = "cpu-pilot-probe")]
    if crate::cpu_pilot::thinking() { return; }
    super::lobby::update();
    if !practice::enabled() {
        return;
    }

    let Some(controls) = crate::native_fields::child(p_data as usize, 8) else { return; };
    if !practice::owns_cpu_controls(controls) { return; }
    let controller_data = controls as *mut ControlModuleInternal;

    // Finish the waiting CPU's current action without native AI or mash starting
    // another one. This writes only this callback's verified current controller.
    if crate::replay_capture::quiescing_capture() {
        neutral_controls(&mut *controller_data);
        return;
    }

    // Check if we need to begin playback this frame due to a mash toggle
    // TODO: Setup STARTING_STATUS based on current playback slot here

    // This check prevents out of shield if mash exiting is on
    if read(&INPUT_RECORD) == None && !crate::replay_preview::occupied() {
        should_mash_playback();
    }

    let Some(cpu_module_accessor) = try_get_module_accessor(FighterId::CPU) else {
        // Sometimes we can try to grab their module accessor before they are valid?
        return;
    };

    if crate::replay_preview::occupied()
        && !crate::replay_preview::can_emit(&mut *cpu_module_accessor) {
        neutral_controls(&mut *controller_data);
        return;
    }

    if read(&INPUT_RECORD) == Pause {
        let lockout_frame = read(&LOCKOUT_FRAME);
        match lockout_frame.cmp(&0) {
            Ordering::Greater => assign(&LOCKOUT_FRAME, lockout_frame - 1),
            Ordering::Equal => {
                assign(&INPUT_RECORD, Record);
                assign(&POSSESSION, if crate::training::named_macros::capture_active() { Cpu } else { Standby });
            }
            Ordering::Less => error!("LOCKOUT_FRAME OUT OF BOUNDS"),
        }
    }

    let input_record = read(&INPUT_RECORD);
    if input_record == Playback && crate::training::named_macros::playback_active()
        && !crate::training::named_macros::prepare_playback_start(&mut *cpu_module_accessor)
    {
        neutral_controls(&mut *controller_data);
        return;
    }
    if input_record == Record || input_record == Playback {
        // if we aren't facing the way we were when we initially recorded, we reverse horizontal inputs
        let mut x_input_multiplier = read(&RECORDED_LR) * read(&CURRENT_LR);
        // Don't flip Shulk's dial inputs
        let fighter_kind = utility::get_kind(&mut *cpu_module_accessor);
        if fighter_kind == *FIGHTER_KIND_SHULK {
            let circle_menu_flag = WorkModule::is_flag(
                &mut *cpu_module_accessor,
                *FIGHTER_SHULK_INSTANCE_WORK_ID_FLAG_SPECIAL_N_CIRCLE_MENU,
            );
            if circle_menu_flag {
                // While in dial, don't flip horizontal inputs
                x_input_multiplier = 1.0;
            }
            // If we have issues with the frame after the dial comes out, change condition to
            //  circle_menu_flag && FIGHTER_SHULK_INSTANCE_WORK_ID_INT_SPECIAL_N_DECIDE_INTERVAL_FRAME > 1
        }
        // Prevent us from falling off of the ledge in standby
        if StatusModule::status_kind(cpu_module_accessor) == *FIGHTER_STATUS_KIND_CLIFF_WAIT
            && is_standby()
            && WorkModule::get_int(
                cpu_module_accessor,
                *FIGHTER_STATUS_CLIFF_WORK_INT_CATCH_REST_TIME,
            ) < 50
        {
            WorkModule::set_int(
                cpu_module_accessor,
                200,
                *FIGHTER_STATUS_CLIFF_WORK_INT_CATCH_REST_TIME,
            );
        }

        let mut input_record_frame = lock_write(&INPUT_RECORD_FRAME);
        if crate::training::named_macros::capture_active()
            && !crate::training::named_macros::has_captured_frame(*input_record_frame)
        {
            // The first FIM after lockout supplies frame0. Do not skip it by
            // advancing the CPU clock before that input actually exists.
            neutral_controls(&mut *controller_data);
            return;
        }
        // Named capture and playback read their own buffer. The frame counter
        // below is the only advance; the panel clock is not fed from here.
        let mut saved_mapped_inputs = if crate::training::named_macros::using_frames() {
            crate::training::named_macros::frame_at(*input_record_frame)
        } else {
            let slot = if input_record == Record {
                read(&CURRENT_RECORD_SLOT)
            } else {
                read(&CURRENT_PLAYBACK_SLOT)
            };
            let p1_final_mapping = lock_write(&P1_FINAL_MAPPING);
            p1_final_mapping[slot][*input_record_frame]
        };
        let mut buffer_frame = lock_write(&BUFFER_FRAME);
        if (0 < *buffer_frame) && (*buffer_frame <= 3) {
            // Our option is already buffered, now we need to 0 out inputs to make sure our future controls act like flicks/presses instead of holding the button
            saved_mapped_inputs = MappedInputs::empty();
        }

        (*controller_data).buttons = saved_mapped_inputs.buttons;
        // Native FIM/getter calibration uses 120 controller units per raw unit.
        // Keep the historical numbered-slot conversion outside named recordings.
        let raw_scale = if crate::training::named_macros::using_frames() {
            STICK_CLAMP_MULTIPLIER
        } else { 1.0 / i8::MAX as f32 };
        (*controller_data).stick_x =
            x_input_multiplier * saved_mapped_inputs.lstick_x as f32 * raw_scale;
        (*controller_data).stick_y = saved_mapped_inputs.lstick_y as f32 * raw_scale;
        // Clamp stick inputs for separate part of structure
        let mut clamped_lstick_x = x_input_multiplier
            * ((saved_mapped_inputs.lstick_x as f32) * STICK_CLAMP_MULTIPLIER).clamp(-1.0, 1.0);
        let mut clamped_lstick_y =
            ((saved_mapped_inputs.lstick_y as f32) * STICK_CLAMP_MULTIPLIER).clamp(-1.0, 1.0);
        clamped_lstick_x = if clamped_lstick_x.abs() >= STICK_NEUTRAL {
            clamped_lstick_x
        } else {
            0.0
        };
        clamped_lstick_y = if clamped_lstick_y.abs() >= STICK_NEUTRAL {
            clamped_lstick_y
        } else {
            0.0
        };
        (*controller_data).clamped_lstick_x = clamped_lstick_x;
        (*controller_data).clamped_lstick_y = clamped_lstick_y;

        // Native 13.0.5 does not consume MappedInputs' trailing two bytes as a
        // second stick. The upstream "clamped_rstick" fields are rumble state;
        // assigning the portable right-stick samples there would corrupt it.
        #[cfg(feature = "named-input-trace")]
        if input_record == Playback && crate::training::named_macros::using_frames() {
            crate::training::named_macros::trace_write(
                *input_record_frame, &saved_mapped_inputs, *buffer_frame > 0, x_input_multiplier,
            );
        }

        // Keep counting frames, unless we're in standby waiting for an input, or are buffering an option
        // When buffering an option, we keep inputting the first frame of input during the buffer window
        if *buffer_frame > 0 {
            *buffer_frame -= 1;
        } else if *input_record_frame < read(&CURRENT_FRAME_LENGTH)
            - usize::from(!crate::training::named_macros::using_frames())
            && read(&POSSESSION) != Standby
        {
            *input_record_frame += 1;
        }
        let consumed = *input_record_frame;
        drop(buffer_frame);
        drop(input_record_frame);
        crate::replay_preview::progress(consumed);
    }
}

pub fn is_playback() -> bool {
    let input_record = read(&INPUT_RECORD);
    input_record == Record || input_record == Playback
}

pub fn is_recording() -> bool {
    read(&INPUT_RECORD) == Record
}

pub unsafe fn is_standby() -> bool {
    let possession = read(&POSSESSION);
    possession == Standby || possession == Lockout
}

extern "C" {
    // TODO: we should be using this from skyline
    #[link_name = "\u{1}_ZN3app8lua_bind31ModelModule__set_color_rgb_implEPNS_26BattleObjectModuleAccessorEfffNS_16MODEL_COLOR_TYPEE"]
    pub fn set_color_rgb_2(
        arg1: *mut BattleObjectModuleAccessor,
        arg2: f32,
        arg3: f32,
        arg4: f32,
        arg5: i32,
    );
}

pub fn init() {
    skyline::install_hooks!(set_cpu_controls);
}
