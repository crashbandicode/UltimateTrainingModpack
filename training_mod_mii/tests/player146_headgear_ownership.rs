//! Fresh-O admission for each Training row. Synthetic face bytes stand in for a
//! saved Mii; they are not a native fixture. Player index 0 is that row's own
//! record, not a copy of CPU index 1.

use training_mod_mii::{
    backend::Layout,
    inventory::{MiiClass, CHARINFO_LEN},
    lifetime::{Binding, Configuration, Tokens},
    native_values::{Mirrors, RowIds, RECORD_LEN},
    ordinary_cpu::{self as cpu, HatPublication},
    ordinary_player::{self, Owner, SameClass, SameClassValues},
};

const OUTFIT: u16 = 68;
const OUTFIT_HASH: u64 = 0x1af17e2dff;
const PLAIN_HAT: u16 = 0;
const PLAIN_HAT_HASH: u64 = 0x0fa9efa5ea;
const ARMORED_HAT: u16 = 3;
const ARMORED_HAT_HASH: u64 = 0x1111111111;

fn rows(hat: u16, hat_hash: u64) -> RowIds {
    RowIds {
        outfit: (0x13u64 << 56) | (u64::from(OUTFIT) << 40) | OUTFIT_HASH,
        hat: (0x5au64 << 56) | (u64::from(hat) << 40) | hat_hash,
    }
}

fn mii(class: MiiClass, hat: u16, hat_hash: u64, face: u8) -> Configuration {
    Configuration::from_observed(
        class,
        Tokens { outfit: OUTFIT, hat, colour: 4, voice: 37, specials: [0; 4] },
        [face; CHARINFO_LEN],
        rows(hat, hat_hash),
    )
    .unwrap()
}

fn blank_descriptor() -> [u8; 0x70] {
    let mut descriptor = [0u8; 0x70];
    descriptor[0x46..0x48].copy_from_slice(&0u16.to_le_bytes());
    descriptor
}

fn player_owner() -> (Binding, Owner) {
    let owner = Owner {
        entry: 0x1000, row: 0x2000, information: 0x3000, object: 0x4000,
        index: 0, selector: 0, alias: 0,
    };
    let binding = Binding {
        epoch: 5, owner: 0, entry: owner.entry, row: owner.row,
        holder: 0x5000, manager: 0x6000,
        vector_begin: 0x7000, vector_end: 0x7000 + 4 * RECORD_LEN,
        record_index: 0,
    };
    (binding, owner)
}

fn mirrors(tokens: Tokens) -> Mirrors {
    Mirrors {
        index: 0, tokens,
        fe: [0, tokens.outfit, tokens.hat, tokens.colour, tokens.voice],
        d: [0, tokens.outfit, tokens.hat, tokens.colour],
        d_specials: tokens.specials.map(u32::from),
        owner_index: 0,
        owner: [tokens.outfit, tokens.hat, tokens.colour, tokens.voice].map(u32::from),
    }
}

fn publication<'a>(
    descriptor: &'a [u8; 0x70], owner: u32, before: u16, target: u16, opt_in: bool,
    payload: Option<Configuration>,
) -> HatPublication<'a> {
    HatPublication {
        layout: Layout::Training, owner, source: false, opt_in,
        initialized: false, fingerprints: opt_in,
        before, target,
        destination_index: u16::from_le_bytes(descriptor[0x46..0x48].try_into().unwrap()),
        original_index: 0,
        descriptor, payload,
    }
}

#[test]
fn fresh_o_uses_each_rows_own_index_for_all_three_classes() {
    assert!(!Layout::Training.selector(0));
    assert!(Layout::Training.selector(1));
    for class in [MiiClass::Brawler, MiiClass::Swordfighter, MiiClass::Gunner] {
        let plain = mii(class, PLAIN_HAT, PLAIN_HAT_HASH, 0x11);
        let armored = mii(class, ARMORED_HAT, ARMORED_HAT_HASH, 0x11);
        assert_eq!(cpu::headgear_reload(Layout::Training, 0, plain.tokens().hat, armored.tokens().hat), Ok(true));
        assert_eq!(cpu::headgear_reload(Layout::Training, 1, plain.tokens().hat, armored.tokens().hat), Ok(true));
        assert_eq!(cpu::headgear_reload(Layout::Training, 2, plain.tokens().hat, armored.tokens().hat), Ok(true));
        assert_eq!(cpu::headgear_reload(Layout::Training, 3, plain.tokens().hat, armored.tokens().hat), Ok(true));
        let mut player_staged = ordinary_player::staged_descriptor(blank_descriptor(), Some(armored));
        player_staged[0x46..0x48].copy_from_slice(&0u16.to_le_bytes());
        assert!(publication(&player_staged, 0, PLAIN_HAT, ARMORED_HAT, true, Some(armored)).validate().is_ok());
        let cpu_staged = cpu::staged_descriptor(blank_descriptor(), Some(armored));
        assert_eq!(u16::from_le_bytes(cpu_staged[0x46..0x48].try_into().unwrap()), 1);
        assert!(publication(&cpu_staged, 0, PLAIN_HAT, ARMORED_HAT, true, Some(armored)).validate().is_err());
        assert!(publication(&cpu_staged, 1, PLAIN_HAT, ARMORED_HAT, true, Some(armored)).validate().is_ok());
        let helper = cpu::staged_descriptor_for(2, blank_descriptor(), Some(armored)).unwrap();
        assert_eq!(u16::from_le_bytes(helper[0x46..0x48].try_into().unwrap()), 2);
        assert!(publication(&helper, 2, PLAIN_HAT, ARMORED_HAT, true, Some(armored)).validate().is_ok());
        assert_eq!(
            cpu::headgear_reload(Layout::Ldn, 2, PLAIN_HAT, ARMORED_HAT),
            Err("headgear row is not on this layout"),
        );
        assert_eq!(cpu::headgear_reload(Layout::Ldn, 0, PLAIN_HAT, ARMORED_HAT), Ok(true));
        assert_eq!(cpu::cpu_hat_route(Layout::Ldn, 4, PLAIN_HAT, ARMORED_HAT), Ok(true));
        assert_eq!(cpu::cpu_hat_route(Layout::Quickplay, 1, PLAIN_HAT, ARMORED_HAT), Err("CPU unchanged hat required"));
        assert!(!cpu::retained_same_class_candidate(plain, armored));
    }
    let origin = blank_descriptor();
    let unchanged = publication(&origin, 0, PLAIN_HAT, PLAIN_HAT, false, None);
    assert!(unchanged.validate().is_ok());

    let (binding, owner) = player_owner();
    let mut shaped = origin;
    shaped[0x18..0x1c].copy_from_slice(&(MiiClass::Gunner as u32).to_le_bytes());
    shaped[0x1c..0x20].copy_from_slice(&u32::MAX.to_le_bytes());
    shaped[0x20..0x24].copy_from_slice(&u32::MAX.to_le_bytes());
    let tokens = [OUTFIT, PLAIN_HAT, 4, 37];
    let header = [0u32, MiiClass::Gunner as u32, u32::MAX, u32::MAX, 0];
    assert!(!cpu::outgoing_object_matches_for(
        owner, &shaped, tokens, owner.object, header, owner.index,
        tokens.map(u32::from),
    ));
    let cpu_owner = Owner { index: 1, selector: 1, ..owner };
    let cpu_header = [1u32, MiiClass::Gunner as u32, u32::MAX, u32::MAX, 0];
    assert!(cpu::outgoing_object_matches_for(
        cpu_owner, &shaped, tokens, cpu_owner.object, cpu_header, cpu_owner.index,
        tokens.map(u32::from),
    ));
    assert_eq!(binding.owner, 0);
    assert_eq!(binding.record_index, 0);
}

#[test]
fn player_same_class_proof_rejects_a_changed_hat_and_keeps_an_equal_hat() {
    let (binding, owner) = player_owner();
    let before = mii(MiiClass::Gunner, PLAIN_HAT, PLAIN_HAT_HASH, 0x11);
    let same_hat = mii(MiiClass::Gunner, PLAIN_HAT, PLAIN_HAT_HASH, 0x22);
    let changed_hat = mii(MiiClass::Gunner, ARMORED_HAT, ARMORED_HAT_HASH, 0x11);
    let descriptor = ordinary_player::staged_descriptor(blank_descriptor(), Some(before));
    assert_eq!(u32::from_le_bytes(descriptor[0x18..0x1c].try_into().unwrap()), MiiClass::Gunner as u32);
    assert_eq!(descriptor[0x41], 0);
    let mut record = [0u8; RECORD_LEN];
    record[..CHARINFO_LEN].copy_from_slice(before.charinfo());
    let current = SameClassValues {
        binding, thread: 0x9000, owner,
        header: [0, MiiClass::Gunner as u32, u32::MAX, u32::MAX, 0],
        mirrors: mirrors(before.tokens()),
        initialized: true, task_idle: true, rebuild_open: true,
    };
    let same_rows = [rows(PLAIN_HAT, PLAIN_HAT_HASH); 2];
    assert!(SameClass::prepare(before, same_hat, current, &descriptor, same_rows, &record).is_ok());
    assert_eq!(
        SameClass::prepare(
            before, changed_hat, current, &descriptor,
            [rows(PLAIN_HAT, PLAIN_HAT_HASH), rows(ARMORED_HAT, ARMORED_HAT_HASH)],
            &record,
        )
        .err(),
        Some("same-class hat changed"),
    );
}

#[test]
fn fresh_publication_keeps_equal_hat_and_rejects_a_foreign_index() {
    let armored = mii(MiiClass::Swordfighter, ARMORED_HAT, ARMORED_HAT_HASH, 0x11);
    let mut row = ordinary_player::staged_descriptor(blank_descriptor(), Some(armored));
    row[0x46..0x48].copy_from_slice(&0u16.to_le_bytes());
    let admitted = publication(&row, 0, PLAIN_HAT, ARMORED_HAT, true, Some(armored));
    assert!(admitted.validate().is_ok());
    row[0x46..0x48].copy_from_slice(&1u16.to_le_bytes());
    assert!(publication(&row, 0, PLAIN_HAT, ARMORED_HAT, true, Some(armored)).validate().is_err());
    assert_eq!(
        cpu::headgear_reload(Layout::Training, 4, PLAIN_HAT, ARMORED_HAT),
        Err("headgear row is not on this layout"),
    );
    assert_eq!(cpu::headgear_reload(Layout::Training, 0, ARMORED_HAT, ARMORED_HAT), Ok(false));
}
