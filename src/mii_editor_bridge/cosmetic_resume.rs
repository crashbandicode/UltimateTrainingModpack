//! Unfinished Headgear, Outfit, Color, Voice, and Random.
//!
//! Random never leaves Settings: it is selector area 1. The child pages are
//! editor states 5, 6, 7, and 9. Voice's native entry also pushes state 11;
//! that preview is not the Test preview unless the stack slot under it is the
//! Test state. Snapshots store the cursor and value fields the page handlers
//! already read. They do not set a commit flag and do not write the draft.
use super::{lua_vm::{Error, Value, Vm}, native, BASE};
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

const TAG: u64 = 0x5a00000000000000;
const TAG_MASK: u64 = 0xff00000000000000;
const ID_MASK: u64 = 0xffffffffff;
/// Bridge bound, not a native hat-list length. A larger index is abandoned.
const GEAR_CURSOR_MAX: u32 = 4095;
/// Native color browse rejects an index above 11 (`cmp #0xb`).
const COLOR_MAX: u32 = 11;
/// Bridge bounds. The voice script's `NUM_VOICE_TYPE` is not a fixed immediate.
const VOICE_CURSOR_MAX: i32 = 128;
const VOICE_AXIS_MAX: i32 = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GearSelection {
    pub cursor: u32,
    pub live: u64,
    pub paired: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ColorSelection {
    pub cursor: u32,
    pub paired: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct VoiceSelection {
    pub cursor: i32,
    pub voice_type: i32,
    pub voice_pitch: i32,
}

fn tagged(value: u64) -> bool {
    value & TAG_MASK == TAG && value & ID_MASK != 0
}

pub(super) fn gear(cursor: u32, live: u64, paired: u64) -> Option<GearSelection> {
    (cursor <= GEAR_CURSOR_MAX && tagged(live) && tagged(paired))
        .then_some(GearSelection { cursor, live, paired })
}

pub(super) fn color(cursor: u32, paired: u32) -> Option<ColorSelection> {
    (cursor <= COLOR_MAX).then_some(ColorSelection { cursor, paired })
}

pub(super) fn voice(cursor: i32, voice_type: i32, voice_pitch: i32) -> Option<VoiceSelection> {
    ((0..=VOICE_CURSOR_MAX).contains(&cursor)
        && (0..=VOICE_AXIS_MAX).contains(&voice_type)
        && (0..=VOICE_AXIS_MAX).contains(&voice_pitch))
        .then_some(VoiceSelection { cursor, voice_type, voice_pitch })
}

/// Component offsets written back for an unfinished page. Commit gates and the
/// draft stores (`+0x140` gear id, color `+0xd8`, voice flag `+0x100`) are absent.
/// Color `+0xe0` is captured with the index and not written: this component
/// family also uses `+0xe0` as its script-running word.
pub(super) const GEAR_COMPONENT_WRITES: [usize; 3] = [0x148, 0x150, 0x164];
pub(super) const COLOR_COMPONENT_WRITES: [usize; 1] = [0xdc];
pub(super) const CONTEXT_CURSOR: usize = 0x1c8;
#[cfg(test)]
const COMMIT_OFFSETS: [usize; 5] = [0xb8, 0xe8, 0x100, 0x140, 0x168];

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

pub(super) fn clear() {
    *WAIT.lock().unwrap() = None;
    *VOICE.lock().unwrap() = None;
}

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
    // `+0xe8` set and `+0x168` set is the native decide/commit pair.
    if native::read::<u32>(component, 0xe8) != 0 || native::read::<u8>(component, 0x168) != 0 {
        return Shot::Wait;
    }
    match gear(
        native::read(component, GEAR_COMPONENT_WRITES[2]),
        native::read(component, GEAR_COMPONENT_WRITES[0]),
        native::read(component, GEAR_COMPONENT_WRITES[1]),
    ) {
        Some(selection) => { clear(); Shot::Found(selection) }
        None => abandon_or_wait(context, state, 4),
    }
}

pub(super) unsafe fn capture_color(context: usize, state: i32) -> Shot<ColorSelection> {
    let Some(component) = pointer(context, HOLD_COLOR) else {
        return abandon_or_wait(context, state, 2);
    };
    if native::read::<u32>(context, 0x14c) < 1 {
        return abandon_or_wait(context, state, 3);
    }
    if native::read::<u32>(component, 0xb8) != 0 || native::read::<u8>(component, 0xe8) != 0 {
        return Shot::Wait;
    }
    match color(native::read(component, COLOR_COMPONENT_WRITES[0]), native::read(component, 0xe0)) {
        Some(selection) => { clear(); Shot::Found(selection) }
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
        Ok(selection) => { clear(); Shot::Found(selection) }
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
    let Some(component) = pointer(context, holder) else { return; };
    if native::read::<u32>(context, 0x14c) < 1
        || native::read::<u32>(component, 0xe8) != 0
        || native::read::<u8>(component, 0x168) != 0 { return; }
    native::write(component, GEAR_COMPONENT_WRITES[0], wanted.live);
    native::write(component, GEAR_COMPONENT_WRITES[1], wanted.paired);
    native::write(component, GEAR_COMPONENT_WRITES[2], wanted.cursor);
    native::write(context, CONTEXT_CURSOR, wanted.cursor);
}

unsafe fn apply_color(context: usize, wanted: ColorSelection) {
    let Some(component) = pointer(context, HOLD_COLOR) else { return; };
    if native::read::<u32>(context, 0x14c) < 1
        || native::read::<u32>(component, 0xb8) != 0
        || native::read::<u8>(component, 0xe8) != 0 { return; }
    native::write(component, COLOR_COMPONENT_WRITES[0], wanted.cursor);
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
    match capture_gear(context, state) {
        Shot::Found(seen) if seen == wanted => Poll::Ready,
        Shot::Abandoned { detail, .. } => Poll::Failed(detail),
        Shot::Wait | Shot::Found(_) => Poll::Pending,
    }
}

pub(super) unsafe fn poll_color(context: usize, wanted: ColorSelection) -> Poll {
    if native::read::<i32>(context, 0x148) != 7 { return Poll::Pending; }
    match capture_color(context, 7) {
        // `paired` (+0xe0) is recorded and not written back.
        Shot::Found(seen) if seen.cursor == wanted.cursor => Poll::Ready,
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
        let live = TAG | 0x11;
        let paired = TAG | 0x22;
        assert_eq!(gear(4, live, paired), Some(GearSelection { cursor: 4, live, paired }));
        assert_eq!(gear(4, 0x11, paired), None, "untagged id is not a hat or outfit");
        assert_eq!(gear(GEAR_CURSOR_MAX + 1, live, paired), None);
        assert_eq!(color(11, 0), Some(ColorSelection { cursor: 11, paired: 0 }));
        assert_eq!(color(3, 99), Some(ColorSelection { cursor: 3, paired: 99 }));
        assert_eq!(color(12, 0), None);
        assert_eq!(voice(3, 1, 2), Some(VoiceSelection { cursor: 3, voice_type: 1, voice_pitch: 2 }));
        assert_eq!(voice(-1, 1, 2), None);
    }

    #[test]
    fn restore_does_not_write_commit_fields() {
        let gear_writes = GEAR_COMPONENT_WRITES.iter().copied().chain([CONTEXT_CURSOR]);
        let color_writes = COLOR_COMPONENT_WRITES.iter().copied();
        assert!(COMMIT_OFFSETS.iter().all(|offset| !gear_writes.clone().any(|write| write == *offset)));
        assert!(COMMIT_OFFSETS.iter().all(|offset| !color_writes.clone().any(|write| write == *offset)));
    }

    #[test]
    fn random_is_not_a_child_state() {
        assert_eq!(leaf(3, None), Leaf::Settings);
        assert_ne!(leaf(3, None), Leaf::Headgear);
    }

    /// Listing `set_voice_type`: a requested None pair writes pitch selectable
    /// false, then an old None pair writes it true. Equal None therefore ends
    /// selectable. The helper's last write is the opposite of that finished pair.
    fn setter_leaves_pitch_selectable(old_none: bool, requested_none: bool) -> bool {
        let mut selectable = true;
        if requested_none { selectable = false; }
        if old_none { selectable = true; }
        selectable
    }

    #[test]
    fn voice_none_pitch_stays_disabled_when_the_old_pair_was_none() {
        assert!(setter_leaves_pitch_selectable(true, true), "equal None re-enables pitch");
        assert!(!setter_leaves_pitch_selectable(false, true));
        assert!(setter_leaves_pitch_selectable(true, false));
        assert!(setter_leaves_pitch_selectable(false, false), "neither branch writes");
        let source = include_str!("lua_helpers/voice_focus.lua");
        let type_at = source.find("page:set_voice_type(").expect("type transition");
        let pitch_at = source.find("page:set_voice_pitch(").expect("pitch labels");
        let selectable_at = source.find("page.selector:set_selectable(1,").expect("pitch id 1");
        assert!(type_at < pitch_at && pitch_at < selectable_at);
        assert!(!source.contains("page.voice_type ="));
        assert!(!source.contains("page.voice_pitch ="));
        assert!(source.contains("none ~= true"));
        assert!(!source.contains("set_selectable(1, true)"));
        assert!(!source.contains("set_selectable(1, false)"));
    }
}
