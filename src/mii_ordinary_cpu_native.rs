//! CPU1 state commits around the shared selector transaction reads.
use crate::mii_native_access::*;
use super::super::mii_ordinary_player::native::{check,pointer,span,Scalars,mirrors,
    helpers,helper_snapshot,final_registry};
use super::super::mii_selector_native::{self as shared,Map,same_map};
use training_mod_consts::mii::{lifetime::{Binding,Configuration},native_values as values,
    ordinary_cpu as policy,ordinary_player::{SameClass,SameClassValues},role_owner,
    inventory::{MiiClass,Snapshot as Catalog}};
pub(super) use shared::{same_owned,Snapshot};
fn owner()->Result<usize,&'static str> {super::super::mii_backend::cpu().map(|id|id as usize).ok_or("CPU backend unavailable")}
unsafe fn map(entry:usize)->Result<Map,&'static str> {shared::map(entry,owner()?)}
unsafe fn pool(entry:usize,epoch:u64)->Result<Binding,&'static str> {shared::pool(entry,epoch,owner()?)}
unsafe fn current_locked(m:Map,b:Binding,kind:u32,staged:Option<(&[u8;0x70],usize)>,helpers:helpers::View)
    ->Result<Snapshot,&'static str> {shared::current_locked(m,b,kind,staged,helpers,None)}
pub(super) unsafe fn snapshot(entry:usize,epoch:u64,kind:u32)->Result<Snapshot,&'static str> {
    shared::snapshot(entry,epoch,kind,owner()?,None)
}
pub(super) unsafe fn target_rows(c:Configuration)->Result<(),&'static str> {shared::target_rows(c)}
/// Copied phase values under CPU1's own current map/protected-record union lock.
unsafe fn same_class_values(now:&Snapshot)->Result<SameClassValues,&'static str> {
    let o=now.owners[now.binding.owner as usize].ok_or("CPU same-class owner absent")?;
    check(span(o.object,0xb0,true),"CPU same-class writable O span")?;
    let task:usize=rd(o.object,0xa8);
    let task_idle=task==0 || (aligned(task)&&span(task,0x14,false)
        && (&*((task+0x10) as *const std::sync::atomic::AtomicU32))
            .load(std::sync::atomic::Ordering::Acquire)==0);
    let result=SameClassValues {binding:now.binding,thread:now.thread,owner:o,
        header:rd(o.object,0x54),
        mirrors:values::Mirrors::decode(rd(o.row,0x1a8),rd(o.entry,0x46),
            rd(o.information,0x72),rd(o.information,0x390),rd(o.object,0x68)),
        initialized:rd::<u8>(o.object,0x4c)&1!=0,task_idle,
        rebuild_open:rd::<u8>(o.information,0x94)==0};
    let table=pointer(super::base(),OWNER_TABLE,0x130,false)?;
    check(rd::<usize>(table,0xe8+now.binding.owner as usize*8)==o.object && rd::<usize>(o.entry,0xf8)==o.information
        && rd::<usize>(o.object,0xa8)==task,"CPU same-class read authority changed")?;
    Ok(result)
}
pub(super) unsafe fn prepare_same_class(expected:Snapshot,before:Configuration,after:Configuration)
    ->Result<SameClass,&'static str> {
    let rows=[super::super::mii_ordinary_player::native::target_rows(before)?,
        super::super::mii_ordinary_player::native::target_rows(after)?];
    let b=pool(expected.binding.entry,expected.binding.epoch)?;let m=map(b.entry)?;
    check(b==expected.binding,"CPU same-class preflight G changed")?;
    let helpers=helper_snapshot()?;
    let g=PoolLock::acquire(super::base(),b.manager);
    let now=current_locked(m,b,expected.original_kind,None,helpers)?;
    check(now==expected,"CPU same-class preflight snapshot changed")?;
    let at=values::record_address(b,b).map_err(|_|"CPU same-class record bounds")?;
    check(rd::<usize>(at,0x58)!=0 && rd::<usize>(at,0x60)!=0,
        "CPU same-class active Mii resource pair absent")?;
    let proof=SameClass::prepare_cpu_in(now.layout,before,after,same_class_values(&now)?,
        &now.descriptor,rows,&now.record)?;
    drop(g);Ok(proof)
}
/// Select the observed native configuration branch once. Failed proof never tries its peer.
pub(super) unsafe fn publish_same_class(track:&super::Track,selected:usize,
    evidence:&mut Option<Publication>)->Result<bool,&'static str> {
    let proof=track.same_class.ok_or("CPU same-class proof absent")?;
    check(track.publish==Some(proof.target()),"CPU same-class target changed")?;
    let rows=[super::super::mii_ordinary_player::native::target_rows(proof.before())?,
        super::super::mii_ordinary_player::native::target_rows(proof.target())?];
    check(aligned(selected)&&span(selected,0xb0,false),"CPU same-class selected O span")?;
    if rd::<u8>(selected,0x4c)&1==0 {
        publish(track,selected,evidence)?;return Ok(false);
    }
    let m=map(track.ticket.entry)?;let b=pool(track.ticket.entry,track.ticket.epoch)?;
    check(b==track.before.binding,"CPU same-class phase G changed")?;
    let helpers=helper_snapshot()?;
    check(helpers==track.before.helpers,"CPU same-class source provenance changed")?;
    let g=PoolLock::acquire(super::base(),b.manager);
    let now=current_locked(m,b,track.before.original_kind,
        Some((&track.descriptor,selected)),helpers)?;
    retained_origin(track)?;
    check(same_map(&now,&track.before) && now.protected==track.before.protected
        && now.records==track.before.records,"CPU same-class protected union changed")?;
    check(now.index==track.before.index && now.scalars==track.before.scalars
        && now.record==track.before.record && now.control==track.before.control,
        "CPU same-class old scalars/control/record changed")?;
    check(rd::<u8>(b.entry,0x5920)==3 && rd::<u8>(b.entry,0x591c)==6,
        "CPU same-class native loader6 required")?;
    let observed=same_class_values(&now)?;
    let update=proof.prepare_write(observed,&track.descriptor,rows,&now.record)?;
    let at=values::record_address(b,b).map_err(|_|"CPU same-class final record bounds")?;
    let targets=Targets {e:b.row,fe:b.entry,d:observed.owner.information,o:observed.owner.object};
    let mut state=super::STATE.lock().unwrap();
    let fresh=state.track.as_mut().ok_or("CPU same-class ticket retired")?;
    check(fresh.ticket==track.ticket && fresh.same_class==Some(proof)
        && fresh.destination==track.destination && fresh.gate.may_publish(track.ticket,6)
        && super::epoch()==track.ticket.epoch,"CPU same-class final ticket/epoch changed")?;
    let registry=final_registry(helpers,Some(track.token))?;
    // Existing same-class write set only: payload and outfit/colour/voice/specials.
    // No index/header/flag/task/resource-pair or vector write in this branch.
    copy_update(at,&update);write_tokens(&targets,update.tokens().into());
    fresh.retained_o=true;
    let published=fresh.gate.publish(track.ticket,6);debug_assert!(published);
    drop(registry);drop(state);drop(g);Ok(true)
}
pub(super) use shared::Publication;
/// The native origin can precede its selector destination (LDN). Keep that record
/// untouched and compare it under the shared transaction's current G lock.
unsafe fn retained_origin(track:&super::Track)->Result<(),&'static str> {
    if track.origin_index==track.before.binding.record_index {return Ok(());}
    let Some(expected)=track.origin_record else{return Ok(());};
    let b=Binding {record_index:track.origin_index,..track.before.binding};
    let at=values::record_address(b,b).map_err(|_|"CPU retained origin bounds")?;
    check(span(at,values::RECORD_LEN,false),"CPU retained origin span")?;
    check(rd::<[u8;104]>(at,0)==expected,"CPU retained origin record changed")
}
fn target(track:&super::Track)->shared::PublicationTarget<'_> {
    shared::PublicationTarget {before:&track.before,descriptor:&track.descriptor,
        payload:track.publish.filter(|_|u16::from_le_bytes([track.descriptor[0x46],track.descriptor[0x47]])==track.before.binding.record_index),
        scalars:track.publish.map_or(track.origin_scalars,Scalars::saved),source:None}
}
pub(super) unsafe fn publish(track:&super::Track,selected:usize,evidence:&mut Option<Publication>)
    ->Result<(),&'static str> {
    let publication=shared::publication(target(track),selected,evidence)?;
    retained_origin(track)?;
    let mut state=super::STATE.lock().unwrap();
    let fresh=state.track.as_mut().ok_or("CPU ticket retired")?;
    check(fresh.ticket==track.ticket && fresh.gate.may_publish(track.ticket,6)
        && fresh.destination==track.destination && super::epoch()==track.ticket.epoch,
        "CPU final ticket/epoch changed")?;
    let registry=final_registry(publication.observed.before.helpers,Some(track.token))?;
    publication.write();
    let published=fresh.gate.publish(track.ticket,6);debug_assert!(published);
    drop(registry);drop(state);drop(publication);Ok(())
}
pub(super) unsafe fn completed(track:&super::Track)->Result<Snapshot,&'static str> {
    let rows=track.publish.map(|c|super::super::mii_ordinary_player::native::target_rows(c)).transpose()?;
    let completion=shared::completed(target(track))?;
    let now=completion.now;
    retained_origin(track)?;
    if let (Some(c),Some(rows))=(track.publish,rows) {
        let own=now.owners[now.binding.owner as usize].ok_or("CPU ready owner absent")?;
        role_owner::validate_saved(c,now.index,mirrors(own),rows,&now.current_record)?;
        check(now.current_record[88..96]!=[0;8] && now.current_record[96..104]!=[0;8],
            "CPU native-consumed current resource pair absent")?;
    }
    if track.retained_o {
        let proof=track.same_class.ok_or("CPU same-class completion proof absent")?;
        proof.completed(same_class_values(&now)?,&track.descriptor)?;
    }
    let mut state=super::STATE.lock().unwrap();
    check(state.track.as_ref().is_some_and(|t|t.ticket==track.ticket&&t.gate.can_complete(track.ticket))
        && super::epoch()==track.ticket.epoch,"CPU ready ticket/epoch changed before commit")?;
    let owner=state.owner.as_mut().ok_or("CPU ready owner retired")?;
    let mut registry=final_registry(now.helpers,Some(track.token))?;
    owner.expected=now;owner.live=track.destination;
    registry.commit(now.authority(),now.original_kind,super::descriptor_kind(&now.descriptor));
    drop(registry);drop(state);drop(completion);Ok(now)
}
/// Reuse the native before/after proof while the generic roster still holds INSTALL.
pub(super) unsafe fn generic_completed(g:&super::Generic,kind:u32,cursor:Option<usize>)
    ->Result<Snapshot,&'static str> {
    let completion=shared::completed(shared::PublicationTarget {
        before:&g.before,descriptor:&g.descriptor,payload:None,scalars:g.before.scalars,source:None,
    })?;
    let now=completion.now;
    let mut state=super::STATE.lock().unwrap();
    let transaction_busy=state.departed || state.track.is_some() || state.request.is_some()
        || super::epoch()!=g.before.binding.epoch;
    let reservation_held=state.reservation.held();
    let owner=state.owner.as_mut().filter(|o|o.expected==g.expected && o.live==g.live)
        .ok_or("generic CPU owner changed during INSTALL")?;
    let destination=policy::generic_ordinary_commit_in(now.layout,owner.origin.index,policy::GenericOrdinaryFacts {
        reservation_held,transaction_busy,origin_is_saved:owner.origin_saved.is_some(),
        live_is_saved:owner.live.target.saved().is_some(),kind,origin_kind:owner.origin.original_kind,
        published_kind:super::descriptor_kind(&now.descriptor),index:now.index,
        descriptor_equals_origin:now.descriptor==owner.origin.descriptor,roster_cursor:cursor,
    })?;
    let registry=final_registry(now.helpers,Some(g.token))?;
    owner.expected=now;owner.live=destination;
    drop(registry);drop(state);drop(completion);Ok(now)
}
/// Diagnostic only, after a refused completion: the fresh snapshot (or its refusal reason) plus
/// raw per-owner index mirrors and CPU O/D/task words that the snapshot does not carry.
/// Read-only; grants no authority and changes no decision or native state.
pub(super) struct Failure {
    pub now: Result<Snapshot,&'static str>,
    /// Per owner: E+1a8, FE+46, D+72, O+68.
    pub mirrors: [Option<[u16;4]>;8],
    /// Per owner: FE+18 kind, E+8 row kind.
    pub kinds: [Option<(u32,i8)>;8],
    pub fe_tokens: Option<[u16;5]>, pub d_tokens: Option<[u16;4]>,
    pub o_init: Option<u8>, pub o_words: Option<[u32;10]>,
    pub task: Option<usize>, pub outstanding: Option<u32>,
}
pub(super) unsafe fn completion_failure(track:&super::Track)->Option<Failure> {
    let m=map(track.ticket.entry).ok()?;let b=pool(track.ticket.entry,track.ticket.epoch).ok()?;
    let base=super::base();
    let helpers=helper_snapshot().ok()?;
    let g=PoolLock::acquire(base,b.manager);
    let mut f=Failure {now:current_locked(m,b,track.before.original_kind,None,helpers),
        mirrors:[None;8],kinds:[None;8],fe_tokens:None,d_tokens:None,
        o_init:None,o_words:None,task:None,outstanding:None};
    let table=pointer(base,OWNER_TABLE,0x130,false).ok();
    for (id,fe) in m.entries.iter().copied().enumerate() {
        let e=base+0x52c5730+id*0x260;
        if fe==0 || !aligned(fe) || !span(fe,0x5a88,false) || !span(e,0x260,false) {continue;}
        let (Ok(d),Some(o))=(pointer(fe,0xf8,0x3a0,false),
            table.and_then(|t|pointer(t,0xe8+id*8,0xb0,false).ok())) else {continue;};
        f.mirrors[id]=Some([rd(e,0x1a8),rd(fe,0x46),rd(d,0x72),rd(o,0x68)]);
        f.kinds[id]=Some((rd(fe,0x18),rd(e,8)));
        if id==b.owner as usize {
            f.fe_tokens=Some(rd(fe,0x46));f.d_tokens=Some(rd(d,0x72));
            f.o_init=Some(rd(o,0x4c));f.o_words=Some(rd(o,0x54));
            let task:usize=rd(o,0xa8);f.task=Some(task);
            if task!=0 && aligned(task) && span(task,0x14,false) {
                f.outstanding=Some((&*((task+0x10) as *const std::sync::atomic::AtomicU32))
                    .load(std::sync::atomic::Ordering::Acquire));
            }
        }
    }
    drop(g);Some(f)
}

/// Saved CPU capture retains original helper tokens/CharInfo, even with certified ordinary FE kinds.
/// Later tickets retain their full protected owner/record union byte comparisons.
pub(super) unsafe fn capture_origin(entry:usize,epoch:u64,kind:u32,catalog:&Catalog,opening:u64)
    ->Result<(),helpers::CaptureError> {
    let previous={let state=super::STATE.lock().unwrap();
        state.owner.as_ref().map(|o|(o.expected,state.reservation.held()
            || o.live.target.publish(o.origin_saved).is_some()))};
    if let Some((expected,retained))=previous {
        let now=snapshot(entry,epoch,kind)?;
        check(same_owned(&now,&expected,retained),"menu current owner changed")?;
        let mut state=super::STATE.lock().unwrap();
        check(super::super::mii_saved_alpha::menu_epoch()==opening
            && state.capture && state.track.is_none() && super::epoch()==epoch
            && state.owner.as_ref().is_some_and(|o|o.expected==expected),"menu owner retired")?;
        let registry=final_registry(now.helpers,None)?;
        state.capture=false;state.menu_valid=true;
        drop(registry);return Ok(());
    }
    check(catalog.is_complete(),"CPU origin catalog incomplete")?;
    let before=snapshot(entry,epoch,kind)?;
    let c=if MiiClass::from_kind(kind).is_some() {
        check(policy::supported_kind(kind),"CPU origin class not qualified")?;
        let m=map(entry)?;let b=pool(entry,epoch)?;
        let helpers=helper_snapshot()?;
        let g=PoolLock::acquire(super::base(),b.manager);
        let fresh=current_locked(m,b,kind,None,helpers)?;
        check(fresh==before,"CPU origin changed before catalog resolution")?;
        let observed=mirrors(fresh.owners[b.owner as usize].unwrap());
        let saved=role_owner::saved_origin(catalog,kind,observed,&fresh.current_record);
        drop(g);
        let c=match saved {
            Some(c)=>c,
            None if fresh.layout==super::super::mii_backend::Layout::Ldn=>{
                // Native waiting CPUs may use a built-in Mii absent from the saved catalog.
                // Copy its actual values; do not synthesize a preset or write a saved file.
                // Native getters run outside G. The common second snapshot below must still
                // exactly match this source before its origin can be committed.
                let tokens=observed.tokens;
                let rows=values::RowIds {
                    outfit:lookup_row_id(super::base(),OUTFIT_TABLE,tokens.outfit)?,
                    hat:lookup_row_id(super::base(),HAT_TABLE,tokens.hat)?,
                };
                let mut charinfo=[0;88];
                charinfo.copy_from_slice(&fresh.current_record[..88]);
                Configuration::from_observed(MiiClass::from_kind(kind).unwrap(),tokens,charinfo,rows)
                    .map_err(|_|"native CPU origin values or row identity invalid")?
            },
            None=>return Err(helpers::CaptureError::Refused("no exact original saved CPU configuration")),
        };
        Some(c)
    } else {check(before.layout==super::super::mii_backend::Layout::Ldn || before.index==0,"ordinary CPU origin index changed")?;None};
    let rows=c.map(|c|super::super::mii_ordinary_player::native::target_rows(c)).transpose()?;
    let m=map(entry)?;let b=pool(entry,epoch)?;
    let helpers=helper_snapshot()?;
    let g=PoolLock::acquire(super::base(),b.manager);
    let now=current_locked(m,b,kind,None,helpers)?;
    check(now==before && now.thread!=0,"CPU origin changed across row validation")?;
    if let (Some(c),Some(rows))=(c,rows) {
        role_owner::validate_saved(c,now.index,mirrors(now.owners[b.owner as usize].unwrap()),rows,&now.current_record)?;
        // Training's helpers are saved-origin copies. LDN's independently indexed
        // current records are qualified/protected, never assumed to copy the CPU.
        for id in now.layout.helpers().iter().copied().map(|id|id as usize).filter(|_|now.layout==super::super::mii_backend::Layout::Training) {
            let owner=now.owners[id].ok_or("CPU origin helper absent")?;
            let record=now.records.iter().flatten().find(|(index,_)|*index==id as u16)
                .ok_or("CPU origin helper record absent")?;
            let original_kind=match now.helpers.slots[now.layout.helper(id).unwrap()] {
                helpers::Helper::Ready {proof,..}=>i64::from(proof.original.kind),
                helpers::Helper::Unmanaged=>i64::from(rd::<u32>(owner.entry,0x18)),
                helpers::Helper::Unavailable {..}=>return Err(helpers::CaptureError::Refused("CPU helper authority unavailable")),
            };
            check(original_kind==i64::from(kind),"CPU original helper class differs")?;
            role_owner::validate_saved(c,id as u16,mirrors(owner),rows,&record.1)?;
        }
    }
    let mut state=super::STATE.lock().unwrap();
    check(super::super::mii_saved_alpha::menu_epoch()==opening
        && state.capture && state.owner.is_none() && state.track.is_none() && !state.departed
        && super::epoch()==epoch,"CPU origin capture retired")?;
    let mut registry=final_registry(helpers,None)?;
    state.owner=Some(super::Owner{origin:now,origin_saved:c,expected:now,
        live:super::Destination{target:super::Target::ORIGINAL,cursor:None}});
    state.capture=false;state.menu_valid=true;
    registry.commit(now.authority(),kind,kind);
    drop(registry);drop(state);drop(g);
    super::super::observe_mii_origin(super::super::Role::Cpu,epoch,kind,super::descriptor_forms(&now.descriptor),c);
    Ok(())
}
