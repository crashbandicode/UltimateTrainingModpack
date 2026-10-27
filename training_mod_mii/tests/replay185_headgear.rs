//! Source candidate tests, not native loading acceptance.
//! Independent facts: REPLAY185 native CFMK/companion/import agree on Gunner,
//! outfit75/hat125/colour0/voice37/specials2221 and the canonical below. Only
//! CharInfo's noncanonical create-id/name prefix is redacted. Hat7 is explicitly
//! a synthetic variation; its resource presence is not claimed.
use training_mod_mii::{
    backend::Layout,
    draft::{Library, Saved},
    inventory::{build_snapshot, MiiClass},
    lifetime::{Binding, Configuration, Tokens},
    native_values::{Mirrors, RowIds},
    ordinary_cpu::{self as cpu, HatPublication},
    ordinary_player::{self, Owner, SameClass, SameClassValues},
    replay::ReplayMiis,
    selection::{self, Action, Active, PlayerLive, PlayerOrigin, Preferences, Role},
};

const REPLAY_ROWS: RowIds = RowIds { outfit: 0x1300_4b14_67a2_d93a, hat: 0x5a00_7d15_3956_e6b8 };
const HATLESS_ROWS: RowIds = RowIds { outfit: 0x1300_441a_f17e_2dff, hat: 0x5a00_000f_a9ef_a5ea };
const OTHER_ROWS: RowIds = RowIds { outfit: 0x1300_4b14_67a2_d93a, hat: 0x5a00_0712_3456_789a };
// mii-presets.json / CPU preference, independently captured, unaffected by redaction.
const CPU_PREF: &str = "4a0000000000000000000000000000000000000000000000000000000000000000000000000000000b0040400000050900004b08000a0807060100120c08070601010c0204090d0808060108050108051114050401040214000202020100253ad9a26714b8e6563915871dab5712";

fn companion() -> Vec<u8> {
    let mut bytes = vec![1, 1, 1, 0x4a, 1, 0]; // schema,count,entry,kind,source index
    bytes.extend_from_slice(&[0; 0x26]); // redacted, noncanonical metadata only
    bytes.extend_from_slice(&[
        0,11,0,64,64,0,0,5,9,0,0,75,8,0,10,8,7,6,1,0,18,12,8,7,6,
        1,1,12,2,4,9,13,8,8,6,1,8,5,1,8,5,17,20,5,4,1,4,2,20,0,
    ]);
    bytes.extend_from_slice(&[75,0,125,0,0,0,37,0,2,2,2,1]);
    bytes
}
fn replay() -> Configuration {
    let bytes = companion();
    assert_eq!(bytes.len(), 106);
    let miis = ReplayMiis::from_bytes(&bytes).unwrap();
    let raw = miis.for_track(1, 0x4a).unwrap();
    assert_eq!((raw.entry(), raw.class(), raw.source_index()), (1, MiiClass::Gunner, 1));
    assert_eq!(raw.tokens(), Tokens { outfit:75, hat:125, colour:0, voice:37, specials:[2,2,2,1] });
    let c = raw.resolve(REPLAY_ROWS).unwrap();
    assert_eq!(c.canonical().to_hex(), CPU_PREF);
    c
}
fn hatless() -> Configuration {
    // Saved11194 token/row facts; synthetic face, not a native save fixture.
    Configuration::from_observed(MiiClass::Gunner,
        Tokens { outfit:68, hat:0, colour:4, voice:37, specials:[0;4] },
        [0;88], HATLESS_ROWS).unwrap()
}
fn other() -> Configuration {
    let c = replay();
    Configuration::from_observed(MiiClass::Gunner, Tokens { hat:7, ..c.tokens() },
        *c.charinfo(), OTHER_ROWS).unwrap()
}
fn origin() -> [u8;0x70] {
    let mut d = [0xa5;0x70];
    d[4..8].copy_from_slice(&1u32.to_le_bytes());
    d[0x18..0x1c].copy_from_slice(&10u32.to_le_bytes()); // captured CPU Ness
    d[0x37] = 77; // live native-rank regression must remain preserved
    d[0x46..0x48].copy_from_slice(&0u16.to_le_bytes());
    d[0x4a..0x4c].copy_from_slice(&0u16.to_le_bytes());
    d
}
fn publication(d: &[u8;0x70], before:u16, target:u16, payload:Option<Configuration>) -> HatPublication<'_> {
    HatPublication { layout:Layout::Training, owner:1, source:false, opt_in:true,
        initialized:false, fingerprints:true, before, target, descriptor:d, payload }
}

#[test]
fn captured_replay_import_and_menu_request_reach_qualified_training_headgear_route() {
    let c = replay();
    let native = build_snapshot(vec![]);
    let mut library = Library::default();
    assert_eq!(library.import(c, "Replay Gunner", &native), Ok(Saved::Created(c.canonical())));
    let catalog = native.with_library(&library);
    let old = hatless().canonical();
    let before = Preferences::default().with(Role::Cpu, MiiClass::Gunner, Some(old)).unwrap();
    let after = Preferences::default().with(Role::Cpu, MiiClass::Gunner, Some(c.canonical())).unwrap();
    let active = Active { player:PlayerLive::NotMii, cpu:Some((MiiClass::Gunner, old)) };
    let actions = selection::on_menu_close(&catalog, &before, &after, PlayerOrigin::NotMii, active);
    assert_eq!(actions.len(), 1);
    assert!(matches!(actions[0], Action::ReloadCpu(_)));
    let request = cpu::Request::from_menu(7, 7, active.cpu, Some(old), actions[0]).unwrap();
    let choice = request.choice().unwrap();
    assert_eq!(choice.slot_id, None); // imported preset is not a native save slot
    let target = catalog.configuration(choice).unwrap();
    assert_eq!(target, c);
    assert_eq!(cpu::cpu_hat_route(Layout::Training, 0, target.tokens().hat), Ok(true));
    let d = cpu::staged_descriptor(origin(), Some(target));
    assert_eq!(publication(&d, 0, 125, Some(target)).validate(), Ok(()));
}

#[test]
fn forward_descriptor_stages_exact_hat_and_preserves_native_rank_and_unowned_bytes() {
    let original = origin();
    for c in [replay(), other()] {
        let d = cpu::staged_descriptor_in(Layout::Training, 1, original, Some(c)).unwrap();
        assert_eq!(&d[0x46..0x50], &[1,0,75,0,c.tokens().hat as u8,0,0,0,37,0]);
        assert_eq!(d[0x37], 77);
        for (i, byte) in d.iter().enumerate() {
            if !(0x14..0x24).contains(&i) && !(0x28..0x2c).contains(&i)
                && i!=0x43 && !(0x46..0x50).contains(&i) {
                assert_eq!(*byte, original[i], "unowned descriptor byte {i:#x}");
            }
        }
        assert_eq!(publication(&d, 0, c.tokens().hat, Some(c)).validate(), Ok(()));
    }
    assert_eq!(original, origin());
}

#[test]
fn same_class_changed_hat_always_selects_fresh_owner_in_both_directions() {
    for (before, after) in [(hatless(),replay()), (replay(),hatless()),
        (replay(),other()), (other(),replay())] {
        assert!(!cpu::retained_same_class_candidate(before, after));
        assert_eq!(cpu::cpu_hat_route(Layout::Training, before.tokens().hat, after.tokens().hat), Ok(true));
        let d = cpu::staged_descriptor(origin(), Some(after));
        let phase = publication(&d, before.tokens().hat, after.tokens().hat, Some(after));
        assert!(phase.validate().is_ok());
        assert!(HatPublication { initialized:true, ..phase }.validate().is_err());
    }
    let c = replay();
    let same_hat = Configuration::from_observed(MiiClass::Gunner, Tokens { colour:1, ..c.tokens() },
        *c.charinfo(), REPLAY_ROWS).unwrap();
    assert!(cpu::retained_same_class_candidate(c, same_hat));
    assert_eq!(cpu::cpu_hat_route(Layout::Training, 125, 125), Ok(false));
}

#[test]
fn return_to_ordinary_origin_needs_no_payload_and_retains_native_record_pair() {
    let c = replay();
    let original = origin();
    let returned = cpu::staged_descriptor(original, None);
    assert_eq!(returned, original);
    assert_eq!(cpu::cpu_hat_route(Layout::Training, c.tokens().hat, 0), Ok(true));
    assert!(publication(&returned, 125, 0, None).validate().is_ok());
    let record = ordinary_player::retained_record([0xa5;0x68], Some(c));
    assert_eq!(ordinary_player::retained_record(record, None), record);
    assert_eq!(&record[88..], &[0xa5;16]);
    let mut wrong_index = returned;
    wrong_index[0x46..0x48].copy_from_slice(&1u16.to_le_bytes());
    assert!(publication(&wrong_index, 125, 0, None).validate().is_err());
    // Saved-origin return uses the existing Configuration and retained destination.
    let saved_origin = cpu::staged_descriptor(origin(), Some(other()));
    assert!(publication(&saved_origin, 125, 7, Some(other())).validate().is_ok());
}

#[test]
fn publication_refuses_missing_opt_in_wrong_role_layout_source_latch_and_fingerprint() {
    let c = replay();
    let d = cpu::staged_descriptor(origin(), Some(c));
    let valid = publication(&d, 0, 125, Some(c));
    for invalid in [
        HatPublication { opt_in:false, ..valid },
        HatPublication { owner:0, ..valid },
        HatPublication { owner:2, ..valid },
        HatPublication { owner:3, ..valid },
        HatPublication { layout:Layout::Ldn, owner:4, ..valid },
        HatPublication { layout:Layout::Quickplay, owner:4, ..valid },
        HatPublication { layout:Layout::Ldn, ..valid },
        HatPublication { layout:Layout::Quickplay, ..valid },
        HatPublication { source:true, ..valid },
        HatPublication { initialized:true, ..valid },
        HatPublication { fingerprints:false, ..valid },
        HatPublication { target:7, ..valid },
        HatPublication { target:u16::MAX, ..valid },
        HatPublication { payload:Some(hatless()), ..valid },
        HatPublication { payload:None, ..valid },
    ] { assert!(invalid.validate().is_err()); }
    for offset in [0x18,0x46,0x4a,0x4b] {
        let mut bad = d; bad[offset] ^= 1;
        assert!(HatPublication { descriptor:&bad, ..valid }.validate().is_err());
    }
    // Neither layout nor helper policy is globally relaxed by this CPU opt-in.
    for layout in [Layout::Ldn,Layout::Quickplay] {
        assert_eq!(cpu::cpu_hat_route(layout, 0, 125), Err("CPU unchanged hat required"));
        assert_eq!(cpu::cpu_hat_route(layout, 125, 0), Err("CPU unchanged hat required"));
        assert_eq!(cpu::cpu_hat_route(layout, 125, 125), Ok(false));
    }
    assert!(cpu::helper_configuration(c, [0,0]).is_err());
    assert_eq!(cpu::helper_configuration(c, [0,125]), Ok(c));
    let equal = HatPublication { before:125, opt_in:false, fingerprints:false, ..valid };
    assert!(equal.validate().is_ok());
    assert!(HatPublication { opt_in:true, ..equal }.validate().is_err());
}

#[test]
fn replay_and_restored_configuration_still_reject_invalid_hat_rows() {
    let bytes = companion();
    let miis = ReplayMiis::from_bytes(&bytes).unwrap();
    let raw = miis.for_track(1, 0x4a).unwrap();
    assert!(raw.resolve(HATLESS_ROWS).is_err());
    assert!(raw.resolve(RowIds { hat:0x1300_7d15_3956_e6b8, ..REPLAY_ROWS }).is_err());
    let mut invalid = bytes.clone();
    invalid[96..98].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(ReplayMiis::from_bytes(&invalid).is_err());
    assert!(ReplayMiis::from_bytes(&bytes[..105]).is_err());
    let mut stored = serde_json::to_value(replay()).unwrap();
    stored["tags"][1] = serde_json::json!(0x5aff_ff15_3956_e6b8u64);
    assert!(serde_json::from_value::<Configuration>(stored).is_err());
}

#[test]
fn retained_same_class_proof_keeps_its_original_changed_hat_refusal() {
    let before = hatless(); let after = replay(); let t = before.tokens();
    let d = cpu::staged_descriptor(origin(), Some(before));
    let owner = Owner { entry:0x1000,row:0x2000,information:0x3000,object:0x4000,
        index:1,selector:1,alias:0 };
    let values = SameClassValues {
        binding:Binding { epoch:7,owner:1,entry:owner.entry,row:owner.row,holder:0x5000,
            manager:0x6000,vector_begin:0x7000,vector_end:0x7000+4*0x68,record_index:1 },
        thread:0x9000,owner,header:[1,0x4a,u32::MAX,u32::MAX,u32::from(d[0x41])],
        mirrors:Mirrors { index:1,tokens:t,fe:[1,68,0,4,37],d:[1,68,0,4],
            d_specials:[0;4],owner_index:1,owner:[68,0,4,37] },
        initialized:true,task_idle:true,rebuild_open:true,
    };
    let record = ordinary_player::retained_record([0xa5;0x68], Some(before));
    assert_eq!(SameClass::prepare_cpu_in(Layout::Training,before,after,values,&d,
        [HATLESS_ROWS,REPLAY_ROWS],&record).err(), Some("same-class hat changed"));
}

#[test]
fn existing_completion_verifier_requires_target_hat_in_all_four_mirrors() {
    for (c,rows) in [(replay(),REPLAY_ROWS),(other(),OTHER_ROWS)] {
        let t=c.tokens();
        let mirrors=Mirrors {index:1,tokens:t,fe:[1,75,t.hat,0,37],d:[1,75,t.hat,0],
            d_specials:[2,2,2,1],owner_index:1,owner:[75,u32::from(t.hat),0,37]};
        let record=ordinary_player::retained_record([0xa5;0x68],Some(c));
        assert!(training_mod_mii::role_owner::validate_saved(c,1,mirrors,rows,&record).is_ok());
        for field in 0..4 {
            let mut stale=mirrors;
            match field {
                0=>stale.tokens.hat=0,
                1=>stale.fe[2]=0,
                2=>stale.d[2]=0,
                _=>stale.owner[1]=0,
            }
            assert!(training_mod_mii::role_owner::validate_saved(c,1,stale,rows,&record).is_err());
        }
    }
}

#[test]
fn source_wiring_validates_before_publication_and_preserves_native_completion() {
    // Static integration regression: host value tests cannot execute Horizon locks
    // or INSTALL. Check the real native source's write boundary, not a test writer.
    let shared = include_str!("../../src/mii_selector_native.rs");
    let publication = shared.split("pub(crate) unsafe fn publication(").nth(1).unwrap()
        .split("impl PublicationGuard").next().unwrap();
    assert!(publication.find("policy::HatPublication").unwrap()
        < publication.find("Ok(PublicationGuard").unwrap());
    assert!(publication.contains("}.validate()?;"));
    assert!(!publication.contains("wr("));
    assert!(!publication.contains("wr::<"));
    assert!(!publication.contains(".write("));
    assert!(publication.contains("!observed.initialized"));
    assert!(publication.contains("observed.outstanding == Some(0)"));
    let adapter = include_str!("../../src/mii_ordinary_cpu_native.rs");
    let publish = adapter.split("pub(super) unsafe fn publish(track:").nth(1).unwrap()
        .split("pub(super) unsafe fn completed").next().unwrap();
    for gate in ["shared::publication(target(track)", "retained_origin(track)?",
        "fresh.gate.may_publish", "final_registry("] {
        assert!(publish.find(gate).unwrap() < publish.find("publication.write();").unwrap());
    }
    let cpu = include_str!("../../src/mii_ordinary_cpu.rs");
    assert!(cpu.contains("current.scalars.e.hat,publish.map_or(origin.scalars.e.hat"));
    assert!(cpu.contains("(Some(before),Some(after)) if !hat_change"));
    assert!(cpu.contains("retained_same_class_candidate(before,after)"));
    let complete = shared.split("pub(crate) unsafe fn completed(").nth(1).unwrap();
    for proof in ["now.descriptor == *descriptor", "now.scalars == scalars",
        "rd::<[u16; 4]>(own.information, 0x72) == tokens[..4]",
        "rd::<[u32; 4]>(own.object, 0x6c)", "native resource pair absent"] {
        assert!(complete.contains(proof));
    }
    assert!(adapter.contains("role_owner::validate_saved(c,now.index,mirrors(own),rows,&now.current_record)?"));
    // These other writer families retain their own hat-equality constraints.
    assert!(include_str!("../src/ordinary_player.rs").contains("same-class hat changed"));
    let native=include_str!("../../src/mii_ordinary_native.rs");
    assert!(native.contains("unqualified hat change"));
    let scalars=native.split("pub(crate) unsafe fn write(self,e:usize,d:usize)").nth(1).unwrap()
        .split("struct Protected").next().unwrap();
    assert!(scalars.contains("wr(e,0x1ac,self.e.hat)"));
    assert!(!scalars.contains("wr(d,0x76"));
    assert!(include_str!("../../src/mii_managed_helper.rs").contains("hat_change: false"));
}
