//! Ordinary CPU1's native selector assignment. Not a free-slot allocator.
use crate::{lifetime::{Binding, Configuration}, ordinary_player::Owner};

pub const OWNER: usize = 1;
pub const DESTINATION: u16 = 1;

/// Exact native CPU descriptor receipts qualify all three classes. Runtime adapter acceptance is separate.
pub fn supported_kind(kind: u32) -> bool { matches!(kind, 0x48..=0x4a) }

/// Shared fresh-versus-retained decision for every present row.
/// `Ok(false)` keeps the retained path. `Ok(true)` is the generic fresh-O
/// constructor, which reads entry id `FE+0xe0` in `0..8` and that row's own
/// descriptor. Training, LDN, and Quickplay use `layout.owners()` (0/1/2/3 or
/// 0/4/5/6). Match modes never become a layout. Brawler, Swordfighter and
/// Gunner are not special-cased here.
pub fn headgear_reload(layout:crate::backend::Layout,owner:usize,before:u16,target:u16)
    ->Result<bool,&'static str> {
    if before==target {return Ok(false);}
    if target==u16::MAX {return Err("invalid headgear row");}
    if owner>=8 || !layout.owners().contains(&owner) {
        return Err("headgear row is not on this layout");
    }
    Ok(true)
}

/// CPU wrapper around [`headgear_reload`] for the layout's physical CPU.
/// Training passes owner 1. Waiting passes owner 4. Any other owner keeps the
/// historical refusal and does not borrow the constant Training slot.
pub fn cpu_hat_route(layout:crate::backend::Layout,owner:usize,before:u16,target:u16)->Result<bool,&'static str> {
    if owner!=layout.visible()[1] as usize {return Err("CPU unchanged hat required");}
    match headgear_reload(layout,owner,before,target) {
        Ok(changed)=>Ok(changed),
        Err(_) if target==u16::MAX=>Err("CPU invalid headgear row"),
        Err(reason)=>Err(reason),
    }
}

/// A changed headgear row cannot reuse the retained O's kind*42+3 resource key.
/// This is branch selection only; SameClass still validates all of its old proofs.
pub fn retained_same_class_candidate(before:Configuration,after:Configuration)->bool {
    before.canonical().class==after.canonical().class && before.tokens().hat==after.tokens().hat
}

/// Copied phase facts, not native write authority. The shared publisher additionally
/// holds G and proves the executing ticket, protected union, task and source registry.
#[derive(Clone, Copy)]
pub struct HatPublication<'a> {
    pub layout:crate::backend::Layout,
    pub owner:u32,
    pub source:bool,
    pub opt_in:bool,
    pub initialized:bool,
    pub fingerprints:bool,
    pub before:u16,
    pub target:u16,
    /// Selector record `Binding.record_index` actually written by publication.
    /// Saved payloads must publish this row. It is not the active `E+0x1a8`.
    pub destination_index:u16,
    /// Immutable origin `E+0x1a8` captured with the owner. Origin restore
    /// publishes this row and does not copy CharInfo onto the selector record.
    pub original_index:u16,
    pub descriptor:&'a [u8;0x70],
    pub payload:Option<Configuration>,
}
impl HatPublication<'_> {
    pub fn validate(&self)->Result<(),&'static str> {
        // Preserve the equal-hat contract for every existing publisher.
        if self.before==self.target && !self.opt_in {return Ok(());}
        // A saved payload publishes the selector record that `record_address`
        // writes. An empty payload restores the immutable origin index. The
        // active snapshot index is neither value once a saved row is live.
        // A source token is only the helper lease.
        let helper=self.layout.helper(self.owner as usize).is_some();
        let index=u16::from_le_bytes(self.descriptor[0x46..0x48].try_into().unwrap());
        let fresh=headgear_reload(self.layout,self.owner as usize,self.before,self.target)?;
        if self.owner>=8 || !self.opt_in || self.initialized || !self.fingerprints
            || (self.source && !helper) || !fresh {
            return Err("unqualified headgear publication");
        }
        if let Some(c)=self.payload {
            if !supported_kind(c.canonical().class as u32) {return Err("Mii class not qualified");}
        }
        if u16::from_le_bytes(self.descriptor[0x4a..0x4c].try_into().unwrap())!=self.target {
            return Err("headgear descriptor/scalar mismatch");
        }
        if let Some(c)=self.payload {
            if c.tokens().hat!=self.target
                || u32::from_le_bytes(self.descriptor[0x18..0x1c].try_into().unwrap())!=c.canonical().class as u32
                || index!=self.destination_index
                || self.destination_index!=self.owner as u16 {
                return Err("headgear payload/row mismatch");
            }
        } else if index!=self.original_index {
            return Err("headgear retained origin index mismatch");
        }
        // None restores the retained origin record: the adapter supplies
        // origin scalars and an origin-based descriptor, not a fabricated payload.
        Ok(())
    }
}

// Native FE+37 mirrors the AI rank setter, whose supported domain is 0..=100.
// This byte is live native state; the captured origin remains immutable.
const NATIVE_RANK: usize = 0x37;

pub fn native_rank(descriptor:&[u8;0x70])->Result<u8,&'static str> {
    let rank=descriptor[NATIVE_RANK];
    if rank>100 {return Err("CPU native rank outside supported range");}
    Ok(rank)
}

/// Construct an owned descriptor value carrying the qualified current rank.
/// This grants no native access, recapture permission or transaction authority.
pub fn with_native_rank(mut original:[u8;0x70],current:&[u8;0x70])->Result<[u8;0x70],&'static str> {
    native_rank(&original)?;
    original[NATIVE_RANK]=native_rank(current)?;
    Ok(original)
}

// FE+0x35 is the AI control type paired with rank. `main+0x6b82b0` stores its
// argument at actor AI+0xc08, then indexes a 16-byte behavior row only when the
// sign-extended byte is <= 0x27. The live FE writers call that setter and then
// store the same byte: `main+0x151be88`, `main+0x151c4b4`, and `main+0x151c520`.
// Fighter control select `main+0x64fc68` reads FE+0x35 into that setter, then
// reads FE+0x37 into the rank setter `main+0x6b8400`. An unchanged byte keeps
// any value. A change is qualified only inside 0..=0x27.
const NATIVE_CONTROL: usize = 0x35;
const NATIVE_CONTROL_MAX: u8 = 0x27;

pub fn native_control(descriptor:&[u8;0x70])->Result<u8,&'static str> {
    let control=descriptor[NATIVE_CONTROL];
    if control>NATIVE_CONTROL_MAX {return Err("CPU native control type outside supported range");}
    Ok(control)
}

fn copy_changed_control(expected:&[u8;0x70],current:&[u8;0x70],descriptor:&mut [u8;0x70])
    ->Result<(),&'static str> {
    if expected[NATIVE_CONTROL]==current[NATIVE_CONTROL] {return Ok(());}
    native_control(expected)?;
    descriptor[NATIVE_CONTROL]=native_control(current)?;
    Ok(())
}

/// Origin value plus the qualified current rank and, when it changed, the
/// qualified current control type. Kind, tokens, records, and index stay.
pub fn with_native_live_metadata(original:[u8;0x70],current:&[u8;0x70])->Result<[u8;0x70],&'static str> {
    let mut descriptor=with_native_rank(original,current)?;
    copy_changed_control(&original,current,&mut descriptor)?;
    Ok(descriptor)
}

/// Idle menu/preflight recapture's descriptor-only policy. The native adapter must still run
/// every non-descriptor ownership check and recheck state/provenance at commit.
/// Transactions continue comparing their frozen descriptors byte-for-byte.
/// Rank and a qualified FE+0x35 change are the only descriptor bytes adopted.
pub fn recapture_rank(layout:crate::backend::Layout,expected:Binding,current:Binding,
    expected_descriptor:&[u8;0x70],current_descriptor:&[u8;0x70])->Result<[u8;0x70],&'static str> {
    if expected!=current || current.epoch==0 || current.owner!=layout.visible()[1] {
        return Err("CPU rank recapture owner/lifetime changed");
    }
    let mut descriptor=with_native_rank(*expected_descriptor,current_descriptor)?;
    copy_changed_control(expected_descriptor,current_descriptor,&mut descriptor)
        .map_err(|_| "CPU rank recapture changed non-rank descriptor bytes")?;
    if descriptor!=*current_descriptor {return Err("CPU rank recapture changed non-rank descriptor bytes");}
    Ok(descriptor)
}

/// Preflight refresh before a ticket exists. Rank must already match. Only a
/// qualified FE+0x35 change is copied. Any other byte, including rank, refuses.
pub fn refresh_control_type(layout:crate::backend::Layout,expected:Binding,current:Binding,
    expected_descriptor:&[u8;0x70],current_descriptor:&[u8;0x70])->Result<[u8;0x70],&'static str> {
    if expected!=current || current.epoch==0 || current.owner!=layout.visible()[1] {
        return Err("CPU control refresh owner/lifetime changed");
    }
    let mut descriptor=*expected_descriptor;
    copy_changed_control(expected_descriptor,current_descriptor,&mut descriptor)?;
    if descriptor!=*current_descriptor {return Err("CPU control refresh changed other descriptor bytes");}
    Ok(descriptor)
}

/// Offsets inside the 0x70 descriptor, excluding native rank, where the frozen
/// and live bytes differ. Unused tail pairs stay zero. This grants no recapture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NonRankDiff {
    pub count: u8,
    pub pairs: [(u8, u8, u8); 0x70],
}
pub fn non_rank_descriptor_diff(expected: &[u8; 0x70], current: &[u8; 0x70]) -> NonRankDiff {
    let mut diff = NonRankDiff { count: 0, pairs: [(0, 0, 0); 0x70] };
    for offset in 0..0x70 {
        if offset == NATIVE_RANK || expected[offset] == current[offset] { continue; }
        diff.pairs[diff.count as usize] = (offset as u8, expected[offset], current[offset]);
        diff.count += 1;
    }
    diff
}

/// Field flags sampled beside a refused non-rank descriptor. Indices are the
/// captured and live E+1a8 values, not a physical-slot assignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecaptureFlags {
    pub index_eq: bool,
    pub scalars_eq: bool,
    pub selector_record_eq: bool,
    pub current_record_eq: bool,
    pub captured_index: u16,
    pub live_index: u16,
}

/// Same owner epoch and the same byte tuple plus flags have already been logged.
pub fn recapture_observation_seen(
    seen: Option<(u64, NonRankDiff, RecaptureFlags)>,
    epoch: u64,
    diff: NonRankDiff,
    flags: RecaptureFlags,
) -> bool {
    seen == Some((epoch, diff, flags))
}

/// One stack line fits every non-rank pair. `None` means `out` was too small.
pub const RECAPTURE_DIAGNOSTIC_BYTES: usize = 1400;
pub fn write_recapture_diagnostic(out: &mut [u8], diff: &NonRankDiff, flags: &RecaptureFlags) -> Option<usize> {
    fn push(out: &mut [u8], n: &mut usize, bytes: &[u8]) -> bool {
        if *n + bytes.len() > out.len() { return false; }
        out[*n..*n + bytes.len()].copy_from_slice(bytes);
        *n += bytes.len();
        true
    }
    fn push_str(out: &mut [u8], n: &mut usize, text: &str) -> bool { push(out, n, text.as_bytes()) }
    fn push_dec(out: &mut [u8], n: &mut usize, mut value: u16) -> bool {
        let mut buf = [0u8; 5];
        let mut i = 5;
        loop {
            i -= 1;
            buf[i] = b'0' + (value % 10) as u8;
            value /= 10;
            if value == 0 { break; }
        }
        push(out, n, &buf[i..])
    }
    fn push_hex(out: &mut [u8], n: &mut usize, value: u8) -> bool {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        push(out, n, &[HEX[(value >> 4) as usize], HEX[(value & 0xf) as usize]])
    }
    fn push_flag(out: &mut [u8], n: &mut usize, label: &str, value: bool) -> bool {
        push_str(out, n, label) && push(out, n, if value { b"1" } else { b"0" })
    }
    let mut n = 0usize;
    if !push_str(out, &mut n, "recapture non-rank n=")
        || !push_dec(out, &mut n, u16::from(diff.count))
        || !push_flag(out, &mut n, " index_eq=", flags.index_eq)
        || !push_flag(out, &mut n, " scalars_eq=", flags.scalars_eq)
        || !push_flag(out, &mut n, " selector_record_eq=", flags.selector_record_eq)
        || !push_flag(out, &mut n, " current_record_eq=", flags.current_record_eq)
        || !push_str(out, &mut n, " captured_index=")
        || !push_dec(out, &mut n, flags.captured_index)
        || !push_str(out, &mut n, " live_index=")
        || !push_dec(out, &mut n, flags.live_index)
    {
        return None;
    }
    for pair in diff.pairs.iter().take(usize::from(diff.count)) {
        if !push(out, &mut n, b" ")
            || !push_hex(out, &mut n, pair.0)
            || !push(out, &mut n, b":")
            || !push_hex(out, &mut n, pair.1)
            || !push(out, &mut n, b">")
            || !push_hex(out, &mut n, pair.2)
        {
            return None;
        }
    }
    Some(n)
}

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

/// Preserve the chosen exact configuration. A hat that differs from the
/// captured helper tokens is that configuration's own hat; the helper adapter
/// routes it to fresh O instead of substituting another Mii or refusing it.
pub fn helper_configuration(configuration: Configuration, _inherited_hats: [u16; 2])
    -> Result<Configuration, &'static str> {
    Ok(configuration)
}

/// Callback admission for one helper slot. `captured[ordinal]` is that slot's
/// immutable origin hat. [`headgear_reload`] accepts an equal hat on every
/// layout and a changed hat for a helper that belongs to that layout.
/// Training uses slots 2/3. Waiting uses slots 5/6. A foreign slot is refused.
/// This does not rewrite the captured pair, pick another Mii, or grant a lease.
pub fn helper_slot_admission(layout:crate::backend::Layout,slot:usize,captured:[u16;2],hat:u16)->bool {
    let Some(ordinal)=layout.helper(slot) else {return false;};
    headgear_reload(layout,slot,captured[ordinal],hat).is_ok()
}

/// Saved CharInfo is copied to `binding.record_index`. Keep that payload only
/// when the published row is that same destination. An origin index publishes
/// the untouched origin record and must not receive the saved bytes.
pub fn saved_payload_routes_to_destination(published_index:u16,destination_index:u16)->bool {
    published_index==destination_index
}

/// No selector allocator: only the caller's already-proved owner is accepted.
pub fn staged_descriptor_for(owner: usize, original: [u8; 0x70], target: Option<Configuration>) -> Option<[u8; 0x70]> {
    staged_descriptor_in(crate::backend::Layout::Training,owner,original,target)
}
pub fn staged_descriptor_in(layout:crate::backend::Layout,owner:usize,original:[u8;0x70],target:Option<Configuration>)->Option<[u8;0x70]> {
    if !layout.selector(owner) { return None; }
    let mut result = crate::ordinary_player::staged_descriptor(original, target);
    if target.is_some() {
        // Saved row is the validated physical slot. That slot is also
        // Binding.record_index, the record publication writes. The captured
        // origin index stays in `original` and is not copied here.
        result[0x46..0x48].copy_from_slice(&(owner as u16).to_le_bytes());
    }
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
mod native_rank_tests {
    use super::*;
    use crate::backend::Layout;

    // Synthetic values only. The oracle is the native rank domain and exact
    // preservation of every other descriptor byte, not a copied game descriptor.
    fn original(layout:Layout)->(Binding,[u8;0x70]) {
        let owner=layout.visible()[1];
        let binding=Binding {epoch:7,owner,entry:0x1000,row:0x2000,holder:0x3000,
            manager:0x4000,vector_begin:0x5000,vector_end:0x5000+32*0x68,record_index:owner as u16};
        let mut descriptor=[0;0x70];
        descriptor[4..8].copy_from_slice(&owner.to_le_bytes());
        descriptor[0x14..0x18].copy_from_slice(&1u32.to_le_bytes());
        descriptor[0x18..0x1c].copy_from_slice(&9u32.to_le_bytes());
        descriptor[0x1c..0x24].fill(0xff);
        descriptor[0x28..0x2c].copy_from_slice(&1u32.to_le_bytes());
        descriptor[0x34]=1;descriptor[0x35]=1;descriptor[0x41]=1;
        let index=if layout.waiting() {26u16}else{0};
        descriptor[0x46..0x48].copy_from_slice(&index.to_le_bytes());
        (binding,descriptor)
    }

    #[test]
    fn rank_recapture_accepts_trigger_variant_boundaries_and_noop() {
        for layout in [Layout::Training,Layout::Ldn,Layout::Quickplay] {
            let (binding,origin)=original(layout);
            for (before,after) in [(5,15),(15,20),(0,50),(50,77),(77,100),(100,0),(0,0)] {
                let mut expected=origin;expected[0x37]=before;
                let mut current=expected;current[0x37]=after;
                assert_eq!(recapture_rank(layout,binding,binding,&expected,&current),Ok(current));
                assert_eq!(native_rank(&current),Ok(after));
                assert_eq!(expected[0x37],before);
                assert_eq!(origin[0x37],0);
            }
        }
    }

    #[test]
    fn rank_recapture_rejects_each_non_rank_byte_including_adjacent_role_fields() {
        let (binding,expected)=original(Layout::Ldn);
        let mut current=expected;current[0x37]=50;
        assert_eq!(recapture_rank(Layout::Ldn,binding,binding,&expected,&current),Ok(current));
        for offset in (0..0x70).filter(|offset|!matches!(*offset,0x35|0x37)) {
            let mut changed=current;changed[offset]^=1;
            assert!(recapture_rank(Layout::Ldn,binding,binding,&expected,&changed).is_err(),
                "unexpected descriptor change at {offset:#x}");
        }
    }

    #[test]
    fn control_type_recapture_accepts_only_the_indexed_domain() {
        let (binding,origin)=original(Layout::Ldn);
        let mut current=origin;current[0x35]=0x22;current[0x37]=50;
        assert_eq!(recapture_rank(Layout::Ldn,binding,binding,&origin,&current),Ok(current));
        assert_eq!(origin[0x35],1);
        assert_eq!(refresh_control_type(Layout::Ldn,binding,binding,&origin,&current),
            Err("CPU control refresh changed other descriptor bytes"));
        let mut control_only=origin;control_only[0x35]=0x22;
        assert_eq!(refresh_control_type(Layout::Ldn,binding,binding,&origin,&control_only),Ok(control_only));
        for rejected in [0x28u8,0xff] {
            let mut high=origin;high[0x35]=rejected;
            assert!(recapture_rank(Layout::Ldn,binding,binding,&origin,&high).is_err());
            assert!(refresh_control_type(Layout::Ldn,binding,binding,&origin,&high).is_err());
        }
        let mut adjacent=control_only;adjacent[0x36]^=1;
        assert!(recapture_rank(Layout::Ldn,binding,binding,&origin,&adjacent).is_err());
        assert!(refresh_control_type(Layout::Ldn,binding,binding,&origin,&adjacent).is_err());
        let stable=[0x40u8;0x70];
        assert_eq!(recapture_rank(Layout::Ldn,binding,binding,&stable,&stable),Ok(stable));
        let live=with_native_live_metadata(origin,&control_only).unwrap();
        assert_eq!(live[0x35],0x22);
        assert_eq!(&live[0x34..0x35],&origin[0x34..0x35]);
        assert_eq!(&live[0x36..0x46],&origin[0x36..0x46]);
        let restored=staged_descriptor_in(Layout::Ldn,binding.owner as usize,live,None).unwrap();
        assert_eq!(restored[0x35],0x22);
        assert_eq!(u32::from_le_bytes(restored[0x18..0x1c].try_into().unwrap()),
            u32::from_le_bytes(origin[0x18..0x1c].try_into().unwrap()));
        assert_eq!(restored[0x46..0x48],origin[0x46..0x48]);
    }

    #[test]
    fn unsupported_rank_is_rejected_on_either_side_even_without_drift() {
        let (binding,valid)=original(Layout::Training);
        for rank in [101,255] {
            let mut invalid=valid;invalid[0x37]=rank;
            assert!(native_rank(&invalid).is_err());
            assert!(with_native_rank(valid,&invalid).is_err());
            assert!(with_native_rank(invalid,&valid).is_err());
            for (expected,current) in [(valid,invalid),(invalid,valid),(invalid,invalid)] {
                assert!(recapture_rank(Layout::Training,binding,binding,&expected,&current).is_err());
            }
        }
    }

    #[test]
    fn rank_recapture_rejects_changed_binding_and_non_cpu_or_retired_lifetimes() {
        let (binding,expected)=original(Layout::Ldn);
        let mut current=expected;current[0x37]=77;
        for changed in [
            Binding {epoch:binding.epoch+1,..binding},
            Binding {owner:5,..binding},
            Binding {entry:binding.entry+8,..binding},
            Binding {row:binding.row+8,..binding},
            Binding {holder:binding.holder+8,..binding},
            Binding {manager:binding.manager+8,..binding},
            Binding {vector_begin:binding.vector_begin+0x68,..binding},
            Binding {vector_end:binding.vector_end+0x68,..binding},
            Binding {record_index:binding.record_index+1,..binding},
        ] {
            assert!(recapture_rank(Layout::Ldn,binding,changed,&expected,&current).is_err());
        }
        for invalid in [Binding {epoch:0,..binding},Binding {owner:0,..binding},Binding {owner:5,..binding}] {
            assert!(recapture_rank(Layout::Ldn,invalid,invalid,&expected,&current).is_err());
        }
        assert!(recapture_rank(Layout::Training,binding,binding,&expected,&current).is_err());
    }

    #[test]
    fn saved_original_and_saved_reuse_retain_live_rank_without_mutating_origin() {
        use crate::{inventory::MiiClass,lifetime::Tokens,native_values::RowIds};
        let saved=Configuration::from_observed(MiiClass::Swordfighter,
            Tokens {outfit:0,hat:0,colour:4,voice:37,specials:[1,0,0,0]},[0;88],
            RowIds {outfit:0x1300001bfef7a5f7,hat:0x5a00000fa9efa5ea}).unwrap();
        for layout in [Layout::Training,Layout::Ldn,Layout::Quickplay] {
            for original_kind in [9u32,0x48] {
                let (binding,mut origin)=original(layout);
                origin[0x18..0x1c].copy_from_slice(&original_kind.to_le_bytes());
                let frozen_origin=origin;
                let mut observed=origin;observed[0x37]=50;
                let expected=recapture_rank(layout,binding,binding,&origin,&observed).unwrap();
                let live_origin=with_native_rank(origin,&expected).unwrap();
                let first=staged_descriptor_in(layout,binding.owner as usize,live_origin,Some(saved)).unwrap();
                assert_eq!(first[0x37],50);
                assert_eq!(u32::from_le_bytes(first[0x18..0x1c].try_into().unwrap()),0x49);
                assert_eq!(u16::from_le_bytes(first[0x46..0x48].try_into().unwrap()),binding.record_index);
                assert_eq!(binding.record_index,binding.owner as u16);
                assert_eq!(&first[0x34..0x37],&origin[0x34..0x37]);
                assert_eq!(&first[0x38..0x43],&origin[0x38..0x43]);

                // After exact completion, native changes rank while this saved Mii is live.
                let mut changed=first;changed[0x37]=77;
                let expected=recapture_rank(layout,binding,binding,&first,&changed).unwrap();
                let live_origin=with_native_rank(origin,&expected).unwrap();
                let returned=staged_descriptor_in(layout,binding.owner as usize,live_origin,None).unwrap();
                let mut wanted=origin;wanted[0x37]=77;
                assert_eq!(returned,wanted); // Original identity/index/role, current native rank.
                assert_ne!(returned,origin);
                let reused=staged_descriptor_in(layout,binding.owner as usize,
                    with_native_rank(origin,&returned).unwrap(),Some(saved)).unwrap();
                let mut wanted=first;wanted[0x37]=77;
                assert_eq!(reused,wanted);
                assert_eq!(origin,frozen_origin);
            }
        }
    }

    #[test]
    fn rank_aware_generic_original_classification_stays_exact_for_other_bytes() {
        for layout in [Layout::Training,Layout::Ldn,Layout::Quickplay] {
            let (_,origin)=original(layout);
            let index=u16::from_le_bytes(origin[0x46..0x48].try_into().unwrap());
            let mut current=origin;current[0x37]=77;
            let classify=|descriptor:[u8;0x70]| {
                let live_origin=with_native_live_metadata(origin,&descriptor)?;
                generic_ordinary_commit_in(layout,index,GenericOrdinaryFacts {
                    reservation_held:false,origin_is_saved:false,live_is_saved:false,transaction_busy:false,
                    kind:9,origin_kind:9,published_kind:9,index,
                    descriptor_equals_origin:descriptor==live_origin,roster_cursor:Some(2),
                })
            };
            assert_eq!(classify(current),Ok(Destination {target:Target::ORIGINAL,cursor:None}));
            let mut control=current;control[0x35]=0x22;
            assert_eq!(classify(control),Ok(Destination {target:Target::ORIGINAL,cursor:None}));
            for offset in [0x34,0x36,0x38,0x43] {
                let mut changed=current;changed[offset]^=1;
                assert_eq!(classify(changed),Ok(Destination {target:Target::fighter(9),cursor:Some(2)}));
            }
            current[0x37]=101;
            assert!(classify(current).is_err());
            assert_eq!(origin[0x37],0);
        }
    }
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

#[cfg(test)]
mod recapture_diagnostic_tests {
    use super::*;
    use crate::backend::Layout;
    use crate::lifetime::Binding;

    fn flags(index_eq: bool) -> RecaptureFlags {
        RecaptureFlags {
            index_eq, scalars_eq: true, selector_record_eq: false, current_record_eq: true,
            captured_index: 4, live_index: 26,
        }
    }
    fn ldn_binding() -> Binding {
        let owner = Layout::Ldn.visible()[1];
        Binding { epoch: 7, owner, entry: 0x1000, row: 0x2000, holder: 0x3000,
            manager: 0x4000, vector_begin: 0x5000, vector_end: 0x5000 + 32 * 0x68,
            record_index: owner as u16 }
    }

    #[test]
    fn rank_only_change_has_no_pairs_and_recapture_still_accepts_it() {
        let binding = ldn_binding();
        let expected = [0u8; 0x70];
        let mut current = expected;
        current[0x37] = 50;
        assert_eq!(non_rank_descriptor_diff(&expected, &current).count, 0);
        assert_eq!(recapture_rank(Layout::Ldn, binding, binding, &expected, &current), Ok(current));
    }

    #[test]
    fn non_rank_bytes_stay_refused_and_the_line_lists_each_offset() {
        let binding = ldn_binding();
        let expected = [0u8; 0x70];
        let mut current = expected;
        current[0x04] = 0x0a;
        current[0x37] = 50;
        current[0x46] = 0x1a;
        assert_eq!(
            recapture_rank(Layout::Ldn, binding, binding, &expected, &current),
            Err("CPU rank recapture changed non-rank descriptor bytes"),
        );
        let diff = non_rank_descriptor_diff(&expected, &current);
        assert_eq!(diff.count, 2);
        assert_eq!(diff.pairs[0], (0x04, 0x00, 0x0a));
        assert_eq!(diff.pairs[1], (0x46, 0x00, 0x1a));
        assert!(diff.pairs[2..].iter().all(|pair| *pair == (0, 0, 0)));
        let mut buf = [0u8; RECAPTURE_DIAGNOSTIC_BYTES];
        let n = write_recapture_diagnostic(&mut buf, &diff, &flags(false)).unwrap();
        let line = std::str::from_utf8(&buf[..n]).unwrap();
        assert_eq!(
            line,
            "recapture non-rank n=2 index_eq=0 scalars_eq=1 selector_record_eq=0 current_record_eq=1 captured_index=4 live_index=26 04:00>0a 46:00>1a",
        );
        assert!(!line.contains(" 37:"));
    }

    #[test]
    fn every_non_rank_offset_fits_once_per_epoch_tuple() {
        let mut expected = [0x10u8; 0x70];
        expected[0x37] = 0;
        let current = [0x11u8; 0x70];
        let diff = non_rank_descriptor_diff(&expected, &current);
        assert_eq!(diff.count, 111);
        assert!(diff.pairs[..111].iter().all(|pair| pair.0 != 0x37));
        assert_eq!(diff.pairs[0].0, 0x00);
        assert_eq!(diff.pairs[110].0, 0x6f);
        let mut buf = [0u8; RECAPTURE_DIAGNOSTIC_BYTES];
        let n = write_recapture_diagnostic(&mut buf, &diff, &flags(true)).unwrap();
        assert!(n <= RECAPTURE_DIAGNOSTIC_BYTES);
        let line = std::str::from_utf8(&buf[..n]).unwrap();
        assert!(line.starts_with("recapture non-rank n=111 "));
        assert!(line.contains(" 36:10>11 "));
        assert!(line.contains(" 38:10>11 "));
        assert!(line.ends_with(" 6f:10>11"));
        assert!(!line.contains(" 37:"));
        assert!(write_recapture_diagnostic(&mut [0u8; 8], &diff, &flags(true)).is_none());
        let seen = Some((9u64, diff, flags(true)));
        assert!(recapture_observation_seen(seen, 9, diff, flags(true)));
        assert!(!recapture_observation_seen(None, 9, diff, flags(true)));
        assert!(!recapture_observation_seen(seen, 10, diff, flags(true)));
        assert!(!recapture_observation_seen(seen, 9, diff, flags(false)));
        let mut changed = diff;
        changed.pairs[0].2 ^= 1;
        assert!(!recapture_observation_seen(seen, 9, changed, flags(true)));
    }
}
