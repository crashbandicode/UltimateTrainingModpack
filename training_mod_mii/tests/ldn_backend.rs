//! Bounded production-policy checks for the observed LDN owner/index split and return.
use training_mod_mii::{backend::Layout, inventory::{build_snapshot, RawSlot, TaggedId},
    lifetime::{Binding, Configuration}, native_values, ordinary_cpu, ordinary_player::{Owner, Gate, Ticket},
    remembered::Value, role_owner};

#[allow(dead_code)]
#[path = "../../src/character_memory.rs"]
mod character_memory;
#[allow(dead_code)]
#[path = "../../src/source_lifetime.rs"]
mod source_lifetime;

fn owners() -> [Option<Owner>; 8] {
    // run18's four native selectors/current indices; addresses are synthetic.
    let mut rows=[None;8];
    for (id,index) in [(0,0),(4,26),(5,27),(6,28)] {
        rows[id]=Some(Owner {entry:0x1000+id*0x100,row:0x2000+id*0x100,
            information:0x3000+id*0x100,object:0x4000+id*0x100,index,selector:id as u8,alias:0});
    }
    rows
}

#[test]
fn backend20_ldn_current_and_destination_union_is_bounded_and_exclusive() {
    let rows=owners();
    let kinds=rows.map(|row|row.map(|_|0x49));
    assert!(role_owner::claims_in(Layout::Ldn,&rows,&kinds).is_some());
    assert!(role_owner::claims_in(Layout::Training,&rows,&kinds).is_none());
    for (id,expected) in [(4,[0,27,5,28,6]),(5,[0,26,4,28,6]),(6,[0,26,4,27,5])] {
        let union=ordinary_cpu::assignment_in(Layout::Ldn,id,&rows).unwrap();
        assert_eq!(union.map(|v|v.unwrap_or(u16::MAX)),
            [expected[0],expected[1],expected[2],expected[3],expected[4],u16::MAX,u16::MAX,u16::MAX]);
        let own=rows[id].unwrap();
        let binding=Binding {epoch:7,owner:id as u32,entry:own.entry,row:own.row,
            holder:0x5000,manager:0x6000,vector_begin:0x7000,vector_end:0x7000+29*104,record_index:id as u16};
        assert!(native_values::record_address(binding,binding).is_ok());
        for index in union.into_iter().flatten() {
            let b=Binding {record_index:index,..binding};
            assert!(native_values::record_address(b,b).is_ok());
            let truncated=Binding {vector_end:b.vector_begin+usize::from(index)*104,..b};
            assert!(native_values::record_address(truncated,truncated).is_err());
        }
        assert!(ordinary_cpu::publication_indices_in(Layout::Ldn,id,own.index,id as u16,own.index,id as u16,own.index));
        assert!(!ordinary_cpu::publication_indices_in(Layout::Ldn,id,own.index,id as u16,own.index,own.index,own.index));
    }
    let mut collision=rows;collision[5].as_mut().unwrap().index=4;
    assert!(ordinary_cpu::assignment_in(Layout::Ldn,4,&collision).is_none());
    assert!(role_owner::claims_in(Layout::Ldn,&collision,&kinds).is_none());
    collision=rows;collision[6].as_mut().unwrap().object=rows[4].unwrap().object;
    assert!(role_owner::claims_in(Layout::Ldn,&collision,&kinds).is_none());
    collision=rows;collision[4].as_mut().unwrap().alias=1;
    assert!(ordinary_cpu::assignment_in(Layout::Ldn,4,&collision).is_none());
}

#[test]
fn backend20_ordinary_shared_zero_does_not_claim_player_payload() {
    let rows=owners().map(|row|row.map(|o|Owner {index:0,..o}));
    let mut kinds=rows.map(|row|row.map(|_|9));kinds[0]=Some(0x49);
    assert!(role_owner::claims_in(Layout::Ldn,&rows,&kinds).is_some());
    assert!(!role_owner::current_record_claim(9,0));
    assert!(role_owner::current_record_claim(0x49,0));
    assert!(role_owner::current_record_claim(9,26));
    kinds[4]=Some(0x49);
    assert!(role_owner::claims_in(Layout::Ldn,&rows,&kinds).is_none());
}

#[test]
fn backend20_logical_cpu_generation_and_exact_source_token_reject_reused_addresses() {
    use source_lifetime::{NativeEntry,SourceBook,SourceKey};
    let mut generations=[17,23];
    let logical=Layout::Ldn.logical(4).unwrap();
    let old=Ticket {epoch:generations[logical],entry:0x1400,sequence:1};
    let mut gate=Gate::new(old).unwrap();
    assert!(gate.enter(old,6));assert!(gate.publish(old,6));gate.leave(old,14);
    assert!(gate.can_complete(old));
    generations[logical]+=1;gate.depart();
    assert_eq!(generations,[17,24]);
    let fresh=Ticket {epoch:generations[logical],..old};
    assert!(!gate.can_complete(old));assert!(!gate.can_complete(fresh));
    let mut book=SourceBook::<8,1>::new();
    let key=SourceKey {kind:0x49,costume:0};
    let load=book.begin_install(5,book.epoch(),NativeEntry(0x1500),key).unwrap();
    book.depart_scene().unwrap();
    assert!(book.complete(load).is_err());
    assert_eq!(book.native_destroyed(load.source()),Ok(0));
    let replacement=book.begin_install(5,book.epoch(),NativeEntry(0x1500),key).unwrap();
    assert_ne!(replacement,load);assert!(book.complete(load).is_err());
    assert!(book.complete(replacement).is_ok());
}

fn configurations() -> (Configuration,Configuration) {
    let catalog=build_snapshot(vec![RawSlot {index:0,slot_id:2622,name_len:1,
        name_units:[65,0,0,0,0,0,0,0,0,0],colour:4,voice:37,specials:[1,0,0,0],
        tag_outfit:TaggedId(0x13002b147bc59435),tag_hat:TaggedId(0x5a00000fa9efa5ea),
        tag_chara:TaggedId(0xc1004d15d7061f77),charinfo:[0;88]}]);
    let saved=catalog.configs[0].clone();
    let original=Configuration::from_saved(&saved).unwrap();
    let mut different=saved;different.charinfo[0]=1;
    (original,Configuration::from_saved(&different).unwrap())
}

#[test]
fn backend20_same_kind_same_canonical_return_keeps_exact_configuration_and_cursor() {
    let (original,accepted)=configurations();
    assert_eq!(original.canonical(),accepted.canonical());assert_ne!(original,accepted);
    let native=Value {fighter:0x49,forms:[0x49,u32::MAX,u32::MAX],cursor:None,
        configuration:Some(original),owned:true,original:true};
    let wanted=Value {configuration:Some(accepted),cursor:Some(2),original:false,..native};
    let mut memory=character_memory::WaitingSession::empty();
    memory.scene_changed(true,13,0x02010000);
    memory.observe(0,native);memory.observe(1,native);
    memory.commit(1,wanted); // Only the accepted completion is remembered.
    memory.player_departed();memory.scene_changed(true,1,0x02010000);
    memory.scene_changed(true,13,0x02010000);memory.observe(1,native);
    assert_eq!(memory.restore_target(1),Some(wanted));
    assert!(!role_owner::payload_already_active(native.configuration,wanted.configuration,true));
    assert!(!Value {original:true,..wanted}.matches_original(native.fighter,native.forms,native.configuration));
    assert!(native.matches_original(native.fighter,native.forms,native.configuration));
    memory.manual_request(1); // Interrupted replacement never overwrites the accepted value.
    memory.scene_changed(true,1,0x02010000);memory.scene_changed(true,13,0x02010000);
    assert_eq!(memory.restore_target(1),Some(wanted));
    memory.commit(1,wanted);assert_eq!(memory.restore_target(1),None);
    memory.player_departed();assert!(memory.observe(0,native));
    assert_eq!(memory.restore_target(1),None); // Fresh room forgets both roles.
    memory.scene_changed(true,1,0x02010000);
    assert_eq!(memory.restore_target(0),Some(native));assert_eq!(memory.restore_target(1),None);
}
