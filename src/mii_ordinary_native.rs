//! Native reads/publication for ordinary-origin Player0 only.
//! No converter, record allocation, flag/reset/handle writes, or saved-file IO.
use crate::mii_native_access::*;
use std::convert::TryInto;
use training_mod_consts::mii::{
    lifetime::{Binding, Configuration},
    native_values::{self as values, RowIds},
    ordinary_player::{Owner, SameClass, SameClassValues},
    role_owner::{self, Authority, Committed},
    inventory::{MiiClass,Snapshot as Catalog},
};

// Native publication: G -> role/helper transaction STATE -> COMMITTED.
// SourceBook updates: source STATE -> helper transaction STATE -> COMMITTED,
// with registry released before helper release; no G acquisition under source STATE.
// Reads release COMMITTED before STATE access. Registry operations call no native code.
#[path="mii_source_provenance.rs"]
#[cfg_attr(not(feature="mii-managed-source-coexist-alpha"), allow(dead_code))]
pub(crate) mod helpers;
pub(crate) struct Registry {
    visible: [Option<Committed>;8],
    pub(crate) helpers: helpers::View,
}
static COMMITTED: std::sync::Mutex<Registry> = std::sync::Mutex::new(Registry {
    visible: [None;8], helpers: helpers::View::EMPTY,
});
pub(crate) fn registry()->std::sync::MutexGuard<'static,Registry> { COMMITTED.lock().unwrap() }
pub(crate) fn helper_snapshot()->Result<helpers::View,&'static str> {
    let layout=super::super::mii_backend::current().ok_or("Mii backend unavailable")?;
    let mut r=registry();
    if r.helpers.slots==[helpers::Helper::Unmanaged;2] {r.helpers.layout=layout;}
    let view=r.helpers;check(view.layout==layout,"old helper backend retained")?;view.available()?;Ok(view)
}
pub(crate) fn helper_snapshot_for(source:Option<crate::source_lifetime::SourceToken>)->Result<helpers::View,&'static str> {
    if source.is_none() {return helper_snapshot();}
    let view=registry().helpers;
    view.available_for(source)?;Ok(view)
}
pub(crate) fn check_helpers(view:helpers::View)->Result<(),&'static str> {check_helpers_for(view,None)}
pub(crate) fn check_helpers_for(view:helpers::View,source:Option<crate::source_lifetime::SourceToken>)->Result<(),&'static str> {
    let guard=registry();
    guard.helpers.available_for(source)?;
    check(guard.helpers==view,"source provenance changed during native read")
}
pub(crate) fn final_registry(view:helpers::View,token:Option<super::InstallToken>)
    ->Result<std::sync::MutexGuard<'static,Registry>,&'static str> {final_registry_for(view,token,None)}
pub(crate) fn final_registry_for(view:helpers::View,token:Option<super::InstallToken>,source:Option<crate::source_lifetime::SourceToken>)
    ->Result<std::sync::MutexGuard<'static,Registry>,&'static str> {
    let guard=registry();
    match token {
        Some(token)=>check(super::INSTALL.owns(token),"exact INSTALL owner changed")?,
        None=>check(!super::INSTALL.active(),helpers::CAPTURE_BUSY)?,
    }
    guard.helpers.available_for(source)?;
    check(guard.helpers==view,"source provenance changed before commit")?;
    Ok(guard)
}
impl Registry {
    pub(crate) fn commit(&mut self,authority:Authority,original_kind:u32,kind:u32) {
        self.visible[authority.binding.owner as usize]=Some(Committed{authority,original_kind,kind});
    }
}
pub(crate) fn authority(b:Binding,thread:usize,map:[usize;3],entries:[usize;8],id:u32,owner:Owner)->Authority {
    Authority {binding:Binding {owner:id,entry:owner.entry,row:owner.row,
        epoch:super::super::mii_saved_alpha::current_generation(if id==0 {0}else{1}),
        record_index:u16::from(owner.selector),..b},thread,map,entries,owner}
}
/// Called with freshly rederived current authority and G held. Original E identity
/// remains fixed; only changed Mii kinds need an exact committed registration.
pub(crate) fn foreign_kind(now:Authority,original_kind:u32,row_kind:i8,kind:u32)->bool {
    let registry=COMMITTED.lock().unwrap();
    role_owner::foreign_kind(now,original_kind,row_kind,kind,
        registry.visible.get(now.binding.owner as usize).copied().flatten())
}
pub(crate) fn retire_authority(id:usize,entry:usize) {
    let mut registry=COMMITTED.lock().unwrap();
    if registry.visible[id].is_some_and(|c|c.authority.binding.entry==entry) {registry.visible[id]=None;}
}
pub(crate) unsafe fn mirrors(o:Owner)->values::Mirrors {
    values::Mirrors::decode(rd(o.row,0x1a8),rd(o.entry,0x46),rd(o.information,0x72),
        rd(o.information,0x390),rd(o.object,0x68))
}
pub(crate) unsafe fn control(row:usize)->[u8;7] {
    [0xa,0x6b,0x79,0x78,0x19,0x1a,0xa7].map(|offset|rd(row,offset))
}
pub(crate) unsafe fn helper_kind(view:helpers::View,id:usize,owner:Owner,kind:u32)->bool {
    let Some(ordinal)=view.ordinal(id) else {
        return !cfg!(feature="mii-managed-source-coexist-alpha")
            && i64::from(rd::<i8>(owner.row,8))==i64::from(kind);
    };
    let now=helpers::Proof {original:helpers::OriginalRow {row:owner.row,
        kind:rd(owner.row,8),index:owner.index,selector:owner.selector,control:control(owner.row)},
        thread:skyline::nn::os::GetCurrentThread() as usize,owner,kind,costume:rd(owner.object,0x64)};
    let managed=matches!(view.slots.get(ordinal),Some(helpers::Helper::Ready{..}));
    let parked=!managed || (rd::<u32>(owner.object,0x54)==id as u32
        && super::super::native_source::parked_ready(owner.entry as *const u8,id as u32,kind));
    if !view.accepts(id,now,parked) {return false;}
    if let Some(Some(mii))=view.miis.get(ordinal) {
        let b=mii.binding;
        if b.epoch!=super::super::mii_saved_alpha::current_generation(1)
            || rd::<usize>(super::base(),POOL_HOLDER)!=b.holder
            || !span(b.holder,8,false) || rd::<usize>(b.holder,0)!=b.manager
            || !span(b.manager,0x24e0,false) || rd::<usize>(b.manager,0x24d0)!=b.vector_begin
            || rd::<usize>(b.manager,0x24d8)!=b.vector_end {return false;}
        let Ok(at)=values::record_address(b,b) else {return false;};
        if !span(at,values::RECORD_LEN,false) || !mii.matches(b,&rd(owner.entry,0),&rd(at,0),mirrors(owner)) {return false;}
    }
    true
}

#[repr(C)]
#[derive(Default)]
struct MemoryInfo {
    address: u64, size: u64, kind: u32, attributes: u32, permission: u32,
    ipc: u32, device: u32, padding: u32,
}
/// Horizon QueryMemory, the same pinned SVC ABI as the local Deluxe SDK reader.
/// One finite span per object; does not pin lifetime. Current callback/map/thread supplies lifetime.
pub(crate) unsafe fn span(address: usize, size: usize, write: bool) -> bool {
    let Some(end) = address.checked_add(size) else { return false };
    if address < 0x10000 || size == 0 { return false; }
    let mut info = MemoryInfo::default();
    let result: u64;
    core::arch::asm!("svc 0x6",
        inout("x0") &mut info as *mut MemoryInfo as u64 => result,
        lateout("x1") _, in("x2") address as u64,
        clobber_abi("C"), options(nostack));
    let required = if write { 3 } else { 1 };
    result as u32 == 0 && info.permission & required == required
        && info.address <= address as u64
        && info.address.checked_add(info.size).is_some_and(|e| end as u64 <= e)
}
pub(crate) fn check(ok: bool, reason: &'static str) -> Result<(), &'static str> {
    if ok { Ok(()) } else { Err(reason) }
}
pub(crate) unsafe fn pointer(at: usize, offset: usize, size: usize, write: bool) -> Result<usize, &'static str> {
    let slot=at.checked_add(offset).ok_or("pointer overflow")?;
    check(span(slot,8,false), "pointer slot unmapped")?;
    let p: usize = rd(slot,0);
    check(aligned(p) && span(p, size, write), "target span unavailable")?;
    Ok(p)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Scalars {
    pub e: Tokens,
    pub d_outfit: u16,
    pub d_colour: u16,
    pub d_specials: [u32; 4],
}
impl Scalars {
    pub(crate) unsafe fn read(e: usize, d: usize) -> Self {
        Self { e: Tokens::from_row(e), d_outfit: rd(d,0x74),
            d_colour: rd(d,0x78), d_specials: rd(d,0x390) }
    }
    pub(crate) fn saved(c: Configuration) -> Self {
        let t=c.tokens();
        Self { e:t.into(), d_outfit:t.outfit,d_colour:t.colour,d_specials:t.specials.map(u32::from) }
    }
    /// Only the individually qualified E and D scalars. FE/O publication belongs to native code.
    pub(crate) unsafe fn write(self,e:usize,d:usize) {
        wr(e,0x1aa,self.e.outfit);wr(e,0x1ae,self.e.colour);wr(e,0x1b0,self.e.voice);
        for (i,v) in self.e.specials.iter().enumerate() { wr(e,0x1b2+i,*v); }
        wr(d,0x74,self.d_outfit);wr(d,0x78,self.d_colour);
        for (i,v) in self.d_specials.iter().enumerate() { wr(d,0x390+i*4,*v); }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Protected {
    owner: Owner,
    descriptor: [u8; 0x70],
    e: [u8;14], d: [u8;8], specials:[u32;4], o:[u8;20], control:[u8;7],
    record:[u8;values::RECORD_LEN],
    current_record:Option<[u8;values::RECORD_LEN]>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub layout:super::super::mii_backend::Layout,
    pub helpers: helpers::View,
    pub binding: Binding,
    pub thread: usize,
    pub original_kind: u32,
    pub descriptor: [u8;0x70],
    pub scalars: Scalars,
    pub record:[u8;values::RECORD_LEN],
    control:[u8;7],
    symbol:usize, manager:usize, internal:usize,
    entries:[usize;8], owners:[Option<Owner>;8], protected:[Option<Protected>;8],
}
impl Snapshot {
    fn authority(&self)->Authority {
        authority(self.binding,self.thread,[self.symbol,self.manager,self.internal],
            self.entries,0,self.owners[0].unwrap())
    }
}
pub(super) fn same_owned(a:&Snapshot,b:&Snapshot)->bool {
    a.binding==b.binding && a.thread!=0 && a.thread==b.thread
        && a.symbol==b.symbol && a.manager==b.manager && a.internal==b.internal
        && a.entries==b.entries && a.owners[0]==b.owners[0] && a.control==b.control
        && a.descriptor==b.descriptor && a.scalars==b.scalars && a.record==b.record
}
#[derive(Clone, Copy)]
struct Map { layout:super::super::mii_backend::Layout,symbol:usize, manager:usize, internal:usize, entries:[usize;8] }
unsafe fn map(entry: usize) -> Result<Map,&'static str> {
    let layout=super::super::mii_backend::current().ok_or("Mii backend unavailable")?;
    let symbol=training_mod_sync::read(&crate::common::FIGHTER_MANAGER_ADDR);
    let manager=pointer(symbol,0,8,false)?;
    let internal=pointer(manager,0,0xa4,false)?;
    let entries=std::array::from_fn(|id| {
        smash::app::lua_bind::FighterManager::get_fighter_entry(
            manager as *mut smash::app::FighterManager,
            smash::app::FighterEntryID(id as i32)) as usize
    });
    check(entries[0]==entry && rd::<[usize;8]>(internal,0x20)==entries
        && rd::<[usize;8]>(internal,0x60)==[0;8],"current primary owner map")?;
    check(rd::<u32>(internal,0xa0) as usize==entries.iter().filter(|p| **p!=0).count(),
        "current map count")?;
    if cfg!(feature="mii-managed-source-coexist-alpha") || layout.waiting() {
        check(layout.shape(&entries),"Mii backend owner shape differs")?;
    }
    if layout.waiting() {
        check(super::super::native_source::exact_waiting_layout(entries[5] as *const u8,5),"waiting row layout changed")?;
    }
    Ok(Map{layout,symbol,manager,internal,entries})
}
unsafe fn pool(base:usize, entry:usize, epoch:u64) -> Result<Binding,&'static str> {
    let holder=pointer(base,POOL_HOLDER,8,false)?;
    let manager=pointer(holder,0,0x24e0,true)?;
    let row=base.checked_add(0x52c5730).ok_or("E overflow")?;
    check(span(row,0x260,true)&&span(entry,0x5a88,true),"current FE/E spans")?;
    let b=Binding {epoch,owner:0,entry,row,holder,manager,
        vector_begin:rd(manager,0x24d0),vector_end:rd(manager,0x24d8),
        record_index:rd(row,0x1a8)};
    let record=values::record_address(b,b).map_err(|_|"native-selected record bounds")?;
    check(span(record,values::RECORD_LEN,true),"selected record span")?;
    Ok(b)
}
unsafe fn current_locked(base:usize, m:Map, locked:Binding,
    original_kind:u32, staged:Option<(&[u8;0x70],usize)>, helpers:helpers::View)
    -> Result<Snapshot,&'static str> {
    let entry=locked.entry;let epoch=locked.epoch;
    check_helpers(helpers)?;
    check(rd::<usize>(m.symbol,0)==m.manager && rd::<usize>(m.manager,0)==m.internal
        && rd::<[usize;8]>(m.internal,0x20)==m.entries
        && rd::<[usize;8]>(m.internal,0x60)==[0;8],"owner map changed")?;
    let b=pool(base,entry,epoch)?;
    check(helpers.layout==m.layout,"helper backend changed")?;
    check(b==locked,"G/vector/index changed under lock")?;
    let table=pointer(base,OWNER_TABLE,0x130,false)?;
    let mut owners=[None;8];
    let mut kinds=[None;8];
    let mut protected=[None;8];
    for (id,fe) in m.entries.iter().copied().enumerate() {
        if fe==0 { continue; }
        let e=base+0x52c5730+id*0x260;
        check(aligned(fe)&&span(fe,0x5a88,false)&&span(e,0x260,false),"other owner FE/E span")?;
        let d=pointer(fe,0xf8,0x3a0,id==0)?;
        let o=pointer(table,0xe8+id*8,0xb0,false)?;
        let index: u16=rd(e,0x1a8);
        check(rd::<u32>(fe,0xe0)==id as u32 && rd::<u32>(e,0x14)==id as u32
            && rd::<u32>(fe,4)==u32::from(rd::<u8>(e,0xa7))
            && rd::<u16>(fe,0x46)==index && rd::<u16>(d,0x72)==index,
            "owner/index mirror changed")?;
        if id==0 {check(role_owner::original_row_matches(original_kind,rd(e,8)),
            "original row identity changed")?;}
        if !(id==0 && staged.is_some()) {
            check(rd::<u16>(o,0x68)==index,"owner object index mismatch")?;
        }
        let owner=Owner {entry:fe,row:e,information:d,object:o,index,
            selector:rd(e,0xa7),alias:rd(table,0x128+id)};
        owners[id]=Some(owner);
        let kind=rd::<u32>(fe,0x18);
        kinds[id]=Some(kind);
        if !(id==0 && staged.is_some()) {
            check(rd::<u32>(o,0x58)==kind,"current owner header kind differs")?;
        }
        if id!=0 {
            if m.layout.logical(id as u32).is_some() {
                let current=authority(b,skyline::nn::os::GetCurrentThread() as usize,
                    [m.symbol,m.manager,m.internal],m.entries,id as u32,owner);
                let original=super::super::original_kind(super::super::Role::Cpu)
                    .ok_or("CPU original absent")?;
                check(foreign_kind(current,original,rd(e,8),kind),"foreign original row/kind authority changed")?;
            } else {check(helper_kind(helpers,id,owner,kind),"helper row kind/provenance changed")?;}
            let other=Binding {owner:id as u32,entry:fe,row:e,record_index:u16::from(owner.selector),..b};
            let at=values::record_address(other,other).map_err(|_|"protected record bounds")?;
            check(span(at,values::RECORD_LEN,false),"protected record span")?;
            let cur=Binding {record_index:index,..other};
            let current_at=values::record_address(cur,cur).map_err(|_|"protected current record bounds")?;
            check(span(current_at,values::RECORD_LEN,false),"protected current record span")?;
            protected[id]=Some(Protected{owner,descriptor:rd(fe,0),e:rd(e,0x1a8),
                d:rd(d,0x72),specials:rd(d,0x390),o:rd(o,0x68),control:control(e),record:rd(at,0),
                // Ordinary index0 is a placeholder, not a foreign claim on Player0's G0.
                current_record:role_owner::current_record_claim(kind,index).then(||rd(current_at,0))});
        }
    }
    check(if m.layout.waiting() {role_owner::claims_in(m.layout,&owners,&kinds).is_some()}
        else {training_mod_consts::mii::ordinary_player::unique_player(&owners,&kinds)},
        "owner/selector/index alias or inactive owner conflict")?;
    let owner=owners[0].ok_or("Player0 absent")?;
    if let Some((_,o))=staged { check(owner.object==o,"selected native O differs")?; }
    let descriptor:[u8;0x70]=rd(entry,0);
    if let Some((expected,_))=staged { check(descriptor==*expected,"incoming descriptor differs")?; }
    let at=values::record_address(b,b).map_err(|_|"selected bounds changed")?;
    check(rd::<usize>(m.symbol,0)==m.manager && rd::<usize>(m.manager,0)==m.internal
        && rd::<[usize;8]>(m.internal,0x20)==m.entries
        && rd::<[usize;8]>(m.internal,0x60)==[0;8]
        && rd::<usize>(base,OWNER_TABLE)==table
        && rd::<usize>(table,0xe8)==owner.object
        && rd::<usize>(entry,0xf8)==owner.information,
        "owner authority changed during bounded read")?;
    check_helpers(helpers)?;
    Ok(Snapshot {layout:m.layout,helpers,binding:b,thread:skyline::nn::os::GetCurrentThread() as usize,
        original_kind,descriptor,scalars:Scalars::read(b.row,owner.information),record:rd(at,0),control:control(b.row),
        symbol:m.symbol,manager:m.manager,internal:m.internal,entries:m.entries,owners,protected})
}
pub(super) unsafe fn snapshot(entry:usize,epoch:u64,original_kind:u32) -> Result<Snapshot,&'static str> {
    let helpers=helper_snapshot()?;
    let base=super::base();
    let m=map(entry)?;
    let b=pool(base,entry,epoch)?;
    let g=PoolLock::acquire(base,b.manager);
    let result=current_locked(base,m,b,original_kind,None,helpers);
    drop(g);
    result
}
pub(crate) unsafe fn target_rows(c:Configuration) -> Result<RowIds,&'static str> {
    let base=super::base();let t=c.tokens();
    let ids=RowIds {outfit:lookup_row_id(base,OUTFIT_TABLE,t.outfit)?,
        hat:lookup_row_id(base,HAT_TABLE,t.hat)?};
    ids.validate(c).map_err(|_|"saved appearance hint/hash mismatch")?;
    Ok(ids)
}

/// Call only inside this adapter's freshly rederived all-owner/G critical section.
/// The returned values are comparison evidence, never a saved pointer to replay later.
unsafe fn same_class_values(now:&Snapshot)->Result<SameClassValues,&'static str> {
    let o=now.owners[0].ok_or("same-class Player0 absent")?;
    check(span(o.object,0xb0,true),"same-class writable O span")?;
    let task:usize=rd(o.object,0xa8);
    let task_idle=task==0 || (aligned(task)&&span(task,0x24,false)
        && crate::native_fields::load_acquire_u32(task + 0x10)==0);
    let values=SameClassValues {
        binding:now.binding,thread:now.thread,owner:o,
        header:rd(o.object,0x54),
        mirrors:values::Mirrors::decode(rd(o.row,0x1a8),rd(o.entry,0x46),
            rd(o.information,0x72),rd(o.information,0x390),rd(o.object,0x68)),
        initialized:rd::<u8>(o.object,0x4c)&1!=0,task_idle,
        rebuild_open:rd::<u8>(o.information,0x94)==0,
    };
    let table=pointer(super::base(),OWNER_TABLE,0x130,false)?;
    check(rd::<usize>(table,0xe8)==o.object && rd::<usize>(o.entry,0xf8)==o.information
        && rd::<usize>(o.object,0xa8)==task,"same-class read authority changed")?;
    Ok(values)
}

/// Preflight: every mirror still describes the BEFORE configuration. No writes.
pub(super) unsafe fn prepare_same_class(expected:Snapshot,before:Configuration,after:Configuration)
    ->Result<SameClass,&'static str> {
    let rows=[target_rows(before)?,target_rows(after)?];
    let base=super::base();let b=expected.binding;
    let m=map(b.entry)?;let current=pool(base,b.entry,b.epoch)?;
    check(current==b,"same-class preflight binding changed")?;
    let helpers=helper_snapshot()?;
    let g=PoolLock::acquire(base,b.manager);
    let now=current_locked(base,m,b,expected.original_kind,None,helpers)?;
    check(now==expected,"same-class preflight snapshot changed")?;
    let at=values::record_address(b,b).map_err(|_|"same-class record bounds")?;
    check(rd::<usize>(at,0x58)!=0 && rd::<usize>(at,0x60)!=0,
        "same-class active Mii resource pair absent")?;
    let proof=SameClass::prepare(before,after,same_class_values(&now)?,
        &now.descriptor,rows,&now.record)?;
    drop(g);
    Ok(proof)
}

/// Explicit native branch selection, never a fallback after a failed proof. The unchanged
/// publisher rechecks bit0 clear under current authority; the retained arm requires bit0 set.
pub(super) unsafe fn publish_same_class(track:&super::Track,selected:usize)
    ->Result<bool,&'static str> {
    let proof=track.same_class.ok_or("same-class proof absent")?;
    check(track.publish==Some(proof.target()),"same-class target changed")?;
    // Row getters are native calls and run before acquiring G or adapter STATE, for either arm.
    let rows=[target_rows(proof.before())?,target_rows(proof.target())?];
    check(aligned(selected)&&span(selected,0xb0,false),"same-class selected O span")?;
    if rd::<u8>(selected,0x4c)&1==0 {
        // Native itself will publish its header and FE tokens to this selected O.
        // A flag/authority change during rederivation refuses inside publish; do not switch arms.
        publish(track,selected)?;
        return Ok(false);
    }
    let base=super::base();let entry=track.ticket.entry;
    let m=map(entry)?;let b=pool(base,entry,track.ticket.epoch)?;
    check(b==track.before.binding,"same-class phase binding differs")?;
    let helpers=helper_snapshot()?;
    check(helpers==track.before.helpers,"same-class source provenance changed")?;
    let g=PoolLock::acquire(base,b.manager);
    let now=current_locked(base,m,b,track.before.original_kind,
        Some((&track.descriptor,selected)),helpers)?;
    check(now.thread!=0 && now.thread==track.before.thread
        && now.symbol==track.before.symbol && now.manager==track.before.manager
        && now.internal==track.before.internal && now.entries==track.before.entries
        && now.protected==track.before.protected,"same-class protected authority changed")?;
    check(now.record==track.before.record && now.scalars==track.before.scalars,
        "same-class owned record/scalars changed")?;
    check(rd::<u8>(entry,0x5920)==3 && rd::<u8>(entry,0x591c)==6,
        "same-class not active native loader6")?;
    let observed=same_class_values(&now)?;
    let update=proof.prepare_write(observed,&track.descriptor,rows,&now.record)?;
    let record=values::record_address(b,b).map_err(|_|"same-class record changed")?;
    let targets=Targets {e:b.row,fe:entry,d:observed.owner.information,o:observed.owner.object};
    let mut state=super::STATE.lock().unwrap();
    let fresh=state.track.as_mut().ok_or("same-class ticket retired")?;
    check(fresh.ticket==track.ticket && fresh.same_class==Some(proof)
        && fresh.gate.may_publish(track.ticket,6) && super::epoch()==track.ticket.epoch,
        "same-class phase ticket/epoch changed")?;
    let registry=final_registry(helpers,Some(track.token))?;
    // No fallible operation, row getter or native callback after the first write.
    copy_update(record,&update);
    write_tokens(&targets,update.tokens().into());
    fresh.retained_o=true;
    let published=fresh.gate.publish(track.ticket,6);
    debug_assert!(published);
    drop(registry);drop(state);drop(g);
    Ok(true)
}

/// All checks precede the write. Lock order: G -> final adapter state lock, solely for the
/// bounded raw publication. No path holds adapter state while acquiring G or calling native.
pub(super) unsafe fn publish(track:&super::Track, selected:usize) -> Result<(),&'static str> {
    if let Some(c)=track.publish {target_rows(c)?;}
    let base=super::base();let entry=track.ticket.entry;
    let m=map(entry)?;
    let b=pool(base,entry,track.ticket.epoch)?;
    check(b==track.before.binding,"phase binding differs")?;
    let helpers=helper_snapshot()?;
    check(helpers==track.before.helpers,"phase source provenance changed")?;
    let g=PoolLock::acquire(base,b.manager);
    let now=current_locked(base,m,b,track.before.original_kind,
        Some((&track.descriptor,selected)),helpers)?;
    check(now.thread!=0 && now.thread==track.before.thread,"phase native thread differs")?;
    check(now.symbol==track.before.symbol && now.manager==track.before.manager
        && now.internal==track.before.internal && now.entries==track.before.entries
        && now.protected==track.before.protected,"protected owner state changed")?;
    check(now.record==track.before.record && now.scalars==track.before.scalars,
        "owned record/scalars changed before publication")?;
    let o=now.owners[0].ok_or("Player0 absent")?;
    check(rd::<u8>(entry,0x5920)==3 && rd::<u8>(entry,0x591c)==6,
        "not active native loader6")?;
    check(rd::<u8>(selected,0x4c)&1==0,"selected O already initialized")?;
    let task:usize=rd(selected,0xa8);
    // O+a8 owns a native 0x80-byte job: +0x10 is the outstanding counter.
    // Queue3549720 increments it; worker354a7e0 decrements after the callback.
    // Match that release with Acquire; +0x20 is not this job's status field.
    check(task==0 || (aligned(task)&&span(task,0x24,false)
        && crate::native_fields::load_acquire_u32(task + 0x10)==0),
        "old O task not quiescent")?;
    if track.publish.is_some() {
        check(rd::<u8>(o.information,0x94)==0,"native Mii rebuild gate is closed")?;
    }
    // State/epoch is checked again immediately at the publication linearization point.
    let mut state=super::STATE.lock().unwrap();
    let fresh=state.track.as_mut().ok_or("ticket retired")?;
    check(fresh.ticket==track.ticket && fresh.gate.may_publish(track.ticket,6)
        && super::epoch()==track.ticket.epoch,"phase ticket/epoch changed")?;
    let record=values::record_address(b,b).map_err(|_|"record changed")?;
    let scalars=track.publish.map_or(track.origin_scalars,Scalars::saved);
    check(scalars.e.hat==now.scalars.e.hat,"unqualified hat change")?;
    let registry=final_registry(helpers,Some(track.token))?;
    // No fallible operation or native callback after this point.
    if track.publish.is_some() {
        let update=training_mod_consts::mii::ordinary_player::retained_record(now.record,track.publish);
        for (off,byte) in update[..CHARINFO_LEN].iter().enumerate() {wr(record,off,*byte);}
    }
    scalars.write(b.row,o.information);
    let published=fresh.gate.publish(track.ticket,6);
    debug_assert!(published);
    drop(registry);drop(state);
    drop(g);
    Ok(())
}
/// Current ready callback after native construction. Native replaces resource/control words;
/// compare G payload and all owned mirrors, never fabricate or clear its shared_ptr.
pub(super) unsafe fn completed(track:&super::Track) -> Result<Snapshot,&'static str> {
    let base=super::base();
    let m=map(track.ticket.entry)?;
    let b=pool(base,track.ticket.entry,track.ticket.epoch)?;
    let helpers=helper_snapshot()?;
    check(helpers==track.before.helpers,"ready source provenance changed")?;
    let g=PoolLock::acquire(base,b.manager);
    let now=current_locked(base,m,b,track.before.original_kind,None,helpers)?;
    check(now.binding==track.before.binding && now.thread==track.before.thread
        && now.symbol==track.before.symbol && now.manager==track.before.manager
        && now.internal==track.before.internal && now.entries==track.before.entries
        && now.control==track.before.control
        && now.protected==track.before.protected,"ready authority/protected state changed")?;
    check(now.descriptor==track.descriptor,"ready descriptor changed")?;
    let wanted=track.publish.map_or(track.origin_scalars,Scalars::saved);
    check(now.scalars==wanted,"ready E/D configuration mismatch")?;
    let owner=now.owners[0].ok_or("ready Player0 absent")?;
    let at=values::record_address(b,b).map_err(|_|"ready record bounds")?;
    let bytes:[u8;values::RECORD_LEN]=rd(at,0);
    let expected=track.publish.map(|c|*c.charinfo())
        .unwrap_or_else(||track.before.record[..CHARINFO_LEN].try_into().unwrap());
    check(bytes[..CHARINFO_LEN]==expected,"ready retained/target CharInfo mismatch")?;
    let tokens:[u16;5]=rd(track.ticket.entry,0x46);
    check(rd::<[u16;4]>(owner.information,0x72)==tokens[..4],
        "native D descriptor mirror mismatch")?;
    check(rd::<[u32;4]>(owner.object,0x6c)==[
        u32::from(tokens[1]),u32::from(tokens[2]),u32::from(tokens[3]),u32::from(tokens[4])],
        "native O descriptor tokens mismatch")?;
    if track.publish.is_some() {
        check(rd::<usize>(at,0x58)!=0 && rd::<usize>(at,0x60)!=0,"native Mii resource pair absent")?;
    }
    if track.retained_o {
        let proof=track.same_class.ok_or("same-class completion proof absent")?;
        proof.completed(same_class_values(&now)?,&track.descriptor)?;
    }
    let mut state=super::STATE.lock().unwrap();
    check(state.track.as_ref().is_some_and(|t|t.ticket==track.ticket
        && t.gate.can_complete(track.ticket)) && super::epoch()==track.ticket.epoch,
        "ready ticket/epoch changed before commit")?;
    let owner=state.owner.as_mut().ok_or("ready owner retired")?;
    let mut registry=final_registry(helpers,Some(track.token))?;
    owner.expected=now;owner.active=track.publish;owner.cursor=track.destination.cursor;
    registry.commit(now.authority(),now.original_kind,super::descriptor_kind(&now.descriptor));
    drop(registry);drop(state);drop(g);
    Ok(now)
}

/// Ready proof for the generic roster loader, which writes nothing here. Compares the fresh
/// state only with the receipt's `before` snapshot and the staged generic descriptor; arbitrary
/// current state is never adopted. Native may replace the O object, so O identity is not compared.
/// Lock order G -> adapter STATE -> COMMITTED; the registry is only fenced, never written.
pub(super) unsafe fn generic_completed(g:&super::Generic,kind:u32) -> Result<Snapshot,&'static str> {
    let before=&g.before;
    let base=super::base();
    let m=map(before.binding.entry)?;
    let b=pool(base,before.binding.entry,before.binding.epoch)?;
    check(b==before.binding,"generic ready G identity changed")?;
    let helpers=helper_snapshot()?;
    check(helpers==before.helpers,"generic ready source provenance changed")?;
    let lock=PoolLock::acquire(base,b.manager);
    let now=current_locked(base,m,b,before.original_kind,None,helpers)?;
    check(now.binding==before.binding && now.thread==before.thread
        && now.symbol==before.symbol && now.manager==before.manager
        && now.internal==before.internal && now.entries==before.entries
        && now.control==before.control
        && now.protected==before.protected,"generic ready authority/protected state changed")?;
    check(now.descriptor==g.descriptor,"generic ready descriptor differs")?;
    check(now.scalars==before.scalars,"generic ready E/D configuration changed")?;
    check(now.record[..CHARINFO_LEN]==before.record[..CHARINFO_LEN],"generic ready CharInfo changed")?;
    check(now.record==before.record,"generic ready retained G resource changed")?;
    let (old,own)=(before.owners[0].ok_or("generic ready prior Player0 absent")?,
        now.owners[0].ok_or("generic ready Player0 absent")?);
    check(own.entry==old.entry && own.row==old.row && own.selector==old.selector
        && own.alias==old.alias,"generic ready Player0 identity changed")?;
    let word=|offset:usize|u32::from_le_bytes(g.descriptor[offset..offset+4].try_into().unwrap());
    check(rd::<[u32;5]>(own.object,0x54)==[0,word(0x18),word(0x1c),word(0x20),u32::from(g.descriptor[0x41])],
        "generic ready native O header differs")?;
    let task:usize=rd(own.object,0xa8);
    check(task==0 || (aligned(task)&&span(task,0x24,false)
        && crate::native_fields::load_acquire_u32(task + 0x10)==0),"generic ready task outstanding")?;
    let tokens:[u16;5]=rd(before.binding.entry,0x46);
    check(rd::<[u16;4]>(own.information,0x72)==tokens[..4]
        && rd::<[u32;4]>(own.object,0x6c)==[
            u32::from(tokens[1]),u32::from(tokens[2]),u32::from(tokens[3]),u32::from(tokens[4])],
        "generic ready native D/O propagation differs")?;
    let mut state=super::STATE.lock().unwrap();
    let transaction_busy=state.track.is_some() || state.request.is_some()
        || super::epoch()!=before.binding.epoch;
    let owner=state.owner.as_mut().filter(|o|o.expected==g.expected)
        .ok_or("generic Player0 owner changed during INSTALL")?;
    training_mod_consts::mii::ordinary_player::generic_ordinary_commit(
        training_mod_consts::mii::ordinary_player::GenericOrdinaryFacts {
            transaction_busy,origin_is_saved:owner.origin_saved.is_some(),
            active_is_saved:owner.active.is_some(),cursor_owned:owner.cursor.is_some(),
            kind,published_kind:super::descriptor_kind(&now.descriptor),
            index:own.index,original_index:old.index,
        })?;
    let registry=final_registry(helpers,Some(g.token))?;
    owner.expected=now;
    drop(registry);drop(state);drop(lock);
    Ok(now)
}

/// Exact catalog origin capture reuses the same reader and claim authority as later tickets.
/// Row getters run outside G; the final full snapshot is rederived before installing an owner.
pub(super) unsafe fn capture_origin(entry:usize,epoch:u64,kind:u32,catalog:&Catalog,opening:u64)
    ->Result<(),helpers::CaptureError> {
    let previous=super::STATE.lock().unwrap().owner.as_ref().map(|o|o.expected);
    if let Some(expected)=previous {
        let now=snapshot(entry,epoch,kind)?;
        check(same_owned(&now,&expected),"menu current owner changed")?;
        let mut state=super::STATE.lock().unwrap();
        check(super::super::mii_saved_alpha::menu_epoch()==opening
            && state.capture && state.track.is_none() && super::epoch()==epoch
            && state.owner.as_ref().is_some_and(|o|o.expected==expected),"menu owner retired")?;
        let registry=final_registry(now.helpers,None)?;
        state.capture=false;state.menu_valid=true;
        drop(registry);return Ok(());
    }
    let before=snapshot(entry,epoch,kind)?;
    let c=if MiiClass::from_kind(kind).is_some() {
        let o=before.owners[0].ok_or("saved origin owner absent")?;
        // This first copied selection is not publication authority; it is rechecked below.
        let base=super::base();let m=map(entry)?;let b=pool(base,entry,epoch)?;
        let helpers=helper_snapshot()?;
        let g=PoolLock::acquire(base,b.manager);
        let fresh=current_locked(base,m,b,kind,None,helpers)?;
        check(fresh==before,"origin changed before catalog resolution")?;
        let c=role_owner::saved_origin(catalog,kind,mirrors(o),&fresh.record)
            .ok_or("no exact original saved configuration")?;
        drop(g);Some(c)
    } else {None};
    let rows=c.map(|c|target_rows(c)).transpose()?;
    let base=super::base();let m=map(entry)?;let b=pool(base,entry,epoch)?;
    let helpers=helper_snapshot()?;
    let g=PoolLock::acquire(base,b.manager);
    let now=current_locked(base,m,b,kind,None,helpers)?;
    check(now==before && now.thread!=0,"origin changed across row validation")?;
    if let (Some(c),Some(rows))=(c,rows) {
        role_owner::validate_saved(c,0,mirrors(now.owners[0].unwrap()),rows,&now.record)?;
    }
    let mut state=super::STATE.lock().unwrap();
    check(super::super::mii_saved_alpha::menu_epoch()==opening
        && state.capture && state.owner.is_none() && state.track.is_none()
        && super::epoch()==epoch,"origin capture retired")?;
    let mut registry=final_registry(helpers,None)?;
    state.owner=Some(super::Owner{origin:now,origin_saved:c,expected:now,active:c,cursor:None});
    state.capture=false;state.menu_valid=true;
    registry.commit(now.authority(),kind,kind);
    drop(registry);drop(state);drop(g);
    super::super::observe_mii_origin(super::super::Role::Player,epoch,kind,super::descriptor_forms(&now.descriptor),c);
    Ok(())
}
