//! Quickplay waiting shares the local-wireless backend shape and pure CPU policy; its
//! mode word, match excursion, portrait admission and menu-catalog ownership stay
//! distinct. Host value checks only: no native adapters, files or threads.
extern crate self as training_mod_consts;
pub use training_mod_mii as mii;

use training_mod_mii::{
    backend::Layout,
    catalog_request::{Kind, Live, Requests, Step, Verdict},
    ordinary_cpu::{self, Destination, GenericOrdinaryFacts, Target},
    ordinary_player::Owner,
    role_owner,
};

#[allow(dead_code)]
#[path = "../../src/character_memory.rs"]
mod character_memory;
#[allow(dead_code)]
#[path = "../../src/mii_hud_portrait_policy.rs"]
mod portrait_policy;

fn owners() -> [Option<Owner>; 8] {
    // run18's four native selectors/current indices; addresses are synthetic.
    let mut rows = [None; 8];
    for (id, index) in [(0, 0), (4, 26), (5, 27), (6, 28)] {
        rows[id] = Some(Owner { entry: 0x1000 + id * 0x100, row: 0x2000 + id * 0x100,
            information: 0x3000 + id * 0x100, object: 0x4000 + id * 0x100, index, selector: id as u8, alias: 0 });
    }
    rows
}

#[test]
fn quickplay_waiting_is_its_own_backend_with_the_shared_shape() {
    assert_eq!(Layout::scene(true, 0x0d, 0x07010101, true), Some(Layout::Quickplay));
    assert_eq!(Layout::scene(true, 0x0d, 0x02010000, true), Some(Layout::Ldn));
    assert_eq!(Layout::scene(true, 0x0d, 0x07010300, true), None); // arena family
    assert_eq!(Layout::scene(true, 0x0e, 0x07010101, true), None); // online match melee
    assert_eq!(Layout::scene(true, 0x0d, 0x07010101, false), None); // practice not active
    let entries = owners().map(|o| o.map_or(0, |o| o.entry));
    assert!(Layout::Quickplay.shape(&entries) && Layout::Ldn.shape(&entries) && !Layout::Training.shape(&entries));
    assert_eq!(Layout::Quickplay.visible(), [0, 4]);
    assert_eq!(Layout::Quickplay.helpers(), [5, 6]);
}

#[test]
fn quickplay_pure_cpu_policy_is_the_local_wireless_policy() {
    let rows = owners();
    let kinds = rows.map(|row| row.map(|_| 0x49));
    assert_eq!(role_owner::claims_in(Layout::Quickplay, &rows, &kinds), role_owner::claims_in(Layout::Ldn, &rows, &kinds));
    assert!(role_owner::claims_in(Layout::Quickplay, &rows, &kinds).is_some());
    assert!(role_owner::claims_in(Layout::Training, &rows, &kinds).is_none());
    for id in [4usize, 5, 6] {
        assert_eq!(ordinary_cpu::assignment_in(Layout::Quickplay, id, &rows), ordinary_cpu::assignment_in(Layout::Ldn, id, &rows));
        assert!(ordinary_cpu::assignment_in(Layout::Quickplay, id, &rows).is_some());
        let own = rows[id].unwrap();
        assert!(ordinary_cpu::publication_indices_in(Layout::Quickplay, id, own.index, id as u16, own.index, id as u16, own.index));
        assert!(!ordinary_cpu::publication_indices_in(Layout::Quickplay, id, own.index, id as u16, own.index, own.index, own.index));
        assert!(!ordinary_cpu::publication_indices_in(Layout::Training, id, own.index, id as u16, own.index, id as u16, own.index));
    }
    assert!(ordinary_cpu::assignment_in(Layout::Quickplay, 1, &rows).is_none()); // not a waiting selector
    let facts = GenericOrdinaryFacts { reservation_held: false, origin_is_saved: false, live_is_saved: false,
        transaction_busy: false, kind: 9, origin_kind: 9, published_kind: 9, index: 26,
        descriptor_equals_origin: true, roster_cursor: None };
    assert_eq!(ordinary_cpu::generic_ordinary_commit_in(Layout::Quickplay, 26, facts),
        Ok(Destination { target: Target::ORIGINAL, cursor: None }));
    assert_eq!(ordinary_cpu::generic_ordinary_commit_in(Layout::Quickplay, 26, facts),
        ordinary_cpu::generic_ordinary_commit_in(Layout::Ldn, 26, facts));
    assert!(ordinary_cpu::generic_ordinary_commit_in(Layout::Training, 26, facts).is_err()); // Training keeps index 0
}

#[test]
fn quickplay_match_excursion_restores_both_roles_in_the_same_lobby() {
    let mut memory = character_memory::WaitingSession::empty();
    memory.scene_changed(true, 0x0d, 0x07010101);
    memory.observe(0, 10);
    memory.observe(1, 20);
    memory.commit(0, 30);
    memory.commit(1, 40);
    memory.player_departed();
    assert!(memory.scene_changed(true, 0x0e, 0x07010101)); // the match keeps the mode word
    assert!(memory.scene_changed(true, 0x0d, 0x07010101));
    assert!(!memory.observe(1, 20));
    assert!(!memory.observe(0, 10));
    assert_eq!(memory.restore_target(0), Some(30));
    assert_eq!(memory.restore_target(1), Some(40));
    // An unobserved intermediate melee clears rather than guesses.
    let mut unknown = character_memory::WaitingSession::empty();
    unknown.scene_changed(true, 0x0d, 0x07010101);
    unknown.commit(0, 30);
    assert!(!unknown.scene_changed(true, 0x20, 0x07010101));
    unknown.scene_changed(true, 0x0d, 0x07010101);
    assert_eq!(unknown.restore_target(0), None);
}

#[test]
fn portrait_stores_admit_quickplay_waiting_and_refuse_its_match_and_arenas() {
    assert!(portrait_policy::may_write_scene(0x0d, 0x07010101));
    assert!(portrait_policy::may_write_scene(0x0d, 0x02010000));
    assert!(portrait_policy::may_write_scene(0x20, 0x04040000));
    assert!(!portrait_policy::may_write_scene(0x0e, 0x07010101));
    assert!(!portrait_policy::may_write_scene(0x0d, 0x07010300));
    assert!(!portrait_policy::may_write_scene(0x0d, 0x07010400));
}

#[test]
fn quickplay_menu_catalog_owns_its_opening_over_the_editor_only_catalog() {
    let live = Live { menu_active: true, opening: 12, owners: [3, 5], auto: None };
    let menu = Kind::Menu { opening: 12, owners: [3, 5] };
    let mut requests = Requests::new();
    assert!(matches!(requests.request(menu).step, Step::Submit(kind) if kind == menu));
    let editor = requests.request(Kind::Editor { opening: 12 });
    assert!(matches!(editor.step, Step::Wait));
    assert_eq!(editor.dropped, Some(Kind::Editor { opening: 12 }));
    let effect = requests.complete(requests.serial(), live);
    assert_eq!(effect.verdict, Verdict::Menu { opening: 12, owners: [3, 5] });
    assert!(matches!(effect.step, Step::Wait));
    assert!(!requests.pending());
}
