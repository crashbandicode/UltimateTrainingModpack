//! Default-off ordinary CPU1 cycle adapter. Original index0 and retained destination1 are distinct.
//! INSTALL submission and phase-time publication are distinct commits.
use super::*;
use std::sync::Arc;
use std::convert::TryInto;
use training_mod_consts::mii::{
    inventory::{MiiClass,Snapshot as Catalog}, lifetime::Configuration,
    ordinary_player::{Gate,Phase,SameClass,Ticket},
    ordinary_cpu::{Destination,NonRankDiff,RecaptureFlags,Request,Target,TargetKind,
        non_rank_descriptor_diff,recapture_observation_seen,write_recapture_diagnostic,
        RECAPTURE_DIAGNOSTIC_BYTES},
    selection::Action,
};
use training_mod_tui::mii_menu::Context as MenuContext;
use training_mod_consts::mii_cycle::{self,Identity,Live};
#[path="mii_ordinary_cpu_native.rs"] mod native;
use super::mii_ordinary_player::native::{self as provenance,Scalars};

static ENABLED:AtomicBool=AtomicBool::new(false);
static STATE:Mutex<State>=Mutex::new(State {
    owner:None,track:None,catalog:None,capture:false,capture_at:(0,0),menu_valid:false,request:None,next:1,
    reservation:training_mod_consts::mii::ordinary_cpu::Reservation::empty(),departed:false,replay_result:None,generic:None,restore:None,
    recapture_log:None,
});
struct Owner {
    origin:native::Snapshot,
    expected:native::Snapshot,
    origin_saved:Option<Configuration>,
    live:Destination,
}
#[derive(Clone)]
struct Track {
    ticket:Ticket, gate:Gate, token:InstallToken, installing:bool, retained_o:bool,

    data:Arc<Prepared>,
}
impl std::ops::Deref for Track {
    type Target=Prepared;
    fn deref(&self)->&Prepared {&self.data}
}
struct Prepared {
    before:native::Snapshot, origin_scalars:Scalars,
    hat_change:bool,
    origin_index:u16,origin_record:Option<[u8;104]>,
    descriptor:[u8;0x70], publish:Option<Configuration>, destination:Destination, same_class:Option<SameClass>, replay:Option<training_mod_consts::mii::replay::Selection>,
}
/// Read-only receipt for an ordinary load kept on the generic roster path.
struct Generic {
    token:InstallToken,before:native::Snapshot,expected:native::Snapshot,live:Destination,
    descriptor:[u8;0x70],
}
struct State {
    owner:Option<Owner>,track:Option<Track>,catalog:Option<Arc<Catalog>>,
    capture:bool,capture_at:(u64,u64),menu_valid:bool,request:Option<Request>,next:u64,
    reservation:training_mod_consts::mii::ordinary_cpu::Reservation,departed:bool,
    replay_result:Option<training_mod_consts::mii::replay::SelectionReceipt>,
    generic:Option<Generic>,
    restore:Option<WaitingValue>,
    /// Last printed non-rank recapture tuple. Not read on an accepted capture.
    recapture_log:Option<(u64,NonRankDiff,RecaptureFlags)>,
}
fn base()->usize { BASE.load(Ordering::Acquire) }
fn epoch()->u64 { mii_saved_alpha::current_generation(1) }
fn note(reason:&str) { trace!("[MiiOrdinaryCpu] {reason}"); }
/// Menu recapture already refused the descriptor. Log the locked snapshots once
/// per owner epoch and observed tuple. Does not accept bytes or change slots.
fn note_non_rank_recapture(expected:&native::Snapshot, now:&native::Snapshot) {
    let diff=non_rank_descriptor_diff(&expected.descriptor,&now.descriptor);
    if diff.count==0 {return;}
    let flags=RecaptureFlags {
        index_eq:expected.index==now.index,
        scalars_eq:expected.scalars==now.scalars,
        selector_record_eq:expected.record==now.record,
        current_record_eq:expected.current_record==now.current_record,
        captured_index:expected.index,
        live_index:now.index,
    };
    let epoch=expected.binding.epoch;
    if recapture_observation_seen(STATE.lock().unwrap().recapture_log,epoch,diff,flags) {return;}
    let mut buf=[0u8;RECAPTURE_DIAGNOSTIC_BYTES];
    let Some(n)=write_recapture_diagnostic(&mut buf,&diff,&flags) else {return;};
    let Ok(line)=core::str::from_utf8(&buf[..n]) else {return;};
    let print={
        let mut s=STATE.lock().unwrap();
        if recapture_observation_seen(s.recapture_log,epoch,diff,flags) {false}
        else {s.recapture_log=Some((epoch,diff,flags));true}
    };
    if print {note(line);}
}

// Called only after the existing ordinary gate's fingerprint/install succeeds.
pub(super) fn init() {
    ENABLED.store(true,Ordering::Release);
    note("enabled: CPU1 ordinary index0/native selector1 cycle alpha; ordered targets and same-class menu reload");
}
pub(super) fn enabled()->bool {ENABLED.load(Ordering::Acquire)}
pub(super) fn writers_reserved(role:usize,record:u16)->bool {
    let s=STATE.lock().unwrap();
    let physical=s.owner.as_ref().map_or(1,|o|o.origin.binding.owner as usize);
    s.reservation.held() && (role==1 || role==physical || record==physical as u16)
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
pub(super) fn refresh_library(library:&training_mod_consts::mii::draft::Library) {
    let mut s=STATE.lock().unwrap();
    if let Some(catalog)=s.catalog.take() {s.catalog=Some(Arc::new((*catalog).clone().with_library(library)));}
}
pub(super) unsafe fn menu_capture_pending(entry:*const u8)->bool {
    let s=STATE.lock().unwrap();
    s.capture && (s.owner.as_ref().is_some_and(|o|o.origin.binding.entry==entry as usize)
        || original_kind(Role::Cpu)==Some(read::<u32>(entry,0x18)))
}
#[cfg(feature="mii-managed-helper-alpha")]
pub(super) fn helper_origin_captured()->bool {
    let s=STATE.lock().unwrap();
    !s.departed && !s.capture && s.track.is_none()
        && s.owner.as_ref().is_some_and(|o|o.origin.binding.epoch==epoch())
}
#[cfg(feature="mii-managed-helper-alpha")]
pub(super) fn helper_origin_hats()->Option<[u16;2]> {
    let s=STATE.lock().unwrap();
    s.owner.as_ref().filter(|o|!s.departed && o.origin.binding.epoch==epoch())
        .and_then(|o|o.origin.helper_hats())
}
pub(super) fn menu_qualified()->bool {
    let s=STATE.lock().unwrap();
    !s.departed && s.menu_valid && !s.capture && s.track.is_none() && s.request.is_none()
        && s.owner.as_ref().is_some_and(|o|o.origin.binding.epoch==epoch())
}
/// Only the ordinary adapter supplies active CPU canonical identity to the existing menu.
pub(super) fn cpu_context()->Option<(MiiClass,training_mod_consts::mii::inventory::Canonical)> {
    let s=STATE.lock().unwrap();
    if s.departed || !s.menu_valid || s.capture || s.track.is_some() || s.request.is_some() {return None;}
    s.owner.as_ref().filter(|o|o.origin.binding.epoch==epoch())
        .and_then(|o|o.live.target.publish(o.origin_saved)).map(|c|(c.canonical().class,c.canonical()))
}
/// Captured saved-Mii original for menu numbered-cycle exposure.
/// The live class is not an original. None leaves the menu unresolved.
pub(super) fn cpu_original_canonical()->Option<training_mod_consts::mii::inventory::Canonical> {
    let s=STATE.lock().unwrap();
    if s.departed || !s.menu_valid || s.capture || s.track.is_some() || s.request.is_some() {return None;}
    s.owner.as_ref().filter(|o|o.origin.binding.epoch==epoch())
        .and_then(|o|o.origin_saved).map(|configuration|configuration.canonical())
}
pub(super) fn replay_catalog()->Option<Arc<Catalog>> {
    let s=STATE.lock().unwrap();
    (!s.departed && s.menu_valid && !s.capture && s.track.is_none() && s.request.is_none()
        && s.owner.as_ref().is_some_and(|o|o.origin.binding.epoch==epoch()))
        .then(||s.catalog.as_ref().filter(|c|c.is_complete()).map(Arc::clone)).flatten()
}
/// Settled loaded CPU configuration. This is the live original, saved or
/// discovered body, and only when its class is the loaded descriptor kind.
/// A replay receipt that has not become that body is not returned.
pub(super) fn current_configuration()->Option<Configuration> {
    let s=STATE.lock().unwrap();
    if s.departed || !s.menu_valid || s.capture || s.track.is_some() || s.request.is_some() {return None;}
    let owner=s.owner.as_ref().filter(|o|
        o.origin.binding.epoch==epoch() && o.origin.layout.logical(o.origin.binding.owner)==Some(1))?;
    let configuration=owner.live.target.publish(owner.origin_saved)?;
    (configuration.canonical().class as u32==descriptor_kind(&owner.expected.descriptor))
        .then_some(configuration)
}
pub(super) fn replay_current()->Option<Configuration> {
    let s=STATE.lock().unwrap();
    s.owner.as_ref().filter(|o|!s.departed && s.track.is_none() && s.request.is_none()
        && o.origin.binding.epoch==epoch()).and_then(|o|o.live.target.publish(o.origin_saved))
}
pub(super) fn replay_ready(selection:training_mod_consts::mii::replay::Selection,kind:u32)->bool {
    let s=STATE.lock().unwrap();
    s.replay_result.and_then(|receipt|receipt.for_selection(selection,epoch(),super::mii_replay::revision(selection.source)))==Some(Ok(()))
        && !s.departed && s.track.is_none() && s.request.is_none()
        && s.owner.as_ref().is_some_and(|o|o.origin.binding.epoch==epoch()
            && selection.ready(kind,o.live.target.publish(o.origin_saved)))
}
pub(super) fn replay_refusal(selection:training_mod_consts::mii::replay::Selection)->Option<&'static str> {
    STATE.lock().unwrap().replay_result
        .and_then(|receipt|receipt.for_selection(selection,epoch(),super::mii_replay::revision(selection.source)))
        .and_then(Result::err)
}
fn refuse_request(request:Request,reason:&'static str) {
    let mut s=STATE.lock().unwrap();
    let mut restore_refused=false;
    if s.request==Some(request) {
        if let Some(selection)=request.replay() {
            s.replay_result=Some(training_mod_consts::mii::replay::SelectionReceipt {
                selection,outcome:Err(reason)});
        }
        s.request=None;
        restore_refused=s.restore.is_some();
        s.restore=None;
    }
    drop(s);
    if restore_refused {
        WAITING_MEMORY.lock().unwrap().manual_request(Role::Cpu as usize);
        notify(Role::Cpu,"Practice fighter could not be restored".into());
    }
}
pub(super) fn request_replay(selection:training_mod_consts::mii::replay::Selection)->bool {
    request_replay_intent(selection,false)
}
pub(super) fn request_reacknowledge(selection:training_mod_consts::mii::replay::Selection)->bool {
    request_replay_intent(selection,true)
}
fn request_replay_intent(selection:training_mod_consts::mii::replay::Selection,receipt_only:bool)->bool {
    if TrioFighter::from_kind(selection.target.kind()).is_none() {return false;}
    let mut s=STATE.lock().unwrap();
    if !enabled() || s.departed || !s.menu_valid || s.capture || s.track.is_some() || s.request.is_some()
        || !selection.current(epoch(),super::mii_replay::revision(selection.source))
        || s.owner.as_ref().is_none_or(|o|o.origin.binding.epoch!=epoch()) {return false;}
    s.restore=None;
    if receipt_only {
        let current=s.owner.as_ref().and_then(|o|o.live.target.publish(o.origin_saved));
        if selection.target.configuration().is_none() || current!=selection.target.configuration()
            || s.replay_result.and_then(|r|r.for_selection(selection,epoch(),super::mii_replay::revision(selection.source)))==Some(Ok(())) {
            return false;
        }
        s.request=Request::acknowledge_replay(selection);
    } else {s.request=Request::from_replay(selection);}
    if s.request.is_some() {s.replay_result=None;true} else {false}
}
fn request_current(request:Request,active:Option<Configuration>)->bool {
    request.current(mii_saved_alpha::menu_epoch(),active.map(|c|c.canonical()))
        && request.replay().is_none_or(|r|r.current(epoch(),super::mii_replay::revision(r.source)))
}
/// Consume ordinary-owner actions even when rejected; never fall through to SavedRole.
pub(super) fn menu_close(context:MenuContext,action:Action)->bool {
    if !matches!(action,Action::ReloadCpu(_)) {return false;}
    let mut s=STATE.lock().unwrap();
    let Some(o)=s.owner.as_ref() else {return enabled();};
    if !ENABLED.load(Ordering::Acquire) || !s.menu_valid || s.departed || s.track.is_some() || s.request.is_some() {
        drop(s);note("CPU menu-close refused: owned transaction busy");return true;
    }
    let request=Request::from_menu(mii_saved_alpha::menu_epoch(),context.epoch,
        context.active.cpu,o.live.target.publish(o.origin_saved).map(|c|c.canonical()),action);
    s.request=request;s.restore=None;
    if request.is_some() {WAITING_MEMORY.lock().unwrap().manual_request(Role::Cpu as usize);}
    if request.is_none() {drop(s);note("CPU menu-close refused: active configuration/epoch changed");}
    true
}
fn clear_request(request:Request) {
    let mut s=STATE.lock().unwrap();
    let restore_refused=s.request==Some(request) && s.restore.is_some();
    if s.request==Some(request) {s.request=None;s.restore=None;}
    drop(s);
    if restore_refused {
        WAITING_MEMORY.lock().unwrap().manual_request(Role::Cpu as usize);
        notify(Role::Cpu,"Practice fighter could not be restored".into());
    }
}
pub(super) fn owns(entry:usize)->bool {
    STATE.lock().unwrap().owner.as_ref().is_some_and(|o|o.origin.binding.entry==entry)
}
pub(super) fn request_restore(value:WaitingValue)->bool {
    let mut s=STATE.lock().unwrap();
    if !mii_backend::current().is_some_and(|l|l.waiting()) || !enabled() {return false;}
    // A mixed CPU roster is intercepted even when neither current nor remembered body is Mii.
    let mixed=setting(&MENU).cpu_roster.fighters().iter()
        .any(|f|MiiClass::from_kind(f.kind()).is_some());
    let needed=value.owned || original_kind(Role::Cpu).is_some_and(|k|MiiClass::from_kind(k).is_some())
        || s.reservation.held() || mixed || s.owner.as_ref().is_some_and(|o|o.live.target.saved().is_some());
    if !needed {return false;}
    if s.departed || s.capture || s.track.is_some() || s.request.is_some()
        || s.owner.as_ref().is_none_or(|o|o.origin.binding.epoch!=epoch()) {return true;}
    if WAITING_MEMORY.lock().unwrap().restore_target(Role::Cpu as usize)!=Some(value) {return true;}
    s.restore=Some(value);s.request=Some(Request::CYCLE);
    true
}
pub(super) fn intercept(role:Role,selection:Selection,user_request:bool)->Option<bool> {
    if role!=Role::Cpu || !ENABLED.load(Ordering::Acquire) {return None;}
    let mut s=STATE.lock().unwrap();
    let Some(owner)=s.owner.as_ref() else {
        let wants_mii=setting(&MENU).cpu_roster.fighters().iter()
            .any(|f|MiiClass::from_kind(f.kind()).is_some());
        let direct=if let Selection::Direct(f)=selection {Some(f.kind())} else {None};
        if training_mod_consts::mii::auto_init::requires_owner(original_kind(Role::Cpu),wants_mii,direct) {
            drop(s);note("CPU cycle refused: original assignment not captured");
            if user_request && mii_backend::current().is_some() {
                notify(Role::Cpu,"Wait for the practice character to finish loading".into());
            }
            return Some(true);
        }
        return None;
    };
    let requested=setting(&MENU).cpu_roster.fighters().iter()
        .any(|f|MiiClass::from_kind(f.kind()).is_some())
        || matches!(selection,Selection::Direct(f) if MiiClass::from_kind(f.kind()).is_some());
    if !s.reservation.routes(requested,owner.origin_saved.is_some() || owner.live.target.saved().is_some()) {return None;}
    // Automatic partner reconciliation may repeat while an owned operation
    // waits. Coalesce it without a per-frame synchronous diagnostic write.
    if matches!(selection,Selection::RefreshPartners)
        && (s.departed || s.request.is_some() || s.track.is_some()) {return Some(true);}
    // Reservation starts at the INSTALL ticket, never at this menu/catalog capture.
    let request=match selection {
        Selection::Cycle=>Some(Request::CYCLE),
        Selection::Direct(f)=>Request::direct(mii_saved_alpha::menu_epoch(),f.kind()),
        Selection::RefreshPartners=>Some(Request::REFRESH_PARTNERS),
    };
    let manual_direct=user_request && matches!(selection,Selection::Direct(_));
    // Before INSTALL begins, an explicit pick supersedes a pending lobby restore.
    let replace_restore=manual_direct && s.restore.is_some();
    if !s.departed && s.track.is_none() && (s.request.is_none() || replace_restore) && request.is_some() {
        s.restore=None;
        if user_request {WAITING_MEMORY.lock().unwrap().manual_request(Role::Cpu as usize);}
        s.request=request;
    } else {
        drop(s);note("request refused: owned CPU request busy or unsupported");
        if manual_direct {notify(Role::Cpu,"Finish the current practice action first".into());}
    }
    Some(true)
}

unsafe fn permitted()->bool {
    mii_backend::ready() && crate::common::is_ready_go()
        && !menu::captures_input() && !lobby::native_ui_busy()
        && !button_config::has_pending_request() && !INSTALL.active()
        && !training::save_states::is_loading() && !training::recording_or_playback()
        && !crate::replay_preview::occupied()
        && STATES.iter().all(|s| {let s=s.lock().unwrap();s.active.is_none()&&s.pending.is_none()})
}
unsafe fn ready(entry:*const u8,kind:u32)->bool {
    let Some(owner)=mii_backend::cpu() else{return false;};
    if MiiClass::from_kind(kind).is_some() {ready_single_mii(entry,owner)==Some(kind)}
    else {ready_forms(entry,owner).is_some_and(|(_,k)|k==kind)}
}
fn descriptor_forms(d:&[u8;0x70])->Forms {
    let kind=descriptor_kind(d);
    if kind==0x4b && u32::from_le_bytes(d[0x14..0x18].try_into().unwrap())==1 {
        Forms::solo_popo()
    } else {Forms::starting(kind)}
}
unsafe fn ready_descriptor(entry:*const u8,d:&[u8;0x70])->bool {
    if MiiClass::from_kind(descriptor_kind(d)).is_some() {ready(entry,descriptor_kind(d))}
    else {mii_backend::cpu().is_some_and(|owner|ready_forms(entry,owner).is_some_and(|(forms,_)|forms==descriptor_forms(d)))}
}
fn descriptor_kind(d:&[u8;0x70])->u32 {u32::from_le_bytes(d[0x18..0x1c].try_into().unwrap())}
#[inline(never)]
pub(super) unsafe fn service(entry:*mut u8,owner:u32)->bool {
    #[cfg(feature = "console-swap-trace")]
    let _swap_stack = crate::practice_trace::scope(0x1080011);
    if !matches!(owner,1|4) || !ENABLED.load(Ordering::Acquire) {return false;}
    let tracked=STATE.lock().unwrap().track.clone();
    if let Some(t)=tracked {
        if t.ticket.entry!=entry as usize {return false;}
        if matches!(t.gate.phase(),Phase::Refused|Phase::Departed) {return true;}
        if t.gate.can_complete(t.ticket) && ready_descriptor(entry,&t.descriptor)
            && read::<u8>(entry,0x5918)==0 {complete(entry,&t);}
        return true;
    }
    if STATE.lock().unwrap().departed {return true;}
    if mii_backend::cpu()!=Some(owner) {return false;}
    let needs_capture=STATE.lock().unwrap().capture;
    if needs_capture {capture(entry);}
    if STATE.lock().unwrap().request.is_none() {return false;}
    begin(entry)
}

#[inline(never)]
unsafe fn complete(entry:*const u8,t:&Track) {
    #[cfg(feature = "console-swap-trace")]
    let _swap_stack = crate::practice_trace::scope(0x1080014);
            let result=native::completed(t);
            // Diagnostic only: bounded read-only values for a refused comparison, read under G
            // before STATE (existing G->STATE order). No decision depends on them.
            let failure=if result.is_err() {native::completion_failure(t)} else {None};
            let mut s=STATE.lock().unwrap();
            if !s.track.as_ref().is_some_and(|now|now.ticket==t.ticket) {return;}
            match result {
                Ok(now) if epoch()==t.ticket.epoch => {
                    let Some(o)=s.owner.as_mut() else {
                        s.track.as_mut().unwrap().gate.refuse();return;
                    };
                    // Native completion committed owner/foreign-kind evidence under G -> STATE.
                    debug_assert_eq!(o.expected,now);s.track=None;s.request=None;s.restore=None;
                    s.replay_result=t.replay.filter(|r|r.current(epoch(),super::mii_replay::revision(r.source)))
                        .map(|selection|training_mod_consts::mii::replay::SelectionReceipt {selection,outcome:Ok(())});
                    drop(s);
                    #[cfg(feature="mii-hud-portrait-alpha")]
                    publish_mii_presentation(entry,t.before.binding.owner,t.ticket.epoch,descriptor_kind(&t.descriptor),
                        if t.destination.target==Target::ORIGINAL {
                            super::PortraitIntent::Original {
                                original_kind: super::original_kind(super::Role::Cpu).unwrap_or(u32::MAX),
                            }
                        } else if let Some(c)=t.publish {
                            super::PortraitIntent::Exact(c)
                        } else {
                            super::PortraitIntent::NotMii
                        });
                    #[cfg(not(feature="mii-hud-portrait-alpha"))]
                    publish_mii_presentation(entry,t.before.binding.owner,t.ticket.epoch,descriptor_kind(&t.descriptor));
                    let released=finish_install(t.token,"ordinary CPU1 exact native ready");
                    if t.replay.is_none() {state_for(Role::Cpu).lock().unwrap().cursor=t.destination.cursor;}
                    publish_partners(Role::Cpu,descriptor_forms(&t.descriptor));
                    if t.replay.is_none() {remember_owned(Role::Cpu,t.ticket.epoch,descriptor_kind(&t.descriptor),descriptor_forms(&t.descriptor),
                        t.publish,t.destination.cursor,t.destination.target==Target::ORIGINAL);}
                    if let Some(fighter)=TrioFighter::from_kind(descriptor_kind(&t.descriptor)).filter(|_|t.replay.is_none()) {
                        Role::Cpu.sync_selection(fighter);
                    }
                    trace!("[MiiOrdinaryCpu] ready ticket={:?} target={:#x} index={} resource={:02x?} released={} saved_files_written=false",
                        t.ticket,descriptor_kind(&t.descriptor),now.index,
                        &now.record[88..104],released);
                    trace!("[MiiOrdinaryCpu] completion ticket={:?} current={:?}",t.ticket,now);
                    notify(Role::Cpu,match t.destination.target.kind() {
                        TargetKind::Saved=>"Saved Mii applied",TargetKind::Original=>"Original fighter restored",
                        TargetKind::Fighter(_)=>"CPU fighter applied"}.into());
                }
                other => {
                    s.track.as_mut().unwrap().gate.refuse();drop(s);
                    let reason=other.err().unwrap_or("ticket epoch changed before completion");
                    match failure {
                        Some(native::Failure {now,mirrors,kinds,fe_tokens,d_tokens,o_init,o_words,task,outstanding}) => {
                            trace!("[MiiOrdinaryCpu] terminal ready comparison refusal; INSTALL retained ticket={:?} reason={} mirrors(E1a8,FE46,D72,O68)={:?} kinds(FE18,E8)={:?} FEtokens={:?} Dtokens={:?} Oinit={:?} O54..7b={:x?} task={:x?} outstanding={:?} now={:?}",
                                t.ticket,reason,mirrors,kinds,fe_tokens,d_tokens,o_init,o_words,task,outstanding,now);
                        }
                        None => {
                            trace!("[MiiOrdinaryCpu] terminal ready comparison refusal; INSTALL retained ticket={:?} reason={} failure_values=unavailable",
                                t.ticket,reason);
                        }
                    }
                }
            }
}

#[inline(never)]
unsafe fn capture(entry:*mut u8) {
    #[cfg(feature = "console-swap-trace")]
    let _swap_stack = crate::practice_trace::scope(0x1080012);
    let kind=read::<u32>(entry,0x18);
    let captured=STATE.lock().unwrap().owner.as_ref().map(|o|o.origin.original_kind);
    let Some(original)=captured.or_else(||original_kind(Role::Cpu)) else {return;};
    if (captured.is_none() && original!=kind) || !ready(entry,kind)
        || INSTALL.active() || !mii_backend::ready() {return;}
    let (catalog,at)={let s=STATE.lock().unwrap();(s.catalog.as_ref().map(Arc::clone),s.capture_at)};
    let Some(catalog)=catalog else {return;};
    // Keep the arm's generation/opening through native capture; never adopt a later lifetime.
    let result=if at==(epoch(),mii_saved_alpha::menu_epoch()) {
        native::capture_origin(entry as usize,at.0,original,&catalog,at.1)
    } else {Err(provenance::helpers::CaptureError::Refused("catalog capture retired"))};
    match result {
        Ok(())=>note("catalog owner qualified; immutable origin retained"),
        Err(provenance::helpers::CaptureError::Busy)=>{},
        Err(provenance::helpers::CaptureError::Refused(reason))=>{
            let mut s=STATE.lock().unwrap();
            if s.capture_at==at {s.capture=false;}
            drop(s);note(reason);
        }
    }
}

/// One idle snapshot. A busy read, a failed protected-union compare, or a
/// changed reservation/epoch leaves `expected` unchanged. The returned value
/// replaces only the descriptor; records, scalars, owners, and control stay
/// the caller's expected snapshot.
unsafe fn refresh_idle_metadata(entry:*mut u8,kind:u32,expected:native::Snapshot,live:Destination,request:Request,retained:bool,active_present:bool)->native::Snapshot {
    let generation=epoch();
    let reservation={
        let state=STATE.lock().unwrap();
        if INSTALL.active() || state.reservation.held()!=retained {return expected;}
        state.reservation
    };
    let Ok(now)=native::snapshot(entry as usize,generation,kind) else {return expected;};
    if now.binding.epoch!=generation {return expected;}
    let Ok(descriptor)=training_mod_consts::mii::ordinary_cpu::recapture_rank(
        now.layout,expected.binding,now.binding,&expected.descriptor,&now.descriptor) else {return expected;};
    if descriptor==expected.descriptor {return expected;}
    let comparison=native::Snapshot {descriptor,..expected};
    // Protected union first. A mismatch returns the caller's expected snapshot
    // and does not write STATE.
    if !native::same_owned(&now,&comparison,retained || active_present) {return expected;}
    let mut state=STATE.lock().unwrap();
    if INSTALL.active() || state.reservation!=reservation || epoch()!=generation
        || state.departed || state.track.is_some() || state.generic.is_some() || state.request!=Some(request)
        || !state.owner.as_ref().is_some_and(|owner|owner.expected==expected && owner.live==live) {
        return expected;
    }
    state.owner.as_mut().unwrap().expected.descriptor=comparison.descriptor;
    drop(state);
    comparison
}

#[inline(never)]
unsafe fn begin(entry:*mut u8)->bool {
    #[cfg(feature = "console-swap-trace")]
    let _swap_stack = crate::practice_trace::scope(0x1080013);
    if !permitted() {return false;}
    let pending={
        let s=STATE.lock().unwrap();
        s.owner.as_ref().zip(s.request).map(|(o,r)|
             (o.origin,o.origin_saved,o.expected,o.live,r,s.catalog.as_ref().map(Arc::clone),s.reservation.held(),s.restore))
    };
    let Some((origin,origin_saved,expected,live,request,Some(catalog),retained,restore))=pending else{return false;};
    let active=live.target.publish(origin_saved);
    if !request_current(request,active) {
        clear_request(request);note("CPU request epoch/configuration changed");
        if request.direct_kind().is_some() {notify(Role::Cpu,"Character choice expired; select it again".into());}
        return false;
    }
    if !ready_descriptor(entry,&expected.descriptor) {return false;}
    // No ticket exists yet. Waiting practice can change the native AI rank
    // while the menu is open. Use the same descriptor-only qualification as
    // menu recapture: FE+0x35 control and FE+0x37 rank, within their native
    // domains. Every other byte and protected owner field must still match;
    // commit rechecks INSTALL, reservation, and epoch. The later fresh preflight
    // and all transaction phases remain byte-exact; origin is never updated.
    let expected=refresh_idle_metadata(entry,origin.original_kind,expected,live,request,retained,active.is_some());
    // Keep the immutable identity/role origin, but carry the qualified native
    // rank and control type. The fresh preflight and every transaction phase
    // below remain byte-exact against this refreshed expected descriptor.
    let live_origin=match training_mod_consts::mii::ordinary_cpu::with_native_live_metadata(origin.descriptor,&expected.descriptor) {
        Ok(descriptor)=>descriptor,
        Err(reason)=>{refuse_request(request,reason);note(reason);return false;}
    };
    let menu=setting(&MENU);
    // Only source/catalog/row target qualification can skip a cycle choice.
    // Current-owner, SameClass, INSTALL and publication checks remain below this resolver.
    let mii_target=|c:Configuration|->Result<(Target,[u8;0x70]),&'static str> {
        if !training_mod_consts::mii::ordinary_cpu::supported_kind(c.canonical().class as u32) {
            return Err("CPU class not qualified");
        }
        if training_mod_consts::mii::ordinary_cpu::cpu_hat_route(origin.layout,origin.binding.owner as usize,expected.scalars.e.hat,c.tokens().hat)?
            && !provenance::cpu_headgear_fingerprints() {return Err("CPU headgear fingerprint mismatch");}
        native::target_rows(c)?;
        Ok((Target::mii(c),training_mod_consts::mii::ordinary_cpu::staged_descriptor_in(origin.layout,origin.binding.owner as usize,live_origin,Some(c)).ok_or("CPU backend selector changed")?))
    };
    let qualify=|step:mii_cycle::Step|->Result<(Target,[u8;0x70]),&'static str> {
        let mut descriptor=live_origin;
        let target=match step.target {
            mii_cycle::Target::Saved(choice)=>{
                let c=catalog.configuration(choice)
                    .ok_or("saved target unavailable")?;
                return mii_target(c);
            }
            mii_cycle::Target::Original=>{
                if let Some(c)=origin_saved {native::target_rows(c)?;}
                Target::ORIGINAL
            },
            mii_cycle::Target::Fighter(kind)=>{
                let forms=restore.map_or_else(||requested_forms(&menu,Role::Cpu,kind),|v|v.forms);
                descriptor[0x14..0x18].copy_from_slice(&forms.count.to_le_bytes());
                for (i,k) in forms.kinds.iter().enumerate() {
                    descriptor[0x18+i*4..0x1c+i*4].copy_from_slice(&k.to_le_bytes());
                }
                descriptor[0x28..0x2c].copy_from_slice(&forms.actor_count.to_le_bytes());
                descriptor[0x43]=0;
                let index=if origin.layout.waiting() {origin.index}else{0};
                descriptor[0x46..0x48].copy_from_slice(&index.to_le_bytes());
                if kind==origin.original_kind && descriptor==live_origin {Target::ORIGINAL}
                else {Target::fighter(kind)}
            }
        };
        Ok((target,descriptor))
    };
    let resolved=if let Some(value)=restore {
        let original=TrioFighter::from_kind(origin.original_kind).is_some_and(|fighter|
            value.matches_original(fighter,descriptor_forms(&origin.descriptor),origin_saved));
        let step=mii_cycle::Step {cursor:value.cursor,target:if original {mii_cycle::Target::Original}
            else {mii_cycle::Target::Fighter(value.fighter.kind())}};
        let result=if original {qualify(step)} else if let Some(c)=value.configuration {mii_target(c)}else{qualify(step)};
        match result {Ok(value)=>Some((step,value)),Err(reason)=>{refuse_request(request,reason);note(reason);return false;}}
    } else if request.refresh_partners() {
        mii_cycle::refresh_partners(descriptor_kind(&expected.descriptor),origin.original_kind,live.cursor,false)
            .and_then(|step|match qualify(step) {
                Ok(value)=>Some((step,value)),
                Err(reason)=>{note(reason);None}
            })
    } else if let Some(replay)=request.replay() {
        let step=mii_cycle::Step {target:mii_cycle::Target::Fighter(replay.target.kind()),cursor:live.cursor};
        let result=match replay.target.configuration() {Some(c)=>mii_target(c),None=>qualify(step)};
        match result {Ok(value)=>Some((step,value)),Err(reason)=>{
            refuse_request(request,reason);note(reason);return false;
        }}
    } else if let Some(kind)=request.direct_kind() {
        let result=mii_cycle::direct(kind,&catalog,&menu::mii_preferences(),
            training_mod_consts::mii::selection::Role::Cpu,
            origin_saved.map_or(Identity::Fighter(origin.original_kind),|c|Identity::Saved(c.canonical())))
            .ok_or("selected CPU character unavailable")
            .and_then(|step|qualify(step).map(|value|(step,value)));
        match result {Ok(value)=>Some(value),Err(reason)=>{
            refuse_request(request,reason);note(reason);
            notify(Role::Cpu,"This CPU character cannot be loaded".into());return false;
        }}
    } else if let Some(choice)=request.choice() {
        let step=mii_cycle::Step{target:mii_cycle::Target::Saved(choice),cursor:live.cursor};
        match qualify(step) {
            Ok(value)=>Some((step,value)),
            Err(reason)=>{
                clear_request(request);
                trace!("[MiiOrdinaryCpu] menu-close choice refused class={:?} slot={:?} reason={}",
                    choice.canonical.class,choice.slot_id,reason);
                notify(Role::Cpu,"This CPU Mii configuration cannot be loaded".into());
                return false;
            }
        }
    } else {
        let identity=active.map_or(Identity::Fighter(descriptor_kind(&expected.descriptor)),
            |c|Identity::Saved(c.canonical()));
        let current=if live.cursor.is_none() && live.target==Target::ORIGINAL {Live::Original}
            else {Live::Numbered{identity,cursor:live.cursor}};
        mii_cycle::next_qualified_cpu(&menu.cpu_roster.fighters(),&catalog,&menu::mii_preferences(),
            origin_saved.map_or(Identity::Fighter(origin.original_kind),|c|Identity::Saved(c.canonical())),current,|step|match qualify(step) {
                Ok(value)=>Some(value),
                Err(reason)=>{
                    // Source-only refusal; no ticket, live cursor or preference publication.
                    if let mii_cycle::Target::Saved(choice)=step.target {
                        trace!("[MiiOrdinaryCpu] skipped unqualified cycle choice class={:?} slot={:?} reason={}",
                            choice.canonical.class,choice.slot_id,reason);
                        notify(Role::Cpu,format!("CPU {:?} choice skipped",choice.canonical.class));
                    }
                    None
                }
            })
    };
    let Some((step,(target,descriptor)))=resolved else {
        clear_request(request);
        if !request.refresh_partners() {
            note("CPU cycle has no qualified choice");
            notify(Role::Cpu,"No available CPU cycle characters".into());
        }
        return false;
    };
    let destination=Destination{target,cursor:step.cursor};
    let publish=target.publish(origin_saved);
    let current=match native::snapshot(entry as usize,epoch(),origin.original_kind) {
        Ok(now) if native::same_owned(&now,&expected,retained || active.is_some())=>now,
        Err(provenance::helpers::BUSY)=>return false,
        Err(reason)=>{refuse_request(request,reason);note(reason);return false;}
        Ok(_)=>{refuse_request(request,"preflight current configuration changed");note("preflight current configuration changed");return false;}
    };
    // Include saved/ordinary returns. Admission and phase publication both recheck
    // the current-to-target transition; the origin's hat alone is insufficient.
    let hat_change=match training_mod_consts::mii::ordinary_cpu::cpu_hat_route(current.layout,
        current.binding.owner as usize,current.scalars.e.hat,publish.map_or(origin.scalars.e.hat,|c|c.tokens().hat)) {
        Ok(changed) if !changed || provenance::cpu_headgear_fingerprints()=>changed,
        Ok(_)=>{refuse_request(request,"CPU headgear fingerprint mismatch");note("CPU headgear fingerprint mismatch");return false;}
        Err(reason)=>{refuse_request(request,reason);note(reason);return false;}
    };
    // A qualified already-active group/configuration advances only its numbered cursor;
    // no INSTALL or record publication is needed.
    if descriptor==current.descriptor && ready(entry,descriptor_kind(&descriptor))
        && if request.replay().is_some() || restore.is_some() {active==publish} else {
            training_mod_consts::mii::role_owner::payload_already_active(active,publish,
                destination.target==Target::ORIGINAL)
        } {
        let mut s=STATE.lock().unwrap();
        if !s.departed && s.track.is_none() && s.request==Some(request)
            && request_current(request,active) {
            if let Some(o)=s.owner.as_mut().filter(|o|o.expected==expected && o.live==live) {
                let Ok(registry)=provenance::final_registry(current.helpers,None) else{return false;};
                // A deduplicated canonical choice may name another saved-row copy.
                // Keep the actually qualified payload; only its cycle position changes.
                o.live=Destination {cursor:destination.cursor,..o.live};
                s.replay_result=request.replay().map(|selection|training_mod_consts::mii::replay::SelectionReceipt {selection,outcome:Ok(())});s.request=None;s.restore=None;drop(registry);drop(s);
                if request.replay().is_none() {state_for(Role::Cpu).lock().unwrap().cursor=destination.cursor;}
                publish_partners(Role::Cpu,descriptor_forms(&descriptor));
                if request.replay().is_none() {remember_owned(Role::Cpu,current.binding.epoch,descriptor_kind(&descriptor),descriptor_forms(&descriptor),
                    active,destination.cursor,destination.target==Target::ORIGINAL);}
                if let Some(fighter)=TrioFighter::from_kind(descriptor_kind(&descriptor)).filter(|_|request.replay().is_none()) {
                    Role::Cpu.sync_selection(fighter);
                }
                if !request.refresh_partners() {notify(Role::Cpu,"CPU choice already active".into());}
            }
        }
        return false;
    }
    // Receipt renewal cannot become a fighter change if user intent or native
    // state changed since it was queued. Only the qualified no-INSTALL branch above applies.
    if !request.can_install() {clear_request(request);return false;}
    let same_class=match (active,publish) {
        (Some(before),Some(after)) if !hat_change
            && training_mod_consts::mii::ordinary_cpu::retained_same_class_candidate(before,after)
            && current.index==current.binding.record_index
            && u16::from_le_bytes([descriptor[0x46],descriptor[0x47]])==current.binding.record_index=>
            match native::prepare_same_class(current,before,after) {
                Ok(proof)=>Some(proof),Err(reason)=>{refuse_request(request,reason);note(reason);return false;}
            },
        _=>None,
    };
    if !permitted(){return false;}
    let Some(token)=INSTALL.try_begin(true) else{return false;};
    if native::snapshot(entry as usize,current.binding.epoch,origin.original_kind)!=Ok(current) {
        finish_install(token,"CPU preflight changed before INSTALL ownership");return false;
    }
    let track={
        let mut s=STATE.lock().unwrap();
        if s.departed || s.track.is_some() || s.request!=Some(request) || epoch()!=current.binding.epoch
            || !request_current(request,active)
            || !s.owner.as_ref().is_some_and(|o|o.expected==expected && o.live==live) {
            drop(s);finish_install(token,"ordinary pre-submit refusal");return false;
        }
        let sequence=s.next;
        let Some(next)=sequence.checked_add(1) else {
            s.request=None;drop(s);finish_install(token,"ordinary ticket exhausted");return false;
        };
        let ticket=Ticket{epoch:epoch(),entry:entry as usize,sequence};
        let t=Track {ticket,gate:Gate::new(ticket).unwrap(),token,installing:true,retained_o:false,
            data:Arc::new(Prepared {before:current,origin_scalars:origin.scalars,hat_change,
                origin_index:origin.index,origin_record:training_mod_consts::mii::role_owner::current_record_claim(origin.original_kind,origin.index)
                    .then_some(origin.current_record),
                descriptor,publish,destination,same_class,replay:request.replay()})};
        let registry=match provenance::final_registry(current.helpers,Some(token)) {
            Ok(registry)=>registry,
            Err(_)=>{drop(s);finish_install(token,"CPU source provenance changed before submit");return false;}
        };
        if !s.reservation.acquire(INSTALL.owns(token)) {
            drop(registry);drop(s);finish_install(token,"CPU reservation refused before INSTALL");return false;
        }
        s.next=next;s.request=None;s.track=Some(t.clone());drop(registry);t
    };
    let staged=native_fighter_install::Descriptor::from_bytes(&track.descriptor);
    training::clear_session_state();
    trace!("[MiiOrdinaryCpu] submit ticket={:?} outgoing={:#x} target={:#x} index={} cursor={:?} resource_before={:02x?} saved_files_written=false",
        track.ticket,descriptor_kind(&current.descriptor),descriptor_kind(&track.descriptor),current.index,
        track.destination.cursor,&current.record[88..104]);
    trace!("[MiiOrdinaryCpu] staged ticket={:?} descriptor={:02x?}",track.ticket,track.descriptor);
    native_fighter_install::install(base(),entry,&staged,native_fighter_install::Flags::Reload);
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
    if !super::mii_ordinary_player::native::span(entry as usize,0x5a88,false) || epoch()!=t.ticket.epoch {
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
        trace!("[MiiOrdinaryCpu] native_return ticket={:?} phase={:?} loader={} published={} INSTALL_active={}",
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
    let mut evidence=None;
    let result=if track.same_class.is_some() {
        native::publish_same_class(&track,selected,&mut evidence)
    } else {native::publish(&track,selected,&mut evidence).map(|()|false)};
    if let Some(e)=evidence {
        trace!("[MiiOrdinaryCpu] prequeue ticket={:?} selected={:#x} initialized={} header={:x?} Oindex={} Otokens={:x?} task={:#x} outstanding={:?} Dindex={} rebuild={} current={:?}",
            track.ticket,e.selected,e.initialized,e.header,e.o_index,e.o_tokens,e.task,e.outstanding,e.d_index,e.rebuild,e.before);
    }
    match result {
        Ok(retained_o)=>{
            trace!("[MiiOrdinaryCpu] published ticket={:?} target={:#x} O={:#x} index={} retained_same_class_O={} native_resources_untouched=true",
                track.ticket,descriptor_kind(&track.descriptor),selected,track.before.binding.record_index,retained_o);
            true
        }
        Err(reason)=>{refuse(entry,reason);false}
    }
}
pub(super) fn refuse(entry:usize,reason:&str) {
    let mut s=STATE.lock().unwrap();
    let event=s.track.as_mut().filter(|t|t.ticket.entry==entry).map(|t| {
        t.gate.refuse();(t.ticket,t.gate.phase(),t.gate.published())
    });
    drop(s);
    if let Some((ticket,phase,published))=event {
        trace!("[MiiOrdinaryCpu] refused ticket={:?} phase={:?} published={} INSTALL_active={} reason={}",
            ticket,phase,published,INSTALL.active(),reason);
    }
}
/// Capture the current qualified owner, including already committed helper/P1
/// changes. A missing receipt never alters the generic load or grants authority.
#[inline(never)]
pub(super) unsafe fn begin_generic(entry:*mut u8,token:InstallToken,descriptor:&[u8;0x70]) {
    #[cfg(feature = "console-swap-trace")]
    let _swap_stack = crate::practice_trace::scope(0x1080015);
    if !enabled() {return;}
    let pending={
        let mut s=STATE.lock().unwrap();
        s.generic=None;
        let busy=s.departed || s.track.is_some() || s.request.is_some() || s.reservation.held();
        s.owner.as_ref().filter(|o|o.origin.binding.entry==entry as usize).map(|o|(
            o.origin.original_kind,o.expected,o.live,
            busy || o.origin_saved.is_some() || o.live.target.saved().is_some()))
    };
    let Some((original_kind,expected,live,busy))=pending else {return;};
    if busy || MiiClass::from_kind(descriptor_kind(descriptor)).is_some() {
        note("generic CPU begin not recorded: owned transaction or Mii target");return;
    }
    let before=match native::snapshot(entry as usize,epoch(),original_kind) {
        Ok(now) if native::same_owned(&now,&expected,false) && now.index==expected.index=>now,
        Ok(_)=>{note("generic CPU began from an unqualified owner state");return;}
        Err(reason)=>{note(reason);return;}
    };
    let mut s=STATE.lock().unwrap();
    if s.departed || s.track.is_some() || s.request.is_some() || s.reservation.held()
        || epoch()!=before.binding.epoch
        || !s.owner.as_ref().is_some_and(|o|o.expected==expected && o.live==live) {
        drop(s);note("generic CPU begin retired");return;
    }
    s.generic=Some(Generic {token,before,expected,live,descriptor:*descriptor});
}
#[inline(never)]
pub(super) unsafe fn complete_generic(entry:*mut u8,token:InstallToken,kind:u32,cursor:Option<usize>) {
    #[cfg(feature = "console-swap-trace")]
    let _swap_stack = crate::practice_trace::scope(0x1080016);
    if !enabled() {return;}
    let generic={
        let mut s=STATE.lock().unwrap();
        if s.generic.as_ref().is_some_and(|g|g.token==token && g.before.binding.entry==entry as usize) {
            s.generic.take()
        } else {None}
    };
    let Some(g)=generic else {return;};
    match native::generic_completed(&g,kind,cursor) {
        Ok(now)=>trace!("[MiiOrdinaryCpu] generic CPU owner committed kind={:#x} index={} cursor={:?}",
            kind,now.index,cursor),
        Err(reason)=>trace!("[MiiOrdinaryCpu] generic CPU commit refused; owner unchanged reason={}",reason),
    }
}
pub(super) fn scene_changed() {
    let mut s=STATE.lock().unwrap();s.capture=false;s.menu_valid=false;s.catalog=None;s.request=None;s.replay_result=None;s.recapture_log=None;
    s.departed=true;s.generic=None;s.restore=None;
    if let Some(t)=s.track.as_mut(){t.gate.depart();}
    // A completed or returned transaction still retains native G1 until exact destruction.
    if !s.reservation.held() {s.owner=None;s.departed=false;}
}
pub(super) fn on_destroy(entry:*const u8) {
    let token={
        let mut s=STATE.lock().unwrap();
        if s.generic.as_ref().is_some_and(|g|g.before.binding.entry==entry as usize) {s.generic=None;}
        let exact=s.owner.as_ref().is_some_and(|o|o.origin.binding.entry==entry as usize);
        if exact {s.owner=None;s.request=None;s.restore=None;s.replay_result=None;s.capture=false;s.menu_valid=false;s.recapture_log=None;s.reservation.retire(true);s.departed=false;}
        if s.track.as_ref().is_some_and(|t|t.ticket.entry==entry as usize) {
            s.track.take().map(|t|(t.token,t.ticket,t.gate.phase()))
        } else {None}
    };
    for id in [1,4] {super::mii_ordinary_player::native::retire_authority(id,entry as usize);}
    if let Some((token,ticket,phase))=token {
        let released=finish_install(token,"ordinary CPU1 exact destructor");
        trace!("[MiiOrdinaryCpu] destroyed ticket={:?} phase={:?} INSTALL_released={} cleanup_writes=false",
            ticket,phase,released);
    }
}
