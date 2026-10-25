//! Ordinary CPU1's native selector assignment. Not a free-slot allocator.
use crate::{lifetime::Configuration, ordinary_player::Owner};

pub const OWNER: usize = 1;
pub const DESTINATION: u16 = 1;

/// Exact native CPU descriptor receipts qualify all three classes. Runtime adapter acceptance is separate.
pub fn supported_kind(kind: u32) -> bool { matches!(kind, 0x48..=0x4a) }

/// Frozen native target; an ordinary numbered fighter is not the immutable origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind { Original, Fighter(u32), Saved }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {kind:TargetKind,configuration:Option<Configuration>}
impl Target {
    pub const ORIGINAL:Self=Self {kind:TargetKind::Original,configuration:None};
    pub fn fighter(kind:u32)->Self {Self {kind:TargetKind::Fighter(kind),configuration:None}}
    pub fn mii(c:Configuration)->Self {Self {kind:TargetKind::Saved,configuration:Some(c)}}
    pub fn kind(self)->TargetKind {self.kind}
    pub fn saved(self)->Option<Configuration> {self.configuration}
    /// Commit identity is independent of the payload published for an Original return.
    pub fn publish(self, original:Option<Configuration>)->Option<Configuration> {
        if self.kind==TargetKind::Original {original} else {self.configuration}
    }
    pub fn index(self)->u16 {if self.configuration.is_some() {DESTINATION} else {0}}
}
/// Prepared before INSTALL. The owner commits this value only after Gate::can_complete
/// and the full current native comparison; a refused ticket cannot advance the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Destination {pub target:Target,pub cursor:Option<usize>}

/// Epoch-bound same-class menu intent. CPU original identity remains outside SavedRole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {selection:Option<MenuSelection>,replay:Option<crate::replay::Selection>,receipt_only:bool,direct:Option<(u64,u32)>,refresh_partners:bool}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MenuSelection {epoch:u64,active:crate::inventory::Canonical,choice:crate::selection::Choice}
impl Request {
    pub const CYCLE:Self=Self {selection:None,replay:None,receipt_only:false,direct:None,refresh_partners:false};
    pub const REFRESH_PARTNERS:Self=Self {refresh_partners:true,..Self::CYCLE};
    pub fn refresh_partners(self)->bool {self.refresh_partners}
    pub fn direct(epoch:u64,kind:u32)->Option<Self> {
        (epoch!=0).then_some(Self {direct:Some((epoch,kind)),..Self::CYCLE})
    }
    pub fn direct_kind(self)->Option<u32> {self.direct.map(|(_,kind)|kind)}
    pub fn from_replay(selection:crate::replay::Selection)->Option<Self> {
        if selection.target.configuration().is_none()
            && crate::inventory::MiiClass::from_kind(selection.target.kind()).is_some() {return None;}
        selection.current(selection.generation,selection.revision)
            .then_some(Self {replay:Some(selection),..Self::CYCLE})
    }
    /// Renew an already-active exact Mii receipt; this intent never authorizes INSTALL.
    pub fn acknowledge_replay(selection:crate::replay::Selection)->Option<Self> {
        selection.target.configuration()?;
        let mut request=Self::from_replay(selection)?;
        request.receipt_only=true;
        Some(request)
    }
    pub fn can_install(self)->bool {!self.receipt_only}
    pub fn replay(self)->Option<crate::replay::Selection> {self.replay}
    pub fn from_menu(epoch:u64,context_epoch:u64,
        live:Option<(crate::inventory::MiiClass,crate::inventory::Canonical)>,
        current:Option<crate::inventory::Canonical>,action:crate::selection::Action)->Option<Self> {
        let (Some((class,canonical)),Some(active),crate::selection::Action::ReloadCpu(choice))=
            (live,current,action) else {return None};
        (epoch!=0 && epoch==context_epoch && canonical==active && class==active.class
            && choice.canonical.class==class && choice.canonical!=active)
            .then_some(Self {selection:Some(MenuSelection {epoch,active,choice}),..Self::CYCLE})
    }
    pub fn current(self,epoch:u64,active:Option<crate::inventory::Canonical>)->bool {
        self.direct.is_none_or(|(opening,_)|opening==epoch)
            && self.selection.is_none_or(|s|epoch==s.epoch && active==Some(s.active))
    }
    pub fn choice(self)->Option<crate::selection::Choice> {self.selection.map(|s|s.choice)}
}

/// Catalog capture is read-only. Exclusive retention begins only with an actual CPU ticket.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reservation(bool);
impl Reservation {
    pub const fn empty() -> Self { Self(false) }
    pub fn held(self) -> bool { self.0 }
    /// A retained CPU payload blocks CPU ownership and every claim touching record1.
    pub fn conflicts(self, role:usize, record:u16)->bool {
        self.0 && (role==OWNER || record==DESTINATION)
    }
    pub fn routes(self, requested: bool, active: bool) -> bool { self.0 || requested || active }
    pub fn acquire(&mut self, install_owned: bool) -> bool {
        if !install_owned { return false; }
        self.0 = true; true
    }
    pub fn retire(&mut self, exact_owner: bool) { if exact_owner { self.0 = false; } }
}

/// Identity publication after the generic loader's native completion proof.
/// This does not grant INSTALL, reservation, or menu qualification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenericOrdinaryFacts {
    pub reservation_held: bool,
    pub origin_is_saved: bool,
    pub live_is_saved: bool,
    pub transaction_busy: bool,
    pub kind: u32,
    pub origin_kind: u32,
    pub published_kind: u32,
    pub index: u16,
    pub descriptor_equals_origin: bool,
    pub roster_cursor: Option<usize>,
}

pub fn generic_ordinary_commit(f: GenericOrdinaryFacts) -> Result<Destination, &'static str> {
    generic_ordinary_commit_in(crate::backend::Layout::Training,0,f)
}
pub fn generic_ordinary_commit_in(layout:crate::backend::Layout,original_index:u16,f:GenericOrdinaryFacts)->Result<Destination,&'static str> {
    if f.transaction_busy { return Err("generic ordinary CPU transaction busy"); }
    if f.reservation_held { return Err("generic ordinary CPU destination reserved"); }
    if f.origin_is_saved || f.live_is_saved { return Err("saved CPU stays on the owned adapter"); }
    if f.kind != f.published_kind { return Err("generic ordinary CPU published kind differs"); }
    if crate::inventory::MiiClass::from_kind(f.kind).is_some() {
        return Err("Mii CPU transition is not a generic ordinary commit");
    }
    if f.index != if layout==crate::backend::Layout::Training {0}else{original_index} { return Err("generic ordinary CPU changed native original index"); }
    if f.kind == f.origin_kind && f.descriptor_equals_origin {
        Ok(Destination { target: Target::ORIGINAL, cursor: None })
    } else {
        Ok(Destination { target: Target::fighter(f.kind), cursor: f.roster_cursor })
    }
}

#[cfg(test)]
mod generic_completion_tests {
    use super::*;

    fn loaded(kind: u32) -> GenericOrdinaryFacts {
        GenericOrdinaryFacts { reservation_held:false,origin_is_saved:false,live_is_saved:false,
            transaction_busy:false,kind,origin_kind:0x1e,published_kind:kind,index:0,
            descriptor_equals_origin:false,roster_cursor:Some(0) }
    }

    #[test]
    fn ordinary_cycle_commits_kirby_mario_and_exact_origin() {
        assert!(!Reservation::empty().routes(false,false));
        for (kind,cursor) in [(0x6,0),(0x0,1)] {
            let f=GenericOrdinaryFacts {roster_cursor:Some(cursor),..loaded(kind)};
            assert_eq!(generic_ordinary_commit(f),Ok(Destination {target:Target::fighter(kind),cursor:Some(cursor)}));
        }
        for kind in [0x1e,0x0] {
            let f=GenericOrdinaryFacts {origin_kind:kind,descriptor_equals_origin:true,..loaded(kind)};
            assert_eq!(generic_ordinary_commit(f),Ok(Destination {target:Target::ORIGINAL,cursor:None}));
            assert_eq!(generic_ordinary_commit(GenericOrdinaryFacts {descriptor_equals_origin:false,..f}),
                Ok(Destination {target:Target::fighter(kind),cursor:Some(0)}));
        }
    }

    #[test]
    fn ordinary_completion_cannot_adopt_saved_reserved_or_unfinished_owners() {
        let f=loaded(0x6);
        for invalid in [
            GenericOrdinaryFacts {transaction_busy:true,..f},
            GenericOrdinaryFacts {reservation_held:true,..f},
            GenericOrdinaryFacts {origin_is_saved:true,..f},
            GenericOrdinaryFacts {live_is_saved:true,..f},
            GenericOrdinaryFacts {published_kind:0x0,..f},
            GenericOrdinaryFacts {index:1,..f},
        ] {assert!(generic_ordinary_commit(invalid).is_err());}
        for kind in [0x48,0x49,0x4a] {
            assert!(generic_ordinary_commit(loaded(kind)).is_err());
        }
    }
}

/// Native Training's four present rows include inactive helpers. Their current index AND
/// selector protect records. Repeated original index0 is permitted; destination1 is not shared.
pub fn assignment(owners: &[Option<Owner>; 8]) -> Option<[Option<u16>; 8]> {
    assignment_for(OWNER, owners)
}

/// One proved selector destination; callers supply live ownership and phase authority.
pub fn assignment_for(destination: usize, owners: &[Option<Owner>; 8]) -> Option<[Option<u16>; 8]> {
    assignment_in(crate::backend::Layout::Training,destination,owners)
}
pub fn assignment_in(layout:crate::backend::Layout,destination:usize,owners:&[Option<Owner>;8])->Option<[Option<u16>;8]> {
    if !layout.selector(destination) {return None;}
    if !layout.shape(&owners.map(|o|o.map_or(0,|o|o.entry))) {
        return None;
    }
    let mut protected = [None; 8];
    let mut used = 0;
    for id in layout.owners() {
        let owner=owners[id]?;
        if owner.selector != id as u8 || owner.alias != 0
            || [owner.entry, owner.row, owner.information, owner.object].iter()
                .any(|p| *p == 0 || !p.is_multiple_of(8)) {
            return None;
        }
        for other in owners[..id].iter().flatten() {
            if owner.entry == other.entry || owner.row == other.row
                || owner.information == other.information || owner.object == other.object {
                return None;
            }
        }
        if id == destination {
            if layout==crate::backend::Layout::Training && owner.index != 0 && owner.index != destination as u16 { return None; }
        } else {
            if owner.index == destination as u16 || usize::from(owner.selector) == destination {
                return None;
            }
            for index in [owner.index,u16::from(owner.selector)] {
                if !protected[..used].contains(&Some(index)) {
                    protected[used]=Some(index);used+=1;
                }
            }
        }
    }
    // Original record0 remains protected on forward, return, and reuse.
    if !protected[..used].contains(&Some(0)) {protected[used]=Some(0);}
    Some(protected)
}

pub fn staged_descriptor(original: [u8; 0x70], target: Option<Configuration>) -> [u8; 0x70] {
    staged_descriptor_for(OWNER, original, target).expect("CPU selector")
}


/// Preserve only captures needed by this request. Unknown original kind waits
/// for the existing native original-kind publication, not catalog qualification.
pub fn helper_captures_ready(mii_request: bool, original_cpu: Option<u32>,
    player_captured: bool, cpu_captured: bool) -> bool {
    if mii_request { return player_captured && cpu_captured; }
    match original_cpu {
        Some(kind) if supported_kind(kind) => cpu_captured,
        Some(_) => true,
        None => false,
    }
}

/// Preserve the chosen exact configuration; never substitute a different hat or
/// modify preferences to fit a helper. Live publication rechecks its selected row.
pub fn helper_configuration(configuration: Configuration, inherited_hats: [u16; 2])
    -> Result<Configuration, &'static str> {
    if inherited_hats.contains(&configuration.tokens().hat) { Ok(configuration) }
    else { Err("This Mii's hat does not match either captured helper") }
}

/// No selector allocator: only the caller's already-proved owner1/2/3 is accepted.
pub fn staged_descriptor_for(owner: usize, original: [u8; 0x70], target: Option<Configuration>) -> Option<[u8; 0x70]> {
    staged_descriptor_in(crate::backend::Layout::Training,owner,original,target)
}
pub fn staged_descriptor_in(layout:crate::backend::Layout,owner:usize,original:[u8;0x70],target:Option<Configuration>)->Option<[u8;0x70]> {
    if !layout.selector(owner) { return None; }
    let mut result = crate::ordinary_player::staged_descriptor(original, target);
    if target.is_some() { result[0x46..0x48].copy_from_slice(&(owner as u16).to_le_bytes()); }
    Some(result)
}

/// The mixed phase is explicit: native FE already contains target; E and D still contain before.
pub fn publication_indices(before:u16,target_index:u16,e:u16,fe:u16,d:u16)->bool {
    // This is only descriptor-shape evidence. The caller still requires the executing
    // typed ticket, current protected union, loader6 and selected-O phase proof.
    publication_indices_for(OWNER, before, target_index, e, fe, d)
}
pub fn publication_indices_for(owner:usize,before:u16,target_index:u16,e:u16,fe:u16,d:u16)->bool {
    publication_indices_in(crate::backend::Layout::Training,owner,before,target_index,e,fe,d)
}
pub fn publication_indices_in(layout:crate::backend::Layout,owner:usize,before:u16,target_index:u16,e:u16,fe:u16,d:u16)->bool {
    layout.selector(owner) && (layout.waiting() || before==0 || before==owner as u16)
        && (layout.waiting() || target_index==0 || target_index==owner as u16)
        && e==before && d==before && fe==target_index
}


/// At prequeue FE contains the target, but native has not written O's target header.
/// Compare copied outgoing values only; current map/phase/lock authority remains the caller's.
pub fn outgoing_object_matches(
    before: Owner,
    descriptor: &[u8; 0x70],
    before_tokens: [u16; 4],
    selected: usize,
    header: [u32; 5],
    index: u16,
    tokens: [u32; 4],
) -> bool {
    usize::from(before.selector)==OWNER
        && outgoing_object_matches_for(before, descriptor, before_tokens, selected, header, index, tokens)
}
pub fn outgoing_object_matches_for(
    before: Owner, descriptor: &[u8; 0x70], before_tokens: [u16; 4],
    selected: usize, header: [u32; 5], index: u16, tokens: [u32; 4],
) -> bool {
    let word = |offset: usize| u32::from_le_bytes([
        descriptor[offset], descriptor[offset + 1],
        descriptor[offset + 2], descriptor[offset + 3],
    ]);
    matches!(before.selector,1..=6)
        && selected == before.object
        && header == [u32::from(before.selector), word(0x18), word(0x1c), word(0x20),
            u32::from(descriptor[0x41])]
        && index == before.index
        && tokens == before_tokens.map(u32::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rows() -> [Option<Owner>; 8] {
        core::array::from_fn(|id| (id < 4).then_some(Owner {
            entry: 0x1000 + id*0x100, row: 0x2000 + id*0x100,
            information: 0x3000 + id*0x100, object: 0x4000 + id*0x100,
            index: 0, selector: id as u8, alias: 0,
        }))
    }

    #[test]
    fn outgoing_object_uses_old_header_before_native_publication() {
        // Retained combined06 #36/#38/#39: outgoing Luigi, FE already Gunner,
        // O still [owner1, Luigi, -1, -1, 1], index0 and zero tokens. Addresses normalized.
        let before = rows()[OWNER].unwrap();
        let mut descriptor = [0u8; 0x70];
        descriptor[0x18..0x1c].copy_from_slice(&9u32.to_le_bytes());
        descriptor[0x1c..0x20].copy_from_slice(&u32::MAX.to_le_bytes());
        descriptor[0x20..0x24].copy_from_slice(&u32::MAX.to_le_bytes());
        descriptor[0x41] = 1;
        let old_header = [1, 9, u32::MAX, u32::MAX, 1];
        let accepts = |object, header, index, tokens| outgoing_object_matches(
            before, &descriptor, [0; 4], object, header, index, tokens);
        assert!(accepts(before.object, old_header, 0, [0; 4]));
        assert!(!accepts(before.object, [1, 0x4a, u32::MAX, u32::MAX, 1], 0, [0; 4]));
        assert!(!accepts(before.object, [0; 5], 0, [0; 4]));
        for field in 0..5 {
            let mut changed = old_header;
            changed[field] ^= 1;
            assert!(!accepts(before.object, changed, 0, [0; 4]));
        }
        for object in [0, rows()[0].unwrap().object, before.object + 8] {
            assert!(!accepts(object, old_header, 0, [0; 4]));
        }
        assert!(!accepts(before.object, old_header, 1, [0; 4]));
        for field in 0..4 {
            let mut changed = [0; 4];
            changed[field] = 1;
            assert!(!accepts(before.object, old_header, 0, changed));
        }
        // Nonzero value-only variant checks token order/index1, not native route qualification.
        let saved = Owner {index: 1, ..before};
        descriptor[0x18..0x1c].copy_from_slice(&0x4au32.to_le_bytes());
        assert!(outgoing_object_matches(saved, &descriptor, [68, 0, 4, 37],
            saved.object, [1, 0x4a, u32::MAX, u32::MAX, 1], 1, [68, 0, 4, 37]));
        assert!(!outgoing_object_matches(saved, &descriptor, [68, 0, 4, 37],
            saved.object, [1, 0x4a, u32::MAX, u32::MAX, 1], 1, [68, 0, 37, 4]));
    }

    #[test]
    fn original_alias_does_not_license_destination_or_inactive_selector_alias() {
        let original = rows();
        assert_eq!(assignment(&original), Some([Some(0),Some(2),Some(3),None,None,None,None,None]));
        let mut saved = original; saved[1].as_mut().unwrap().index = 1;
        assert_eq!(assignment(&saved), assignment(&original));
        for id in [0, 2, 3] {
            let mut changed = original; changed[id].as_mut().unwrap().index = 1;
            assert_eq!(assignment(&changed), None);
            changed = original; changed[id].as_mut().unwrap().selector = 1;
            assert_eq!(assignment(&changed), None);
        }
        let mut changed = original; changed[2] = None;
        assert_eq!(assignment(&changed), None);
        changed = original; changed[1].as_mut().unwrap().object = changed[0].unwrap().object;
        assert_eq!(assignment(&changed), None);
        changed = original; changed[1].as_mut().unwrap().alias = 1;
        assert_eq!(assignment(&changed), None);
    }
    #[test]
    fn native_fe_assignment_keeps_typed_ordinary_phase_and_old_mirrors() {
        assert!(publication_indices(1,Target::ORIGINAL.index(),1,0,1));
        assert!(publication_indices(0,Target::fighter(0x24).index(),0,0,0));
        for (before,e,fe,d) in [(1,0,0,1),(1,1,1,1),(1,1,0,0),(2,2,0,2)] {
            assert!(!publication_indices(before,Target::ORIGINAL.index(),e,fe,d));
        }
    }
    #[test]
    fn staged_cpu_preserves_native_role_and_original_return_is_byte_exact() {
        use crate::inventory::{build_snapshot,MiiClass,RawSlot,TaggedId};
        let info=[0u8;88];
        let catalog=build_snapshot(vec![RawSlot {index:0,slot_id:2622,name_len:1,
            name_units:[65,0,0,0,0,0,0,0,0,0],colour:4,voice:37,specials:[1,0,0,0],
            tag_outfit:TaggedId(0x1300291bfef7a5f7),tag_hat:TaggedId(0x5a00000fa9efa5ea),
            tag_chara:TaggedId(0xc100000000000000|MiiClass::Swordfighter.chara_hash40()),charinfo:info}]);
        let saved=Configuration::from_saved(&catalog.configs[0]).unwrap();
        let mut original=[0xa5;0x70];
        original[4..8].copy_from_slice(&1u32.to_le_bytes());
        original[0x18..0x1c].copy_from_slice(&9u32.to_le_bytes());
        original[0x46..0x48].copy_from_slice(&0u16.to_le_bytes());
        let target=staged_descriptor(original,Some(saved));
        assert_eq!(&target[4..8],&original[4..8]);
        assert_eq!(&target[0x34..0x43],&original[0x34..0x43]);
        assert_eq!(&target[0x46..0x48],&1u16.to_le_bytes());
        assert_eq!(u32::from_le_bytes(target[0x14..0x18].try_into().unwrap()),1);
        assert_eq!(u32::from_le_bytes(target[0x28..0x2c].try_into().unwrap()),1);
        assert_eq!(staged_descriptor(original,None),original);
        assert_eq!(staged_descriptor(original,Some(saved)),target); // fresh reuse constructs same value
        assert!(publication_indices(0,Target::mii(saved).index(),0,1,0));
        assert!(publication_indices(1,Target::mii(saved).index(),1,1,1));
        assert!(!publication_indices(1,Target::mii(saved).index(),1,1,0));
        let original=Destination {target:Target::ORIGINAL,cursor:None};
        let planned=Destination {target:Target::mii(saved),cursor:Some(2)};
        let ticket=crate::ordinary_player::Ticket {epoch:9,entry:0x1100,sequence:3};
        let mut gate=crate::ordinary_player::Gate::new(ticket).unwrap();
        let mut live=original;
        gate.refuse();
        if gate.can_complete(ticket) {live=planned;}
        assert_eq!(live,original);
        assert_eq!(Target::ORIGINAL.publish(Some(saved)),Some(saved));
        assert_eq!(Target::ORIGINAL.publish(None),None);
        assert_eq!(Target::fighter(9).publish(Some(saved)),None);
        assert_eq!(Target::mii(saved).publish(None),Some(saved));
        assert_eq!(planned.target,Target::mii(saved));
        assert_eq!(planned.cursor,Some(2));
        assert!(supported_kind(0x49));assert!(supported_kind(0x4a));assert!(supported_kind(0x48));assert!(!supported_kind(0x47));
    }
    #[test]
    fn refused_native_continuation_cannot_release_the_destination_or_start_reuse() {
        use crate::ordinary_player::{Gate,Phase,Ticket};
        let ticket=Ticket{epoch:5,entry:0x1100,sequence:1};
        let mut lease=Reservation::empty();assert!(lease.acquire(true));
        let mut gate=Gate::new(ticket).unwrap();
        assert!(gate.enter(ticket,6));
        gate.refuse();gate.leave(ticket,6);
        assert_eq!(gate.phase(),Phase::Refused);
        assert!(!gate.enter(ticket,7));
        assert!(!gate.can_complete(ticket));
        assert!(lease.held()); // both unpublished and terminal published failures retain reservation
        lease.retire(false);assert!(lease.held());
        lease.retire(true);assert!(!lease.held());
    }
    #[test]
    fn menu_capture_does_not_acquire_but_ticket_blocks_other_writers_until_exact_retirement() {
        let mut lease=Reservation::empty();
        assert!(!lease.held());
        assert!(!lease.routes(false,false)); // an unrelated menu/P1 action retains its existing path
        assert!(lease.routes(true,false)); // configured CPU request reaches this adapter
        assert!(!lease.acquire(false)); // another INSTALL user cannot be bypassed
        assert!(!lease.held());
        assert!(lease.acquire(true));
        assert!(lease.held());
        assert!(!lease.conflicts(0,0));
        assert!(lease.conflicts(1,1));
        assert!(lease.conflicts(0,1));
        assert!(lease.conflicts(1,0));
        assert!(lease.routes(false,false)); // even returned-original G1 stays reserved
        lease.retire(false);assert!(lease.held()); // unrelated entry destruction
        lease.retire(true);assert!(!lease.held());
        assert!(!lease.routes(false,false));
    }
    #[test]
    fn receipt_renewal_cannot_authorize_install_after_the_cpu_changes() {
        use crate::inventory::{build_snapshot,MiiClass,RawSlot,TaggedId};
        let catalog=build_snapshot(vec![RawSlot {index:0,slot_id:2622,name_len:1,
            name_units:[65,0,0,0,0,0,0,0,0,0],colour:4,voice:37,specials:[1,0,0,0],
            tag_outfit:TaggedId(0x1300291bfef7a5f7),tag_hat:TaggedId(0x5a00000fa9efa5ea),
            tag_chara:TaggedId(0xc100000000000000|MiiClass::Swordfighter.chara_hash40()),charinfo:[0;88]}]);
        let configuration=Configuration::from_saved(&catalog.configs[0]).unwrap();
        let selection=crate::replay::Selection {source:crate::replay::SelectionSource::NamedMacro,
            generation:7,revision:11,target:crate::replay::Target::mii(configuration)};
        let explicit=Request::from_replay(selection).unwrap();
        let renewal=Request::acknowledge_replay(selection).unwrap();
        assert_ne!(explicit,renewal);
        assert!(explicit.can_install());
        assert!(!renewal.can_install());
        assert_eq!(renewal.replay(),Some(selection));
        // Same queued request remains non-installing when a different CPU wins.
        assert!(!selection.target.ready(9,None));
        assert!(!renewal.can_install());
        assert!(Request::CYCLE.can_install());
        let ordinary=crate::replay::Selection {target:crate::replay::Target::fighter(9),..selection};
        assert!(Request::acknowledge_replay(ordinary).is_none());
    }

}

#[cfg(test)]
mod helper_tests {
    use super::*;
    #[test]
    fn helper_selectors_keep_foreign_union_and_old_object_phase() {
        let rows=std::array::from_fn(|id| (id<4).then_some(Owner {
            entry:0x1000+id*0x100,row:0x2000+id*0x100,information:0x3000+id*0x100,
            object:0x4000+id*0x100,index:0,selector:id as u8,alias:0}));
        let mut descriptor=[0u8;0x70];
        descriptor[0x18..0x1c].copy_from_slice(&9u32.to_le_bytes());
        descriptor[0x1c..0x24].fill(0xff);descriptor[0x41]=1;
        for owner in [2usize,3] {
            let protected=assignment_for(owner,&rows).unwrap();
            assert_eq!(protected[..3],[Some(0),Some(1),Some(if owner==2 {3}else{2})]);
            let mut own=rows;own[owner].as_mut().unwrap().index=owner as u16;
            assert_eq!(assignment_for(owner,&own),Some(protected));
            for other in (0..4).filter(|id|*id!=owner) {
                let mut foreign=rows;foreign[other].as_mut().unwrap().index=owner as u16;
                assert!(assignment_for(owner,&foreign).is_none());
            }
            let before=rows[owner].unwrap();let header=[owner as u32,9,u32::MAX,u32::MAX,1];
            assert!(outgoing_object_matches_for(before,&descriptor,[0;4],before.object,header,0,[0;4]));
            assert!(!outgoing_object_matches_for(before,&descriptor,[0;4],before.object+8,header,0,[0;4]));
            assert!(!outgoing_object_matches_for(before,&descriptor,[0;4],before.object,header,owner as u16,[0;4]));
            assert!(publication_indices_for(owner,0,owner as u16,0,owner as u16,0));
            assert!(!publication_indices_for(owner,0,owner as u16,0,owner as u16,owner as u16));
        }
        // Ordinary requests do not depend on Player/catalog capture, but a saved
        // CPU origin must retain its helper copies until CPU capture succeeds.
        assert!(helper_captures_ready(false,Some(9),false,false));
        assert!(!helper_captures_ready(false,None,true,true));
        assert!(!helper_captures_ready(false,Some(0x48),true,false));
        assert!(helper_captures_ready(false,Some(0x48),false,true));
        assert!(!helper_captures_ready(true,Some(9),false,true));
        assert!(!helper_captures_ready(true,Some(9),true,false));
        assert!(helper_captures_ready(true,Some(9),true,true));
        for denied in [0usize,4,7,8] {
            assert!(assignment_for(denied,&rows).is_none());
            assert!(staged_descriptor_for(denied,descriptor,None).is_none());
        }
    }
}
