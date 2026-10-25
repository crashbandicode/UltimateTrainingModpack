//! Native entry before the Settings coroutine starts. Pushing from a running
//! Settings screen would leave its input registered beneath the reopened page.
use super::{native, ui_resume::Screen};

pub(super) enum Entry { Wait, Entered, Refused }

pub(super) unsafe fn enter(context: usize, screen: Screen) -> Entry {
    if native::read::<i32>(context, 0x148) != 0 { return Entry::Refused; }
    let level: i32 = native::read(context, 0x144);
    if !(0..=16).contains(&level) { return Entry::Refused; }
    let top: i32 = native::read(context, 0xf8 + level as usize * 4);
    if top == 0 { return Entry::Wait; }
    // An unlock dialog keeps its native priority. No unqualified rearrangement
    // of that stack is attempted; the checkpoint remains available.
    if top != 3 { return Entry::Refused; }
    let main_window: usize = native::read(context, 0x178);
    if !native::aligned(main_window) || native::read::<u8>(main_window, 0x70) != 1
        || native::read::<i32>(context, 0x168) != 0 { return Entry::Refused; }
    let target = match screen {
        Screen::Settings(_) => return Entry::Entered,
        Screen::Name(text) => {
            if !super::name_keyboard::prepare_resume(context, text) { return Entry::Refused; }
            4
        }
        Screen::Headgear(_) => 5,
        Screen::Outfit(_) => 6,
        Screen::Color(_) => 7,
        Screen::Specials(_) => 8,
        // Voice entry pushes its page and then the same preview state Test uses.
        Screen::Voice(_) => 9,
        Screen::Test => 10,
    };
    native::write(context, 0xf8 + (level as usize + 1) * 4, target);
    let end = if target == 9 || target == 10 {
        native::write(context, 0xf8 + (level as usize + 2) * 4, 11i32);
        level + 2
    } else { level + 1 };
    native::write(context, 0x144, end);
    Entry::Entered
}
