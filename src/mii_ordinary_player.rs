//! Default-off Player0 adapter with an immutable ordinary or saved origin.
//! INSTALL submission and phase-time publication are distinct commits.
use super::*;
use std::sync::Arc;
use std::convert::TryInto;
use training_mod_consts::mii::{
    inventory::{MiiClass,Snapshot as Catalog}, lifetime::Configuration,
    ordinary_player::{Gate,Phase,Request,SameClass,Ticket},
    selection::{Action,PlayerLive,PlayerOrigin},
};
use training_mod_tui::mii_menu::Context as MenuContext;
use training_mod_consts::mii_cycle::{self,Identity,Live};
#[path="mii_ordinary_native.rs"] pub(super) mod native;
#[path="mii_ordinary_gate.rs"] mod hook;

static ENABLED:AtomicBool=AtomicBool::new(false);
static STATE:Mutex<State>=Mutex::new(State {
    owner:None,track:None,catalog:None,capture:false,capture_at:(0,0),menu_valid:false,request:None,next:1,
    generic:None,restore:None,
});
struct Owner {
    origin:native::Snapshot,
    expected:native::Snapshot,
    origin_saved:Option<Configuration>,
    active:Option<Configuration>,
    cursor:Option<usize>,
}
#[derive(Clone)]
struct Track {
    ticket:Ticket, gate:Gate, token:InstallToken, installing:bool,
    retained_o:bool,
    data:Arc<Prepared>,
}
impl std::ops::Deref for Track {
    type Target=Prepared;
    fn deref(&self)->&Prepared {&self.data}
}
struct Prepared {
    before:native::Snapshot, origin_scalars:native::Scalars,
    descriptor:[u8;0x70], publish:Option<Configuration>,
    destination:mii_cycle::Step,
    same_class:Option<SameClass>,
}
/// Read-only receipt for an ordinary Player0 load kept on the generic roster path.
/// It carries no authority; `generic_completed` re-proves everything against `before`.
struct Generic {
    token:InstallToken,before:native::Snapshot,expected:native::Snapshot,descriptor:[u8;0x70],
}
struct State {
    owner:Option<Owner>,track:Option<Track>,catalog:Option<Arc<Catalog>>,
    capture:bool,capture_at:(u64,u64),menu_valid:bool,request:Option<Request>,next:u64,
    generic:Option<Generic>,
    restore:Option<WaitingValue>,
}
fn base()->usize { BASE.load(Ordering::Acquire) }
fn epoch()->u64 { mii_saved_alpha::current_generation(0) }
fn note(reason:&str) { trace!("[MiiOrdinaryPlayer] {reason}"); }

pub(super) unsafe fn init(base:usize)->bool {
    if !mii_saved_fingerprints::valid(base) || !hook::valid(base) {
        note("disabled: prequeue/service fingerprint mismatch");
        return false;
    }
    hook::install(base);
    ENABLED.store(true,Ordering::Release);
    note("enabled: staged Player0 alpha; one owner for ordinary and saved origins");
    true
}
pub(super) fn catalog_ready(catalog:Arc<Catalog>,generation:u64,opening:u64)->bool {
    if !ENABLED.load(Ordering::Acquire) {return false;}
    let mut s=STATE.lock().unwrap();
    if epoch()!=generation || mii_saved_alpha::menu_epoch()!=opening {return false;}
    let complete=catalog.is_complete();
    s.catalog=Some(catalog);s.menu_valid=false;s.capture=complete && s.track.is_none();
    s.capture_at=(generation,opening);
    s.capture
}
#[cfg(feature="mii-ordinary-cpu-alpha")]
pub(super) fn refresh_library(library:&training_mod_consts::mii::draft::Library) {
    let mut s=STATE.lock().unwrap();
    if let Some(catalog)=s.catalog.take() {s.catalog=Some(Arc::new((*catalog).clone().with_library(library)));}
}
pub(super) unsafe fn menu_capture_pending(entry:*const u8)->bool {
    let s=STATE.lock().unwrap();
    s.capture && (s.owner.as_ref().is_some_and(|o|o.origin.binding.entry==entry as usize)
        || original_kind(Role::Player)==Some(read::<u32>(entry,0x18)))
}
#[cfg(feature="mii-managed-helper-alpha")]
pub(super) fn helper_origin_captured()->bool {
    let s=STATE.lock().unwrap();
    !s.capture && s.track.is_none() && s.owner.as_ref().is_some_and(|o|o.origin.binding.epoch==epoch())
}
pub(super) fn player_context()->Option<(PlayerOrigin,PlayerLive)> {
    let s=STATE.lock().unwrap();
    if !s.menu_valid || s.capture || s.track.is_some() || s.request.is_some() {return None;}
    s.owner.as_ref().filter(|o|o.origin.binding.epoch==epoch()).map(|o| {
        let original=o.origin_saved.map_or(PlayerOrigin::NotMii,
            |c|PlayerOrigin::Mii(training_mod_consts::mii::selection::Origin(c.canonical())));
        let live=if o.cursor.is_none() && o.expected.descriptor==o.origin.descriptor
            && o.active==o.origin_saved {PlayerLive::Origin}
            else {o.active.map_or(PlayerLive::Origin,|c|
                PlayerLive::Cycled{class:c.canonical().class,canonical:c.canonical()})};
        (original,live)
    })
}
/// Consume ordinary Player actions even when refused, so they cannot fall through to SavedRole.
pub(super) fn menu_close(context:MenuContext,action:Action)->bool {
    if !matches!(action,Action::ReloadPlayer(_)) {return false;}
    let mut s=STATE.lock().unwrap();
    let Some(owner)=s.owner.as_ref() else {
        return enabled();
    };
    if !ENABLED.load(Ordering::Acquire) || !s.menu_valid || s.track.is_some() || s.request.is_some() {
        drop(s);note("menu-close request refused: ordinary owner busy");return true;
    }
    let original=owner.origin_saved.map_or(PlayerOrigin::NotMii,
        |c|PlayerOrigin::Mii(training_mod_consts::mii::selection::Origin(c.canonical())));
    if context.original!=original {drop(s);note("menu original identity changed");return true;}
    let request=Request::from_menu(mii_saved_alpha::menu_epoch(),context.epoch,
        context.original,context.active.player,owner.active.map(|c|c.canonical()),action);
    s.request=request;s.restore=None;
    if request.is_some() {WAITING_MEMORY.lock().unwrap().manual_request(Role::Player as usize);}
    if request.is_none() {
        drop(s);note("menu-close request refused: ordinary active configuration changed or out of scope");
    }
    true
}
fn clear_request(request:Request) {
    let mut s=STATE.lock().unwrap();
    let restore_refused=s.request==Some(request) && s.restore.is_some();
    if s.request==Some(request) {s.request=None;s.restore=None;}
    drop(s);
    if restore_refused {
        WAITING_MEMORY.lock().unwrap().manual_request(Role::Player as usize);
        notify(Role::Player,"Practice fighter could not be restored".into());
    }
}
pub(super) fn enabled()->bool {ENABLED.load(Ordering::Acquire)}
pub(super) fn owns(entry:usize)->bool {
    STATE.lock().unwrap().owner.as_ref().is_some_and(|o|o.origin.binding.entry==entry)
}
pub(super) fn request_restore(value:WaitingValue)->bool {
    let mut s=STATE.lock().unwrap();
    if mii_backend::current()!=Some(mii_backend::Layout::Ldn) || !enabled() {return false;}
    // Mixed rosters are owned by intercept even when the remembered fighter is ordinary.
    // Restore the exact remembered value rather than resolving a new menu choice.
    let mixed=setting(&MENU).player_roster.fighters().iter()
        .any(|f|MiiClass::from_kind(f.kind()).is_some());
    let needed=value.owned || original_kind(Role::Player).is_some_and(|k|MiiClass::from_kind(k).is_some())
        || s.owner.as_ref().is_some_and(|o|o.active.is_some() || o.cursor.is_some() || mixed);
    if !needed {return false;}
    if s.capture || s.track.is_some() || s.request.is_some()
        || s.owner.as_ref().is_none_or(|o|o.origin.binding.epoch!=epoch()) {return true;}
    if WAITING_MEMORY.lock().unwrap().restore_target(Role::Player as usize)!=Some(value) {return true;}
    s.restore=Some(value);s.request=Some(Request::CYCLE);
    true
}
pub(super) fn intercept(role:Role,selection:Selection,user_request:bool)->Option<bool> {
    if role!=Role::Player || !ENABLED.load(Ordering::Acquire) {return None;}
    let mut s=STATE.lock().unwrap();
    let Some(owner)=s.owner.as_ref() else {
        #[cfg(feature="mii-ordinary-cpu-alpha")]
        let automatic=super::mii_auto_init::pending();
        #[cfg(not(feature="mii-ordinary-cpu-alpha"))]
        let automatic=false;
        let qualifying=unsafe {mii_backend::ready()} && (automatic || s.capture);
        let roster=qualifying && matches!(selection,Selection::Cycle) && setting(&MENU).player_roster.fighters().iter()
            .any(|f|MiiClass::from_kind(f.kind()).is_some());
        let direct=if let Selection::Direct(f)=selection {Some(f.kind())} else {None};
        if training_mod_consts::mii::auto_init::requires_owner(original_kind(Role::Player),roster,direct) {
            drop(s);note("Mii request refused: original assignment not captured");
            if user_request && mii_backend::current().is_some() {
                notify(Role::Player,"Wait for the practice character to finish loading".into());
            }
            return Some(true);
        }
        return None;
    };
    // A captured mixed roster remains in this adapter, including its ordinary entries.
    let has_mii=setting(&MENU).player_roster.fighters().iter()
        .any(|f|MiiClass::from_kind(f.kind()).is_some());
    let handles=owner.origin_saved.is_some() || owner.active.is_some() || owner.cursor.is_some() || has_mii
        || matches!(selection,Selection::Direct(f) if MiiClass::from_kind(f.kind()).is_some());
    if !handles {return None;}
    let request=match selection {
        Selection::Cycle=>Some(Request::CYCLE),
        Selection::Direct(f)=>Request::direct(mii_saved_alpha::menu_epoch(),f.kind()),
        Selection::RefreshPartners=>None,
    };
    let manual_direct=user_request && matches!(selection,Selection::Direct(_));
    // Before INSTALL begins, an explicit pick supersedes a pending lobby restore.
    let replace_restore=manual_direct && s.restore.is_some();
    if s.track.is_none() && (s.request.is_none() || replace_restore) && request.is_some() {
        s.restore=None;
        if user_request {WAITING_MEMORY.lock().unwrap().manual_request(Role::Player as usize);}
        s.request=request;
    } else {
        drop(s);note("request refused: owned Player request busy or unsupported");
        if manual_direct {notify(Role::Player,"Finish the current practice action first".into());}
    }
    Some(true)
}

unsafe fn permitted()->bool {
    #[cfg(feature="mii-ordinary-cpu-alpha")]
    if super::mii_ordinary_cpu::writers_reserved(0,0) {return false;}
    mii_backend::ready() && crate::common::is_ready_go()
        && !menu::captures_input() && !lobby::native_ui_busy()
        && !button_config::has_pending_request() && !INSTALL.active()
        && !training::save_states::is_loading() && !training::recording_or_playback()
        && !crate::replay_preview::occupied()
        && STATES.iter().all(|s| {let s=s.lock().unwrap();s.active.is_none()&&s.pending.is_none()})
}
unsafe fn ready(entry:*const u8,kind:u32)->bool {
    if MiiClass::from_kind(kind).is_some() {ready_single_mii(entry,0)==Some(kind)}
    else {ready_forms(entry,0).is_some_and(|(_,k)|k==kind)}
}
fn descriptor_kind(d:&[u8;0x70])->u32 {u32::from_le_bytes(d[0x18..0x1c].try_into().unwrap())}
fn descriptor_forms(d:&[u8;0x70])->Forms {
    let kind=descriptor_kind(d);
    if kind==0x4b && u32::from_le_bytes(d[0x14..0x18].try_into().unwrap())==1 {
        Forms::solo_popo()
    } else {Forms::starting(kind)}
}
unsafe fn ready_descriptor(entry:*const u8,d:&[u8;0x70])->bool {
    if MiiClass::from_kind(descriptor_kind(d)).is_some() {ready(entry,descriptor_kind(d))}
    else {ready_forms(entry,0).is_some_and(|(forms,_)|forms==descriptor_forms(d))}
}
#[inline(never)]
pub(super) unsafe fn service(entry:*mut u8,owner:u32)->bool {
    #[cfg(feature = "console-swap-trace")]
    let _swap_stack = crate::practice_trace::scope(0x1080001);
    if owner!=0 || !ENABLED.load(Ordering::Acquire) {return false;}
    let tracked=STATE.lock().unwrap().track.clone();
    if let Some(t)=tracked {
        if t.ticket.entry!=entry as usize {return false;}
        if matches!(t.gate.phase(),Phase::Refused|Phase::Departed) {return true;}
        if t.gate.can_complete(t.ticket) && ready_descriptor(entry,&t.descriptor)
            && read::<u8>(entry,0x5918)==0 {complete(entry,&t);}
        return true;
    }
    let needs_capture=STATE.lock().unwrap().capture;
    if needs_capture {capture(entry);}
    if STATE.lock().unwrap().request.is_none() {return false;}
    begin(entry)
}

#[inline(never)]
unsafe fn complete(entry:*const u8,t:&Track) {
    #[cfg(feature = "console-swap-trace")]
    let _swap_stack = crate::practice_trace::scope(0x1080004);
            let result=native::completed(t);
            let mut s=STATE.lock().unwrap();
            if !s.track.as_ref().is_some_and(|now|now.ticket==t.ticket) {return;}
            match result {
                Ok(now) if epoch()==t.ticket.epoch => {
                    let Some(o)=s.owner.as_mut() else {
                        s.track.as_mut().unwrap().gate.refuse();return;
                    };
                    // Native completion committed owner/foreign-kind evidence under G -> STATE.
                    debug_assert_eq!(o.expected,now);
                    s.track=None;s.request=None;s.restore=None;
                    drop(s);
                    publish_mii_presentation(entry,0,t.ticket.epoch,descriptor_kind(&t.descriptor));
                    let released=finish_install(t.token,"ordinary Player0 exact native ready");
                    state_for(Role::Player).lock().unwrap().cursor=t.destination.cursor;
                    publish_partners(Role::Player,descriptor_forms(&t.descriptor));
                    remember_owned(Role::Player,t.ticket.epoch,descriptor_kind(&t.descriptor),descriptor_forms(&t.descriptor),
                        t.publish,t.destination.cursor,t.destination.target==mii_cycle::Target::Original);
                    if let Some(fighter)=TrioFighter::from_kind(descriptor_kind(&t.descriptor)) {
                        Role::Player.sync_selection(fighter);
                    }
                    trace!("[MiiOrdinaryPlayer] ready ticket={:?} target={:#x} index={} resource={:02x?} released={} saved_files_written=false",
                        t.ticket,descriptor_kind(&t.descriptor),now.binding.record_index,
                        &now.record[88..104],released);
                    notify(Role::Player,match t.destination.target {
                        mii_cycle::Target::Saved(_)=>"Saved Mii applied",
                        mii_cycle::Target::Original=>"Original fighter restored",
                        mii_cycle::Target::Fighter(_)=>"Player fighter applied",
                    }.into());
                }
                _ => {s.track.as_mut().unwrap().gate.refuse();drop(s);note("terminal ready comparison refusal; INSTALL retained");}
            }
}

#[inline(never)]
unsafe fn capture(entry:*mut u8) {
    #[cfg(feature = "console-swap-trace")]
    let _swap_stack = crate::practice_trace::scope(0x1080002);
    let kind=read::<u32>(entry,0x18);
    let captured=STATE.lock().unwrap().owner.as_ref().map(|o|o.origin.original_kind);
    let Some(original)=captured.or_else(||original_kind(Role::Player)) else {return;};
    if (captured.is_none() && original!=kind) || !ready(entry,kind)
        || INSTALL.active() || !mii_backend::ready() {return;}
    let (catalog,at)={let s=STATE.lock().unwrap();(s.catalog.as_ref().map(Arc::clone),s.capture_at)};
    let Some(catalog)=catalog else {return;};
    // Keep the arm's generation/opening through native capture; never adopt a later lifetime.
    let result=if at==(epoch(),mii_saved_alpha::menu_epoch()) {
        native::capture_origin(entry as usize,at.0,original,&catalog,at.1)
    } else {Err(native::helpers::CaptureError::Refused("catalog capture retired"))};
    match result {
        Ok(())=>note("catalog owner qualified; immutable origin retained"),
        Err(native::helpers::CaptureError::Busy)=>{},
        Err(native::helpers::CaptureError::Refused(reason))=>{
            let mut s=STATE.lock().unwrap();
            if s.capture_at==at {s.capture=false;}
            drop(s);note(reason);
        }
    }
}

#[inline(never)]
unsafe fn begin(entry:*mut u8)->bool {
    #[cfg(feature = "console-swap-trace")]
    let _swap_stack = crate::practice_trace::scope(0x1080003);
    if !permitted() {return false;}
    let pending={
        let s=STATE.lock().unwrap();
        s.owner.as_ref().zip(s.request).map(|(o,request)|
             (o.origin,o.origin_saved,o.expected,o.active,o.cursor,s.catalog.as_ref().map(Arc::clone),request,s.restore))
    };
    let Some((origin,origin_saved,expected,active,cursor,Some(catalog),request,restore))=pending else{return false;};
    if !request.current(mii_saved_alpha::menu_epoch(),active.map(|c|c.canonical())) {
        clear_request(request);note("menu-close request expired before preparation");
        if request.direct_kind().is_some() {notify(Role::Player,"Character choice expired; select it again".into());}
        return false;
    }
    if !ready_descriptor(entry,&expected.descriptor) {return false;}
    let menu=setting(&MENU);
    // Only catalog/target-row qualification may skip a numbered choice. Current
    // owner, native publication and completion failures never select a fallback.
    let qualify=|step:mii_cycle::Step|->Result<(Option<Configuration>,[u8;0x70]),&'static str> {
        match step.target {
            mii_cycle::Target::Original=>{
                if let Some(c)=origin_saved {native::target_rows(c)?;}
                let mut descriptor=origin.descriptor;
                if origin.original_kind==0x4b {
                    let forms=requested_forms_at(&menu,Role::Player,0x4b,None);
                    descriptor[0x14..0x18].copy_from_slice(&forms.count.to_le_bytes());
                    for (i,k) in forms.kinds.iter().enumerate() {
                        descriptor[0x18+i*4..0x1c+i*4].copy_from_slice(&k.to_le_bytes());
                    }
                    descriptor[0x28..0x2c].copy_from_slice(&forms.actor_count.to_le_bytes());
                }
                Ok((step.target.publish(origin_saved,None)?,descriptor))
            },
            mii_cycle::Target::Saved(choice)=>{
                let target=catalog.configuration(choice)
                    .ok_or("selected saved configuration unavailable")?;
                if target.tokens().hat!=origin.scalars.e.hat {
                    return Err("unchanged native hat required");
                }
                native::target_rows(target)?;
                Ok((step.target.publish(origin_saved,Some(target))?,training_mod_consts::mii::ordinary_player::staged_descriptor(
                    origin.descriptor,Some(target))))
            }
            mii_cycle::Target::Fighter(kind)=>{
                let forms=restore.map_or_else(||requested_forms_at(&menu,Role::Player,kind,step.cursor),|v|v.forms);
                let mut descriptor=origin.descriptor;
                descriptor[0x14..0x18].copy_from_slice(&forms.count.to_le_bytes());
                for (i,k) in forms.kinds.iter().enumerate() {
                    descriptor[0x18+i*4..0x1c+i*4].copy_from_slice(&k.to_le_bytes());
                }
                descriptor[0x28..0x2c].copy_from_slice(&forms.actor_count.to_le_bytes());
                descriptor[0x43]=0;
                descriptor[0x46..0x48].copy_from_slice(&0u16.to_le_bytes());
                Ok((step.target.publish(origin_saved,None)?,descriptor))
            }
        }
    };
    let resolved=if let Some(value)=restore {
        let original=TrioFighter::from_kind(origin.original_kind).is_some_and(|fighter|
            value.matches_original(fighter,descriptor_forms(&origin.descriptor),origin_saved));
        let step=mii_cycle::Step {cursor:value.cursor,target:if original {mii_cycle::Target::Original}
            else if let Some(c)=value.configuration {mii_cycle::Target::Saved(training_mod_consts::mii::selection::Choice {
                canonical:c.canonical(),slot_id:None,source:training_mod_consts::mii::selection::Source::Preference})}
            else {mii_cycle::Target::Fighter(value.fighter.kind())}};
        let result=if original {qualify(step)} else if let Some(c)=value.configuration {
            if c.tokens().hat!=origin.scalars.e.hat {Err("remembered Player hat differs")}
            else {native::target_rows(c).map(|_|(Some(c),training_mod_consts::mii::ordinary_player::staged_descriptor(origin.descriptor,Some(c))))}
        } else {qualify(step)};
        match result {Ok(value)=>Some((step,value)),Err(reason)=>{clear_request(request);note(reason);return false;}}
    } else if let Some(kind)=request.direct_kind() {
        let result=mii_cycle::direct(kind,&catalog,&menu::mii_preferences(),
            training_mod_consts::mii::selection::Role::Player,
            origin_saved.map_or(Identity::Fighter(origin.original_kind),|c|Identity::Saved(c.canonical())))
            .ok_or("selected Player character unavailable")
            .and_then(|step|qualify(step).map(|value|(step,value)));
        match result {Ok(value)=>Some(value),Err(reason)=>{
            clear_request(request);note(reason);
            notify(Role::Player,"This Player character cannot be loaded".into());return false;
        }}
    } else if let Some(choice)=request.choice() {
        // A menu selection applies that exact configuration and retains order.
        let step=mii_cycle::Step{target:mii_cycle::Target::Saved(choice),cursor};
        match qualify(step) {
            Ok(value)=>Some((step,value)),
            Err(reason)=>{
                clear_request(request);note(reason);
                notify(Role::Player,"This Player Mii configuration cannot be loaded".into());
                return false;
            }
        }
    } else {
        let identity=active.map_or(Identity::Fighter(descriptor_kind(&expected.descriptor)),
            |c|Identity::Saved(c.canonical()));
        let live=if cursor.is_none() && active==origin_saved && (expected.descriptor==origin.descriptor
            || (origin.original_kind==0x4b && descriptor_kind(&expected.descriptor)==0x4b)) {
            Live::Original
        } else {Live::Numbered{identity,cursor}};
        mii_cycle::next_qualified_player_with_ice_climbers(&menu.player_roster.fighters(),&catalog,&menu::mii_preferences(),
            origin_saved.map_or(Identity::Fighter(origin.original_kind),|c|Identity::Saved(c.canonical())),live,
            menu.solo_popo_original!=menu.solo_popo_player_cycle,|step|match qualify(step) {
                Ok(value)=>Some(value),
                Err(reason)=>{
                    if let mii_cycle::Target::Saved(choice)=step.target {
                        trace!("[MiiOrdinaryPlayer] skipped unqualified cycle choice class={:?} slot={:?} reason={}",
                            choice.canonical.class,choice.slot_id,reason);
                        notify(Role::Player,format!("Player {:?} choice skipped",choice.canonical.class));
                    }
                    None
                }
            })
    };
    let Some((destination,(publish,descriptor)))=resolved else {
        clear_request(request);note("Player cycle has no qualified choice");return false;
    };
    let current=match native::snapshot(entry as usize,epoch(),origin.original_kind) {
        Ok(now) if native::same_owned(&now,&expected)=>now,
        Err(native::helpers::BUSY)=>return false,
        Err(reason)=>{clear_request(request);note(reason);return false;}
        Ok(_)=>{clear_request(request);note("preflight current configuration changed");return false;}
    };
    // Identical configuration/group needs only an order update, never INSTALL.
    if descriptor==current.descriptor && ready(entry,descriptor_kind(&descriptor))
        && training_mod_consts::mii::role_owner::payload_already_active(active,publish,
            restore.is_some() || destination.target==mii_cycle::Target::Original) {
        let mut s=STATE.lock().unwrap();
        if s.track.is_none() && s.request==Some(request)
            && request.current(mii_saved_alpha::menu_epoch(),active.map(|c|c.canonical())) {
            if let Some(o)=s.owner.as_mut().filter(|o|
                o.expected==expected && o.active==active && o.cursor==cursor) {
                let Ok(registry)=native::final_registry(current.helpers,None) else{return false;};
                o.cursor=destination.cursor;s.request=None;s.restore=None;drop(registry);drop(s);
                state_for(Role::Player).lock().unwrap().cursor=destination.cursor;
                publish_partners(Role::Player,descriptor_forms(&descriptor));
                remember_owned(Role::Player,current.binding.epoch,descriptor_kind(&descriptor),descriptor_forms(&descriptor),
                    active,destination.cursor,destination.target==mii_cycle::Target::Original);
                if let Some(fighter)=TrioFighter::from_kind(descriptor_kind(&descriptor)) {
                    Role::Player.sync_selection(fighter);
                }
                notify(Role::Player,"Player choice already active".into());
            }
        }
        return false;
    }
    let same_class=match (active,publish) {
        (Some(before),Some(after)) if before.canonical().class==after.canonical().class=>
            match native::prepare_same_class(current,before,after) {
                Ok(proof)=>Some(proof),
                Err(reason)=>{clear_request(request);note(reason);return false;}
            },
        _=>None,
    };
    if !permitted(){return false;}
    let Some(token)=INSTALL.try_begin(true) else{return false;};
    // The preflight predates INSTALL ownership; freeze all foreign bytes/provenance anew.
    if native::snapshot(entry as usize,current.binding.epoch,origin.original_kind)!=Ok(current) {
        finish_install(token,"Player preflight changed before INSTALL ownership");return false;
    }
    let track={
        let mut s=STATE.lock().unwrap();
        if s.track.is_some() || s.request!=Some(request) || epoch()!=current.binding.epoch
            || !request.current(mii_saved_alpha::menu_epoch(),
                s.owner.as_ref().and_then(|o|o.active.map(|c|c.canonical())))
            || !s.owner.as_ref().is_some_and(|o|
                o.expected==expected && o.active==active && o.cursor==cursor) {
            drop(s);finish_install(token,"ordinary pre-submit refusal");return false;
        }
        let sequence=s.next;
        let Some(next)=sequence.checked_add(1) else {
            s.request=None;drop(s);finish_install(token,"ordinary ticket exhausted");return false;
        };
        let ticket=Ticket{epoch:epoch(),entry:entry as usize,sequence};
        let t=Track {ticket,gate:Gate::new(ticket).unwrap(),token,installing:true,retained_o:false,
            data:Arc::new(Prepared {before:current,origin_scalars:origin.scalars,
                descriptor,publish,destination,same_class})};
        let registry=match native::final_registry(current.helpers,Some(token)) {
            Ok(registry)=>registry,
            Err(_)=>{drop(s);finish_install(token,"Player source provenance changed before submit");return false;}
        };
        s.next=next;s.request=None;s.track=Some(t.clone());drop(registry);t
    };
    // Outgoing FE is still untouched. INSTALL copies the separately staged 0x70-byte descriptor.
    let mut aligned_descriptor=[0u64;14];
    std::ptr::copy_nonoverlapping(track.descriptor.as_ptr(),aligned_descriptor.as_mut_ptr().cast(),0x70);
    training::clear_session_state();
    trace!("[MiiOrdinaryPlayer] submit ticket={:?} outgoing={:#x} target={:#x} index={} resource_before={:02x?} saved_files_written=false",
        track.ticket,descriptor_kind(&current.descriptor),descriptor_kind(&track.descriptor),current.binding.record_index,&current.record[88..104]);
    let install:unsafe extern "C" fn(*mut u8,*const u64,*const u8)=std::mem::transmute(base()+0x653240);
    install(entry,aligned_descriptor.as_ptr(),[1u8,0,0].as_ptr());
    if let Some(current)=STATE.lock().unwrap().track.as_mut().filter(|t|t.ticket==track.ticket) {
        current.installing=false;
    }
    true
}

/// Wrapper ownership is separate from the observer/service callback's Rust early return.
pub(super) unsafe fn enter_native(entry:*mut u8)->bool {
    let mut s=STATE.lock().unwrap();
    let Some(t)=s.track.as_mut().filter(|t|t.ticket.entry==entry as usize) else{return true;};
    if t.installing {return false;} // Suppress a same-entry callback nested inside INSTALL.
    if !native::span(entry as usize,0x5a88,false) || epoch()!=t.ticket.epoch {
        t.gate.refuse();return false;
    }
    t.gate.enter(t.ticket,read::<u8>(entry,0x591c))
}
pub(super) unsafe fn leave_native(entry:*mut u8) {
    let event={
        let mut s=STATE.lock().unwrap();
        s.track.as_mut().filter(|t|t.ticket.entry==entry as usize).and_then(|t| {
            let loader=read::<u8>(entry,0x591c);
            t.gate.leave(t.ticket,loader);
            matches!(t.gate.phase(),Phase::Refused|Phase::Departed)
                .then_some((t.ticket,t.gate.phase(),loader,t.gate.published()))
        })
    };
    // This executes after the original service actually returned through its native epilogue.
    // Subsequent refused calls never enter native and cannot flood this event.
    if let Some((ticket,phase,loader,published))=event {
        trace!("[MiiOrdinaryPlayer] native_return ticket={:?} phase={:?} loader={} published={} INSTALL_active={}",
            ticket,phase,loader,published,INSTALL.active());
    }
}
pub(super) fn tracked(entry:usize)->bool {
    STATE.lock().unwrap().track.as_ref().is_some_and(|t|t.ticket.entry==entry)
}
pub(super) unsafe fn prequeue(entry:usize,selected:usize)->bool {
    let track={
        let s=STATE.lock().unwrap();
        let Some(t)=s.track.as_ref().filter(|t|t.ticket.entry==entry) else{return true;};
        if !t.gate.may_publish(t.ticket,6) {drop(s);refuse(entry,"duplicate/reentrant gate");return false;}
        t.clone()
    };
    let result=if track.same_class.is_some() {
        native::publish_same_class(&track,selected)
    } else {
        native::publish(&track,selected).map(|()|false)
    };
    match result {
        Ok(retained_o)=>{
            trace!("[MiiOrdinaryPlayer] published ticket={:?} target={:#x} O={:#x} index={} retained_same_class_O={} native_resources_untouched=true",
                track.ticket,descriptor_kind(&track.descriptor),selected,track.before.binding.record_index,retained_o);
            true
        }
        Err(reason)=>{refuse(entry,reason);false}
    }
}
fn refuse(entry:usize,reason:&str) {
    let mut s=STATE.lock().unwrap();
    let event=s.track.as_mut().filter(|t|t.ticket.entry==entry).map(|t| {
        t.gate.refuse();(t.ticket,t.gate.phase(),t.gate.published())
    });
    drop(s);
    if let Some((ticket,phase,published))=event {
        trace!("[MiiOrdinaryPlayer] refused ticket={:?} phase={:?} published={} INSTALL_active={} reason={}",
            ticket,phase,published,INSTALL.active(),reason);
    }
}
/// Capture the qualified owner at the generic roster's INSTALL boundary, including already
/// committed helper/CPU changes. A missing receipt never alters the load or grants authority.
#[inline(never)]
pub(super) unsafe fn begin_generic(entry:*mut u8,token:InstallToken,descriptor:&[u8;0x70]) {
    #[cfg(feature = "console-swap-trace")]
    let _swap_stack = crate::practice_trace::scope(0x1080005);
    if !enabled() {return;}
    let pending={
        let mut s=STATE.lock().unwrap();
        s.generic=None;
        let busy=s.track.is_some() || s.request.is_some();
        s.owner.as_ref().filter(|o|o.origin.binding.entry==entry as usize).map(|o|(
            o.origin.original_kind,o.expected,
            busy || o.origin_saved.is_some() || o.active.is_some() || o.cursor.is_some()))
    };
    let Some((original_kind,expected,busy))=pending else {return;};
    if busy || MiiClass::from_kind(descriptor_kind(descriptor)).is_some() {
        note("generic Player0 begin not recorded: owned configuration, transaction or Mii target");return;
    }
    let before=match native::snapshot(entry as usize,epoch(),original_kind) {
        Ok(now) if native::same_owned(&now,&expected)=>now,
        Ok(_)=>{note("generic Player0 began from an unqualified owner state");return;}
        Err(reason)=>{note(reason);return;}
    };
    let mut s=STATE.lock().unwrap();
    if s.track.is_some() || s.request.is_some() || epoch()!=before.binding.epoch
        || !s.owner.as_ref().is_some_and(|o|o.expected==expected && o.origin_saved.is_none()
            && o.active.is_none() && o.cursor.is_none()) {
        drop(s);note("generic Player0 begin retired");return;
    }
    s.generic=Some(Generic {token,before,expected,descriptor:*descriptor});
}
#[inline(never)]
pub(super) unsafe fn complete_generic(entry:*mut u8,token:InstallToken,kind:u32) {
    #[cfg(feature = "console-swap-trace")]
    let _swap_stack = crate::practice_trace::scope(0x1080006);
    if !enabled() {return;}
    let generic={
        let mut s=STATE.lock().unwrap();
        if s.generic.as_ref().is_some_and(|g|g.token==token && g.before.binding.entry==entry as usize) {
            s.generic.take()
        } else {None}
    };
    let Some(g)=generic else {return;};
    match native::generic_completed(&g,kind) {
        Ok(now)=>trace!("[MiiOrdinaryPlayer] generic Player0 owner committed kind={:#x} published={:#x}",
            kind,descriptor_kind(&now.descriptor)),
        Err(reason)=>trace!("[MiiOrdinaryPlayer] generic Player0 commit refused; owner unchanged reason={}",reason),
    }
}
pub(super) fn scene_changed() {
    let mut s=STATE.lock().unwrap();s.capture=false;s.menu_valid=false;s.catalog=None;s.request=None;
    s.generic=None;s.restore=None;
    if let Some(t)=s.track.as_mut(){t.gate.depart();}else{s.owner=None;}
}
pub(super) fn on_destroy(entry:*const u8) {
    let token={
        let mut s=STATE.lock().unwrap();
        if s.generic.as_ref().is_some_and(|g|g.before.binding.entry==entry as usize) {s.generic=None;}
        if s.owner.as_ref().is_some_and(|o|o.origin.binding.entry==entry as usize) {
            s.owner=None;s.request=None;s.restore=None;s.capture=false;s.menu_valid=false;
        }
        if s.track.as_ref().is_some_and(|t|t.ticket.entry==entry as usize) {
            s.track.take().map(|t|(t.token,t.ticket,t.gate.phase()))
        } else {None}
    };
    native::retire_authority(0,entry as usize);
    if let Some((token,ticket,phase))=token {
        let released=finish_install(token,"ordinary Player0 exact destructor");
        trace!("[MiiOrdinaryPlayer] destroyed ticket={:?} phase={:?} INSTALL_released={} cleanup_writes=false",
            ticket,phase,released);
    }
}
