//! Helper callback admission. Captured hats are the before-value. A changed hat
//! is a fresh-O decision for a helper that belongs to the layout. Training uses
//! slots 2/3. Waiting uses slots 5/6. Synthetic hat ids only; no native fixture.

use training_mod_mii::{backend::Layout, ordinary_cpu as cpu};

#[test]
fn training_helpers_admit_a_changed_hat_and_an_unequal_saved_pair() {
    let saved = [0u16, 125];
    assert!(cpu::helper_slot_admission(Layout::Training, 2, saved, 125));
    assert!(cpu::helper_slot_admission(Layout::Training, 3, saved, 0));
    assert!(cpu::helper_slot_admission(Layout::Training, 2, saved, 0));
    assert!(cpu::helper_slot_admission(Layout::Training, 3, saved, 125));
    assert_eq!(cpu::headgear_reload(Layout::Training, 2, 0, 125), Ok(true));
    assert_eq!(cpu::headgear_reload(Layout::Training, 3, 125, 0), Ok(true));
    assert_eq!(cpu::headgear_reload(Layout::Training, 2, 0, 0), Ok(false));
}

#[test]
fn invalid_slots_and_a_max_hat_are_refused() {
    for slot in [0usize, 1, 4, 5, 6, 7, 8] {
        assert!(!cpu::helper_slot_admission(Layout::Training, slot, [0, 125], 125), "{slot}");
    }
    assert!(!cpu::helper_slot_admission(Layout::Training, 2, [0, 125], u16::MAX));
    assert_eq!(
        cpu::headgear_reload(Layout::Training, 2, 0, u16::MAX),
        Err("invalid headgear row"),
    );
}

#[test]
fn waiting_helpers_admit_a_changed_hat_and_reject_training_slots() {
    for layout in [Layout::Ldn, Layout::Quickplay] {
        assert!(cpu::helper_slot_admission(layout, 5, [0, 125], 125));
        assert!(cpu::helper_slot_admission(layout, 6, [0, 125], 0));
        assert!(cpu::helper_slot_admission(layout, 5, [0, 125], 0));
        assert!(cpu::helper_slot_admission(layout, 6, [0, 125], 125));
        assert!(!cpu::helper_slot_admission(layout, 2, [0, 125], 125));
        assert!(!cpu::helper_slot_admission(layout, 3, [0, 125], 0));
        assert_eq!(cpu::headgear_reload(layout, 5, 0, 125), Ok(true));
        assert_eq!(cpu::headgear_reload(layout, 6, 125, 0), Ok(true));
        assert_eq!(cpu::headgear_reload(layout, 5, 0, 0), Ok(false));
    }
}
