//! Ordinary P1 staged reload policy. Values only; no native access or saved-origin weakening.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Owner {
    pub entry: usize,
    pub row: usize,
    pub information: usize,
    pub object: usize,
    pub index: u16,
    pub selector: u8,
    pub alias: u8,
}

/// Player owns claim0; non-Mii index0 rows do not claim its payload.
pub fn unique_player(owners: &[Option<Owner>; 8], kinds: &[Option<u32>; 8]) -> bool {
    owners[0].is_some() && crate::role_owner::claims(owners,kinds).is_some()
}

/// Facts for an ordinary Player0 load that stayed on the generic roster loader.
/// The roster cursor is deliberately absent: its slots are not adapter cycle positions,
/// so the adapter keeps resolving its position by identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenericOrdinaryFacts {
    pub transaction_busy: bool,
    pub origin_is_saved: bool,
    pub active_is_saved: bool,
    pub cursor_owned: bool,
    pub kind: u32,
    pub published_kind: u32,
    pub index: u16,
    pub original_index: u16,
}

/// Permission to refresh `expected` after the native-ready proof. This grants no INSTALL,
/// no cursor and no saved-Mii authority; it only says the adapter owns nothing here.
pub fn generic_ordinary_commit(f: GenericOrdinaryFacts) -> Result<(), &'static str> {
    if f.transaction_busy { return Err("generic ordinary Player0 transaction busy"); }
    if f.origin_is_saved || f.active_is_saved || f.cursor_owned {
        return Err("owned Player0 configuration stays on the adapter");
    }
    if f.kind != f.published_kind { return Err("generic ordinary Player0 published kind differs"); }
    if crate::inventory::MiiClass::from_kind(f.kind).is_some() {
        return Err("Mii Player0 transition is not a generic ordinary commit");
    }
    if f.index != f.original_index { return Err("generic ordinary Player0 changed original index"); }
    Ok(())
}

/// Owned descriptor construction. INSTALL, not the adapter, publishes these bytes to FE.
pub fn staged_descriptor(mut original:[u8;0x70], target:Option<crate::lifetime::Configuration>) -> [u8;0x70] {
    if let Some(c)=target {
        let word=|b:&mut [u8;0x70],o:usize,v:u32|b[o..o+4].copy_from_slice(&v.to_le_bytes());
        word(&mut original,0x14,1);word(&mut original,0x18,c.canonical().class as u32);
        word(&mut original,0x1c,u32::MAX);word(&mut original,0x20,u32::MAX);
        word(&mut original,0x28,1);original[0x43]=0;
        for (off,v) in [(0x48,c.tokens().outfit),(0x4c,c.tokens().colour),(0x4e,c.tokens().voice)] {
            original[off..off+2].copy_from_slice(&v.to_le_bytes());
        }
    }
    original
}
/// Owned value only. Returning ordinary retains the last Mii payload; both native shared_ptr
/// words remain unchanged in this operation. The native adapter writes only the first88 bytes.
pub fn retained_record(mut before:[u8;0x68],target:Option<crate::lifetime::Configuration>) -> [u8;0x68] {
    if let Some(c)=target {before[..88].copy_from_slice(c.charinfo());}
    before
}

/// Copied values from the ordinary adapter's CURRENT all-owner/G critical section.
/// This is not pointer or write authority; native glue must supply spans, exact protected
/// owners, current thread/epoch, INSTALL and the executing ticket before publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SameClassValues {
    pub binding: crate::lifetime::Binding,
    pub thread: usize,
    pub owner: Owner,
    pub header: [u32; 5], // native O+54 owner, +58/+5c/+60 kinds, +64 FE+41
    pub mirrors: crate::native_values::Mirrors,
    pub initialized: bool,
    pub task_idle: bool,
    pub rebuild_open: bool,
}

/// A same-class configuration change belongs to an ordinary origin, never a SavedRole.
/// Its fields are private so publication cannot manufacture a preflight certificate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SameClass {
    before: crate::lifetime::Configuration,
    after: crate::lifetime::Configuration,
    current: SameClassValues,
}
impl SameClass {
    pub fn prepare(
        before: crate::lifetime::Configuration,
        after: crate::lifetime::Configuration,
        current: SameClassValues,
        descriptor: &[u8; 0x70],
        rows: [crate::native_values::RowIds; 2],
        record: &[u8; 0x68],
    ) -> Result<Self, &'static str> {
        Self::prepare_for(0,before,after,current,descriptor,rows,record)
    }
    /// CPU1 has its own current protected-union reader; this does not admit it through
    /// unique_player or change the Player0 constructor's exact authority.
    pub fn prepare_cpu(
        before:crate::lifetime::Configuration,after:crate::lifetime::Configuration,
        current:SameClassValues,descriptor:&[u8;0x70],
        rows:[crate::native_values::RowIds;2],record:&[u8;0x68],
    )->Result<Self,&'static str> {
        Self::prepare_for(1,before,after,current,descriptor,rows,record)
    }
    pub fn prepare_cpu_in(layout:crate::backend::Layout,
        before:crate::lifetime::Configuration,after:crate::lifetime::Configuration,
        current:SameClassValues,descriptor:&[u8;0x70],
        rows:[crate::native_values::RowIds;2],record:&[u8;0x68])->Result<Self,&'static str> {
        Self::prepare_for(layout.visible()[1] as u8,before,after,current,descriptor,rows,record)
    }
    fn prepare_for(role:u8,before:crate::lifetime::Configuration,after:crate::lifetime::Configuration,
        current:SameClassValues,descriptor:&[u8;0x70],
        rows:[crate::native_values::RowIds;2],record:&[u8;0x68])->Result<Self,&'static str> {
        let b=current.binding;
        let o=current.owner;
        if b.owner!=u32::from(role) || b.record_index!=u16::from(role) || b.epoch==0 || current.thread==0
            || o.entry!=b.entry || o.row!=b.row || o.index!=b.record_index
            || o.selector!=role || o.alias!=0
            || [o.entry,o.row,o.information,o.object].iter().any(|p|*p==0 || !p.is_multiple_of(8))
            || before.canonical().class!=after.canonical().class
            || before==after
        {return Err("same-class ordinary owner/target mismatch");}
        crate::native_values::record_address(b,b).map_err(|_|"same-class record binding")?;
        // O+4c bit0 selects native configuration; it is not a stable READY latch
        // (plan2-mii-owner-init-bit01). Publication freshly chooses clear/set;
        // prepare_write still requires set before any retained-O manual write.
        if !current.task_idle {
            return Err("same-class retained O task not quiescent");
        }
        Self::header(current.header,descriptor,before,b.owner)?;
        current.mirrors.validate(b.record_index,before.tokens())
            .map_err(|_|"same-class preflight mirrors differ")?;
        let proof=Self {before,after,current};
        proof.update(rows,record)?;
        Ok(proof)
    }
    pub fn before(self)->crate::lifetime::Configuration {self.before}
    pub fn target(self)->crate::lifetime::Configuration {self.after}
    fn header(header:[u32;5],d:&[u8;0x70],c:crate::lifetime::Configuration,owner:u32)
        ->Result<(),&'static str> {
        let word=|o:usize|u32::from_le_bytes(d[o..o+4].try_into().unwrap());
        if word(0x14)!=1 || word(0x18)!=c.canonical().class as u32
            || word(0x1c)!=u32::MAX || word(0x20)!=u32::MAX
            || header!=[owner,word(0x18),word(0x1c),word(0x20),u32::from(d[0x41])]
        {return Err("same-class current O header/descriptor mismatch");}
        Ok(())
    }
    fn identity(self,now:SameClassValues)->Result<(),&'static str> {
        let a=self.current;
        // D is freshly rederived: it need not retain its old numeric address.
        if now.binding!=a.binding || now.thread!=a.thread || now.header!=a.header
            || now.owner.entry!=a.owner.entry || now.owner.row!=a.owner.row
            || now.owner.object!=a.owner.object || now.owner.index!=a.owner.index
            || now.owner.selector!=a.owner.selector || now.owner.alias!=a.owner.alias
            || now.owner.information==0 || !now.owner.information.is_multiple_of(8)
        {return Err("same-class authority changed");}
        Ok(())
    }
    fn update(self,rows:[crate::native_values::RowIds;2],record:&[u8;0x68])
        ->Result<crate::native_values::RecordUpdate,&'static str> {
        rows[0].validate(self.before).map_err(|_|"same-class before rows")?;
        rows[1].validate(self.after).map_err(|_|"same-class target rows")?;
        if self.before.tokens().hat!=self.after.tokens().hat
            || self.before.canonical().appearance.hat!=self.after.canonical().appearance.hat
        {return Err("same-class hat changed");}
        crate::native_values::RecordUpdate::compare_record(
            *self.before.charinfo(),*self.after.charinfo(),self.after.tokens(),record)
            .map_err(|_|"same-class exact record changed")
    }
    pub fn prepare_write(self,now:SameClassValues,descriptor:&[u8;0x70],
        rows:[crate::native_values::RowIds;2],record:&[u8;0x68])
        ->Result<crate::native_values::RecordUpdate,&'static str> {
        self.identity(now)?;
        Self::header(now.header,descriptor,self.after,now.binding.owner)?;
        if !now.initialized || !now.task_idle || !now.rebuild_open {
            return Err("same-class retained O/task/rebuild gate");
        }
        // Native INSTALL already staged FE's TARGET. E/D/O must still be the old values.
        // Compare both sides explicitly; never rewrite an observed mirror to pass validation.
        let m=now.mirrors;
        let old=self.before.tokens();let new=self.after.tokens();let index=now.binding.record_index;
        if m.index!=index || m.tokens!=old || m.owner_index!=index
            || m.d!=[index,old.outfit,old.hat,old.colour]
            || m.d_specials!=old.specials.map(u32::from)
            || m.owner!=[old.outfit,old.hat,old.colour,old.voice].map(u32::from)
            || m.fe!=[index,new.outfit,new.hat,new.colour,new.voice]
        {return Err("same-class staged FE / old E-D-O mismatch");}
        self.update(rows,record)
    }
    pub fn completed(self,now:SameClassValues,descriptor:&[u8;0x70])
        ->Result<(),&'static str> {
        self.identity(now)?;
        Self::header(now.header,descriptor,self.after,now.binding.owner)?;
        if !now.initialized || !now.task_idle {return Err("same-class ready O/task mismatch");}
        now.mirrors.validate(now.binding.record_index,self.after.tokens())
            .map_err(|_|"same-class ready mirrors mismatch")
    }
}

/// Value-only intent. Menu selection belongs to the captured ordinary owner, never SavedRole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    selection: Option<MenuSelection>,
    direct: Option<(u64, u32)>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MenuSelection {
    menu_epoch: u64,
    active: crate::inventory::Canonical,
    choice: crate::selection::Choice,
}
impl Request {
    pub const CYCLE: Self = Self { selection: None, direct: None };
    /// Explicit character pick, resolved through the owned catalog at preparation.
    /// It never becomes a cycle when the requested class is unavailable.
    pub fn direct(menu_epoch: u64, kind: u32) -> Option<Self> {
        (menu_epoch != 0).then_some(Self { selection: None, direct: Some((menu_epoch, kind)) })
    }
    pub fn direct_kind(self) -> Option<u32> { self.direct.map(|(_, kind)| kind) }
    /// Menu context is comparison evidence only; current comes from the ordinary owner.
    pub fn from_menu(
        menu_epoch: u64,
        context_epoch: u64,
        original: crate::selection::PlayerOrigin,
        live: crate::selection::PlayerLive,
        current: Option<crate::inventory::Canonical>,
        action: crate::selection::Action,
    ) -> Option<Self> {
        use crate::selection::{Action, PlayerLive, PlayerOrigin};
        if menu_epoch == 0 || menu_epoch != context_epoch || original == PlayerOrigin::Unresolved {
            return None;
        }
        let (PlayerLive::Cycled { class, canonical }, Some(active), Action::ReloadPlayer(choice)) =
            (live, current, action) else { return None };
        (canonical == active && class == active.class && choice.canonical.class == class
            && choice.canonical != active
            && !matches!(original, PlayerOrigin::Mii(origin) if choice.canonical == origin.0 || active == origin.0)).then_some(Self {
                selection: Some(MenuSelection { menu_epoch, active, choice }),
                direct: None,
            })
    }
    /// Rechecked before preparation and again when INSTALL ownership is converted into a ticket.
    pub fn current(self, menu_epoch: u64, active: Option<crate::inventory::Canonical>) -> bool {
        if self.direct.is_some_and(|(epoch, _)| epoch != menu_epoch) { return false; }
        match self.selection {
            None => true,
            Some(selection) =>
                selection.menu_epoch == menu_epoch && active == Some(selection.active),
        }
    }
    pub fn choice(self) -> Option<crate::selection::Choice> {
        self.selection.map(|selection| selection.choice)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket { pub epoch: u64, pub entry: usize, pub sequence: u64 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase { Submitted, Executing, Published, Refused, Departed }

/// INSTALL remains owned throughout Refused/Departed. Only exact native destruction may retire it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gate {
    ticket: Ticket,
    phase: Phase,
    executing: bool,
    published: bool,
}
impl Gate {
    pub fn new(ticket: Ticket) -> Option<Self> {
        (ticket.epoch != 0 && ticket.entry != 0 && ticket.sequence != 0).then_some(Self {
            ticket, phase: Phase::Submitted, executing: false, published: false,
        })
    }
    pub fn ticket(self) -> Ticket { self.ticket }
    pub fn phase(self) -> Phase { self.phase }
    pub fn published(self) -> bool { self.published }
    pub fn enter(&mut self, ticket: Ticket, loader: u8) -> bool {
        if ticket != self.ticket || self.executing
            || matches!(self.phase, Phase::Refused | Phase::Departed) {
            return false;
        }
        if loader >= 7 && !self.published {
            self.phase = Phase::Refused;
            return false;
        }
        self.executing = true;
        self.phase = Phase::Executing;
        true
    }
    pub fn may_publish(self, ticket: Ticket, loader: u8) -> bool {
        ticket == self.ticket && self.executing && self.phase == Phase::Executing
            && loader == 6 && !self.published
    }
    pub fn publish(&mut self, ticket: Ticket, loader: u8) -> bool {
        if !self.may_publish(ticket, loader) { self.refuse(); return false; }
        self.published = true;
        self.phase = Phase::Published;
        true
    }
    pub fn leave(&mut self, ticket: Ticket, loader: u8) {
        if ticket != self.ticket { return; }
        self.executing = false;
        if matches!(self.phase, Phase::Refused | Phase::Departed) { return; }
        if loader >= 7 && !self.published { self.phase = Phase::Refused; }
        else if self.published { self.phase = Phase::Published; }
        else { self.phase = Phase::Submitted; }
    }
    pub fn refuse(&mut self) {
        if self.phase != Phase::Departed { self.phase = Phase::Refused; }
    }
    pub fn depart(&mut self) { self.phase = Phase::Departed; }
    pub fn can_complete(self, ticket: Ticket) -> bool {
        ticket == self.ticket && !self.executing && self.phase == Phase::Published
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owners() -> [Option<Owner>; 8] {
        core::array::from_fn(|i| (i < 4).then_some(Owner {
            entry: 0x1000+i*0x100, row: 0x2000+i*0x100,
            information: 0x3000+i*0x100, object: 0x4000+i*0x100,
            index: i as u16, selector: i as u8, alias: 0,
        }))
    }
    fn ticket(sequence: u64) -> Ticket { Ticket { epoch: 7, entry: 0x1000, sequence } }
    #[test]
    fn player_claim_keeps_real_mii_indices_and_accepts_ordinary_placeholders() {
        let baseline=owners();
        let kinds=[Some(0x49);8];
        let kinds=core::array::from_fn(|i|baseline[i].map(|_|kinds[i].unwrap()));
        assert!(unique_player(&baseline,&kinds));
        let mut changed=baseline;changed[3].as_mut().unwrap().index=0;
        assert!(!unique_player(&changed,&kinds));
        let mut ordinary=kinds;ordinary[3]=Some(9);
        assert!(unique_player(&changed,&ordinary));
        changed[2].as_mut().unwrap().object=baseline[0].unwrap().object;
        assert!(!unique_player(&changed,&ordinary));
    }
    #[test]
    fn native_phase_six_bypass_cannot_reach_phase_seven() {
        let t=ticket(1); let mut g=Gate::new(t).unwrap();
        assert!(g.enter(t, 6));
        g.leave(t,7);
        assert_eq!(g.phase(),Phase::Refused);
        assert!(!g.enter(t,7));
        assert!(!g.can_complete(t));
        assert_eq!(g.ticket(),t); // submitted ownership is retained
    }
    #[test]
    fn reentry_duplicate_publication_and_stale_generation_cannot_commit() {
        let t=ticket(1); let mut g=Gate::new(t).unwrap();
        assert!(g.enter(t,6));
        assert!(!g.enter(t,6));
        assert!(!g.may_publish(Ticket { epoch: 8, ..t },6));
        assert!(g.publish(t,6));
        assert!(!g.publish(t,6));
        assert_eq!(g.phase(),Phase::Refused);
    }
    #[test]
    fn forward_return_same_saved_reuse_each_require_a_fresh_published_ticket() {
        for sequence in 1..=3 {
            let t=ticket(sequence); let mut g=Gate::new(t).unwrap();
            assert!(!g.can_complete(t));
            assert!(g.enter(t,4)); g.leave(t,6);
            assert!(g.enter(t,6)); assert!(g.publish(t,6)); g.leave(t,7);
            assert!(g.can_complete(t));
            assert!(!g.can_complete(ticket(sequence+1)));
        }
    }
    #[test]
    fn departure_during_native_call_never_reopens_or_releases_the_ticket() {
        let t=ticket(1); let mut g=Gate::new(t).unwrap();
        assert!(g.enter(t,6)); g.depart();
        assert!(!g.may_publish(t,6)); g.leave(t,6);
        assert!(!g.enter(t,6)); assert_eq!(g.phase(),Phase::Departed);
        assert_eq!(g.ticket(),t);
    }
    fn saved() -> crate::lifetime::Configuration {
        use crate::inventory::{Appearance,Canonical,MiiClass,MiiConfig,TaggedId,CHARINFO_MASK_END};
        let info=[0u8;88];
        let mut masked=info;masked[..CHARINFO_MASK_END].fill(0);
        crate::lifetime::Configuration::from_saved(&MiiConfig {
            slot_id:1,slot_index:0,name:String::new(),charinfo:info,copies:vec![],
            raw_tags:[TaggedId(0x13002b147bc59435),TaggedId(0x5a00000fa9efa5ea),TaggedId(0xc1004d15d7061f77)],
            canonical:Canonical {class:MiiClass::Swordfighter,appearance:Appearance {
                charinfo_masked:masked,specials:[1,2,0,0],colour:4,voice:37,
                outfit:0x147bc59435,hat:0x0fa9efa5ea,chara:MiiClass::Swordfighter.chara_hash40(),
            }},
        }).unwrap()
    }

    type SameClassFixture = (crate::lifetime::Configuration,crate::lifetime::Configuration,
        SameClassValues,[u8;0x70],[crate::native_values::RowIds;2],[u8;0x68]);
    fn same_class_fixture() -> SameClassFixture {
        use crate::inventory::{Appearance,Canonical,MiiClass,MiiConfig,TaggedId,CHARINFO_MASK_END};
        use crate::native_values::{Mirrors,RowIds};
        let before=saved();
        let info=[0x22;88];let mut masked=info;masked[..CHARINFO_MASK_END].fill(0);
        let after=crate::lifetime::Configuration::from_saved(&MiiConfig {
            slot_id:2,slot_index:1,name:String::new(),charinfo:info,copies:vec![],
            raw_tags:[TaggedId(0x13002b147bc59435),TaggedId(0x5a00000fa9efa5ea),TaggedId(0xc1004d15d7061f77)],
            canonical:Canonical {class:MiiClass::Swordfighter,appearance:Appearance {
                charinfo_masked:masked,specials:[0,2,0,0],colour:5,voice:37,
                outfit:0x147bc59435,hat:0x0fa9efa5ea,chara:MiiClass::Swordfighter.chara_hash40(),
            }},
        }).unwrap();
        let owner=owners()[0].unwrap();
        let binding=crate::lifetime::Binding {epoch:5,owner:0,entry:owner.entry,row:owner.row,
            holder:0x5000,manager:0x6000,vector_begin:0x7000,vector_end:0x7000+4*0x68,record_index:0};
        let mut ordinary=[0u8;0x70];ordinary[0x18..0x1c].copy_from_slice(&9u32.to_le_bytes());
        let descriptor=staged_descriptor(ordinary,Some(before));
        let t=before.tokens();
        let values=SameClassValues {binding,thread:0x9000,owner,header:[0,0x49,u32::MAX,u32::MAX,0],
            mirrors:Mirrors {index:0,tokens:t,fe:[0,t.outfit,t.hat,t.colour,t.voice],
                d:[0,t.outfit,t.hat,t.colour],d_specials:t.specials.map(u32::from),owner_index:0,
                owner:[t.outfit,t.hat,t.colour,t.voice].map(u32::from)},
            initialized:true,task_idle:true,rebuild_open:true};
        let rows=[RowIds {outfit:0x13002b147bc59435,hat:0x5a00000fa9efa5ea};2];
        let mut record=[0xa5;0x68];record[..88].copy_from_slice(before.charinfo());
        (before,after,values,descriptor,rows,record)
    }

    #[test]
    fn saved_origin_full_payload_mirrors_and_rows_are_required_for_each_role() {
        let (origin,alternate,values,_,rows,record)=same_class_fixture();
        assert!(crate::role_owner::validate_saved(origin,0,values.mirrors,rows[0],&record).is_ok());
        let mut bytes=record;bytes[0]^=1; // masked catalog identity is unchanged
        assert!(crate::role_owner::validate_saved(origin,0,values.mirrors,rows[0],&bytes).is_err());
        assert!(crate::role_owner::validate_saved(alternate,0,values.mirrors,rows[0],&record).is_err());
        let mut mirrors=values.mirrors;mirrors.d_specials[0]^=1;
        assert!(crate::role_owner::validate_saved(origin,0,mirrors,rows[0],&record).is_err());
        let mut changed_rows=rows[0];changed_rows.outfit^=1;
        assert!(crate::role_owner::validate_saved(origin,0,values.mirrors,changed_rows,&record).is_err());
        // Helpers must be full copies of the immutable origin at their own claims.
        for id in [1,2,3] {
            let mut helper=values.mirrors;helper.index=id;helper.fe[0]=id;
            helper.d[0]=id;helper.owner_index=id;
            assert!(crate::role_owner::validate_saved(origin,id,helper,rows[0],&record).is_ok());
            assert!(crate::role_owner::validate_saved(origin,0,helper,rows[0],&record).is_err());
            assert!(crate::role_owner::validate_saved(origin,id,helper,rows[0],&bytes).is_err());
        }
    }

    #[test]
    fn same_class_saved_original_return_publishes_original_and_retains_native_pair() {
        let (origin,alternate,mut current,original_descriptor,rows,record)=same_class_fixture();
        let alternate_descriptor=staged_descriptor(original_descriptor,Some(alternate));
        let t=alternate.tokens();
        current.mirrors.tokens=t;current.mirrors.fe=[0,t.outfit,t.hat,t.colour,t.voice];
        current.mirrors.d=[0,t.outfit,t.hat,t.colour];current.mirrors.d_specials=t.specials.map(u32::from);
        current.mirrors.owner=[t.outfit,t.hat,t.colour,t.voice].map(u32::from);
        let before=retained_record(record,Some(alternate));
        let proof=SameClass::prepare(alternate,origin,current,&alternate_descriptor,rows,&before).unwrap();
        let origin_tokens=origin.tokens();
        current.mirrors.fe=[0,origin_tokens.outfit,origin_tokens.hat,origin_tokens.colour,origin_tokens.voice];
        let update=proof.prepare_write(current,&original_descriptor,rows,&before).unwrap();
        let mut returned=before;update.apply_to_copy(&mut returned).unwrap();
        assert_eq!(&returned[..88],origin.charinfo());
        assert_eq!(&returned[88..],&before[88..]);
        let mut foreign=before;foreign[0]^=1;
        assert!(proof.prepare_write(current,&original_descriptor,rows,&foreign).is_err());
    }

    #[test]
    fn same_class_cpu_proof_is_exact_owner_one_not_a_player_guard_bypass() {
        let (before,after,mut cpu,mut descriptor,rows,record)=same_class_fixture();
        cpu.binding.owner=1;cpu.binding.record_index=1;
        cpu.owner.index=1;cpu.owner.selector=1;cpu.header[0]=1;
        cpu.mirrors.index=1;cpu.mirrors.owner_index=1;cpu.mirrors.fe[0]=1;cpu.mirrors.d[0]=1;
        descriptor[0x46..0x48].copy_from_slice(&1u16.to_le_bytes());
        assert!(SameClass::prepare(before,after,cpu,&descriptor,rows,&record).is_err());
        let proof=SameClass::prepare_cpu(before,after,cpu,&descriptor,rows,&record).unwrap();
        let target=crate::ordinary_cpu::staged_descriptor(descriptor,Some(after));
        let mut phase=cpu;let t=after.tokens();phase.mirrors.fe=[1,t.outfit,t.hat,t.colour,t.voice];
        assert!(proof.prepare_write(phase,&target,rows,&record).is_ok());
        for field in 0..4 {
            let mut bad=cpu;
            match field {0=>bad.binding.owner=0,1=>bad.owner.selector=0,
                2=>bad.owner.index=0,_=>bad.header[0]=0}
            assert!(SameClass::prepare_cpu(before,after,bad,&descriptor,rows,&record).is_err());
        }
        let choice=crate::selection::Choice {canonical:after.canonical(),slot_id:Some(2),
            source:crate::selection::Source::Preference};
        let request=crate::ordinary_cpu::Request::from_menu(9,9,
            Some((before.canonical().class,before.canonical())),Some(before.canonical()),
            crate::selection::Action::ReloadCpu(choice)).unwrap();
        assert!(request.current(9,Some(before.canonical())));
        assert!(!request.current(10,Some(before.canonical())));
        assert!(!request.current(9,Some(after.canonical())));
        assert_eq!(request.choice(),Some(choice));
        let same=crate::selection::Choice {canonical:before.canonical(),..choice};
        assert!(crate::ordinary_cpu::Request::from_menu(9,9,
            Some((before.canonical().class,before.canonical())),Some(before.canonical()),
            crate::selection::Action::ReloadCpu(same)).is_none());
    }

    #[test]
    fn same_class_publication_requires_staged_fe_but_old_e_d_o() {
        let (before,after,old,descriptor,rows,record)=same_class_fixture();
        let proof=SameClass::prepare(before,after,old,&descriptor,rows,&record).unwrap();
        let target=staged_descriptor(descriptor,Some(after));
        // Native phase4 stages FE only. Before-FE at the commit site is a real mismatch.
        assert!(proof.prepare_write(old,&target,rows,&record).is_err());
        let t=after.tokens();let mut phase=old;
        phase.mirrors.fe=[0,t.outfit,t.hat,t.colour,t.voice];
        assert!(SameClass::prepare(before,after,phase,&descriptor,rows,&record).is_err());
        let update=proof.prepare_write(phase,&target,rows,&record).unwrap();
        let mut result=record;update.apply_to_copy(&mut result).unwrap();
        assert_eq!(&result[..88],after.charinfo());
        assert_eq!(&result[88..],&record[88..]);
        assert!(proof.completed(phase,&target).is_err()); // native completion not yet qualified
        let mut ready=phase;
        ready.mirrors.tokens=t;ready.mirrors.d=[0,t.outfit,t.hat,t.colour];
        ready.mirrors.d_specials=t.specials.map(u32::from);
        ready.mirrors.owner=[t.outfit,t.hat,t.colour,t.voice].map(u32::from);
        assert!(proof.completed(ready,&target).is_ok());
        let mut bad=phase;bad.mirrors.owner[2]=u32::from(t.colour);
        assert!(proof.prepare_write(bad,&target,rows,&record).is_err()); // old O required
        bad=phase;bad.mirrors.d_specials[0]=u32::from(t.specials[0]);
        assert!(proof.prepare_write(bad,&target,rows,&record).is_err()); // old D required
        bad=phase;bad.mirrors.tokens=t;
        assert!(proof.prepare_write(bad,&target,rows,&record).is_err()); // old E required
    }

    #[test]
    fn ldn_cpu_same_class_uses_physical_four_and_exact_unmasked_payload() {
        let (before,_,mut cpu,mut descriptor,rows,record)=same_class_fixture();
        let mut stored=serde_json::to_value(before).unwrap();
        let byte=stored["charinfo"][0].as_u64().unwrap();
        stored["charinfo"][0]=serde_json::json!(byte^1);
        let after:crate::lifetime::Configuration=serde_json::from_value(stored).unwrap();
        assert_eq!(before.canonical(),after.canonical());assert_ne!(before,after);
        cpu.binding.owner=4;cpu.binding.record_index=4;cpu.binding.vector_end=cpu.binding.vector_begin+5*104;
        cpu.owner.index=4;cpu.owner.selector=4;cpu.header[0]=4;
        cpu.mirrors.index=4;cpu.mirrors.owner_index=4;cpu.mirrors.fe[0]=4;cpu.mirrors.d[0]=4;
        descriptor[4..8].copy_from_slice(&4u32.to_le_bytes());
        descriptor[0x46..0x48].copy_from_slice(&4u16.to_le_bytes());
        let rows=[rows[0];2];
        assert!(SameClass::prepare_cpu(before,after,cpu,&descriptor,rows,&record).is_err());
        let proof=SameClass::prepare_cpu_in(crate::backend::Layout::Ldn,before,after,cpu,&descriptor,rows,&record).unwrap();
        let target=crate::ordinary_cpu::staged_descriptor_in(crate::backend::Layout::Ldn,4,descriptor,Some(after)).unwrap();
        let update=proof.prepare_write(cpu,&target,rows,&record).unwrap();
        let mut changed=record;update.apply_to_copy(&mut changed).unwrap();
        assert_eq!(&changed[..88],after.charinfo());assert_eq!(&changed[88..],&record[88..]);
        let mut stale=cpu;stale.binding.epoch+=1;
        assert!(proof.prepare_write(stale,&target,rows,&record).is_err());
    }

    #[test]
    fn same_class_owned_object_header_epoch_and_native_gates_cannot_be_rebased() {
        let (before,after,old,descriptor,rows,record)=same_class_fixture();
        let proof=SameClass::prepare(before,after,old,&descriptor,rows,&record).unwrap();
        let target=staged_descriptor(descriptor,Some(after));let t=after.tokens();
        let mut phase=old;phase.mirrors.fe=[0,t.outfit,t.hat,t.colour,t.voice];
        for changed in 0..8 {
            let mut bad=phase;
            match changed {
                0=>bad.owner.object+=8,
                1=>bad.header[1]=0x4a,
                2=>bad.binding.epoch+=1,
                3=>bad.owner.index=1,
                4=>bad.initialized=false,
                5=>bad.task_idle=false,
                6=>bad.rebuild_open=false,
                _=>bad.owner.alias=1,
            }
            assert!(proof.prepare_write(bad,&target,rows,&record).is_err(),"changed field {changed}");
        }
        let mut changed=record;changed[0]^=1;let untouched=changed;
        assert!(proof.prepare_write(phase,&target,rows,&changed).is_err());
        assert_eq!(changed,untouched);
        let mut bad_rows=rows;bad_rows[1].outfit^=1;
        assert!(proof.prepare_write(phase,&target,bad_rows,&record).is_err());
        assert!(SameClass::prepare(before,before,old,&descriptor,rows,&record).is_err());
        let mut uninitialized=old;uninitialized.initialized=false;
        assert!(SameClass::prepare(before,after,uninitialized,&descriptor,rows,&record).is_ok());
    }
    #[test]
    fn same_class_clear_latch_preflight_does_not_authorize_retained_writes() {
        let (before,after,mut current,descriptor,rows,record)=same_class_fixture();
        // Observed B2: current configuration and quiescent task, configuration latch clear.
        current.initialized=false;
        let proof=SameClass::prepare(before,after,current,&descriptor,rows,&record).unwrap();
        let target=staged_descriptor(descriptor,Some(after));let t=after.tokens();
        let mut phase=current;phase.mirrors.fe=[0,t.outfit,t.hat,t.colour,t.voice];
        assert_eq!(proof.prepare_write(phase,&target,rows,&record).err(),
            Some("same-class retained O/task/rebuild gate"));
        // Only a freshly observed set phase may use the retained-O writer.
        phase.initialized=true;
        let update=proof.prepare_write(phase,&target,rows,&record).unwrap();
        let mut changed=record;update.apply_to_copy(&mut changed).unwrap();
        assert_eq!(&changed[..88],after.charinfo());
        assert_eq!(&changed[88..],&record[88..]);
        let mut bad=current;bad.task_idle=false;
        assert_eq!(SameClass::prepare(before,after,bad,&descriptor,rows,&record).err(),
            Some("same-class retained O task not quiescent"));
        bad=current;bad.header[1]=0x4a;
        assert_eq!(SameClass::prepare(before,after,bad,&descriptor,rows,&record).err(),
            Some("same-class current O header/descriptor mismatch"));
        bad=current;bad.mirrors.d[0]=1;
        assert_eq!(SameClass::prepare(before,after,bad,&descriptor,rows,&record).err(),
            Some("same-class preflight mirrors differ"));
        bad=current;bad.owner.entry+=8;
        assert_eq!(SameClass::prepare(before,after,bad,&descriptor,rows,&record).err(),
            Some("same-class ordinary owner/target mismatch"));
    }
    #[test]
    fn actual_descriptor_return_and_retention_kernel_never_restore_unused_zero_payload() {
        let c=saved();
        let mut ordinary=[0xa5;0x70];
        ordinary[0x18..0x1c].copy_from_slice(&9u32.to_le_bytes());
        ordinary[0x46..0x48].copy_from_slice(&0u16.to_le_bytes());
        ordinary[0x4a..0x4c].copy_from_slice(&0u16.to_le_bytes());
        let saved_descriptor=staged_descriptor(ordinary,Some(c));
        assert_eq!(u32::from_le_bytes(saved_descriptor[0x18..0x1c].try_into().unwrap()),0x49);
        for off in 0..0x70 {
            if !(0x14..0x24).contains(&off) && !(0x28..0x2c).contains(&off)
                && off!=0x43 && !(0x48..0x4a).contains(&off) && !(0x4c..0x50).contains(&off) {
                assert_eq!(saved_descriptor[off],ordinary[off],"unowned descriptor byte {off}");
            }
        }
        assert_eq!(staged_descriptor(ordinary,None),ordinary);
        let mut native=[0x7b;0x68];
        native[..88].fill(0x55); // the unused ordinary payload is not the saved face
        let forward=retained_record(native,Some(c));
        assert_eq!(&forward[..88],c.charinfo());
        assert_eq!(&forward[88..],&native[88..]);
        let returned=retained_record(forward,None);
        assert_eq!(returned,forward);
        assert_eq!(retained_record(returned,Some(c)),returned);
        assert_eq!(staged_descriptor(ordinary,Some(c)),saved_descriptor);
    }

    #[test]
    fn menu_selection_is_bound_to_the_current_ordinary_cycled_class() {
        use crate::selection::{Action, Choice, Origin, PlayerLive, PlayerOrigin, Source};
        let active=saved().canonical();
        let mut wanted=active;
        wanted.appearance.colour=5;
        let choice=Choice {canonical:wanted,slot_id:Some(2900),source:Source::Preference};
        let live=PlayerLive::Cycled {class:active.class,canonical:active};
        let action=Action::ReloadPlayer(choice);
        let request=Request::from_menu(9,9,PlayerOrigin::NotMii,live,Some(active),action).unwrap();
        assert_eq!(request.choice(),Some(choice));
        assert!(request.current(9,Some(active)));
        assert!(!request.current(10,Some(active))); // scene/menu invalidation
        assert!(!request.current(9,Some(wanted))); // another completed selection
        assert!(!request.current(9,None)); // returned to immutable slot0
        for (epoch,origin,current,live,action) in [
            (8,PlayerOrigin::NotMii,Some(active),live,action),
            (9,PlayerOrigin::Mii(Origin(active)),Some(active),live,action),
            (9,PlayerOrigin::NotMii,None,PlayerLive::Origin,action),
            (9,PlayerOrigin::NotMii,Some(wanted),live,action),
            (9,PlayerOrigin::NotMii,Some(active),live,Action::ReloadCpu(choice)),
            (9,PlayerOrigin::NotMii,Some(active),live,Action::ReloadPlayer(Choice {canonical:active,..choice})),
        ] {
            assert!(Request::from_menu(9,epoch,origin,live,current,action).is_none());
        }
        let mut original=active;original.appearance.colour=6;
        assert!(Request::from_menu(9,9,PlayerOrigin::Mii(Origin(original)),live,Some(active),action).is_some());
        assert!(Request::from_menu(9,9,PlayerOrigin::Mii(Origin(wanted)),live,Some(active),action).is_none());
        let mut other=choice;
        other.canonical.class=crate::inventory::MiiClass::Gunner;
        other.canonical.appearance.chara=crate::inventory::MiiClass::Gunner.chara_hash40();
        assert_eq!(crate::inventory::Canonical::from_bytes(&other.canonical.to_bytes()),Ok(other.canonical));
        assert!(Request::from_menu(9,9,PlayerOrigin::NotMii,live,Some(active),
            Action::ReloadPlayer(other)).is_none());
    }

}

#[cfg(test)]
mod generic_completion_tests {
    use super::*;

    fn loaded(kind: u32) -> GenericOrdinaryFacts {
        GenericOrdinaryFacts { transaction_busy:false,origin_is_saved:false,active_is_saved:false,
            cursor_owned:false,kind,published_kind:kind,index:0,original_index:0 }
    }

    #[test]
    fn ordinary_cycle_accepts_kirby_and_kind_zero_mario() {
        for kind in [0x6,0x0,0x1e] {assert_eq!(generic_ordinary_commit(loaded(kind)),Ok(()));}
    }

    #[test]
    fn completion_cannot_adopt_mii_owned_busy_or_moved_state() {
        let f=loaded(0x6);
        for invalid in [
            GenericOrdinaryFacts {transaction_busy:true,..f},
            GenericOrdinaryFacts {origin_is_saved:true,..f},
            GenericOrdinaryFacts {active_is_saved:true,..f},
            GenericOrdinaryFacts {cursor_owned:true,..f},
            GenericOrdinaryFacts {published_kind:0x0,..f},
            GenericOrdinaryFacts {index:1,..f},
        ] {assert!(generic_ordinary_commit(invalid).is_err());}
        for kind in [0x48,0x49,0x4a] {assert!(generic_ordinary_commit(loaded(kind)).is_err());}
    }
}
