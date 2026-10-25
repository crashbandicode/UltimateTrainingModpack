//! Fresh, callback-scoped contracts for Quickplay's waiting practice.
use super::native::{aligned, drained, preview_state, read, write};
use std::mem::transmute;
use std::sync::atomic::{AtomicU64, Ordering};

static LAST_JOIN_OBSERVATION: AtomicU64 = AtomicU64::new(0);

pub(super) struct Owner {
    pub sequence: usize,
    context: usize,
}

/// Child controller shared by both waiting rooms. Run107's Local Wireless
/// callback entered `0x2310570` with this layout: self word, state 3, scene
/// vtable `0x50b7998`, and stack top 3. The parent check is not shared.
struct Child {
    sequence: usize,
    context: usize,
}

unsafe fn child(base: usize, context: usize) -> Option<Child> {
    if !aligned(context) || read::<usize>(context, 0) != context
        || read::<i32>(context, 0x178) != 3 { return None; }
    let sequence: usize = read(context, 0x188);
    let scene = sequence.checked_sub(0x50)?;
    if !aligned(scene) || read::<usize>(scene, 0) != base + 0x50b7998
        || read::<usize>(scene, 0xe8) != context { return None; }
    let level: i32 = read(context, 0x174);
    if !(0..19).contains(&level)
        || read::<i32>(context, 0x128 + level as usize * 4) != 3 { return None; }
    Some(Child { sequence, context })
}

unsafe fn child_age(context: usize) -> u32 { read(context, 0x17c) }
unsafe fn child_result(sequence: usize) -> i32 { read(sequence, 0x40) }
unsafe fn child_drained(sequence: usize) -> bool {
    drained(sequence) && read::<usize>(sequence, 0x38) == 0
}
unsafe fn child_reenter(sequence: usize, context: usize) -> bool {
    if !child_drained(sequence) { return false; }
    // Dispatcher 0x2319200 consumes this byte on this child, runs the exit
    // callback, resets age, and performs the complete state-3 entry.
    write(context, 0x184, 1u8);
    true
}

pub(super) const QUICKPLAY_PARENT: usize = 0x50b9000;
pub(super) const LOCAL_WIRELESS_PARENT: usize = 0x50b8028;

/// Parent frame `0x23374b0` keeps the slot at `+0xd0 + level*4` and the state
/// at `+0x120` equal while waiting. Run107 was level 0, slot 6, state 6.
/// Quickplay's state 8 is a different object and does not pass.
pub(super) fn local_wireless_parent_waiting(level: i32, state: i32, top: i32) -> bool {
    (0..0x13).contains(&level) && state == 6 && top == state
}

impl Owner {
    pub unsafe fn bind(base: usize, context: usize) -> Option<Self> {
        let child = child(base, context)?;
        let parent: usize = read(child.sequence, 0x18);
        let parent_scene = parent.checked_sub(0x50)?;
        if !aligned(parent_scene) || read::<usize>(parent_scene, 0) != base + QUICKPLAY_PARENT
            || read::<usize>(parent, 0x38) != child.sequence - 0x50 { return None; }
        let quickplay: usize = read(parent_scene, 0xe8);
        if !aligned(quickplay) || read::<i32>(quickplay, 0x150) != 8 { return None; }
        Some(Self { sequence: child.sequence, context: child.context })
    }

    pub unsafe fn age(&self) -> u32 { child_age(self.context) }
    pub unsafe fn result(&self) -> i32 { child_result(self.sequence) }
    pub unsafe fn drained(&self) -> bool { child_drained(self.sequence) }
    pub unsafe fn reenter(&self) -> bool { child_reenter(self.sequence, self.context) }
}

/// Local Wireless parent is vtable `0x50b8028`. Its frame, init, and release
/// are the parallel slots of Quickplay's parent, not those functions.
pub(super) struct LdnOwner {
    pub sequence: usize,
    context: usize,
}

impl LdnOwner {
    pub unsafe fn bind(base: usize, context: usize) -> Option<Self> {
        let child = child(base, context)?;
        if read::<usize>(child.context, 0x58) != base + 0x2310570 { return None; }
        let parent: usize = read(child.sequence, 0x18);
        let parent_scene = parent.checked_sub(0x50)?;
        if !aligned(parent_scene)
            || read::<usize>(parent_scene, 0) != base + LOCAL_WIRELESS_PARENT
            || read::<usize>(parent, 0x38) != child.sequence - 0x50 { return None; }
        let parent_context: usize = read(parent_scene, 0xe8);
        if !aligned(parent_context) || read::<usize>(parent_context, 0) != parent
            || read::<usize>(parent_context, 8) != parent_context { return None; }
        let level: i32 = read(parent_context, 0x11c);
        if !(0..0x13).contains(&level) { return None; }
        let top: i32 = read(parent_context, 0xd0 + level as usize * 4);
        let state: i32 = read(parent_context, 0x120);
        if !local_wireless_parent_waiting(level, state, top) { return None; }
        Some(Self { sequence: child.sequence, context: child.context })
    }

    pub unsafe fn age(&self) -> u32 { child_age(self.context) }
    pub unsafe fn result(&self) -> i32 { child_result(self.sequence) }
    pub unsafe fn drained(&self) -> bool { child_drained(self.sequence) }
    pub unsafe fn reenter(&self) -> bool { child_reenter(self.sequence, self.context) }
}

pub(super) unsafe fn service(base: usize) -> Option<usize> {
    let outer: usize = read(base, 0x52c4c58);
    if !aligned(outer) { return None; }
    let holder: usize = read(outer, 8);
    if !aligned(holder) { return None; }
    let service: usize = read(holder, 0x88);
    aligned(service).then_some(service)
}

/// Native read-only match selector, including a match queued behind a join.
/// This leaf's match-only branch starts after the flag dispatch: x0 is its only
/// input, it initializes every scratch register and returns through the common
/// RET. The complete 16bb300..16bb470 body is fingerprinted before installation.
/// Entering that branch changes neither the service's selector flag nor its deque.
#[inline]
unsafe fn peek_match(base: usize, service: usize) -> u64 {
    // Caller supplies this bridge's fingerprint-verified base and a service
    // freshly rebound on the current scene callback. No queue is consumed.
    let peek_match: unsafe extern "C" fn(usize) -> u64 = transmute(base + 0x16bb310);
    peek_match(service)
}

#[inline]
unsafe fn peek_front(base: usize, service: usize) -> u64 {
    // Same ownership/verification contract as peek_match; includes flag dispatch.
    let peek_front: unsafe extern "C" fn(usize) -> u64 = transmute(base + 0x16bb300);
    peek_front(service)
}

pub(super) unsafe fn pending_match(base: usize) -> Option<u64> {
    service(base).map(|service| peek_match(base, service))
}

/// Compare the message's sender slot with the native, mode-aware self slot.
/// The message byte in bits 32..39 is not an identity. Unknown slots must stay
/// queued: draining an unclassified Join could swallow an arriving opponent.
unsafe fn local_join(base: usize, service: usize, event: u64) -> Option<bool> {
    if event & 0xff != 1 { return None; }
    let sender = ((event >> 48) & 0xff) as u8;
    if sender > 7 { return None; }
    let self_slot: unsafe extern "C" fn(usize) -> u8 = transmute(base + 0x1686610);
    let own = self_slot(service);
    // Private qualification evidence, emitted only when the Join/self pair
    // changes. Never take the session lock from the native readiness callback.
    let observation = event | (u64::from(own) << 8);
    if LAST_JOIN_OBSERVATION.load(Ordering::Relaxed) != observation {
        LAST_JOIN_OBSERVATION.store(observation, Ordering::Relaxed);
        let find_member: unsafe extern "C" fn(usize, u64) -> u8 = transmute(base + 0x16f6340);
        let member = find_member(service + 0xa010, read(service, 0x9ff8));
        let member_slot = if member < 16 { read::<u8>(service, 0xa074 + usize::from(member) * 0x78) }
            else { 0xff };
        crate::logging::diagnostic(format_args!(
            "[MiiEditor] join_identity event={event:#x} sender={sender} self_slot={own} local_member={member} member_slot={member_slot} session_kind={} service_mode={}",
            read::<u32>(service, 0x269c0), read::<u32>(service, 0x74)));
    }
    (own <= 7).then_some(sender == own)
}

/// A remote Join hands ownership back to waiting practice before its native
/// battle UI completes the handshake and emits match-start. Keep that Join in
/// the queue for practice; a match-start already queued always takes priority.
/// An unclassified notice also returns ownership rather than stalling behind
/// the shield. A confidently local Join or native Leave notification may leave
/// the editor open. Leave has no scene-transition consumer; the native base
/// network block still pops and delivers it normally.
pub(super) unsafe fn pending_interruption(base: usize) -> Option<u64> {
    let service = service(base)?;
    let event = peek_match(base, service);
    if event != 0 { return Some(event); }
    if read::<u8>(service, 0x26a62) != 0 { return Some(0); }
    let event = peek_front(base, service);
    Some(if passive_notification(base, service, event) { 0 } else { event })
}

unsafe fn passive_notification(base: usize, service: usize, event: u64) -> bool {
    event & 0xff == 2 || local_join(base, service, event) == Some(true)
}

/// Deliver native Leave notifications and confidently local Joins through the
/// scene's own handler. A queued match always takes priority; remote Joins and
/// other notices remain protected for native teardown.
pub(super) unsafe fn can_deliver_notification(base: usize) -> bool {
    let Some(service) = service(base) else { return false; };
    if read::<u8>(service, 0x26a62) != 0 { return false; }
    if peek_match(base, service) != 0 { return false; }
    passive_notification(base, service, peek_front(base, service))
}

pub(super) struct Practice { scene: usize, context: usize }
impl Practice {
    pub unsafe fn bind(base: usize, scene: usize, frame: usize, owner: usize) -> Option<Self> {
        if owner == 0 || !aligned(scene) || !aligned(frame)
            || read::<usize>(scene, 0) != base + 0x50a2e50
            || read::<usize>(frame, 8) != owner
            || read::<usize>(owner, 0x38) != scene { return None; }
        let context: usize = read(scene, 0x58);
        if !aligned(context) { return None; }
        Some(Self { scene, context })
    }

    pub unsafe fn running(&self) -> bool {
        let level: i32 = read(self.context, 0x1fc);
        (0..19).contains(&level) && read::<i32>(self.context, 0x200) == 1
            && read::<u32>(self.context, 0x204) > 0
            && read::<i32>(self.context, 0x1b0 + level as usize * 4) == 1
            && read::<u8>(self.context, 0xf0) == 0
            && read::<i32>(self.context, 0xf4) == 0
    }

    pub unsafe fn match_received(&self) -> bool { read::<i32>(self.context, 0xf4) == 3 }

    pub unsafe fn ready(&self, base: usize) -> bool {
        let gate: unsafe extern "C" fn(usize) -> bool = transmute(base + 0x18512c0);
        self.running() && gate(self.scene)
    }

    pub unsafe fn request_quit(&self, base: usize) -> bool {
        let Some(service) = service(base) else { return false; };
        if !self.ready(base) || read::<u32>(service, 0x26a34) != 0 { return false; }
        let quit: unsafe extern "C" fn(usize) -> bool = transmute(base + 0x1851310);
        quit(self.scene)
    }
}

pub(super) unsafe fn practice_resources_released(base: usize) -> bool {
    let outer: usize = read(base, 0x52b94f8);
    if !aligned(outer) { return false; }
    let fighters: usize = read(outer, 0);
    aligned(fighters) && read::<usize>(fighters, 0x20) == 0
        && preview_state(base).is_some_and(|(state, _)| state == 0)
}

/// The same live Test objects consumed by its qualified exit path. State 10
/// alone can precede asynchronous battle construction, so do not acknowledge
/// restoration until the controller exists and has not requested exit.
pub(super) unsafe fn test_running(base: usize) -> bool {
    let outer: usize = read(base, 0x532f8b0);
    if !aligned(outer) { return false; }
    let preview: usize = read(outer, 8);
    if !aligned(preview) || read::<u8>(preview, 0x2b8) != 2 { return false; }
    let battle: usize = read(preview, 0x340);
    if !aligned(battle) { return false; }
    let resources: usize = read(battle, 0x80);
    if !aligned(resources) { return false; }
    let controller: usize = read(resources, 0xf0);
    aligned(controller) && read::<u32>(controller, 0x25c) != 2
}

/// Callback-scoped native submenu completion, as used by scene pre-emption.
/// Name is eligible only after our asynchronous keyboard has finalized.
pub(super) struct Submenu {
    sequence: usize,
    pub state: i32,
}

impl Submenu {
    pub unsafe fn bind(scene: usize, frame: usize, context: usize, parent: usize) -> Option<Self> {
        if !aligned(scene) || !aligned(frame) || !aligned(context) || !aligned(parent)
            || read::<usize>(scene, 0x58) != context || read::<u32>(context, 0x14c) == 0 {
            return None;
        }
        let state: i32 = read(context, 0x148);
        let level: i32 = read(context, 0x144);
        if !(4..=9).contains(&state)
            || (state == 4 && super::inline_keyboard::active())
            || !(0..19).contains(&level)
            || read::<i32>(context, 0xf8 + level as usize * 4) != state { return None; }
        let sequence: usize = read(frame, 8);
        if !aligned(sequence) || parent.checked_add(0x50) != Some(sequence)
            || read::<usize>(sequence, 0x38) != scene { return None; }
        Some(Self { sequence, state })
    }

    pub unsafe fn complete(self, base: usize) {
        // Only the final self-completion from 17f35bc is reused. Forwarding
        // its match request would target the wrong parent; native practice
        // consumes that still-queued event after the editor has released.
        let complete: unsafe extern "C" fn(usize, i32) = transmute(base + 0x3726af0);
        complete(self.sequence, -2);
    }
}

/// `0x180cb80` pops one editor stack level only after its preview status is
/// ready. Level 0 takes the handler's `b.lt` skip and is not popped. The top
/// slot must still be state 11, the same identity the other interrupt paths use.
pub(super) fn revealed_preview_level(state: i32, level: i32, top: i32) -> Option<i32> {
    if state == 11 && (1..19).contains(&level) && top == 11 {
        Some(level - 1)
    } else {
        None
    }
}

/// Called only while a match interrupt is leaving the editor. `0x180cb80`
/// pops one level after its preview status is at least 4, and returns without
/// popping when `[[base+0x52b94f8]+0]+0x20` is null. Departure can take that
/// early return, so write the success store directly: `context+0x144 -= 1`.
/// Dispatcher `0x180ce00` then enters the revealed page. No commit flag and
/// no `context+0x168` write.
pub(super) unsafe fn release_preview_wait(context: usize) -> Option<i32> {
    if !aligned(context) { return None; }
    let state: i32 = read(context, 0x148);
    let level: i32 = read(context, 0x144);
    if !(0..19).contains(&level) { return None; }
    let top: i32 = read(context, 0xf8 + level as usize * 4);
    let revealed = revealed_preview_level(state, level, top)?;
    write(context, 0x144, revealed);
    Some(revealed)
}

/// Returns one action milestone; repeat calls only wait for native state changes.
pub(super) unsafe fn interrupt_editor(
    base: usize, context: usize, before_cancel: impl FnOnce(),
) -> Option<&'static str> {
    if !aligned(context) || read::<u32>(context, 0x14c) == 0 { return None; }
    let level: i32 = read(context, 0x144);
    if !(0..19).contains(&level) { return None; }
    let current: i32 = read(context, 0x148);
    if read::<i32>(context, 0xf8 + level as usize * 4) != current { return None; }
    let outer: usize = read(base, 0x532f8b0);
    if !aligned(outer) { return None; }
    let preview: usize = read(outer, 8);
    if !aligned(preview) || read::<u8>(preview, 0x2b8) != 2 { return None; }
    if current == 10 {
        let battle: usize = read(preview, 0x340);
        if !aligned(battle) { return None; }
        let resources: usize = read(battle, 0x80);
        if !aligned(resources) { return None; }
        let controller: usize = read(resources, 0xf0);
        if !aligned(controller) || read::<u32>(controller, 0x25c) == 2 { return None; }
        // Qualified candidate trigger only: native Test owns every cleanup step.
        write(controller, 0x25c, 2u32);
        return Some("interrupted_test_return_requested");
    }
    if current == 3 && read::<usize>(preview, 0x330) == 0 {
        // The caller already captured pending UI values separately. This is
        // the native non-saving cancellation path after Test has popped.
        before_cancel();
        // Native menu Cancel's exact result/request pair, after Test has popped.
        write(context, 0x168, 2i32);
        write(context, 0xf8 + level as usize * 4, 1i32);
        let close: unsafe extern "C" fn(usize) = transmute(base + 0x180dab0);
        close(context);
        return Some("interrupted_editor_cancel_requested");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{local_wireless_parent_waiting, LOCAL_WIRELESS_PARENT, QUICKPLAY_PARENT, revealed_preview_level};

    #[test]
    fn preview_wait_pops_one_level_only_while_state_11_is_current() {
        assert_eq!(revealed_preview_level(11, 2, 11), Some(1));
        assert_eq!(revealed_preview_level(11, 1, 11), Some(0));
        assert_eq!(revealed_preview_level(11, 18, 11), Some(17));
        // The handler's `b.lt` leaves a zero level in place.
        assert_eq!(revealed_preview_level(11, 0, 11), None);
        assert_eq!(revealed_preview_level(11, 19, 11), None);
        // Voice and Test themselves are not this pop.
        assert_eq!(revealed_preview_level(9, 2, 9), None);
        assert_eq!(revealed_preview_level(10, 2, 10), None);
        // A stale top slot is not the waiter, even if the state word says 11.
        assert_eq!(revealed_preview_level(11, 2, 9), None);
    }

    #[test]
    fn local_wireless_parent_is_not_quickplay_state_8() {
        assert_ne!(LOCAL_WIRELESS_PARENT, QUICKPLAY_PARENT);
        // Run107 waiting: level 0, state 6, stack top 6.
        assert!(local_wireless_parent_waiting(0, 6, 6));
        assert!(local_wireless_parent_waiting(18, 6, 6));
        // Quickplay's parent state must not open this owner.
        assert!(!local_wireless_parent_waiting(0, 8, 8));
        // The child controller's slot 3 is not this parent's top.
        assert!(!local_wireless_parent_waiting(0, 6, 3));
        assert!(!local_wireless_parent_waiting(0, 5, 5));
        assert!(!local_wireless_parent_waiting(0x13, 6, 6));
        assert!(!local_wireless_parent_waiting(-1, 6, 6));
    }
}
