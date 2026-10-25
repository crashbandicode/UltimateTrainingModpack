//! Unfinished Headgear, Outfit, Color, Voice, and Random.
//!
//! Random never leaves Settings: it is selector area 1. The child pages are
//! editor states 5, 6, 7, and 9. Voice's native entry also pushes state 11;
//! that preview is not the Test preview unless the stack slot under it is the
//! Test state. Voice stores fields its handler already reads. Headgear and
//! Outfit restore `sort_id` before the scroller item. The tagged preview id is
//! read back and is not written. Color calls the selector at ELF `0x376d990`
//! and does not write the component highlight. Nothing here sets a commit
//! flag or the live draft. A checkpoint copy may put the equipped id back
//! into the hat or outfit field when that field still holds the preview.
use super::{lua_vm::{Error, Value, Vm}, native, BASE};
use std::convert::TryFrom;
use std::sync::{atomic::Ordering, Mutex};

/// Editor state 11 is the shared preview wait. Test pushes 10 then 11. Voice
/// pushes 9 then 11. Any other occupant keeps the previous Test treatment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Leaf {
    Settings,
    Name,
    Headgear,
    Outfit,
    Color,
    Specials,
    Voice,
    Test,
    Other(i32),
}

pub(super) fn leaf(state: i32, previous: Option<i32>) -> Leaf {
    match state {
        3 => Leaf::Settings,
        4 => Leaf::Name,
        5 => Leaf::Headgear,
        6 => Leaf::Outfit,
        7 => Leaf::Color,
        8 => Leaf::Specials,
        9 => Leaf::Voice,
        10 => Leaf::Test,
        11 if previous == Some(9) => Leaf::Voice,
        11 => Leaf::Test,
        other => Leaf::Other(other),
    }
}

pub(super) unsafe fn previous_state(context: usize) -> Option<i32> {
    let level: i32 = native::read(context, 0x144);
    if !(1..19).contains(&level) {
        return None;
    }
    Some(native::read(context, 0xf8 + (level as usize - 1) * 4))
}

const HAT_TAG: u64 = 0x5a00000000000000;
const HAT_TAG_MASK: u64 = 0xff00000000000000;
/// Outfit ids in the open Pirate Outfit capture are `0x1300` in the top 16 bits.
const OUTFIT_TAG: u64 = 0x1300000000000000;
const OUTFIT_TAG_MASK: u64 = 0xffff000000000000;
const ID_MASK: u64 = 0xffffffffff;
/// Bridge bound, not a native hat-list length. A larger index is abandoned.
const GEAR_CURSOR_MAX: u32 = 4095;
/// Bridge bound. Captured pages were `sort_id` 0. `set_sort_id` forwards the
/// integer to `sort_id_list` without a visible clamp.
const SORT_MAX: u32 = 255;
/// Native color browse rejects an index above 11 (`cmp #0xb`).
const COLOR_MAX: u32 = 11;
/// Bridge bounds. The voice script's `NUM_VOICE_TYPE` is not a fixed immediate.
const VOICE_CURSOR_MAX: i32 = 128;
const VOICE_AXIS_MAX: i32 = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GearSelection {
    pub cursor: u32,
    pub sort: u32,
    pub live: u64,
    pub paired: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ColorSelection {
    pub cursor: u32,
    /// Equipped index at component `+0xdc`. Not written.
    pub paired: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct VoiceSelection {
    pub cursor: i32,
    pub voice_type: i32,
    pub voice_pitch: i32,
}

fn tagged(value: u64) -> bool {
    let hat = value & HAT_TAG_MASK == HAT_TAG;
    let outfit = value & OUTFIT_TAG_MASK == OUTFIT_TAG;
    (hat || outfit) && value & ID_MASK != 0
}

pub(super) fn gear(cursor: u32, sort: u32, live: u64, paired: u64) -> Option<GearSelection> {
    (cursor <= GEAR_CURSOR_MAX && sort <= SORT_MAX && tagged(live) && tagged(paired))
        .then_some(GearSelection { cursor, sort, live, paired })
}

/// `source_highlight` is selector `+0x254`. A component highlight whose source
/// index differs is a torn read and is not a selection.
pub(super) fn color(cursor: u32, paired: u32, source_highlight: u32, source_decided: u32) -> Option<ColorSelection> {
    (cursor <= COLOR_MAX && paired <= COLOR_MAX && source_highlight == cursor
        && source_decided == u32::MAX)
        .then_some(ColorSelection { cursor, paired })
}

pub(super) fn voice(cursor: i32, voice_type: i32, voice_pitch: i32) -> Option<VoiceSelection> {
    ((0..=VOICE_CURSOR_MAX).contains(&cursor)
        && (0..=VOICE_AXIS_MAX).contains(&voice_type)
        && (0..=VOICE_AXIS_MAX).contains(&voice_pitch))
        .then_some(VoiceSelection { cursor, voice_type, voice_pitch })
}

// Gear restore writes no component word. `+0x140` is the preview id. ELF
// `0x19cc3f0` compares it with the already indexed row and can store decide
// byte `+0x168` on the mismatch side.
/// Component highlight. ELF `0x19c90bc` loads it. When selector `+0x25c` is
/// `-1`, a difference from selector `+0x254` is copied here at `0x19c90cc`
/// and byte `+0xe8` is set. That byte is the highlight-dirty flag. `+0xdc`
/// is the equipped index, copied from `+0x25c` only when that word is not `-1`.
const COLOR_HIGHLIGHT: usize = 0xd8;
const COLOR_EQUIPPED: usize = 0xdc;
const COLOR_SOURCE: usize = 0xd0;
const COLOR_SOURCE_HIGHLIGHT: usize = 0x254;
const COLOR_SOURCE_DECIDED: usize = 0x25c;
/// `selector.select(index, 0)`. `x0` is the object at component `+0xd0`.
/// The call updates the button and stores the index at `+0x250` and `+0x254`.
/// Its body does not store the decided index at `+0x25c`. The component's
/// normal update copies the new highlight and refreshes the preview afterward.
const COLOR_SELECT: usize = 0x376d990;

/// What an open gear component is doing. `running` is the u32 at `+0xe8`.
/// `commit` is the byte at `+0x168`. The preview id is the u64 at `+0x140`;
/// the equipped id is the u64 at `+0x150`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GearWindow {
    /// Page is open and the preview id is a real tagged selection.
    Open(GearSelection),
    /// Decide byte is set. Let that native completion finish.
    Deciding,
    /// Running word is clear. The mirrors have already been dropped.
    Closed,
    /// Running word is neither the open value nor clear.
    Pending,
    /// Page is open, but the preview or equipped id is not a tagged selection.
    Unreadable,
}

/// Ready requires the helper to have run, then the same sort, the same scroller
/// index, and the same tagged id. The id alone is not the row. The index alone
/// is not the item once the sort can change.
pub(super) fn gear_row_ready(wrote: bool, seen: GearSelection, wanted: GearSelection) -> bool {
    wrote
        && seen.sort == wanted.sort
        && seen.cursor == wanted.cursor
        && seen.live == wanted.live
        && seen.paired == wanted.paired
}

/// Saved-row outfit is `+0x130` and hat is `+0x138`. ELF `0x19df080` stores
/// those fields on the packed row. The open page keeps a different equipped
/// id at component `+0x150`. This edits only the checkpoint copy: replace the
/// field when it is exactly the preview id. Any other value, including the
/// equipped id itself, stays. The live draft is not written.
pub(super) fn keep_equipped_part(row: &mut [u8], hat: bool, gear: GearSelection) {
    let offset = if hat { 0x138 } else { 0x130 };
    let Some(field) = row.get(offset..offset + 8) else { return; };
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(field);
    if u64::from_le_bytes(bytes) == gear.live && gear.live != gear.paired {
        row[offset..offset + 8].copy_from_slice(&gear.paired.to_le_bytes());
    }
}

pub(super) fn gear_window(running: u32, commit: u8, preview: u64, equipped: u64) -> GearWindow {
    if commit != 0 { return GearWindow::Deciding; }
    if running == 0 { return GearWindow::Closed; }
    if running != 1 { return GearWindow::Pending; }
    // The scroller index and sort are not known here. Capture fills both.
    match gear(0, 0, preview, equipped) {
        Some(selection) => GearWindow::Open(selection),
        None => GearWindow::Unreadable,
    }
}

pub(super) enum Shot<T> {
    Wait,
    Found(T),
    Abandoned { state: i32, detail: u64 },
}

const HOLD_HEADGEAR: usize = 0x180;
const HOLD_OUTFIT: usize = 0x188;
const HOLD_COLOR: usize = 0x190;
const HOLD_VOICE: usize = 0x1a0;
const ATTEMPTS: u16 = 120;

struct Wait { context: usize, state: i32, frames: u16 }
static WAIT: Mutex<Option<Wait>> = Mutex::new(None);
struct VoiceRestore { context: usize, wanted: VoiceSelection, wrote: bool, failed: Option<u64> }
static VOICE: Mutex<Option<VoiceRestore>> = Mutex::new(None);
struct GearRestore { context: usize, wanted: GearSelection, wrote: bool, failed: Option<u64> }
static GEAR: Mutex<Option<GearRestore>> = Mutex::new(None);
struct ColorRestore { context: usize, wanted: ColorSelection, called: bool }
static COLOR: Mutex<Option<ColorRestore>> = Mutex::new(None);

pub(super) fn clear() {
    clear_wait();
    *VOICE.lock().unwrap() = None;
    *GEAR.lock().unwrap() = None;
    *COLOR.lock().unwrap() = None;
}

// Capture also serves restore polling. A successful observation must not retire
// a setter already issued while its native preview catches up on the next frame.
fn clear_wait() { *WAIT.lock().unwrap() = None; }

fn attempts(context: usize, state: i32) -> u16 {
    let mut slot = WAIT.lock().unwrap();
    if slot.as_ref().is_none_or(|wait| wait.context != context || wait.state != state) {
        *slot = Some(Wait { context, state, frames: 0 });
    }
    let wait = slot.as_mut().unwrap();
    wait.frames = wait.frames.saturating_add(1);
    wait.frames
}

unsafe fn pointer(context: usize, holder: usize) -> Option<usize> {
    let holder: usize = native::read(context, holder);
    if !native::aligned(holder) { return None; }
    let component: usize = native::read(holder, 0x80);
    native::aligned(component).then_some(component)
}

fn abandon_or_wait<T>(context: usize, state: i32, detail: u64) -> Shot<T> {
    // State 11 is released by the preview-wait pop before this capture runs.
    // A page that is still unread after the same bound as the other leaves
    // falls back instead of holding matchmaking open.
    if attempts(context, state) <= ATTEMPTS {
        Shot::Wait
    } else {
        Shot::Abandoned { state, detail }
    }
}

pub(super) unsafe fn capture_gear(context: usize, state: i32) -> Shot<GearSelection> {
    let holder = match state {
        5 => HOLD_HEADGEAR,
        6 => HOLD_OUTFIT,
        _ => return Shot::Abandoned { state, detail: 1 },
    };
    let Some(component) = pointer(context, holder) else {
        return abandon_or_wait(context, state, 2);
    };
    if native::read::<u32>(context, 0x14c) < 1 {
        return abandon_or_wait(context, state, 3);
    }
    let window = gear_window(
        native::read(component, 0xe8),
        native::read(component, 0x168),
        native::read(component, 0x140),
        native::read(component, 0x150),
    );
    match window {
        GearWindow::Open(mut selection) => match read_gear_row(component) {
            Ok(row) => {
                selection.cursor = row.item;
                selection.sort = row.sort;
                clear_wait();
                Shot::Found(selection)
            }
            Err(Error::Binding | Error::Stack | Error::Identity) => abandon_or_wait(context, state, 3),
            Err(error) => Shot::Abandoned { state, detail: 0x100 + error.code() as u64 },
        },
        GearWindow::Unreadable => abandon_or_wait(context, state, 4),
        GearWindow::Deciding => abandon_or_wait(context, state, 5),
        GearWindow::Closed => abandon_or_wait(context, state, 6),
        GearWindow::Pending => abandon_or_wait(context, state, 7),
    }
}

pub(super) fn gear_item(value: i32) -> Option<u32> {
    u32::try_from(value).ok().filter(|item| *item <= GEAR_CURSOR_MAX)
}

struct GearRow { sort: u32, item: u32 }

unsafe fn read_gear_row(component: usize) -> Result<GearRow, Error> {
    let mut vm = Vm::bind_gear(BASE.load(Ordering::Acquire), component)?;
    let page = vm.gear_page(component)?;
    let [done, sort, item] = vm.call::<3>(include_bytes!("lua_helpers/gear_capture.luac"), &[page])?;
    if done.int() != Some(0) { return Err(Error::Value); }
    let sort = u32::try_from(sort.int().ok_or(Error::Value)?).map_err(|_| Error::Value)?;
    if sort > SORT_MAX { return Err(Error::Value); }
    let item = gear_item(item.int().ok_or(Error::Value)?).ok_or(Error::Value)?;
    Ok(GearRow { sort, item })
}

pub(super) unsafe fn capture_color(context: usize, state: i32) -> Shot<ColorSelection> {
    let Some(component) = pointer(context, HOLD_COLOR) else {
        return abandon_or_wait(context, state, 2);
    };
    if native::read::<u32>(context, 0x14c) < 1 {
        return abandon_or_wait(context, state, 3);
    }
    let Some(source) = color_source(component) else {
        return abandon_or_wait(context, state, 4);
    };
    // Byte `+0xe8` is highlight-dirty, set when `0x19c9020` copies `+0x254`
    // into `+0xd8`. It is not a commit, and `+0xb8` stays 2 on an open page.
    match color(
        native::read(component, COLOR_HIGHLIGHT),
        native::read(component, COLOR_EQUIPPED),
        native::read(source, COLOR_SOURCE_HIGHLIGHT),
        native::read(source, COLOR_SOURCE_DECIDED),
    ) {
        Some(selection) => { clear_wait(); Shot::Found(selection) }
        None => abandon_or_wait(context, state, 4),
    }
}

unsafe fn read_voice(component: usize) -> Result<VoiceSelection, Error> {
    let mut vm = Vm::bind(BASE.load(Ordering::Acquire), component)?;
    let page = vm.voice_page()?;
    let [done, cursor, voice_type, voice_pitch] = vm.call::<4>(
        include_bytes!("lua_helpers/voice_capture.luac"), &[page])?;
    if done.int() != Some(0) { return Err(Error::Value); }
    voice(cursor.int().ok_or(Error::Value)?, voice_type.int().ok_or(Error::Value)?,
        voice_pitch.int().ok_or(Error::Value)?).ok_or(Error::Value)
}

pub(super) unsafe fn capture_voice(context: usize, state: i32) -> Shot<VoiceSelection> {
    let Some(component) = pointer(context, HOLD_VOICE) else {
        return abandon_or_wait(context, state, 2);
    };
    if native::read::<i32>(component, 0xe0) != 1 {
        return abandon_or_wait(context, state, 3);
    }
    if native::read::<u8>(component, 0x100) != 0 {
        return Shot::Wait;
    }
    match read_voice(component) {
        Ok(selection) => { clear_wait(); Shot::Found(selection) }
        Err(Error::Binding) => abandon_or_wait(context, state, 3),
        Err(error) => Shot::Abandoned { state, detail: 0x100 + error.code() as u64 },
    }
}

pub(super) unsafe fn prepare(context: usize, screen: &super::ui_resume::Screen) {
    match screen {
        super::ui_resume::Screen::Headgear(wanted) if native::read::<i32>(context, 0x148) == 5 => {
            apply_gear(context, HOLD_HEADGEAR, *wanted);
        }
        super::ui_resume::Screen::Outfit(wanted) if native::read::<i32>(context, 0x148) == 6 => {
            apply_gear(context, HOLD_OUTFIT, *wanted);
        }
        super::ui_resume::Screen::Color(wanted) if native::read::<i32>(context, 0x148) == 7 => {
            apply_color(context, *wanted);
        }
        super::ui_resume::Screen::Voice(wanted) => prepare_voice(context, *wanted),
        _ => {}
    }
}

unsafe fn apply_gear(context: usize, holder: usize, wanted: GearSelection) {
    let failed = {
        let mut job = GEAR.lock().unwrap();
        if job.as_ref().is_none_or(|job| job.context != context || job.wanted != wanted) {
            *job = Some(GearRestore { context, wanted, wrote: false, failed: None });
        }
        let job = job.as_mut().unwrap();
        job.wrote || job.failed.is_some()
    };
    if failed { return; }
    let Some(component) = pointer(context, holder) else { return; };
    let result: Result<(), Error> = (|| {
        let mut vm = Vm::bind_gear(BASE.load(Ordering::Acquire), component)?;
        let page = vm.gear_page(component)?;
        let sort = i32::try_from(wanted.sort).map_err(|_| Error::Value)?;
        let item = i32::try_from(wanted.cursor).map_err(|_| Error::Value)?;
        vm.call::<0>(include_bytes!("lua_helpers/gear_focus.luac"), &[
            page, Value::integer(sort), Value::integer(item),
        ])?;
        Ok(())
    })();
    if let Some(job) = GEAR.lock().unwrap().as_mut() {
        match result {
            Ok(()) => job.wrote = true,
            Err(Error::Binding) => {}
            Err(error) => job.failed = Some(error.code() as u64),
        }
    }
}

unsafe fn color_source(component: usize) -> Option<usize> {
    // Native Color state 2 is the interactive page. Entry/exit use the same
    // allocation, but their selector is not eligible for a restore operation.
    if native::read::<i32>(component, 0xb8) != 2 { return None; }
    let source: usize = native::read(component, COLOR_SOURCE);
    if !native::aligned(source) { return None; }
    let vtable: usize = native::read(source, 0);
    native::aligned(vtable).then_some(source)
}

unsafe fn apply_color(context: usize, wanted: ColorSelection) {
    let called = {
        let mut job = COLOR.lock().unwrap();
        if job.as_ref().is_none_or(|job| job.context != context || job.wanted != wanted) {
            *job = Some(ColorRestore { context, wanted, called: false });
        }
        job.as_ref().unwrap().called
    };
    if called { return; }
    let Some(component) = pointer(context, HOLD_COLOR) else { return; };
    if native::read::<u32>(context, 0x14c) < 1 { return; }
    let Some(source) = color_source(component) else { return; };
    if native::read::<u32>(source, COLOR_SOURCE_DECIDED) != u32::MAX { return; }
    let select: unsafe extern "C" fn(usize, u32, u32) =
        std::mem::transmute(BASE.load(Ordering::Acquire) + COLOR_SELECT);
    select(source, wanted.cursor, 0);
    let published = native::read::<u32>(source, COLOR_SOURCE_HIGHLIGHT) == wanted.cursor
        && native::read::<u32>(source, COLOR_SOURCE_DECIDED) == u32::MAX;
    if !published { return; }
    if let Some(job) = COLOR.lock().unwrap().as_mut() {
        if job.context == context && job.wanted == wanted { job.called = true; }
    }
}

unsafe fn prepare_voice(context: usize, wanted: VoiceSelection) {
    let state: i32 = native::read(context, 0x148);
    let on_page = state == 9 || (state == 11 && previous_state(context) == Some(9));
    if !on_page { return; }
    let failed = {
        let mut job = VOICE.lock().unwrap();
        if job.as_ref().is_none_or(|job| job.context != context || job.wanted != wanted) {
            *job = Some(VoiceRestore { context, wanted, wrote: false, failed: None });
        }
        let job = job.as_mut().unwrap();
        job.wrote || job.failed.is_some()
    };
    if failed { return; }
    let Some(component) = pointer(context, HOLD_VOICE) else { return; };
    if native::read::<i32>(component, 0xe0) != 1 { return; }
    if native::read::<u8>(component, 0x100) != 0 { return; }
    let result: Result<(), Error> = (|| {
        let mut vm = Vm::bind(BASE.load(Ordering::Acquire), component)?;
        let page = vm.voice_page()?;
        vm.call::<0>(include_bytes!("lua_helpers/voice_focus.luac"), &[
            page, Value::integer(wanted.cursor), Value::integer(wanted.voice_type),
            Value::integer(wanted.voice_pitch),
        ])?;
        Ok(())
    })();
    if let Some(job) = VOICE.lock().unwrap().as_mut() {
        match result {
            Ok(()) => job.wrote = true,
            Err(Error::Binding) => {}
            Err(error) => job.failed = Some(error.code() as u64),
        }
    }
}

pub(super) enum Poll { Pending, Ready, Failed(u64) }

pub(super) unsafe fn poll_gear(context: usize, state: i32, wanted: GearSelection) -> Poll {
    if native::read::<i32>(context, 0x148) != state { return Poll::Pending; }
    let (failed, wrote) = GEAR.lock().unwrap().as_ref()
        .map(|job| (job.failed, job.wrote)).unwrap_or((None, false));
    if let Some(detail) = failed { return Poll::Failed(detail); }
    if !wrote { return Poll::Pending; }
    match capture_gear(context, state) {
        Shot::Found(seen) if gear_row_ready(wrote, seen, wanted) => Poll::Ready,
        Shot::Abandoned { detail, .. } => Poll::Failed(detail),
        _ => Poll::Pending,
    }
}

pub(super) unsafe fn poll_color(context: usize, wanted: ColorSelection) -> Poll {
    if native::read::<i32>(context, 0x148) != 7 { return Poll::Pending; }
    if COLOR.lock().unwrap().as_ref().is_none_or(|job|
        job.context != context || job.wanted != wanted || !job.called) { return Poll::Pending; }
    match capture_color(context, 7) {
        Shot::Found(seen) if seen == wanted => Poll::Ready,
        Shot::Abandoned { detail, .. } => Poll::Failed(detail),
        _ => Poll::Pending,
    }
}

pub(super) unsafe fn poll_voice(context: usize, wanted: VoiceSelection) -> Poll {
    let (failed, wrote) = VOICE.lock().unwrap().as_ref()
        .map(|job| (job.failed, job.wrote)).unwrap_or((None, false));
    if let Some(detail) = failed { return Poll::Failed(detail); }
    // voice_focus reconciles pitch button 1 before wrote is set. Matching
    // cursor, type, and pitch is not enough: an equal None restore used to
    // acknowledge while that button was selectable.
    if !wrote { return Poll::Pending; }
    if native::read::<i32>(context, 0x148) != 9 { return Poll::Pending; }
    match capture_voice(context, 9) {
        Shot::Found(seen) if seen == wanted => Poll::Ready,
        Shot::Abandoned { detail, .. } => Poll::Failed(detail),
        _ => Poll::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_screens_and_test_preview_stay() {
        assert_eq!(leaf(3, None), Leaf::Settings);
        assert_eq!(leaf(4, None), Leaf::Name);
        assert_eq!(leaf(8, None), Leaf::Specials);
        assert_eq!(leaf(10, None), Leaf::Test);
        assert_eq!(leaf(11, Some(10)), Leaf::Test);
        assert_eq!(leaf(11, None), Leaf::Test);
        assert_eq!(leaf(11, Some(3)), Leaf::Test);
    }

    #[test]
    fn cosmetic_pages_are_their_own_states() {
        assert_eq!(leaf(5, Some(3)), Leaf::Headgear);
        assert_eq!(leaf(6, Some(3)), Leaf::Outfit);
        assert_eq!(leaf(7, Some(3)), Leaf::Color);
        assert_eq!(leaf(9, Some(3)), Leaf::Voice);
        assert_eq!(leaf(11, Some(9)), Leaf::Voice);
    }

    #[test]
    fn selections_reject_commit_shaped_or_untagged_values() {
        let live = HAT_TAG | 0x11;
        let paired = HAT_TAG | 0x22;
        assert_eq!(gear(4, 0, live, paired), Some(GearSelection { cursor: 4, sort: 0, live, paired }));
        assert_eq!(gear(4, 0, 0x11, paired), None, "untagged id is not a hat or outfit");
        let outfit_live = 0x13002b147bc59435;
        let outfit_paired = 0x1300291cce3b03f5;
        assert_eq!(gear(4, 0, outfit_live, outfit_paired).map(|gear| (gear.live, gear.paired)),
            Some((outfit_live, outfit_paired)));
        assert_eq!(gear(4, 0, 0x13ffff0000000000, outfit_paired), None, "outfit sentinel is not a row");
        assert_eq!(gear(GEAR_CURSOR_MAX + 1, 0, live, paired), None);
        assert_eq!(gear(4, SORT_MAX + 1, live, paired), None);
        assert_eq!(color(11, 0, 11, u32::MAX), Some(ColorSelection { cursor: 11, paired: 0 }));
        assert_eq!(color(3, 99, 3, u32::MAX), None);
        assert_eq!(color(3, 5, 3, 3), None, "native confirmation is already in progress");
        assert_eq!(color(12, 0, 12, u32::MAX), None);
        assert_eq!(color(7, 5, 6, u32::MAX), None, "highlight without its source index");
        assert_eq!(voice(3, 1, 2), Some(VoiceSelection { cursor: 3, voice_type: 1, voice_pitch: 2 }));
        assert_eq!(voice(-1, 1, 2), None);
        assert_eq!(gear_item(7), Some(7));
        assert_eq!(gear_item(0), Some(0));
        assert_eq!(gear_item(-1), None);
    }

    #[test]
    fn gear_ready_needs_the_sort_the_index_and_the_tagged_id() {
        let live = HAT_TAG | 0x11;
        let other = HAT_TAG | 0x33;
        let equipped = HAT_TAG | 0x22;
        let wanted = gear(4, 1, live, equipped).expect("wanted");
        assert!(!gear_row_ready(false, wanted, wanted));
        assert!(gear_row_ready(true, wanted, wanted));
        assert!(!gear_row_ready(true, gear(0, 1, live, equipped).expect("index"), wanted));
        assert!(!gear_row_ready(true, gear(4, 0, live, equipped).expect("sort"), wanted));
        assert!(!gear_row_ready(true, gear(4, 1, other, equipped).expect("id"), wanted));
        assert!(!gear_row_ready(true, gear(4, 1, live, HAT_TAG | 0x44).expect("equipped"), wanted));
    }

    #[test]
    fn open_headgear_keeps_the_preview_id_and_a_closed_page_does_not() {
        // Run109 component 0x11fdc49880 while 1-Up was highlighted and unconfirmed.
        // `+0x148`/`+0x150` were the equipped default. `+0x140` was the other tagged id.
        // `+0x164` was 0. `+0xe8` was 1 and byte `+0x168` was 0.
        const PREVIEW: u64 = 0x5a000114d6b13212;
        const EQUIPPED: u64 = 0x5a00000fa9efa5ea;
        match gear_window(1, 0, PREVIEW, EQUIPPED) {
            GearWindow::Open(selection) => {
                assert_eq!(selection.live, PREVIEW);
                assert_eq!(selection.paired, EQUIPPED);
                assert_ne!(selection.live, selection.paired);
                // Scroller index and sort are filled by the page read, not here.
                assert_eq!(selection.cursor, 0);
                assert_eq!(selection.sort, 0);
            }
            other => panic!("open page did not keep the preview: {:?}", other),
        }
        // Highlight still on the equipped hat: the preview id is that hat, not a cancel.
        match gear_window(1, 0, EQUIPPED, EQUIPPED) {
            GearWindow::Open(selection) => assert_eq!(selection.live, EQUIPPED),
            other => panic!("equipped highlight was dropped: {:?}", other),
        }
        assert_eq!(gear_window(1, 1, PREVIEW, EQUIPPED), GearWindow::Deciding);
        assert_eq!(gear_window(0, 0, 0, 0), GearWindow::Closed);
        assert_eq!(gear_window(2, 0, PREVIEW, EQUIPPED), GearWindow::Pending);
        assert_eq!(gear_window(1, 0, 0, EQUIPPED), GearWindow::Unreadable);
        assert_eq!(gear_window(1, 0, 0x5affff0000000000, EQUIPPED), GearWindow::Unreadable);
        // Pirate Outfit, unconfirmed. Top 16 bits are 0x1300, not the hat tag.
        const OUTFIT_PREVIEW: u64 = 0x13002b147bc59435;
        const OUTFIT_EQUIPPED: u64 = 0x1300291cce3b03f5;
        match gear_window(1, 0, OUTFIT_PREVIEW, OUTFIT_EQUIPPED) {
            GearWindow::Open(selection) => {
                assert_eq!(selection.live, OUTFIT_PREVIEW);
                assert_eq!(selection.paired, OUTFIT_EQUIPPED);
            }
            other => panic!("open outfit was dropped: {:?}", other),
        }
        assert_eq!(gear_window(1, 0, 0x13ffff0000000000, OUTFIT_EQUIPPED), GearWindow::Unreadable);
    }

    #[test]
    fn color_highlight_requires_the_same_source_index() {
        // Cyan was highlight 6 and pink was 7. Equipped blue stayed 5 at +0xdc.
        // Selector +0x25c stays the captured decided word. -1 is all bits set.
        let cyan = color(6, 5, 6, u32::MAX).expect("cyan");
        let pink = color(7, 5, 7, u32::MAX).expect("pink");
        assert_eq!(cyan.cursor, 6);
        assert_eq!(pink.cursor, 7);
        assert_eq!(cyan.paired, 5);
        assert_eq!(pink.paired, 5);
        assert_ne!(cyan.cursor, cyan.paired);
        assert_eq!(color(7, 5, 5, u32::MAX), None);
        assert_ne!(pink, color(7, 6, 7, u32::MAX).expect("equipped changed"));
        assert_eq!(color(7, 5, 7, 5), None, "decided source is not an unfinished highlight");
    }

    #[test]
    fn random_is_not_a_child_state() {
        assert_eq!(leaf(3, None), Leaf::Settings);
        assert_ne!(leaf(3, None), Leaf::Headgear);
    }

    #[test]
    fn checkpoint_keeps_the_equipped_id_when_the_packed_field_is_the_preview() {
        let preview = HAT_TAG | 0x111;
        let equipped = HAT_TAG | 0x222;
        let outfit = OUTFIT_TAG | 0x333;
        let selected_gear = gear(4, 0, preview, equipped).expect("row");
        let mut row = vec![0u8; 0x140];
        row[0x130..0x138].copy_from_slice(&outfit.to_le_bytes());
        row[0x138..0x140].copy_from_slice(&preview.to_le_bytes());
        keep_equipped_part(&mut row, true, selected_gear);
        let at = |row: &[u8], offset: usize| {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&row[offset..offset + 8]);
            u64::from_le_bytes(bytes)
        };
        assert_eq!(at(&row, 0x138), equipped);
        assert_eq!(at(&row, 0x130), outfit);
        keep_equipped_part(&mut row, true, selected_gear);
        assert_eq!(at(&row, 0x138), equipped);
        row[0x138..0x140].copy_from_slice(&(HAT_TAG | 0x444).to_le_bytes());
        keep_equipped_part(&mut row, true, selected_gear);
        assert_eq!(at(&row, 0x138), HAT_TAG | 0x444);
        let outfit_gear = gear(2, 0, outfit, OUTFIT_TAG | 0x555).expect("outfit");
        row[0x130..0x138].copy_from_slice(&outfit.to_le_bytes());
        keep_equipped_part(&mut row, false, outfit_gear);
        assert_eq!(at(&row, 0x130), OUTFIT_TAG | 0x555);
        assert_eq!(at(&row, 0x138), HAT_TAG | 0x444);
        let same = gear(1, 0, equipped, equipped).expect("same");
        row[0x138..0x140].copy_from_slice(&equipped.to_le_bytes());
        keep_equipped_part(&mut row, true, same);
        assert_eq!(at(&row, 0x138), equipped);
        keep_equipped_part(&mut [], true, selected_gear);
    }

}
