//! Default-off request API. Called by the existing StageManager update entry hook.
//! No global match selector, stage/resource pointer, task, or refcount writes.
//! UI integration is feature-gated; actual native mutation acceptance is pending.

#[path = "practice_stage_request_policy.rs"]
mod policy;
use policy::{ActivationRead, Commit, Resource, Scene, View};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use skyline::hooks::{getRegionAddress, Region};

include!("practice_stage_request_words.rs");

static VERIFIED: AtomicBool = AtomicBool::new(false);
static EPOCH: AtomicU64 = AtomicU64::new(1);
static STATE: Mutex<State> = Mutex::new(State::new());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Queued,
    NativeOwned,
    Complete,
    Refused(&'static str),
    /// Native task/stage destruction remains native-owned after publication.
    Departed { published: bool },
    /// No rollback, timeout release, or subsequent request in this scene.
    Retained(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status { pub id: u64, pub stage: u32, pub phase: Phase }

#[derive(Clone, Copy)]
struct Ticket { id: u64, stage: u32, scene: Scene, original: usize }

struct State {
    next: u64,
    seen: Option<(Scene, usize)>,
    queued: Option<Ticket>,
    live: Option<Commit>,
    settled: Option<Commit>,
    status: Option<Status>,
    last_activation: Option<ActivationRead>,
}
impl State {
    const fn new() -> Self {
        Self { next: 0, seen: None, queued: None, live: None, settled: None, status: None, last_activation: None }
    }
    fn phase(&mut self, phase: Phase) {
        if let Some(mut status) = self.status {
            if status.phase != phase {
                status.phase = phase;
                crate::logging::diagnostic(format_args!("[PracticeStageRequest] {status:?}"));
                self.status = Some(status);
                let message = match phase {
                    Phase::Complete => Some("Stage ready".to_owned()),
                    Phase::Refused(why) => Some(format!("Stage unchanged: {why}")),
                    Phase::Retained(why) => Some(format!("Stage request stopped: {why}")),
                    _ => None,
                };
                if let Some(message) = message {
                    crate::training::ui::notifications::clear_notification("Practice Stage");
                    crate::training::ui::notifications::notification("Practice Stage".into(), message, 180);
                }
            }
        }
    }
    fn retire(&mut self) {
        if let (Some(lease),Some(last))=(self.live,self.last_activation) {
            crate::logging::diagnostic(format_args!(
                "[PracticeStageRequest] departed_last_activation lease={lease:?} last_observed={last:?}"));
        }
        if self.queued.is_some() || self.live.is_some() {
            self.phase(Phase::Departed { published: self.live.is_some() });
        }
        self.queued=None; self.live=None; self.settled=None; self.seen=None; self.last_activation=None;
    }
}

/// Exact scene/active identity seen when this menu opened. Never dereferenced by UI.
#[derive(Clone, Copy)]
pub struct MenuContext { scene: Scene, original: usize }

pub fn menu_context() -> Option<MenuContext> {
    if !VERIFIED.load(Ordering::Acquire) { return None; }
    let state = STATE.try_lock().ok()?;
    let (scene, original) = state.seen?;
    (scene.epoch == EPOCH.load(Ordering::Acquire)).then_some(MenuContext { scene, original })
}

/// A UI close may only address the scene/active stage that opened that menu.
pub fn request_in(context: MenuContext, stage: u32) -> Result<u64, &'static str> {
    queue(stage, Some(context))
}

/// Submit once from the existing UI after choosing a catalog stage. Queued means
/// no native mutation yet. The next current update either commits or refuses;
/// nothing retries automatically. The original stage is not a global preference.
pub fn request(stage: u32) -> Result<u64, &'static str> { queue(stage, None) }

fn queue(stage: u32, context: Option<MenuContext>) -> Result<u64, &'static str> {
    if !VERIFIED.load(Ordering::Acquire) { return Err("native contract not verified"); }
    if !training_mod_consts::lobby_stages::STAGES.iter().any(|(id, _)| *id == stage) {
        return Err("stage outside native selectable catalog");
    }
    let mut state=STATE.try_lock().map_err(|_| "request busy")?;
    if state.queued.is_some() || state.live.is_some() { return Err("request already owned"); }
    let (scene, original)=state.seen.ok_or("no current practice update")?;
    if scene.epoch != EPOCH.load(Ordering::Acquire) { return Err("scene departed"); }
    if context.is_some_and(|old| !policy::current_request(old.scene, scene, old.original, original)) {
        return Err("practice changed while menu was open");
    }
    state.next=state.next.checked_add(1).ok_or("request id exhausted")?;
    let id=state.next;
    state.queued=Some(Ticket { id, stage, scene, original });
    state.status=Some(Status { id, stage, phase: Phase::Queued });
    crate::logging::diagnostic(format_args!("[PracticeStageRequest] queued id={id} stage={stage} scene={scene:?} original=0x{original:x}"));
    crate::training::ui::notifications::clear_notification("Practice Stage");
    crate::training::ui::notifications::notification("Practice Stage".into(), "Stage requested".into(), 180);
    Ok(id)
}

pub fn status() -> Option<Status> { STATE.try_lock().ok().and_then(|s| s.status) }

/// Invalidate plugin tickets, even when a departing allocation address is reused.
/// No native dereference or cleanup occurs here. StageManager owns accepted work.
pub(crate) fn depart() {
    EPOCH.fetch_add(1, Ordering::AcqRel);
    if let Ok(mut state)=STATE.try_lock() { state.retire(); }
}

#[repr(C)]
#[derive(Default)]
struct MemoryInfo { address:u64,size:u64,kind:u32,attributes:u32,permission:u32,ipc:u32,device:u32,padding:u32 }
// Pinned QueryMemory ABI reused from mii_ordinary_native::span. Mapping is not
// lifetime: only the current native update's fresh owner chain authorizes access.
unsafe fn span(address:usize,size:usize,write:bool)->bool {
    let Some(end)=address.checked_add(size) else { return false };
    if address<0x10000 || size==0 { return false; }
    let mut info=MemoryInfo::default(); let result:u64;
    core::arch::asm!("svc 0x6",inout("x0") &mut info as *mut MemoryInfo as u64=>result,
        lateout("x1") _,in("x2") address as u64,clobber_abi("C"),options(nostack));
    let required=if write { 3 } else { 1 };
    result as u32==0 && info.permission & required==required && info.address<=address as u64
        && info.address.checked_add(info.size).is_some_and(|v| end as u64<=v)
}
// span() and the fresh StageManager callback qualify these required fields.
// Mapping checks remain local; they do not replace native lifetime authority.
#[inline]
unsafe fn rd<T:Copy>(at:usize)->T { crate::native_fields::read_unchecked(at,0) }
#[inline]
unsafe fn wr<T:Copy>(at:usize,value:T) { crate::native_fields::write_unchecked(at,0,value); }
fn check(ok:bool,why:&'static str)->Result<(),&'static str> { if ok {Ok(())} else {Err(why)} }
unsafe fn pointer(at:usize,size:usize)->Result<usize,&'static str> {
    check(span(at,8,false),"pointer slot unavailable")?;
    let p:usize=rd(at);
    check(p & 7==0 && span(p,size,false),"native object span unavailable")?;
    Ok(p)
}

unsafe fn item_ready(base:usize)->Result<bool,&'static str> {
    // Same bounded predicate as the accepted passive observer, with span checks.
    let manager=pointer(base+0x52c4070,0xa8)?;
    let a=pointer(manager+0xa0,8)?;
    let b=pointer(a,8)?;
    let input=pointer(b,8)?;
    let control=pointer(input,0x30)?;
    let counts=pointer(manager+0x58,0xaa8)?;
    let flags:u32=rd(control+0x2c);
    let dragoon:u64=rd(counts+0x348);
    let blocked=[0xa6,0xab,0xd8,0xb8,0xca,0x128,0x148,0x137,0x102,0x103,0x106,0x10d,0xfd,0xf2,0xe4,0xe1,0x153,0x154];
    Ok(u64::from((flags&7).count_ones()).min(dragoon)<=2 && flags&8==0
        && blocked.iter().all(|k| rd::<u64>(counts+k*8)==0))
}

unsafe fn read(scene:Scene)->Result<View,&'static str> {
    let base=getRegionAddress(Region::Text) as usize;
    check(pointer(base+0x532a9d8,8)?==scene.wrapper,"manager wrapper changed")?;
    check(pointer(scene.wrapper,0x260)?==scene.core && span(scene.core,0x260,true),"manager core changed/unwritable")?;
    let mode=crate::common::game_mode::current().ok_or("mode unavailable")?;
    check(mode.initialized && mode.mode==scene.mode && mode.melee==scene.melee,"scene changed")?;
    let owner=pointer(base+0x532a9d0,32)?;
    let task_owner=pointer(base+0x532f488,8)?;
    let task=pointer(task_owner,16)?;
    let mut resources=[None;4];
    for (i,slot) in resources.iter_mut().enumerate() {
        let p:usize=rd(owner+i*8);
        if p!=0 {
            check(p&7==0 && span(p,0x30,false),"resource span unavailable")?;
            *slot=Some(Resource { address:p,state:rd(p),kind:rd(p+0x20),hash:rd(p+0x28) });
        }
    }
    let core=scene.core;
    let active:i32=rd(core+0x17c);
    check((0..3).contains(&active),"invalid active slot")?;
    let slots:[usize;3]=std::array::from_fn(|i|rd(core+0x110+i*8));
    let object=slots[active as usize];
    check(object&7==0 && span(object,0xb8,false),"active descriptor unavailable")?;
    let descriptor=std::array::from_fn(|i|rd::<u8>(object+8+i));
    Ok(View { scene,resource_owner:owner,task_owner,task,task_state:rd(task),task_fiber:rd(task+8),
        disabled:rd(core+0x178),stop_mask:rd::<u32>(core+0x250)&rd::<u32>(core+0x254),
        state:rd(core+0xbc),active,incoming:rd(core+0xb0),outgoing:rd(core+0xb4),slots,resources,
        descriptor,waiting_ready:crate::training::lobby::active(),
        ui_busy:crate::training::lobby::native_ui_busy(),roster_busy:crate::character_roster::busy(),
        item_ready:item_ready(base).unwrap_or(false) })
}

/// Native task finalization precedes state6. Read the constructed incoming
/// allocation only here; no new per-frame stage-object walk during loading.
unsafe fn activation_read(v:View,c:Commit)->Result<ActivationRead,&'static str> {
    check(c.incoming<3 && c.outgoing<3 && c.incoming!=c.outgoing,"invalid activation pair")?;
    let target=v.slots[c.incoming];
    check(target!=0 && target&7==0 && span(target,0xb8,false),"activation target span unavailable")?;
    let resource=v.resources[c.incoming].ok_or("activation target resource absent")?.address;
    check(resource&7==0 && span(resource,0x501,false),"activation target resource span unavailable")?;
    Ok(ActivationRead { native_ready:rd(v.scene.core+0xdc),ready_bits:rd(v.scene.core+0xe0),
        readiness_stop_mask:rd::<u32>(v.scene.core+0x250)&rd::<u32>(v.scene.core+0x254).wrapping_shl(1),
        target_kind:rd(target+8),target_hash:rd(target+16),target_flag_500:rd(resource+0x500),
        request_kind:rd(v.scene.core),request_hash:rd(v.scene.core+8) })
}

unsafe fn prepare_acknowledgment(v:View,c:Commit,last:&mut Option<ActivationRead>)->Result<Option<Commit>,&'static str> {
    let first=activation_read(v,c)?;
    if last.is_none() {
        crate::logging::diagnostic(format_args!(
            "[PracticeStageRequest] activation_first lease={c:?} readiness={first:?} task=({},0x{:x}) slots={:?} resources={:?}",
            v.task_state,v.task_fiber,v.slots,v.resources));
    }
    *last=Some(first);
    let result=policy::acknowledge(v,c,first)?;
    if result.is_some() {
        // All authority and target reads must still agree immediately before the
        // only new native write. No cached receiver or scene event is invoked.
        let current=read(c.scene)?;
        check(current==v && activation_read(current,c)?==first,"activation changed during preparation")?;
        check(EPOCH.load(Ordering::Acquire)==c.scene.epoch,"activation scene departed")?;
    }
    Ok(result)
}

unsafe fn prepare(ticket:Ticket,settled:Option<Commit>)->Result<(Commit,[u8;0xb0]),&'static str> {
    let first=read(ticket.scene)?;
    check(policy::current_request(ticket.scene,first.scene,ticket.original,first.slots[first.active as usize]),
        "active stage changed after request")?;
    let base=getRegionAddress(Region::Text) as usize;
    let hash_at=base+0x454a7e8+ticket.stage as usize*72+24;
    check(span(hash_at,8,false),"stage catalog span unavailable")?;
    let hash=rd::<u64>(hash_at)&0xffffffffff;
    let plan=policy::plan(first,settled,ticket.stage,hash)?;
    // All fallible reads/checks precede publication, under the current update.
    check(read(ticket.scene)?==first,"native admission changed during preparation")?;
    check(EPOCH.load(Ordering::Acquire)==ticket.scene.epoch,"scene epoch changed")?;
    Ok(plan)
}

unsafe fn publish(c:Commit,descriptor:[u8;0xb0]) {
    // Native 14a544c..5508, using the native full descriptor copy contract.
    // The current update owns core lifetime. No cached pointer is dereferenced
    // without prepare() rederiving the exact singleton/current callback chain.
    for (i,b) in descriptor.iter().copied().enumerate() { wr(c.scene.core+i,b); }
    wr(c.scene.core+0xc8,0x0100u16);
    wr(c.scene.core+0xb0,c.incoming as u32);
    wr(c.scene.core+0xb4,c.outgoing as u32);
    wr(c.scene.core+0xc4,0u32);
    wr(c.scene.core+0xf0,0u8);
    wr(c.scene.core+0xb8,0u8);
    // Publish request state last. Native update owns preload/construction;
    // state6 needs the separately qualified ready acknowledgment below.
    std::sync::atomic::compiler_fence(Ordering::Release);
    wr(c.scene.core+0xbc,1u32);
}

/// Same native entry callback as the passive observer; no competing hook.
pub(crate) unsafe fn update(wrapper:usize,core:usize,mode:u32,melee:u8,active_object:usize) {
    if !VERIFIED.load(Ordering::Acquire) { return; }
    let Ok(mut state)=STATE.try_lock() else { return };
    let scene=Scene { epoch:EPOCH.load(Ordering::Acquire),wrapper,core,mode,melee };
    if state.seen.is_some_and(|(old,_)|old!=scene) { state.retire(); }
    state.seen=Some((scene,active_object));
    if let Some(c)=state.live {
        if matches!(state.status.map(|s|s.phase),Some(Phase::Retained(_))) { return; }
        match read(scene) {
            Ok(v) if policy::complete(v,c) => {
                crate::logging::diagnostic(format_args!(
                    "[PracticeStageRequest] native_complete lease={c:?} state={} active={} pair=({},{}) task=({},0x{:x}) slots={:?} resources={:?}",
                    v.state,v.active,v.incoming,v.outgoing,v.task_state,v.task_fiber,v.slots,v.resources));
                state.live=None; state.settled=Some(c); state.phase(Phase::Complete);
            }
            Ok(v) if !policy::same_owner(v,c) => state.phase(Phase::Retained("native owner changed")),
            Ok(v) if v.resources[3].is_some() => state.phase(Phase::Retained("native match preload owns cache")),
            Err(why) => state.phase(Phase::Retained(why)),
            Ok(v) if v.state==6 => match prepare_acknowledgment(v,c,&mut state.last_activation) {
                Ok(Some(acknowledged)) => {
                    // Preserve both allocation identities before acknowledging.
                    // Native 14a56b4 / 24b9aac write only state7; dc/e0, stage,
                    // observer, task, resource and global rule ownership stay native.
                    state.live=Some(acknowledged);
                    std::sync::atomic::compiler_fence(Ordering::Release);
                    wr(scene.core+0xbc,7u32);
                    crate::logging::diagnostic(format_args!(
                        "[PracticeStageRequest] native_ready_ack lease={acknowledged:?} state=6->7"));
                }
                Ok(None) => {}, // Native outgoing readiness has not acknowledged yet.
                Err(why) => state.phase(Phase::Retained(why)),
            },
            Ok(v) if v.state==0 => state.phase(Phase::Retained("native completion does not match acknowledged lease")),
            _ => {}
        }
        return;
    }
    let Some(ticket)=state.queued.take() else { return };
    if !policy::current_request(ticket.scene,scene,ticket.original,active_object)
        || state.status.map(|s|s.id)!=Some(ticket.id) {
        state.phase(Phase::Refused("stale request")); return;
    }
    match prepare(ticket,state.settled) {
        Ok((commit,descriptor)) => {
            // Keep local ownership before the first write; no fallible operation
            // or native call occurs between first write and state publication.
            state.live=Some(commit); state.settled=None; state.last_activation=None;
            publish(commit,descriptor);
            crate::logging::diagnostic(format_args!("[PracticeStageRequest] published lease={commit:?}"));
            state.phase(Phase::NativeOwned);
        }
        Err(why) => state.phase(Phase::Refused(why)),
    }
}

pub(crate) fn init(base:usize) {
    let verified=NATIVE_WORDS.iter().all(|(offset,words)|unsafe {
        span(base+offset,words.len()*4,false)
            && std::slice::from_raw_parts((base+offset) as *const u32,words.len())==*words
    });
    VERIFIED.store(verified,Ordering::Release);
    crate::logging::diagnostic(format_args!("[PracticeStageRequest] verified={verified} default_off=true ui_wired=true"));
}
